use std::{
    collections::{HashMap, HashSet},
    sync::{LazyLock, Mutex},
};

use idevice::{
    IdeviceError, IdeviceService,
    lockdown::LockdownClient,
    provider::UsbmuxdProvider,
    usbmuxd::{Connection, UsbmuxdAddr, UsbmuxdConnection, UsbmuxdDevice},
};
use serde::Serialize;

use crate::error::{AppError, AppResult};

pub const LABEL: &str = "mad-installer";

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub udid: String,
    pub name: String,
    pub version: String,
    pub product_type: String,
    pub developer_mode: Option<bool>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum DeviceStatus {
    NoService { message: String },
    NoDriver,
    None,
    Locked,
    Trust,
    Denied,
    Error { message: String },
    Ready { device: DeviceInfo },
}

static PAIRING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));
static PAIR_RESULT: LazyLock<Mutex<HashMap<String, &'static str>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static HOST_ID: LazyLock<String> =
    LazyLock::new(|| uuid::Uuid::new_v4().to_string().to_uppercase());

static ADDR: LazyLock<Mutex<Option<UsbmuxdAddr>>> = LazyLock::new(|| Mutex::new(None));

fn candidates() -> Vec<UsbmuxdAddr> {
    let mut list = Vec::new();
    if std::env::var("USBMUXD_SOCKET_ADDRESS").is_ok() {
        if let Ok(a) = UsbmuxdAddr::from_env_var() {
            list.push(a);
        }
    }
    list.push(UsbmuxdAddr::default());
    #[cfg(not(unix))]
    list.push(UsbmuxdAddr::TcpSocket(std::net::SocketAddr::new(
        std::net::Ipv6Addr::LOCALHOST.into(),
        UsbmuxdAddr::DEFAULT_PORT,
    )));
    list
}

pub async fn usbmuxd() -> Result<UsbmuxdConnection, IdeviceError> {
    let cached = ADDR.lock().unwrap().clone();
    if let Some(a) = cached {
        if let Ok(c) = a.connect(0).await {
            return Ok(c);
        }
    }
    let mut last = IdeviceError::NoEstablishedConnection;
    for a in candidates() {
        match a.connect(0).await {
            Ok(c) => {
                *ADDR.lock().unwrap() = Some(a);
                return Ok(c);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

fn addr() -> UsbmuxdAddr {
    ADDR.lock()
        .unwrap()
        .clone()
        .unwrap_or_else(|| UsbmuxdAddr::from_env_var().unwrap_or_default())
}

pub async fn provider_for(udid: &str) -> AppResult<UsbmuxdProvider> {
    let mut mux = usbmuxd()
        .await
        .map_err(|e| AppError::new("noService", e.to_string()))?;
    let dev = mux
        .get_device(udid)
        .await
        .map_err(|_| AppError::new("noDevice", "device disconnected"))?;
    Ok(dev.to_provider(addr(), LABEL))
}

fn pick(devices: Vec<UsbmuxdDevice>) -> Option<UsbmuxdDevice> {
    let mut usb: Vec<_> = devices
        .iter()
        .filter(|d| matches!(d.connection_type, Connection::Usb))
        .cloned()
        .collect();
    if usb.is_empty() {
        devices.into_iter().next()
    } else {
        Some(usb.remove(0))
    }
}

fn start_pairing(dev: UsbmuxdDevice) {
    let udid = dev.udid.clone();
    {
        let mut set = PAIRING.lock().unwrap();
        if set.contains(&udid) {
            return;
        }
        set.insert(udid.clone());
    }
    PAIR_RESULT.lock().unwrap().remove(&udid);
    tauri::async_runtime::spawn(async move {
        let result = tokio::time::timeout(std::time::Duration::from_secs(120), pair(&dev))
            .await
            .unwrap_or(Err(IdeviceError::NoEstablishedConnection));
        let outcome = match result {
            Ok(()) => None,
            Err(IdeviceError::PasswordProtected) => Some("locked"),
            Err(IdeviceError::UserDeniedPairing) => Some("denied"),
            Err(e) => {
                tracing::warn!("pairing failed: {e:?}");
                Some("error")
            }
        };
        if let Some(o) = outcome {
            PAIR_RESULT.lock().unwrap().insert(udid.clone(), o);
        }
        PAIRING.lock().unwrap().remove(&udid);
    });
}

async fn pair(dev: &UsbmuxdDevice) -> Result<(), IdeviceError> {
    let mut mux = usbmuxd().await?;
    let buid = mux.get_buid().await?;
    let provider = dev.to_provider(addr(), LABEL);
    let mut lockdown = LockdownClient::connect(&provider).await?;
    let mut record = lockdown
        .pair(HOST_ID.as_str(), buid, Some("mad installer"))
        .await?;
    record.udid = Some(dev.udid.clone());
    let bytes = record.serialize()?;
    let mut mux = usbmuxd().await?;
    mux.save_pair_record(&dev.udid, bytes).await?;
    Ok(())
}

async fn read_info(dev: &UsbmuxdDevice) -> Result<DeviceInfo, IdeviceError> {
    let mut mux = usbmuxd().await?;
    let pairing = mux.get_pair_record(&dev.udid).await?;
    let provider = dev.to_provider(addr(), LABEL);
    let mut lockdown = LockdownClient::connect(&provider).await?;
    lockdown.start_session(&pairing).await?;
    let text = |v: Result<plist::Value, IdeviceError>| {
        v.ok()
            .and_then(|v| v.as_string().map(str::to_string))
            .unwrap_or_default()
    };
    let name = text(lockdown.get_value(Some("DeviceName"), None).await);
    let version = text(lockdown.get_value(Some("ProductVersion"), None).await);
    let product_type = text(lockdown.get_value(Some("ProductType"), None).await);
    let developer_mode = lockdown
        .get_value(
            Some("DeveloperModeStatus"),
            Some("com.apple.security.mac.amfi"),
        )
        .await
        .ok()
        .and_then(|v| v.as_boolean());
    Ok(DeviceInfo {
        udid: dev.udid.clone(),
        name,
        version,
        product_type,
        developer_mode,
    })
}

#[tauri::command]
pub async fn device_status() -> DeviceStatus {
    match tokio::time::timeout(std::time::Duration::from_secs(8), status()).await {
        Ok(s) => s,
        Err(_) => DeviceStatus::Error {
            message: "device did not answer in 8s".to_string(),
        },
    }
}

async fn status() -> DeviceStatus {
    let mut mux = match usbmuxd().await {
        Ok(m) => m,
        Err(e) => return DeviceStatus::NoService { message: e.to_string() },
    };
    let devices = match mux.get_devices().await {
        Ok(d) => d,
        Err(e) => return DeviceStatus::Error { message: e.to_string() },
    };
    let Some(dev) = pick(devices) else {
        PAIR_RESULT.lock().unwrap().clear();
        if missing_apple_driver().await {
            return DeviceStatus::NoDriver;
        }
        return DeviceStatus::None;
    };

    if PAIRING.lock().unwrap().contains(&dev.udid) {
        return DeviceStatus::Trust;
    }

    let paired = mux.get_pair_record(&dev.udid).await.is_ok();
    if !paired {
        let last = PAIR_RESULT.lock().unwrap().get(&dev.udid).copied();
        start_pairing(dev);
        return match last {
            Some("locked") => DeviceStatus::Locked,
            Some("denied") => DeviceStatus::Denied,
            _ => DeviceStatus::Trust,
        };
    }

    match read_info(&dev).await {
        Ok(device) => DeviceStatus::Ready { device },
        Err(IdeviceError::PasswordProtected) => DeviceStatus::Locked,
        Err(IdeviceError::InvalidHostID) => {
            start_pairing(dev);
            DeviceStatus::Trust
        }
        Err(e) => DeviceStatus::Error { message: e.to_string() },
    }
}

#[cfg(windows)]
async fn missing_apple_driver() -> bool {
    use std::time::{Duration, Instant};
    static CACHE: LazyLock<Mutex<Option<(Instant, bool)>>> = LazyLock::new(|| Mutex::new(None));
    if let Some((at, value)) = *CACHE.lock().unwrap() {
        if at.elapsed() < Duration::from_secs(5) {
            return value;
        }
    }
    let value = tokio::task::spawn_blocking(|| {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("pnputil")
            .args(["/enum-devices", "/connected", "/drivers"])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
        Some(text.contains("usb\\vid_05ac&pid_12") && !text.contains("usbaapl"))
    })
    .await
    .ok()
    .flatten()
    .unwrap_or(false);
    *CACHE.lock().unwrap() = Some((Instant::now(), value));
    value
}

#[cfg(not(windows))]
async fn missing_apple_driver() -> bool {
    false
}
