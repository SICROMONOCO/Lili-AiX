//! OpenSubtitles for the desktop: the account (Settings → Subtitles) and the player's subtitle
//! search. The client is `anilili_core::opensubtitles`.
//!
//! OpenSubtitles hands out a token that lasts a day and no refresh token, so renewing means signing
//! in again with the password, as Android does. The username is kept with the settings; the
//! password goes to the system's credential store (Windows Credential Manager, the macOS keychain),
//! never into the app's database; the token lives in memory only.

use std::sync::{Mutex, OnceLock};

use anilili_core::catalogue as c;
use anilili_core::net::Http;
use anilili_core::opensubtitles::{OpenSubtitles, Refusal, SubtitleFile, SubtitleLink, refusal};
use serde::Serialize;
use tauri::State;

use crate::app::{AppState, CmdResult, CommandError, as_entry, fail};

/// The consumer key, from `build.rs`: empty in a build without one.
const KEY: &str = env!("ANILILI_OPEN_SUBTITLES_KEY");
const USER_SETTING: &str = "opensubtitles_user";
const KEYRING_SERVICE: &str = "app.anilili.desktop.opensubtitles";

static TOKEN: Mutex<Option<String>> = Mutex::new(None);

fn client() -> CmdResult<&'static OpenSubtitles> {
    static CLIENT: OnceLock<Option<OpenSubtitles>> = OnceLock::new();
    let client = CLIENT.get_or_init(|| {
        let user_agent = format!("Anilili Desktop v{}", env!("CARGO_PKG_VERSION"));
        Http::plain().ok().map(|http| OpenSubtitles::new(http, KEY, user_agent))
    });
    match client {
        Some(client) if client.configured() => Ok(client),
        _ => Err(fail(c::SRC_SUBTITLE_UNAVAILABLE, "no OpenSubtitles key in this build")),
    }
}

fn keyring(username: &str) -> CmdResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, username).map_err(|e| fail(c::DB_FAILED, format!("credential store: {e}")))
}

/// Keeps a login: the username with the settings, the password in the credential store.
pub fn remember(state: &AppState, username: &str, password: &str) -> CmdResult<()> {
    keyring(username)?.set_password(password).map_err(|e| fail(c::DB_FAILED, format!("credential store: {e}")))?;
    state.store.set_setting(USER_SETTING, &username.to_owned())?;
    Ok(())
}

/// The saved login, if there is one (a backup can carry it to Android).
pub fn saved_login(state: &AppState) -> Option<(String, String)> {
    let username: String = state.store.setting(USER_SETTING)?;
    let password = keyring(&username).ok()?.get_password().ok()?;
    Some((username, password))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// This build has a consumer key.
    pub configured: bool,
    pub username: Option<String>,
}

#[tauri::command]
pub fn opensubtitles_account(state: State<'_, AppState>) -> Account {
    Account { configured: !KEY.is_empty(), username: state.store.setting(USER_SETTING) }
}

#[tauri::command]
pub async fn opensubtitles_login(state: State<'_, AppState>, username: String, password: String) -> CmdResult<String> {
    let username = username.trim().to_owned();
    if username.is_empty() || password.is_empty() {
        return Err(fail(c::ACC_OPENSUBTITLES_LOGIN, "username or password empty"));
    }
    let token = client()?.login(&username, &password).await.map_err(login_failure)?;
    remember(&state, &username, &password)?;
    *TOKEN.lock().unwrap() = Some(token);
    tracing::info!("signed in to OpenSubtitles");
    Ok(username)
}

/// OpenSubtitles' sign-up page, in the browser: an account is free.
#[tauri::command]
pub fn opensubtitles_signup() -> CmdResult<()> {
    crate::library::open_in_browser("https://www.opensubtitles.com/en/users/sign_up")
}

#[tauri::command]
pub fn opensubtitles_logout(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(username) = state.store.setting::<String>(USER_SETTING) {
        // Already gone from the store is as good as removed.
        if let Ok(entry) = keyring(&username) {
            let _ = entry.delete_credential();
        }
    }
    state.store.delete_setting(USER_SETTING)?;
    *TOKEN.lock().unwrap() = None;
    Ok(())
}

/// Files for a title's episode, in the given languages (ISO 639-1; three at most are asked for).
/// Needs no account.
#[tauri::command]
pub async fn opensubtitles_search(query: String, languages: Vec<String>, season: Option<u32>, episode: Option<f64>) -> CmdResult<Vec<SubtitleFile>> {
    // Half episodes (12.5) aren't numbered on OpenSubtitles: search the title alone for those.
    let episode = episode.filter(|e| e.fract() == 0.0 && *e > 0.0).map(|e| e as u32);
    client()?.search(&query, &languages, season, episode).await.map_err(as_entry(c::SRC_SUBTITLE_SEARCH))
}

/// A download link for one file, for the player to add as a track. Signs in again with the saved
/// login when the day's token has lapsed, once.
#[tauri::command]
pub async fn opensubtitles_download(state: State<'_, AppState>, file_id: i64) -> CmdResult<SubtitleLink> {
    let client = client()?;
    let token = session(&state, false).await?;
    let result = match client.download(&token, file_id).await {
        Err(e) if refusal(&e) == Refusal::SignedOut => {
            let token = session(&state, true).await?;
            client.download(&token, file_id).await
        }
        other => other,
    };
    result.map_err(|e| match refusal(&e) {
        Refusal::Allowance => fail(c::SRC_SUBTITLE_ALLOWANCE, e.to_string()),
        Refusal::SignedOut => fail(c::ACC_OPENSUBTITLES_LOGIN, e.to_string()),
        Refusal::Other => as_entry(c::SRC_SUBTITLE_SEARCH)(e),
    })
}

/// The day's token, signing in with the saved login when there is none (or `renew`).
async fn session(state: &AppState, renew: bool) -> CmdResult<String> {
    if !renew && let Some(token) = TOKEN.lock().unwrap().clone() {
        return Ok(token);
    }
    let (username, password) = saved_login(state).ok_or_else(|| fail(c::ACC_SIGNED_OUT, "no OpenSubtitles login saved"))?;
    let token = client()?.login(&username, &password).await.map_err(login_failure)?;
    *TOKEN.lock().unwrap() = Some(token.clone());
    Ok(token)
}

/// A refused sign-in is the account's ("wrong password"); anything else is the network's.
fn login_failure(e: anilili_core::Error) -> CommandError {
    match refusal(&e) {
        Refusal::SignedOut => fail(c::ACC_OPENSUBTITLES_LOGIN, e.to_string()),
        _ => match &e {
            anilili_core::Error::Http { status: 400..=403, .. } => fail(c::ACC_OPENSUBTITLES_LOGIN, e.to_string()),
            _ => as_entry(c::ACC_OPENSUBTITLES_LOGIN)(e),
        },
    }
}
