use std::{
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};

pub const REPO: &str = "madgram/mad-iOS";

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    digest: Option<String>,
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    published_at: Option<String>,
    prerelease: bool,
    draft: bool,
    assets: Vec<GhAsset>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub tag: String,
    pub version: String,
    pub name: String,
    pub published_at: Option<String>,
    pub prerelease: bool,
    pub asset_name: String,
    pub size: u64,
    pub url: String,
    pub sha256: Option<String>,
    pub cached: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    pub tag: String,
    pub received: u64,
    pub total: u64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IpaInfo {
    pub bundle_id: String,
    pub version: String,
    pub extensions: Vec<String>,
}

pub fn http() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("mad-installer/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| AppError::new("network", e.to_string()))
}

fn cache_dir(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| AppError::new("fs", e.to_string()))?
        .join("ipa");
    std::fs::create_dir_all(&dir).map_err(|e| AppError::new("fs", e.to_string()))?;
    Ok(dir)
}

pub fn cached_path(app: &AppHandle, release: &Release) -> AppResult<PathBuf> {
    Ok(cache_dir(app)?.join(format!("{}-{}", release.tag, release.asset_name)))
}

fn is_cached(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).map(|m| m.len() == size).unwrap_or(false)
}

fn version_from(asset: &str, tag: &str) -> String {
    asset
        .strip_prefix("mad-")
        .and_then(|s| s.strip_suffix(".ipa"))
        .map(str::to_string)
        .unwrap_or_else(|| tag.trim_start_matches('v').to_string())
}

#[tauri::command]
pub async fn list_releases(app: AppHandle) -> AppResult<Vec<Release>> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=50");
    let resp = http()?
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| AppError::new("network", e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::new("github", format!("HTTP {}", resp.status())));
    }
    let releases: Vec<GhRelease> = resp
        .json()
        .await
        .map_err(|e| AppError::new("github", e.to_string()))?;
    let dir = cache_dir(&app)?;
    let mut out = Vec::new();
    for r in releases.into_iter().filter(|r| !r.draft) {
        let Some(asset) = r
            .assets
            .into_iter()
            .find(|a| a.name.starts_with("mad-") && a.name.ends_with(".ipa"))
        else {
            continue;
        };
        let path = dir.join(format!("{}-{}", r.tag_name, asset.name));
        out.push(Release {
            version: version_from(&asset.name, &r.tag_name),
            name: r.name.unwrap_or_else(|| r.tag_name.clone()),
            tag: r.tag_name,
            published_at: r.published_at,
            prerelease: r.prerelease,
            cached: is_cached(&path, asset.size),
            size: asset.size,
            sha256: asset
                .digest
                .and_then(|d| d.strip_prefix("sha256:").map(str::to_lowercase)),
            url: asset.browser_download_url,
            asset_name: asset.name,
        });
    }
    Ok(out)
}

static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub async fn download_release(app: AppHandle, release: Release) -> AppResult<()> {
    download(&app, &release).await.map(|_| ())
}

pub async fn download(app: &AppHandle, release: &Release) -> AppResult<PathBuf> {
    let _lock = DOWNLOAD_LOCK.lock().await;
    let dest = cached_path(app, release)?;
    if is_cached(&dest, release.size) {
        let _ = app.emit(
            "download-progress",
            DownloadProgress {
                tag: release.tag.clone(),
                received: release.size,
                total: release.size,
            },
        );
        return Ok(dest);
    }
    let part = dest.with_extension("part");
    let resp = http()?
        .get(&release.url)
        .send()
        .await
        .map_err(|e| AppError::new("network", e.to_string()))?;
    if !resp.status().is_success() {
        return Err(AppError::new("download", format!("HTTP {}", resp.status())));
    }
    let total = resp.content_length().unwrap_or(release.size);
    let mut file = tokio::fs::File::create(&part)
        .await
        .map_err(|e| AppError::new("fs", e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut received = 0u64;
    let mut last = Instant::now();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| AppError::new("network", e.to_string()))?;
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| AppError::new("fs", e.to_string()))?;
        received += chunk.len() as u64;
        if last.elapsed() > Duration::from_millis(80) {
            last = Instant::now();
            let _ = app.emit("download-progress", DownloadProgress { tag: release.tag.clone(), received, total });
        }
    }
    file.flush().await.map_err(|e| AppError::new("fs", e.to_string()))?;
    drop(file);
    if let Some(expected) = &release.sha256 {
        let actual = hex::encode(hasher.finalize());
        if &actual != expected {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(AppError::new("checksum", "sha256 mismatch"));
        }
    }
    tokio::fs::rename(&part, &dest)
        .await
        .map_err(|e| AppError::new("fs", e.to_string()))?;
    let _ = app.emit("download-progress", DownloadProgress { tag: release.tag.clone(), received: total, total });
    Ok(dest)
}

pub fn inspect(path: &Path) -> AppResult<IpaInfo> {
    let file = std::fs::File::open(path).map_err(|e| AppError::new("fs", e.to_string()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| AppError::new("ipa", e.to_string()))?;
    let names: Vec<String> = zip.file_names().map(str::to_string).collect();
    let main = names
        .iter()
        .find(|n| {
            let parts: Vec<&str> = n.split('/').collect();
            parts.len() == 3 && parts[0] == "Payload" && parts[1].ends_with(".app") && parts[2] == "Info.plist"
        })
        .cloned()
        .ok_or_else(|| AppError::new("ipa", "no Info.plist"))?;
    let read_plist = |zip: &mut zip::ZipArchive<std::fs::File>, name: &str| -> AppResult<plist::Dictionary> {
        let mut buf = Vec::new();
        zip.by_name(name)
            .map_err(|e| AppError::new("ipa", e.to_string()))?
            .read_to_end(&mut buf)
            .map_err(|e| AppError::new("ipa", e.to_string()))?;
        plist::from_bytes(&buf).map_err(|e| AppError::new("ipa", e.to_string()))
    };
    let dict = read_plist(&mut zip, &main)?;
    let get = |d: &plist::Dictionary, k: &str| {
        d.get(k).and_then(|v| v.as_string()).unwrap_or_default().to_string()
    };
    let extensions = names
        .iter()
        .filter(|n| n.contains(".appex/") && n.ends_with(".appex/Info.plist") && n.matches(".appex/").count() == 1)
        .filter_map(|n| read_plist(&mut zip, n).ok().map(|d| get(&d, "CFBundleIdentifier")))
        .collect();
    Ok(IpaInfo {
        bundle_id: get(&dict, "CFBundleIdentifier"),
        version: get(&dict, "CFBundleShortVersionString"),
        extensions,
    })
}
