use std::sync::Arc;

use isideload::{
    anisette::{AnisetteClientInfo, AnisetteDataGenerator, remote_v3::RemoteV3AnisetteProvider},
    auth::{
        apple_account::{AppToken, AppleAccount, TwoFactorCallbackParams, TwoFactorCallbackResponse},
        grandslam::GrandSlam,
    },
    dev::{developer_session::DeveloperSession, teams::TeamsApi},
    util::keyring_storage::KeyringStorage,
};
use keyring::Entry;
use rootcause::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{RwLock, oneshot};

use crate::{
    AppState,
    error::{AppError, AppResult},
};

pub const SERVICE: &str = "mad-installer";
const SESSION_KEY: &str = "session";

#[derive(Serialize, Deserialize)]
struct StoredSession {
    email: String,
    name: Option<String>,
    adsid: String,
    token: String,
    duration: u64,
    expiry: u64,
}

#[derive(Clone)]
pub struct Session {
    pub email: String,
    pub name: Option<String>,
    pub dev: DeveloperSession,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum TfaAnswer {
    Code { code: String },
    Sms { id: u32 },
    Devices,
    Resend,
    Cancel,
}

fn client_info() -> AnisetteClientInfo {
    AnisetteClientInfo {
        client_info: "<Mac15,7> <macOS;27.0;26A5378j> <com.apple.AuthKit/1 (com.apple.akd/1.0)>"
            .to_string(),
        user_agent: "akd/1.0 CFNetwork/811.5.4".to_string(),
    }
}

fn anisette() -> Result<RemoteV3AnisetteProvider, Report> {
    Ok(RemoteV3AnisetteProvider::default()?
        .set_serial_number("0".to_string())
        .set_storage(Box::new(KeyringStorage::new(SERVICE.to_string())))
        .set_client_info_override(Some(client_info())))
}

fn session_entry() -> AppResult<Entry> {
    Entry::new(SERVICE, SESSION_KEY).map_err(|e| AppError::new("keyring", e.to_string()))
}

fn save_session(stored: &StoredSession) {
    if let (Ok(entry), Ok(json)) = (session_entry(), serde_json::to_string(stored)) {
        if let Err(e) = entry.set_password(&json) {
            tracing::warn!("failed to store session: {e}");
        }
    }
}

fn clear_session() {
    if let Ok(entry) = session_entry() {
        let _ = entry.delete_credential();
    }
}

async fn restore() -> Option<Session> {
    let json = session_entry().ok()?.get_password().ok()?;
    let stored: StoredSession = serde_json::from_str(&json).ok()?;
    let generator = AnisetteDataGenerator::new(Arc::new(RwLock::new(anisette().ok()?)));
    let gs = GrandSlam::new(client_info(), false, None).await.ok()?;
    let mut dev = DeveloperSession::new(
        AppToken {
            token: stored.token,
            duration: stored.duration,
            expiry: stored.expiry,
        },
        stored.adsid,
        Arc::new(gs),
        generator,
    );
    match dev.list_teams().await {
        Ok(teams) if !teams.is_empty() => Some(Session {
            email: stored.email,
            name: stored.name,
            dev,
        }),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("stored session rejected: {e}");
            clear_session();
            None
        }
    }
}

#[tauri::command]
pub async fn account_status(state: State<'_, AppState>) -> AppResult<Option<AccountInfo>> {
    let _guard = state.restore_lock.lock().await;
    if let Some(s) = state.session.lock().unwrap().as_ref() {
        return Ok(Some(AccountInfo {
            email: s.email.clone(),
            name: s.name.clone(),
        }));
    }
    let restored = restore().await;
    let info = restored.as_ref().map(|s| AccountInfo {
        email: s.email.clone(),
        name: s.name.clone(),
    });
    *state.session.lock().unwrap() = restored;
    Ok(info)
}

#[tauri::command]
pub async fn login(
    app: AppHandle,
    state: State<'_, AppState>,
    email: String,
    password: String,
) -> AppResult<AccountInfo> {
    let email = email.trim().to_lowercase();
    let handle = app.clone();
    let tfa = move |params: TwoFactorCallbackParams| {
        let handle = handle.clone();
        async move {
            let (tx, rx) = oneshot::channel::<TfaAnswer>();
            *handle.state::<AppState>().tfa.lock().unwrap() = Some(tx);
            handle
                .emit("tfa-required", &params)
                .map_err(|e| report!("emit failed: {e}"))?;
            let answer = rx.await.map_err(|_| report!("2fa cancelled"))?;
            Ok(match answer {
                TfaAnswer::Code { code } => TwoFactorCallbackResponse::SubmitCode(code),
                TfaAnswer::Sms { id } => TwoFactorCallbackResponse::SendSms(id),
                TfaAnswer::Devices => TwoFactorCallbackResponse::SendToDevices,
                TfaAnswer::Resend => TwoFactorCallbackResponse::ResendCode,
                TfaAnswer::Cancel => TwoFactorCallbackResponse::Abort,
            })
        }
    };

    let provider = anisette().map_err(|e| AppError::from_report("anisette", e))?;
    let mut account = AppleAccount::builder(&email)
        .anisette_provider(provider)
        .login(&password, tfa)
        .await
        .map_err(|e| AppError::from_report("auth", e))?;
    drop(password);

    let token = account
        .get_app_token("xcode.auth")
        .await
        .map_err(|e| AppError::from_report("auth", e))?;
    let adsid = account
        .spd
        .as_ref()
        .and_then(|d| d.get("adsid"))
        .and_then(|v| v.as_string())
        .map(str::to_string)
        .ok_or_else(|| AppError::new("auth", "no adsid"))?;
    let name = account
        .get_name()
        .ok()
        .map(|(first, last)| format!("{first} {last}").trim().to_string())
        .filter(|s| !s.is_empty());

    save_session(&StoredSession {
        email: email.clone(),
        name: name.clone(),
        adsid: adsid.clone(),
        token: token.token.clone(),
        duration: token.duration,
        expiry: token.expiry,
    });

    let dev = DeveloperSession::new(
        token,
        adsid,
        account.grandslam_client.clone(),
        account.anisette_generator.clone(),
    );
    *state.session.lock().unwrap() = Some(Session {
        email: email.clone(),
        name: name.clone(),
        dev,
    });
    Ok(AccountInfo { email, name })
}

#[tauri::command]
pub fn answer_tfa(state: State<'_, AppState>, answer: TfaAnswer) {
    if let Some(tx) = state.tfa.lock().unwrap().take() {
        let _ = tx.send(answer);
    }
}

#[tauri::command]
pub fn logout(state: State<'_, AppState>) {
    *state.session.lock().unwrap() = None;
    clear_session();
}
