//! The Simkl tracker: sign-in (device flow, approved in the browser), sign-out, and the pushes.
//!
//! Simkl runs beside the list service. A finished episode, a library change or a status reaches
//! it whether AniList, MyAnimeList or nobody is signed in, and nothing about it touches the
//! library or which account is active. The rules are Android's (`SimklTracker`): see
//! `anilili_core::simkl`.

use anilili_core::catalogue as c;
use anilili_core::simkl::{self, DeviceCode, DevicePoll, SavedAction, SimklTokens};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::app::{AppState, CmdResult, as_entry, fail, report};

pub const SIMKL_TOKENS_KEY: &str = "simkl_tokens";

/// Who is connected, for Settings.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimklAccount {
    pub name: Option<String>,
}

/// The code Settings shows while a sign-in waits for approval.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimklCode {
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    /// Unix seconds.
    pub expires_at: i64,
}

#[tauri::command]
pub async fn simkl_status(state: State<'_, AppState>) -> CmdResult<Option<SimklAccount>> {
    Ok(state.simkl_tokens.lock().await.as_ref().map(|t| SimklAccount { name: t.username.clone() }))
}

/// Starts a sign-in: gets a code, opens simkl.com/pin with it filled in, and polls in the
/// background. `simkl-changed` says when it is approved, a `failure` when it isn't.
#[tauri::command]
pub async fn simkl_login<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<SimklCode> {
    if let Some(previous) = state.simkl_login.lock().unwrap().take() {
        previous.abort();
    }
    let code = state.simkl.device_code().await.map_err(as_entry(c::ACC_SIMKL_LOGIN))?;
    if let Err(e) = crate::library::open_in_browser(&code.verification_uri_complete) {
        tracing::warn!(code = %e.code, "couldn't open the browser for Simkl; the code is on screen");
    }
    let shown = SimklCode {
        user_code: code.user_code.clone(),
        verification_uri: code.verification_uri.clone(),
        verification_uri_complete: code.verification_uri_complete.clone(),
        expires_at: code.expires_at,
    };
    let handle = app.clone();
    let task = tauri::async_runtime::spawn(async move { await_approval(&handle, code).await });
    *state.simkl_login.lock().unwrap() = Some(task);
    Ok(shown)
}

#[tauri::command]
pub fn simkl_login_cancel(state: State<'_, AppState>) {
    if let Some(task) = state.simkl_login.lock().unwrap().take() {
        task.abort();
    }
}

async fn await_approval<R: Runtime>(app: &AppHandle<R>, code: DeviceCode) {
    let state = app.state::<AppState>();
    let mut interval = code.interval;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        match state.simkl.poll_device(&code).await {
            Ok(DevicePoll::Pending) => {}
            Ok(DevicePoll::SlowDown) => interval += 5,
            Ok(DevicePoll::Approved(tokens)) => {
                adopt(app, tokens).await;
                return;
            }
            Ok(DevicePoll::Expired) => {
                report(app, fail(c::ACC_SIMKL_LOGIN, "the code expired before it was approved"));
                let _ = app.emit("simkl-login-ended", ());
                return;
            }
            // One dropped poll on a flaky connection is not a failed sign-in.
            Err(anilili_core::Error::Network(detail)) => tracing::warn!(%detail, "Simkl poll failed; polling on"),
            Err(e) => {
                report(app, as_entry(c::ACC_SIMKL_LOGIN)(e));
                let _ = app.emit("simkl-login-ended", ());
                return;
            }
        }
    }
}

async fn adopt<R: Runtime>(app: &AppHandle<R>, mut tokens: SimklTokens) {
    let state = app.state::<AppState>();
    tokens.username = state.simkl.account_name(&tokens.access_token).await.unwrap_or_else(|e| {
        tracing::warn!(error = %e, "Simkl account name lookup failed");
        None
    });
    let _ = state.store.set_setting(SIMKL_TOKENS_KEY, &tokens);
    let account = SimklAccount { name: tokens.username.clone() };
    *state.simkl_tokens.lock().await = Some(tokens);
    tracing::info!(user = ?account.name, "connected Simkl");
    let _ = app.emit("simkl-changed", Some(account));
}

/// Disconnects here and revokes the grant with Simkl, so it isn't merely this computer forgetting.
#[tauri::command]
pub async fn simkl_logout<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<()> {
    let tokens = state.simkl_tokens.lock().await.take();
    state.store.delete_setting(SIMKL_TOKENS_KEY)?;
    let _ = app.emit("simkl-changed", None::<SimklAccount>);
    if let Some(tokens) = tokens {
        state.simkl.revoke(&tokens).await;
    }
    Ok(())
}

/// A valid access token, refreshed when near expiry (or when `rejected` was just refused). A
/// refusal from Simkl means the grant is gone, and signs out; a network failure keeps it.
pub async fn access_token<R: Runtime>(app: &AppHandle<R>, rejected: Option<&str>) -> Option<String> {
    let state = app.state::<AppState>();
    let mut guard = state.simkl_tokens.lock().await;
    let tokens = guard.clone()?;
    let stale = rejected.is_some_and(|r| r == tokens.access_token);
    if !stale && !tokens.needs_refresh() {
        return Some(tokens.access_token);
    }
    match state.simkl.refresh(&tokens).await {
        Ok(fresh) => {
            let _ = state.store.set_setting(SIMKL_TOKENS_KEY, &fresh);
            let token = fresh.access_token.clone();
            *guard = Some(fresh);
            Some(token)
        }
        Err(anilili_core::Error::Http { status, .. }) if (400..500).contains(&status) && status != 429 => {
            tracing::warn!(status, "Simkl refused the refresh; the grant is gone, disconnecting");
            *guard = None;
            let _ = state.store.delete_setting(SIMKL_TOKENS_KEY);
            let _ = app.emit("simkl-changed", None::<SimklAccount>);
            None
        }
        Err(e) => {
            tracing::warn!(error = %e, "Simkl token refresh failed; keeping the session");
            (!stale).then_some(tokens.access_token)
        }
    }
}

/// What one push asks Simkl to do.
enum Push {
    /// Episodes 1 through n watched.
    Progress(i32),
    /// A library change: the local status, or none for a removal.
    Library(Option<String>),
}

/// Episodes 1 through `episode` watched.
pub fn track_progress<R: Runtime>(app: &AppHandle<R>, anime_id: i64, episode: i32) {
    if episode > 0 {
        spawn_push(app, anime_id, Push::Progress(episode));
    }
}

/// A library change. A save (`planning`) only adds a title Simkl doesn't track, and a removal
/// only takes off one that is still just a plan: Simkl's watch history is never lost from here.
pub fn track_library<R: Runtime>(app: &AppHandle<R>, anime_id: i64, status: Option<&str>) {
    spawn_push(app, anime_id, Push::Library(status.map(str::to_owned)));
}

fn spawn_push<R: Runtime>(app: &AppHandle<R>, anime_id: i64, push: Push) {
    // Catalogue-native titles carry negative, app-owned ids that name nothing on Simkl.
    if anime_id <= 0 {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if state.simkl_tokens.lock().await.is_none() {
            return;
        }
        match deliver(&app, anime_id, &push).await {
            Ok(outcome) => tracing::info!(anime_id, outcome, "Simkl updated"),
            Err(e) => {
                tracing::warn!(anime_id, error = %e, "Simkl update failed");
                report(&app, as_entry(c::ACC_SIMKL_SYNC)(e));
            }
        }
    });
}

async fn deliver<R: Runtime>(app: &AppHandle<R>, anime_id: i64, push: &Push) -> anilili_core::Result<&'static str> {
    let state = app.state::<AppState>();
    let mal_id = state.anilist.mal_id(anime_id).await.unwrap_or_else(|e| {
        tracing::warn!(anime_id, error = %e, "no MAL id for Simkl; sending the AniList id alone");
        None
    });
    let ids = simkl::ids(anime_id, mal_id);
    let matched = match push {
        Push::Progress(episode) => with_token(app, |token| state.simkl.add_to_history(token, simkl::history(ids.clone(), *episode))).await?,
        Push::Library(Some(status)) if status == "completed" => {
            with_token(app, |token| state.simkl.add_to_history(token, simkl::completed(ids.clone()))).await?
        }
        Push::Library(status) => {
            if let Some(to) = status.as_deref().and_then(simkl::list_status) {
                with_token(app, |token| state.simkl.add_to_list(token, simkl::add_to_list(ids.clone(), to))).await?
            } else {
                let saved = status.is_some();
                let watch = with_token(app, |token| state.simkl.watch_state(token, simkl::watched_query(ids.clone()))).await?;
                if !watch.found {
                    false
                } else {
                    match simkl::saved_action(saved, watch.list.as_deref(), watch.watched_before) {
                        SavedAction::AddPlanned => {
                            with_token(app, |token| state.simkl.add_to_list(token, simkl::add_to_list(ids.clone(), "plantowatch"))).await?
                        }
                        SavedAction::Remove => with_token(app, |token| state.simkl.remove_from_history(token, simkl::remove(ids.clone()))).await?,
                        SavedAction::Nothing => return Ok("kept"),
                    }
                }
            }
        }
    };
    Ok(if matched { "sent" } else { "not found on Simkl" })
}

/// Runs one call with a fresh token; on a 401, refreshes once and tries again.
async fn with_token<R, T, F, Fut>(app: &AppHandle<R>, call: F) -> anilili_core::Result<T>
where
    R: Runtime,
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = anilili_core::Result<T>>,
{
    let signed_out = || anilili_core::Error::Other("Simkl is not connected".into());
    let token = access_token(app, None).await.ok_or_else(signed_out)?;
    match call(token.clone()).await {
        Err(anilili_core::Error::Http { status: 401, .. }) => {
            let renewed = access_token(app, Some(&token)).await.ok_or_else(signed_out)?;
            call(renewed).await
        }
        other => other,
    }
}
