mod account;
mod device;
mod driver;
mod error;
mod install;
mod releases;

use std::sync::Mutex;

use tokio::sync::oneshot;

pub struct AppState {
    pub session: Mutex<Option<account::Session>>,
    pub tfa: Mutex<Option<oneshot::Sender<account::TfaAnswer>>>,
    pub certs: Mutex<Option<oneshot::Sender<Option<Vec<String>>>>>,
    pub busy: tokio::sync::Mutex<()>,
    pub restore_lock: tokio::sync::Mutex<()>,
}

#[tauri::command]
fn platform() -> &'static str {
    std::env::consts::OS
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _ = isideload::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            session: Mutex::new(None),
            tfa: Mutex::new(None),
            certs: Mutex::new(None),
            busy: tokio::sync::Mutex::new(()),
            restore_lock: tokio::sync::Mutex::new(()),
        })
        .invoke_handler(tauri::generate_handler![
            platform,
            device::device_status,
            driver::install_apple_driver,
            releases::list_releases,
            releases::download_release,
            account::account_status,
            account::login,
            account::answer_tfa,
            account::logout,
            install::install,
            install::install_record,
            install::answer_certs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running mad installer");
}
