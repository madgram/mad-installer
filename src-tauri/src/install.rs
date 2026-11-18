use std::{collections::HashMap, path::PathBuf};

use chrono::{DateTime, Duration, Utc};
use idevice::{
    IdeviceService, installation_proxy::InstallationProxyClient, lockdown::LockdownClient,
};
use apple_codesign_quick::ProvisioningProfile;
use isideload::{
    dev::{
        app_groups::AppGroupsApi,
        app_ids::{AppIdsApi, Profile},
        certificates::DevelopmentCertificate,
        developer_session::DeveloperSession,
        devices::DevicesApi,
        teams::{DeveloperTeam, TeamsApi},
    },
    sideload::{
        application::{Application, SpecialApp},
        builder::{ExtensionsBehavior, MaxCertsBehavior},
        cert_identity::CertificateIdentity,
        sign,
    },
    util::{callbacks::MaxCertsCallbackBox, keyring_storage::KeyringStorage},
};
use rootcause::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::oneshot;

use crate::{
    AppState,
    account::SERVICE,
    device::provider_for,
    error::{AppError, AppResult},
    releases::{self, Release},
};

const MACHINE_NAME: &str = "mad installer";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstallRecord {
    pub udid: String,
    pub device_name: String,
    pub email: String,
    pub tag: String,
    pub version: String,
    pub bundle_id: String,
    pub installed_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct StepEvent {
    pub step: &'static str,
    pub progress: f32,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CertInfo {
    pub serial: String,
    pub name: Option<String>,
    pub machine: Option<String>,
    pub expires: Option<String>,
}

fn records_path(app: &AppHandle) -> AppResult<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::new("fs", e.to_string()))?;
    std::fs::create_dir_all(&dir).map_err(|e| AppError::new("fs", e.to_string()))?;
    Ok(dir.join("installs.json"))
}

fn load_records(app: &AppHandle) -> HashMap<String, InstallRecord> {
    records_path(app)
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_record(app: &AppHandle, record: InstallRecord) -> AppResult<()> {
    let mut all = load_records(app);
    all.insert(record.udid.clone(), record);
    let json = serde_json::to_vec_pretty(&all).map_err(|e| AppError::new("fs", e.to_string()))?;
    std::fs::write(records_path(app)?, json).map_err(|e| AppError::new("fs", e.to_string()))
}

#[tauri::command]
pub fn install_record(app: AppHandle, udid: String) -> Option<InstallRecord> {
    load_records(&app).remove(&udid)
}

#[tauri::command]
pub fn answer_certs(state: State<'_, AppState>, serials: Option<Vec<String>>) {
    if let Some(tx) = state.certs.lock().unwrap().take() {
        let _ = tx.send(serials);
    }
}

fn step(app: &AppHandle, step: &'static str, progress: f32) {
    let _ = app.emit("install-step", StepEvent { step, progress });
}

fn date_to_utc(d: plist::Date) -> DateTime<Utc> {
    DateTime::<Utc>::from(std::time::SystemTime::from(d))
}

async fn device_name(udid: &str) -> AppResult<String> {
    let provider = provider_for(udid).await?;
    let mut mux = crate::device::usbmuxd()
        .await
        .map_err(|e| AppError::new("noService", e.to_string()))?;
    let pairing = mux
        .get_pair_record(udid)
        .await
        .map_err(|e| AppError::new("device", e.to_string()))?;
    let mut lockdown = LockdownClient::connect(&provider)
        .await
        .map_err(|e| AppError::new("device", e.to_string()))?;
    lockdown
        .start_session(&pairing)
        .await
        .map_err(|e| AppError::from_report("device", e))?;
    Ok(lockdown
        .get_value(Some("DeviceName"), None)
        .await
        .ok()
        .and_then(|v| v.as_string().map(str::to_string))
        .unwrap_or_else(|| "iPhone".to_string()))
}

#[tauri::command]
pub async fn install(
    app: AppHandle,
    state: State<'_, AppState>,
    udid: String,
    release: Release,
) -> AppResult<InstallRecord> {
    let _busy = state
        .busy
        .try_lock()
        .map_err(|_| AppError::new("busy", "already installing"))?;
    let session = state
        .session
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| AppError::new("notLoggedIn", "not logged in"))?;

    step(&app, "download", 0.0);
    let ipa = releases::download(&app, &release).await?;
    let info = releases::inspect(&ipa)?;

    step(&app, "prepare", 0.0);
    let name = device_name(&udid).await?;

    let handle = app.clone();
    let certs_cb: MaxCertsCallbackBox = Box::new(move |certs: Vec<DevelopmentCertificate>| {
        let handle = handle.clone();
        Box::pin(async move {
            let list: Vec<CertInfo> = certs
                .iter()
                .filter_map(|c| {
                    Some(CertInfo {
                        serial: c.serial_number.clone()?,
                        name: c.name.clone(),
                        machine: c.machine_name.clone(),
                        expires: c
                            .expiration_date
                            .map(|d| date_to_utc(d).to_rfc3339()),
                    })
                })
                .collect();
            let (tx, rx) = oneshot::channel();
            *handle.state::<AppState>().certs.lock().unwrap() = Some(tx);
            handle
                .emit("certs-limit", list)
                .map_err(|e| report!("emit failed: {e}"))?;
            rx.await.map_err(|_| report!("cancelled"))
        })
    });

    let mut dev = session.dev.clone();
    let team = dev
        .list_teams()
        .await
        .map_err(|e| AppError::from_report("apple", e))?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::new("apple", "no developer team"))?;
    dev.ensure_device_registered(&team, &name, &udid, None)
        .await
        .map_err(|e| AppError::from_report("apple", e))?;

    step(&app, "sign", 0.0);
    let sign_handle = app.clone();
    let installed = installed_ids(&udid, &info.bundle_id).await;
    let (signed, bundle_id) = sign_mad(
        &mut dev,
        &team,
        &session.email,
        ipa.clone(),
        &installed,
        MaxCertsBehavior::Prompt(certs_cb),
        move |p: f32| {
            let h = sign_handle.clone();
            async move { step(&h, "sign", p) }
        },
    )
    .await
    .map_err(|e| AppError::from_report("sign", e))?;

    step(&app, "install", 0.0);
    let provider = provider_for(&udid).await?;
    let install_handle = app.clone();
    let result = isideload::sideload::install::install_app(&provider, &signed, move |p| {
        step(&install_handle, "install", p as f32 / 100.0)
    })
    .await;

    let extracted = std::env::temp_dir().join(format!(
        "{}_extracted",
        ipa.file_name().unwrap_or_default().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&extracted);
    result.map_err(|e| AppError::from_report("install", e))?;

    let now = Utc::now();
    let expires_at = dev
        .list_app_ids(&team, None)
        .await
        .ok()
        .and_then(|r| r.app_ids.into_iter().find(|a| a.identifier == bundle_id))
        .and_then(|a| a.expiration_date)
        .map(date_to_utc)
        .filter(|d| *d > now)
        .unwrap_or(now + Duration::days(7));

    let record = InstallRecord {
        udid: udid.clone(),
        device_name: name,
        email: session.email.clone(),
        tag: release.tag.clone(),
        version: release.version.clone(),
        bundle_id,
        installed_at: now,
        expires_at,
    };
    save_record(&app, record.clone())?;
    step(&app, "done", 1.0);
    Ok(record)
}

async fn installed_ids(udid: &str, original_id: &str) -> Vec<String> {
    let Ok(provider) = provider_for(udid).await else {
        return Vec::new();
    };
    let Ok(mut proxy) = InstallationProxyClient::connect(&provider).await else {
        return Vec::new();
    };
    let mut ids: Vec<String> = proxy
        .get_apps(Some("User"), None)
        .await
        .map(|apps| apps.into_keys().collect())
        .unwrap_or_default();
    ids.retain(|id| id == original_id || id.starts_with(&format!("{original_id}.")) && !id[original_id.len() + 1..].contains('.'));
    ids.sort_by_key(|id| id.len());
    ids
}

async fn sign_mad<F, Fut>(
    dev: &mut DeveloperSession,
    team: &DeveloperTeam,
    email: &str,
    ipa: PathBuf,
    installed: &[String],
    certs: MaxCertsBehavior<MaxCertsCallbackBox>,
    progress: F,
) -> Result<(PathBuf, String), Report>
where
    F: Fn(f32) -> Fut,
    Fut: Future<Output = ()>,
{
    let storage = KeyringStorage::new(SERVICE.to_string());
    let identity = CertificateIdentity::retrieve(MACHINE_NAME, email, dev, team, &storage, &certs)
        .await
        .context("Failed to retrieve certificate identity")?;
    progress(0.1).await;

    let mut application = Application::new(ipa)?;
    let original_id = application.main_bundle_id()?;
    let app_name = application.main_app_name()?;
    let existing = dev.list_app_ids(team, None).await?;
    let owned = |id: &str| existing.app_ids.iter().any(|a| a.identifier == id);
    let main_id = installed
        .iter()
        .find(|id| owned(id))
        .cloned()
        .or_else(|| owned(&original_id).then(|| original_id.clone()))
        .unwrap_or_else(|| format!("{}.{}", original_id, team.team_id));
    application.update_bundle_id(&original_id, &main_id)?;

    let mut app_ids = application
        .register_app_ids(&ExtensionsBehavior::RegisterAll, dev, team)
        .await?;
    let main_app_id = app_ids
        .iter()
        .find(|a| a.identifier == main_id)
        .cloned()
        .ok_or_else(|| report!("App ID {} was not registered", main_id))?;

    let group_id = format!("group.{main_id}");
    let group = dev.ensure_app_group(team, &app_name, &group_id, None).await?;
    for app_id in app_ids.iter_mut() {
        app_id.ensure_group_feature(dev, team).await?;
        dev.assign_app_group(team, &group, app_id, None).await?;
    }
    progress(0.15).await;

    let special: Option<SpecialApp> = None;
    let main_profile = dev
        .download_team_provisioning_profile(team, &main_app_id, None)
        .await?;
    let mut profiles: Vec<(String, Profile, plist::Dictionary)> = Vec::new();
    for id in app_ids.into_iter().filter(|a| a.identifier != main_id) {
        let profile = dev
            .download_team_provisioning_profile(team, &id, None)
            .await
            .context(format!("Failed to download provisioning profile for {}", id.identifier))?;
        let entitlements = ProvisioningProfile::parse(profile.encoded_profile.as_ref())?
            .entitlements()
            .clone();
        profiles.push((id.identifier.clone(), profile, entitlements));
    }
    progress(0.2).await;

    application.bundle.write_info()?;
    for ext in application.bundle.app_extensions_mut() {
        ext.write_info()?;
    }
    for fw in application.bundle.frameworks_mut() {
        fw.write_info()?;
    }
    progress(0.3).await;

    sign::sign(
        &mut application,
        &identity,
        &main_profile,
        &mut profiles,
        &special,
        team,
        Some(progress),
    )
    .await
    .context("Failed to sign app")?;

    Ok((application.bundle.bundle_dir.clone(), main_id))
}
