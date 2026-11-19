use crate::error::{AppError, AppResult};

#[cfg(windows)]
const SCRIPT: &str = r#"param([string]$Out)
function Done([string]$v) { Set-Content -Path $Out -Value $v; exit }
$viaWu = $null
try {
  $s = New-Object -ComObject Microsoft.Update.Session
  $r = $s.CreateUpdateSearcher().Search("IsInstalled=0 and Type='Driver'")
  $u = New-Object -ComObject Microsoft.Update.UpdateColl
  foreach ($x in $r.Updates) { if ($x.DriverHardwareID -like 'usb\vid_05ac*') { [void]$u.Add($x) } }
  if ($u.Count -gt 0) {
    $d = $s.CreateUpdateDownloader(); $d.Updates = $u; [void]$d.Download()
    $i = $s.CreateUpdateInstaller(); $i.Updates = $u; $res = $i.Install()
    if ($res.ResultCode -eq 2 -or $res.ResultCode -eq 3) {
      pnputil /scan-devices | Out-Null
      if ($res.RebootRequired) { $viaWu = 'reboot' } else { $viaWu = 'installed' }
    }
  }
} catch {}
if ($viaWu) { Done $viaWu }
try {
  if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') {
    $url = 'https://catalog.s.download.windowsupdate.com/c/msdownload/update/driver/drvs/2026/03/101d0b13-d2f1-4b3c-abc1-b44b16f4af01_9d977e0798577e7f495e26fb287eb2f070ebbd9e.cab'
  } else {
    $url = 'https://catalog.s.download.windowsupdate.com/d/msdownload/update/driver/drvs/2023/10/5fb262ea-d52d-46a7-9361-f3260ba57a1a_3e8075a4dded0a795131f82285e2f1a06525ebc2.cab'
  }
  $sha1 = ($url -split '_')[-1].Replace('.cab', '')
  $dir = Join-Path (Split-Path $Out) 'cab'
  New-Item -ItemType Directory -Force -Path $dir | Out-Null
  $cab = Join-Path $dir 'apple.cab'
  [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
  Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $cab
  if ((Get-FileHash -Algorithm SHA1 -Path $cab).Hash.ToLower() -ne $sha1) { Done 'error checksum' }
  expand.exe $cab -F:* $dir | Out-Null
  pnputil /add-driver (Join-Path $dir 'AppleUsb.inf') /install | Out-Null
  $code = $LASTEXITCODE
  pnputil /scan-devices | Out-Null
  if ($code -eq 3010) { Done 'reboot' }
  if ($code -eq 0 -or $code -eq 259) { Done 'installed' }
  Done ('failed pnputil ' + $code)
} catch { Done ('error ' + $_.Exception.Message) }
"#;

#[cfg(windows)]
fn run() -> AppResult<String> {
    use std::os::windows::process::CommandExt;
    let dir = std::env::temp_dir().join(format!("mad-driver-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).map_err(|e| AppError::new("fs", e.to_string()))?;
    let script = dir.join("driver.ps1");
    let out = dir.join("result.txt");
    std::fs::write(&script, SCRIPT).map_err(|e| AppError::new("fs", e.to_string()))?;
    let quote = |p: &std::path::Path| p.to_string_lossy().replace('\'', "''");
    let command = format!(
        "try {{ Start-Process -FilePath powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList '-NoProfile','-ExecutionPolicy','Bypass','-File','\"{}\"','-Out','\"{}\"' }} catch {{ exit 1223 }}",
        quote(&script),
        quote(&out)
    );
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &command])
        .creation_flags(0x0800_0000)
        .status()
        .map_err(|e| AppError::new("driver", e.to_string()))?;
    let result = std::fs::read_to_string(&out).ok().map(|s| s.trim().to_string());
    let _ = std::fs::remove_dir_all(&dir);
    match result {
        Some(r) if !r.is_empty() => Ok(r),
        _ if status.code() == Some(1223) => Ok("cancelled".to_string()),
        _ => Err(AppError::new("driver", format!("no result, exit {:?}", status.code()))),
    }
}

#[cfg(not(windows))]
fn run() -> AppResult<String> {
    Err(AppError::new("driver", "windows only"))
}

#[tauri::command]
pub async fn install_apple_driver() -> AppResult<String> {
    tokio::task::spawn_blocking(run)
        .await
        .map_err(|e| AppError::new("driver", e.to_string()))?
}
