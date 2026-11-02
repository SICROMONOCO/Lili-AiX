//! Library, history, settings and the AniList account.

use anilili_core::anilist::{ListEntry, Viewer};
use anilili_core::mal::{self, MalUser};
use std::collections::HashMap;

use anilili_core::model::{AnimeCard, ContinueEntry, LibraryEntry, Progress};
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindowBuilder};

use anilili_core::catalogue as c;
use anilili_core::store::now;

use crate::app::{AppState, CmdResult, MAL_TOKENS_KEY, Settings, TOKEN_KEY, as_entry, fail, report};
use crate::player::Player;
use crate::with_browser_args;

/// Local library statuses ↔ AniList `MediaListStatus`.
pub fn to_anilist_status(status: &str) -> Option<&'static str> {
    Some(match status {
        "watching" => "CURRENT",
        "planning" => "PLANNING",
        "completed" => "COMPLETED",
        "paused" => "PAUSED",
        "dropped" => "DROPPED",
        _ => return None,
    })
}

#[tauri::command]
pub async fn library_list<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<Vec<LibraryItem>> {
    let mut remote: HashMap<i64, ListEntry> = HashMap::new();
    if signed_in(&state).await {
        // Offline, the list is this computer's as it stands; the next read catches up.
        if let Ok(entries) = account_list(state.clone()).await {
            reconcile(&app, &state, &entries).await;
            remote = entries.into_iter().map(|e| (e.anime.id, e)).collect();
        }
    }
    Ok(state
        .store
        .library()?
        .into_iter()
        .map(|entry| {
            let account = remote.get(&entry.anime.id);
            LibraryItem {
                progress: account.map(|e| e.progress),
                score: account.map(|e| e.score).filter(|s| *s > 0.0),
                account_status: account.map(|e| e.status.clone()),
                entry,
            }
        })
        .collect())
}

/// When the last sync with the account started. One asked for within [SYNC_INTERVAL] of it is
/// dropped, as Android's `LibraryStore.refreshRemoteLibrary` drops one within 30 s.
static LAST_SYNC: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
/// Set while a sync runs, so the window's focus and Home asking together start one, not two.
static SYNCING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

/// A backstop for a window left in front untouched: the triggers in [sync_with_account] cover the
/// rest. The first pass, shortly after launch, also catches a window that opened without focus.
pub fn start_sync<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        loop {
            sync_with_account(&app, false);
            tokio::time::sleep(std::time::Duration::from_secs(10 * 60)).await;
        }
    });
}

/// Brings the library and the account into step the way Android does: whenever the window comes
/// to the front and whenever Home opens, at most every 30 s, and at once (`force`) on sign-in. A
/// change made on the phone or TV used to wait for the ten-minute pass, and sign-in itself waited
/// for it too, so a fresh sign-in showed none of the account's list for up to ten minutes.
pub fn sync_with_account<R: Runtime>(app: &AppHandle<R>, force: bool) {
    // The window can take focus before the state is managed (setup builds the window first).
    if app.try_state::<AppState>().is_none() {
        return;
    }
    {
        let mut last = LAST_SYNC.lock().unwrap();
        if !force && last.is_some_and(|at| at.elapsed() < SYNC_INTERVAL) {
            return;
        }
        if SYNCING.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        *last = Some(std::time::Instant::now());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if !run_sync(&app, &state).await {
            // Failed (offline, the service down): the next trigger tries again, not 30 s later.
            *LAST_SYNC.lock().unwrap() = None;
        }
        SYNCING.store(false, std::sync::atomic::Ordering::Release);
    });
}

/// One pull and reconcile. False when the account couldn't be read.
async fn run_sync<R: Runtime>(app: &AppHandle<R>, state: &State<'_, AppState>) -> bool {
    if !signed_in(state).await {
        return true;
    }
    // Read the account itself: the list is cached for five minutes, and a sync answering from that
    // copy would miss the very change on another device that it ran to pick up.
    forget_list(state);
    match account_list(state.clone()).await {
        Ok(entries) => {
            reconcile(app, state, &entries).await;
            tracing::info!(entries = entries.len(), "synced with the account");
            let _ = app.emit("library-changed", ());
            true
        }
        Err(e) => {
            tracing::warn!(code = %e.code, "syncing with the account failed");
            false
        }
    }
}

/// Home asks for a sync as it opens, as Android's Home does.
#[tauri::command]
pub fn library_sync<R: Runtime>(app: AppHandle<R>) {
    sync_with_account(&app, false);
}

/// A library title, with what the account knows about it beyond the status.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryItem {
    #[serde(flatten)]
    pub entry: LibraryEntry,
    /// Episodes watched, as the account counts them.
    pub progress: Option<i32>,
    /// 0..10.
    pub score: Option<f64>,
    /// The account's own status (`REPEATING` is filed under `watching` here).
    pub account_status: Option<String>,
}

async fn signed_in(state: &AppState) -> bool {
    state.anilist.has_token() || state.mal_access_token().await.is_some()
}

/// The account's status as the library keeps it.
pub fn to_local_status(remote: &str) -> Option<&'static str> {
    Some(match remote {
        "CURRENT" | "REPEATING" => "watching",
        "PLANNING" => "planning",
        "COMPLETED" => "completed",
        "PAUSED" => "paused",
        "DROPPED" => "dropped",
        _ => return None,
    })
}

/// The library and the account made one list again, as Android keeps them: the account wins for
/// the titles it has, except a change made here that it hasn't seen yet (offline, or a push that
/// failed) and is newer than its own, which is pushed instead. A title the account no longer has
/// is dropped here if it had been in step (removed elsewhere), and pushed if it never reached the
/// account. Removals made here that didn't reach it are retried, so a pull doesn't undo them.
async fn reconcile<R: Runtime>(app: &AppHandle<R>, state: &AppState, remote: &[ListEntry]) {
    let local: HashMap<i64, LibraryEntry> = state.store.library().unwrap_or_default().into_iter().map(|e| (e.anime.id, e)).collect();
    let removed: std::collections::HashSet<i64> = state.store.library_tombstones().unwrap_or_default().into_iter().collect();
    let mut seen = std::collections::HashSet::new();
    let mut pushed = false;
    for entry in remote {
        let id = entry.anime.id;
        if !seen.insert(id) {
            continue;
        }
        let Some(status) = to_local_status(&entry.status) else { continue };
        if removed.contains(&id) {
            pushed |= push_logged(app, state, &entry.anime, None).await;
            continue;
        }
        match local.get(&id) {
            Some(mine) if !mine.synced && mine.updated_at >= entry.updated_at => {
                pushed |= push_logged(app, state, &mine.anime, Some(&mine.status)).await;
            }
            Some(mine) if mine.synced && mine.status == status => {}
            _ => {
                let _ = state.store.library_apply_remote(&entry.anime, status, entry.updated_at);
            }
        }
    }
    for (id, mine) in &local {
        if seen.contains(id) {
            continue;
        }
        if mine.synced {
            let _ = state.store.library_drop(*id);
        } else {
            pushed |= push_logged(app, state, &mine.anime, Some(&mine.status)).await;
        }
    }
    for id in removed.iter().filter(|id| !seen.contains(id)) {
        // Already off the account: nothing left to tell it.
        let _ = state.store.library_clear_tombstone(*id);
    }
    if pushed {
        forget_account_lists(state);
    }
}

/// Sends one title's status to the account (or its removal), then records it as in step. False,
/// with the failure reported once, when the account didn't take it: it stays pending for the next
/// pull to retry.
async fn push_logged<R: Runtime>(app: &AppHandle<R>, state: &AppState, anime: &AnimeCard, status: Option<&str>) -> bool {
    match push(state, anime.id, status).await {
        Ok(()) => {
            let _ = match status {
                Some(_) => state.store.library_mark_synced(anime.id),
                None => state.store.library_clear_tombstone(anime.id),
            };
            true
        }
        Err(e) => {
            tracing::warn!(id = anime.id, code = %e.code, "library change didn't reach the account");
            report(app, e);
            false
        }
    }
}

async fn push(state: &AppState, anime_id: i64, status: Option<&str>) -> CmdResult<()> {
    if state.anilist.has_token() {
        match status {
            Some(status) => {
                let remote = to_anilist_status(status).ok_or_else(|| fail(c::ACC_SYNC, format!("unknown status {status}")))?;
                state.anilist.save_status(anime_id, remote).await.map_err(as_entry(c::ACC_SYNC))
            }
            None => state.anilist.delete_entry(anime_id).await.map(|_| ()).map_err(as_entry(c::ACC_SYNC)),
        }
    } else if let Some(token) = state.mal_access_token().await {
        let mal_id = state
            .anilist
            .mal_id(anime_id)
            .await
            .map_err(as_entry(c::ACC_SYNC))?
            .ok_or_else(|| fail(c::ACC_SYNC, format!("AniList {anime_id} has no MyAnimeList entry")))?;
        match status {
            Some(status) => {
                let remote = mal::from_local_status(status).ok_or_else(|| fail(c::ACC_SYNC, format!("unknown status {status}")))?;
                state.mal.update(&token, mal_id, Some(remote), None).await.map_err(as_entry(c::ACC_SYNC))
            }
            None => state.mal.delete(&token, mal_id).await.map_err(as_entry(c::ACC_SYNC)),
        }
    } else {
        Ok(())
    }
}

/// Drops only the cached list (not the profile), so the next read asks the service.
fn forget_list(state: &AppState) {
    let _ = state.store.cache_delete("mal-list");
    if let Some(viewer) = state.viewer.read().unwrap().clone() {
        let _ = state.store.cache_delete(&format!("anilist-list:{}", viewer.id));
    }
}

/// The cached account lists predate a change just pushed: a pull from them would undo it.
fn forget_account_lists(state: &AppState) {
    let viewer = state.viewer.read().unwrap().clone();
    let _ = state.store.cache_delete("mal-list");
    let _ = state.store.cache_delete("account:anilist");
    let _ = state.store.cache_delete("account:mal");
    if let Some(viewer) = viewer {
        let _ = state.store.cache_delete(&format!("anilist-list:{}", viewer.id));
    }
}

#[tauri::command(async)]
pub fn library_status(state: State<'_, AppState>, id: i64) -> Option<String> {
    state.store.library_status(id)
}

#[tauri::command]
pub async fn library_set<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    anime: AnimeCard,
    status: Option<String>,
) -> CmdResult<()> {
    state.store.library_set(&anime, status.as_deref())?;
    // Simkl hears about it whatever list service there is, or none.
    crate::simkl::track_library(&app, anime.id, status.as_deref());
    if !signed_in(&state).await {
        return Ok(());
    }
    // The account mirrors the library: a removal is remembered until it gets there, so a pull
    // before then doesn't put the title back.
    if status.is_none() {
        state.store.library_tombstone(anime.id)?;
    }
    if push_logged(&app, &state, &anime, status.as_deref()).await {
        forget_account_lists(&state);
    }
    Ok(())
}

#[tauri::command(async)]
pub fn progress_for(state: State<'_, AppState>, id: i64) -> CmdResult<Vec<Progress>> {
    Ok(state.store.progress_for(id)?)
}

#[tauri::command]
pub async fn continue_watching(state: State<'_, AppState>) -> CmdResult<Vec<ContinueEntry>> {
    let mut rows = state.store.continue_watching(30)?;
    // Episodes finished on another device move a row on (Android's `historyAfterRemoteProgress`).
    // Only the cached account list is read, so the row never waits on the network; the background
    // sync refreshes it and says so with `library-changed`.
    let watching: HashMap<i64, ListEntry> = cached_account_list(&state)
        .await
        .into_iter()
        .filter(|e| e.status == "CURRENT" || e.status == "REPEATING")
        .map(|e| (e.anime.id, e))
        .collect();
    for row in &mut rows {
        let Some(entry) = watching.get(&row.anime.id) else { continue };
        let Some(target) = advanced_episode(&row.progress, entry) else { continue };
        // A clock this PC kept for that episode still holds.
        let kept = state.store.progress_for(row.anime.id)?.into_iter().find(|p| p.episode == target);
        row.progress = Progress {
            episode: target,
            position: kept.as_ref().map_or(0.0, |p| p.position),
            duration: kept.as_ref().map_or(0.0, |p| p.duration),
            // The row is ordered by when it was last watched, which was on the other device.
            updated_at: row.progress.updated_at.max(entry.updated_at),
            ..row.progress
        };
    }
    rows.sort_by_key(|row| std::cmp::Reverse(row.progress.updated_at));
    // A finished last episode of a finished show is history, not something to continue.
    rows.retain(|row| {
        let total = row.anime.episodes.map(f64::from);
        !(row.progress.finished() && total.is_some_and(|t| row.progress.episode >= t))
    });
    // Taken off the row by hand, until it's watched again.
    let hidden: HashMap<i64, i64> = state.store.setting(CONTINUE_HIDDEN_KEY).unwrap_or_default();
    rows.retain(|row| hidden.get(&row.anime.id).is_none_or(|at| row.progress.updated_at > *at));
    Ok(rows)
}

/// The episode a row should move to when the account has finished more than this PC knows, or
/// `None` when it stays. The account's count is of finished episodes, so the row belongs on the
/// one after it; past the planned run there is nothing to move to, and past what has aired the row
/// rests on the latest aired episode (Android's `remoteRowEpisode`). This PC wins while it is level
/// or ahead: its next push catches the account up.
fn advanced_episode(local: &Progress, account: &ListEntry) -> Option<f64> {
    let count = match account.anime.episodes {
        Some(total) if total > 0 => account.progress.min(total),
        _ => account.progress,
    };
    if count < 1 {
        return None;
    }
    let next = count + 1;
    if account.anime.episodes.is_some_and(|total| total > 0 && next > total) {
        return None;
    }
    let aired = account.anime.next_airing.as_ref().map(|n| n.episode - 1).filter(|&a| a > 0);
    let target = f64::from(match aired {
        Some(aired) if next > aired => count.min(aired),
        _ => next,
    });
    (target > local.episode).then_some(target)
}

/// The account's list as last fetched, without going to the network; empty when signed out or
/// never fetched.
async fn cached_account_list(state: &AppState) -> Vec<ListEntry> {
    if state.anilist.has_token() {
        let viewer = state.viewer.read().unwrap().as_ref().map(|v| v.id);
        return viewer.and_then(|id| state.store.cache_get_stale(&format!("anilist-list:{id}"))).unwrap_or_default();
    }
    if state.mal_access_token().await.is_some() {
        return state.store.cache_get_stale("mal-list").unwrap_or_default();
    }
    Vec::new()
}

const CONTINUE_HIDDEN_KEY: &str = "continue_hidden";

/// Takes a title off Continue watching (Android's `removeHistory`): its progress, and the
/// account's list, stay as they are, and watching it again brings it back.
#[tauri::command]
pub fn continue_hide(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let mut hidden: HashMap<i64, i64> = state.store.setting(CONTINUE_HIDDEN_KEY).unwrap_or_default();
    hidden.insert(id, now());
    Ok(state.store.set_setting(CONTINUE_HIDDEN_KEY, &hidden)?)
}

#[tauri::command(async)]
pub fn clear_history(state: State<'_, AppState>, id: Option<i64>) -> CmdResult<()> {
    Ok(state.store.clear_history(id)?)
}

#[tauri::command(async)]
pub fn clear_cache(state: State<'_, AppState>) -> CmdResult<()> {
    state.sources.lock().unwrap().clear();
    state.titles.lock().unwrap().clear();
    Ok(state.store.cache_clear()?)
}

/// The viewer's own background picture (Settings → Appearance), kept as the data URL the page
/// read it as, in the app's data folder. 20 MB at most: a wallpaper, not a film.
#[tauri::command(async)]
pub fn backdrop_save<R: Runtime>(app: AppHandle<R>, data_url: String) -> CmdResult<()> {
    if !data_url.starts_with("data:image/") || data_url.len() > 20 * 1024 * 1024 {
        return Err("not an image, or larger than 20 MB".into());
    }
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join("backdrop.txt"), data_url)).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command(async)]
pub fn backdrop_load<R: Runtime>(app: AppHandle<R>) -> Option<String> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::read_to_string(dir.join("backdrop.txt")).ok()
}

#[tauri::command]
pub fn settings_get(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
pub fn settings_set<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, settings: Settings) -> CmdResult<()> {
    // Saved with or without a player (one that failed to start mustn't take the settings with it).
    if let Some(player) = app.try_state::<Player>() {
        let applied = settings.clone();
        player.off_ui(move |player| player.apply_settings(&applied));
    }
    if !settings.discord_presence {
        state.discord.clear();
    }
    let alerts_turned_on = settings.release_notifications && !state.settings().release_notifications;
    if crate::glass::wanted(&settings.appearance) != crate::glass::wanted(&state.settings().appearance) {
        crate::glass::apply(&app, &settings.appearance);
    }
    state.save_settings(settings)?;
    if alerts_turned_on {
        crate::notifications::poke(&app);
    }
    Ok(())
}

// ---- AniList account -------------------------------------------------------------------------

const AUTHORIZE: &str = "https://anilist.co/api/v2/oauth/authorize?client_id=45552&response_type=token";
const LOGIN_WINDOW: &str = "anilist-login";

#[tauri::command]
pub async fn auth_status(state: State<'_, AppState>) -> CmdResult<Option<Viewer>> {
    if !state.anilist.has_token() {
        return Ok(None);
    }
    if let Some(viewer) = state.viewer.read().unwrap().clone() {
        return Ok(Some(viewer));
    }
    let viewer = state.anilist.viewer().await?;
    *state.viewer.write().unwrap() = Some(viewer.clone());
    Ok(Some(viewer))
}

/// Opens AniList's login page in its own window and catches the `http://localhost#access_token=…`
/// redirect in the navigation handler, so the token never reaches a real localhost server.
#[tauri::command]
pub async fn auth_login<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    if let Some(window) = app.get_webview_window(LOGIN_WINDOW) {
        let _ = window.set_focus();
        return Ok(());
    }
    let handle = app.clone();
    let url = AUTHORIZE.parse().map_err(|e| fail(c::ACC_ANILIST_LOGIN, format!("{e}")))?;
    with_browser_args(WebviewWindowBuilder::new(&app, LOGIN_WINDOW, WebviewUrl::External(url)))
        .title("Sign in to AniList")
        .inner_size(520.0, 720.0)
        .center()
        .on_navigation(move |url| {
            if url.host_str() != Some("localhost") {
                return true;
            }
            let fragment = url.fragment().unwrap_or_default();
            let token = url::form_urlencoded::parse(fragment.as_bytes()).find(|(k, _)| k == "access_token").map(|(_, v)| v.into_owned());
            let handle = handle.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(token) = token {
                    adopt_anilist_token(&handle, token).await;
                }
                if let Some(window) = handle.get_webview_window(LOGIN_WINDOW) {
                    let _ = window.close();
                }
            });
            false
        })
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Signs in with an AniList token, from the login window or a restored backup.
pub async fn adopt_anilist_token<R: Runtime>(app: &AppHandle<R>, token: String) {
    let state = app.state::<AppState>();
    state.anilist.set_token(Some(token.clone()));
    if let Err(e) = state.store.set_setting(TOKEN_KEY, &token) {
        tracing::error!(error = %e, "saving AniList token failed");
    }
    match state.anilist.viewer().await {
        Ok(viewer) => {
            tracing::info!(user = %viewer.name, "signed in to AniList");
            *state.viewer.write().unwrap() = Some(viewer.clone());
            let _ = app.emit("auth-changed", Some(viewer));
            crate::notifications::poke(app);
            sync_with_account(app, true);
        }
        Err(e) => report(app, as_entry(c::ACC_ANILIST_LOGIN)(e)),
    }
}

#[tauri::command]
pub fn auth_logout<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<()> {
    state.anilist.set_token(None);
    *state.viewer.write().unwrap() = None;
    state.store.delete_setting(TOKEN_KEY)?;
    let _ = app.emit("auth-changed", None::<Viewer>);
    crate::notifications::poke(&app);
    Ok(())
}

// ---- MyAnimeList account ----------------------------------------------------------------------

const MAL_LOGIN_WINDOW: &str = "mal-login";

#[tauri::command]
pub async fn mal_status(state: State<'_, AppState>) -> CmdResult<Option<MalUser>> {
    if let Some(user) = state.mal_user.read().unwrap().clone() {
        return Ok(Some(user));
    }
    let Some(token) = state.mal_access_token().await else { return Ok(None) };
    let user = state.mal.user(&token).await?;
    *state.mal_user.write().unwrap() = Some(user.clone());
    Ok(Some(user))
}

/// MAL login in its own window; the registered `https://localhost/?code=…` redirect is caught in
/// the navigation handler and traded for tokens (PKCE `plain`).
#[tauri::command]
pub async fn mal_login<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(window) = app.get_webview_window(MAL_LOGIN_WINDOW) {
        let _ = window.set_focus();
        return Ok(());
    }
    let (login_state, verifier) = (mal::random_token(24), mal::random_token(32));
    *state.mal_pending.lock().unwrap() = Some((login_state.clone(), verifier));
    let url = mal::authorize_url(&login_state, state.mal_pending.lock().unwrap().as_ref().map(|p| p.1.as_str()).unwrap_or_default());
    let handle = app.clone();
    with_browser_args(WebviewWindowBuilder::new(
        &app,
        MAL_LOGIN_WINDOW,
        WebviewUrl::External(url.parse().map_err(|e| fail(c::ACC_MAL_LOGIN, format!("{e}")))?),
    ))
    .title("Sign in to MyAnimeList")
    .inner_size(520.0, 760.0)
    .center()
    .on_navigation(move |url| {
        if url.host_str() != Some("localhost") {
            return true;
        }
        let query: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
        let handle = handle.clone();
        tauri::async_runtime::spawn(async move {
            finish_mal_login(&handle, query.get("code").cloned(), query.get("state").cloned()).await;
            if let Some(window) = handle.get_webview_window(MAL_LOGIN_WINDOW) {
                let _ = window.close();
            }
        });
        false
    })
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn finish_mal_login<R: Runtime>(app: &AppHandle<R>, code: Option<String>, returned_state: Option<String>) {
    let state = app.state::<AppState>();
    let pending = state.mal_pending.lock().unwrap().take();
    let (Some(code), Some((expected, verifier))) = (code, pending) else { return };
    if returned_state.as_deref() != Some(expected.as_str()) {
        report(app, fail(c::ACC_MAL_LOGIN, "the login state did not match; redirect ignored"));
        return;
    }
    match state.mal.exchange_code(&code, &verifier).await {
        Ok(tokens) => adopt_mal_tokens(app, tokens).await,
        Err(e) => report(app, as_entry(c::ACC_MAL_LOGIN)(e)),
    }
}

/// Signs in with a fresh MyAnimeList token pair, from the login window or a backup's hand-off.
pub async fn adopt_mal_tokens<R: Runtime>(app: &AppHandle<R>, tokens: mal::MalTokens) {
    let state = app.state::<AppState>();
    let _ = state.store.set_setting(MAL_TOKENS_KEY, &tokens);
    let user = state.mal.user(&tokens.access_token).await.ok();
    *state.mal_tokens.lock().await = Some(tokens);
    *state.mal_user.write().unwrap() = user.clone();
    tracing::info!(user = ?user.as_ref().map(|u| &u.name), "signed in to MyAnimeList");
    let _ = app.emit("mal-changed", user);
    sync_with_account(app, true);
}

#[tauri::command]
pub async fn mal_logout<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<()> {
    *state.mal_tokens.lock().await = None;
    *state.mal_user.write().unwrap() = None;
    state.store.delete_setting(MAL_TOKENS_KEY)?;
    let _ = app.emit("mal-changed", None::<MalUser>);
    Ok(())
}

/// The signed-in account's list: AniList when signed in there, else MAL (mapped to AniList titles).
#[tauri::command]
pub async fn account_list(state: State<'_, AppState>) -> CmdResult<Vec<ListEntry>> {
    if state.anilist.has_token() {
        return anilist_list(state).await;
    }
    let token = state.mal_access_token().await.ok_or(c::ACC_SIGNED_OUT)?;
    let key = "mal-list";
    if let Some(hit) = state.store.cache_get(key, 5 * 60) {
        return Ok(hit);
    }
    let entries = state.mal.list(&token).await?;
    let ids: Vec<i64> = entries.iter().map(|e| e.mal_id).collect();
    let cards: std::collections::HashMap<i64, _> = state.anilist.cards_by_mal_ids(&ids).await?.into_iter().collect();
    let list: Vec<ListEntry> = entries
        .into_iter()
        .filter_map(|e| {
            let anime = cards.get(&e.mal_id)?.clone();
            let _ = state.store.remember(&anime);
            Some(ListEntry {
                anime,
                status: mal::to_anilist_status(&e.status, e.rewatching).to_owned(),
                progress: e.progress,
                score: e.score,
                updated_at: e.updated_at,
                studios: Vec::new(),
            })
        })
        .collect();
    let _ = state.store.cache_put(key, &list);
    Ok(list)
}

/// The signed-in account's profile (AniList first, as the list is), for the Profile page.
#[tauri::command]
pub async fn account_profile(state: State<'_, AppState>) -> CmdResult<Option<AccountProfile>> {
    let (service, key) = if state.anilist.has_token() {
        ("anilist", "account:anilist")
    } else if state.mal_access_token().await.is_some() {
        ("mal", "account:mal")
    } else {
        return Ok(None);
    };
    if let Some(hit) = state.store.cache_get::<AccountProfile>(key, 5 * 60) {
        return Ok(Some(hit));
    }
    let account = if service == "anilist" {
        state.anilist.account().await?
    } else {
        let token = state.mal_access_token().await.ok_or(c::ACC_SIGNED_OUT)?;
        state.mal.account(&token).await?
    };
    let profile = AccountProfile { service: service.to_owned(), account };
    let _ = state.store.cache_put(key, &profile);
    Ok(Some(profile))
}

/// Drops the cached account and list, so the next read asks the service again ("Sync now").
#[tauri::command]
pub fn account_refresh(state: State<'_, AppState>) -> CmdResult<()> {
    let viewer = state.viewer.read().unwrap().clone();
    let mut keys = vec!["account:anilist".to_owned(), "account:mal".to_owned(), "mal-list".to_owned()];
    if let Some(viewer) = viewer {
        keys.push(format!("anilist-list:{}", viewer.id));
    }
    for key in keys {
        state.store.cache_delete(&key)?;
    }
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountProfile {
    /// `anilist` or `mal`.
    pub service: String,
    #[serde(flatten)]
    pub account: anilili_core::anilist::Account,
}

/// Opens an account page in the default browser. Only the two services' own pages: this takes a
/// URL from the frontend, so it must not become a way to launch anything else.
#[tauri::command]
pub fn open_account_page(url: String) -> CmdResult<()> {
    let parsed = url::Url::parse(&url).map_err(|e| e.to_string())?;
    let allowed = parsed.scheme() == "https" && matches!(parsed.host_str(), Some("anilist.co" | "myanimelist.net" | "simkl.com"));
    if !allowed {
        return Err(format!("not an account page: {url}").into());
    }
    open_in_browser(parsed.as_str())
}

/// The project's Discord server (About, the context menu and the Discord card's button), by its vanity invite.
#[tauri::command]
pub fn open_discord() -> CmdResult<()> {
    open_in_browser("https://discord.gg/Anilili")
}

pub(crate) fn open_in_browser(url: &str) -> CmdResult<()> {
    #[cfg(windows)]
    std::process::Command::new("explorer").arg(url).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn anilist_list(state: State<'_, AppState>) -> CmdResult<Vec<ListEntry>> {
    let viewer = auth_status(state.clone()).await?.ok_or(c::ACC_SIGNED_OUT)?;
    let key = format!("anilist-list:{}", viewer.id);
    if let Some(hit) = state.store.cache_get(&key, 5 * 60) {
        return Ok(hit);
    }
    let list = state.anilist.list(viewer.id).await?;
    for entry in &list {
        let _ = state.store.remember(&entry.anime);
    }
    let _ = state.store.cache_put(&key, &list);
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anilili_core::model::NextAiring;

    fn entry(progress: i32, episodes: Option<i32>, next_airing: Option<i32>) -> ListEntry {
        ListEntry {
            anime: AnimeCard {
                id: 1,
                title: "Anime".into(),
                title_romaji: None,
                cover: None,
                color: None,
                banner: None,
                format: None,
                status: None,
                episodes,
                score: None,
                year: None,
                season: None,
                genres: vec![],
                next_airing: next_airing.map(|episode| NextAiring { episode, airing_at: 0 }),
                is_adult: false,
            },
            status: "CURRENT".into(),
            progress,
            score: 0.0,
            updated_at: 100,
            studios: vec![],
        }
    }

    fn at(episode: f64) -> Progress {
        Progress { anime_id: 1, episode, position: 600.0, duration: 1400.0, updated_at: 50 }
    }

    #[test]
    fn a_row_moves_on_to_the_episode_after_the_accounts_count() {
        // Episodes 3 to 8 watched on the phone: the row on 3 goes to 9.
        assert_eq!(advanced_episode(&at(3.0), &entry(8, Some(12), None)), Some(9.0));
        // Finished 3 here and the account caught up: the row reads episode 4.
        assert_eq!(advanced_episode(&at(3.0), &entry(3, Some(12), None)), Some(4.0));
        // This PC is level or ahead: it stays.
        assert_eq!(advanced_episode(&at(5.0), &entry(4, Some(12), None)), None);
        assert_eq!(advanced_episode(&at(5.0), &entry(0, Some(12), None)), None);
    }

    #[test]
    fn a_row_never_moves_past_the_run_or_what_has_aired() {
        // Finished everywhere: nothing to move to.
        assert_eq!(advanced_episode(&at(3.0), &entry(12, Some(12), None)), None);
        // A count past the run is the services disagreeing about the season.
        assert_eq!(advanced_episode(&at(3.0), &entry(30, Some(12), None)), None);
        // Caught up on an airing series (7 is next): the row rests on 6, the latest aired.
        assert_eq!(advanced_episode(&at(3.0), &entry(6, Some(12), Some(7))), Some(6.0));
        assert_eq!(advanced_episode(&at(6.0), &entry(6, Some(12), Some(7))), None);
        // No length known yet: the count is all there is.
        assert_eq!(advanced_episode(&at(3.0), &entry(20, None, None)), Some(21.0));
    }
}
