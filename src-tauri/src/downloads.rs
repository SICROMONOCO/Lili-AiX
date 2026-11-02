//! The download queue: two episodes at a time (`PARALLEL_JOBS`), persisted, resumable across
//! restarts.
//!
//! Jobs live in SQLite, so a queue survives closing the app; a job interrupted mid-download goes
//! back to `queued` at the next start (segments are cheap to refetch, so there is no partial
//! resume). The worker re-resolves sources when a job starts, because stream URLs are signed and
//! expire long before a queue drains.
//!
//! Each job carries the download dialog's choices (Android's `SeasonDownloadDialog`): the server
//! to try first, a height ceiling, and whether an episode may move to another server when its own
//! has nothing at or under it. The queue can be paused; it pauses itself when the disk is about
//! to run out (Android's `DownloadStorage` estimates and headroom). When it stops (empty or
//! paused), the batch it just ran becomes one notification (Android's `DownloadBatchNotices`), not
//! one per episode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anilili_core::catalogue as c;
use anilili_core::download::{DownloadRequest, download, hls_heights, safe_name};
use anilili_core::model::{AnimeCard, Audio, DownloadOptions, DownloadRecord, Server, Stream};
use anilili_core::net::Http;
use anilili_core::proxy::{Transform, transform_for};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::Notify;

use crate::app::{AppState, CmdResult, fail};
use crate::catalog::resolve_sources;
use crate::notifications::{self, Post};
use crate::notify;
use crate::player::{LoadRequest, NowPlaying};
use crate::watch::{download_stream, downloadable};

const PAUSED_KEY: &str = "downloads.paused";
/// Free space to leave behind after an episode (Android's `HEADROOM_BYTES`).
const HEADROOM_BYTES: u64 = 1000 * 1024 * 1024;
/// How many servers a quality check or a quality match looks at before giving up.
const QUALITY_PROBES: usize = 5;
/// Episodes downloaded at once. An HLS one already fetches four segments at a time, so two make
/// eight requests to what is often one CDN; more invite 429s.
const PARALLEL_JOBS: usize = 2;

#[derive(Default)]
pub struct Downloads {
    wake: Notify,
    /// Running jobs' cancel flags.
    cancels: Mutex<HashMap<i64, Arc<AtomicBool>>>,
    paused: AtomicBool,
    /// The running jobs' estimated sizes together (`estimated_episode_bytes`), for the disk check.
    reserved: AtomicU64,
    /// Each running job's progress, 0 to 1, for the taskbar.
    progress: Mutex<HashMap<i64, f64>>,
    /// What the queue has finished since it last went idle.
    batch: Mutex<Vec<Finished>>,
}

struct Finished {
    anime_id: i64,
    title: String,
    cover: Option<String>,
    episode: f64,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    id: i64,
    progress: f64,
    bytes: u64,
    /// Bytes a second, averaged over the download so far.
    speed: f64,
    /// Seconds left, once there is enough to judge by.
    eta: Option<f64>,
}

/// Rough size of one episode at a ceiling, deliberately on the high side (Android's
/// `DownloadStorage.estimatedEpisodeBytes`, measured): a warning that comes early beats a
/// download that runs the disk dry. "Best" is unknown until a playlist is read, so middling.
pub fn estimated_episode_bytes(max_height: Option<u32>) -> u64 {
    const MB: u64 = 1024 * 1024;
    match max_height {
        Some(h) if h <= 360 => 90 * MB,
        Some(h) if h <= 480 => 150 * MB,
        Some(h) if h <= 720 => 300 * MB,
        Some(h) if h <= 1080 => 550 * MB,
        _ => 400 * MB,
    }
}

pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    if let Err(e) = state.store.downloads_reset_interrupted() {
        tracing::warn!(error = %e, "resetting interrupted downloads failed");
    }
    let downloads = Downloads::default();
    downloads.paused.store(state.store.setting(PAUSED_KEY).unwrap_or(false), Ordering::Relaxed);
    app.manage(downloads);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut running = tokio::task::JoinSet::new();
        loop {
            let downloads = app.state::<Downloads>();
            while running.len() < PARALLEL_JOBS && !downloads.paused.load(Ordering::Relaxed) {
                let state = app.state::<AppState>();
                let Some(job) = state.store.download_next_queued().ok().flatten() else { break };
                // Claimed before it runs, or the next pick is this job again; and cancellable from
                // the moment it shows as running.
                let _ = state.store.download_update(job.id, "downloading", 0.0, 0, None, None);
                let cancel = Arc::new(AtomicBool::new(false));
                downloads.cancels.lock().unwrap().insert(job.id, Arc::clone(&cancel));
                let app = app.clone();
                running.spawn(async move {
                    let id = job.id;
                    run(&app, job, cancel).await;
                    app.state::<Downloads>().cancels.lock().unwrap().remove(&id);
                });
            }
            if running.is_empty() {
                // Empty or paused. What finished before a pause is announced now, not whenever the
                // queue resumes.
                announce_batch(&app).await;
                downloads.wake.notified().await;
                continue;
            }
            // A slot frees, or something new was queued (or the queue resumed).
            tokio::select! {
                finished = running.join_next() => {
                    if let Some(Err(e)) = finished {
                        tracing::error!(error = %e, "a download task panicked");
                    }
                }
                () = downloads.wake.notified() => {}
            }
        }
    });
}

fn emit_changed<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit("downloads-changed", ());
}

async fn run<R: Runtime>(app: &AppHandle<R>, job: DownloadRecord, cancel: Arc<AtomicBool>) {
    let state = app.state::<AppState>();
    let downloads = app.state::<Downloads>();
    let card = state.store.card(job.anime_id);
    let title = card.as_ref().map(|c| c.title.clone()).unwrap_or_else(|| job.title.clone());
    let folder = download_dir(app, &state).join(safe_name(&title));

    // Before anything is fetched: a disk about to fill pauses the queue, rather than failing every
    // episode after this one in turn. The space must hold every episode running, not just this one.
    let estimate = estimated_episode_bytes(job.max_height);
    let reserved = downloads.reserved.fetch_add(estimate, Ordering::Relaxed) + estimate;
    if let Some(free) = free_bytes(&folder)
        && free < reserved + HEADROOM_BYTES
    {
        downloads.reserved.fetch_sub(estimate, Ordering::Relaxed);
        tracing::warn!(id = job.id, free, reserved, "download paused: disk nearly full");
        let failure = fail(c::DL_NO_SPACE, format!("{free} bytes free"));
        let shown = format!("{} ({})", failure.message, failure.code);
        let _ = state.store.download_update(job.id, "failed", 0.0, 0, None, Some(&shown));
        set_paused(app, true);
        emit_changed(app);
        notifications::post(
            app,
            Post {
                kind: "download",
                tag: "space",
                group: notify::DOWNLOADS,
                title: "Downloads paused",
                body: "The disk is nearly full. Free some space, then resume the queue.",
                route: Some("/downloads"),
                cover: card.as_ref().and_then(|c| c.cover.as_deref()),
                anime_id: Some(job.anime_id),
                toast: state.settings().download_notifications,
            },
        )
        .await;
        return;
    }

    downloads.progress.lock().unwrap().insert(job.id, 0.0);
    taskbar_progress(app);
    emit_changed(app);

    let options = DownloadOptions { provider: job.provider.clone(), max_height: job.max_height, match_quality: job.match_quality, hard_subs: job.hard_subs };
    let result = async {
        let sources = resolve_sources(&state, job.anime_id, job.episode, true).await?;
        let (server, stream) = choose(&state, &sources.servers, job.audio, &options)
            .await
            .ok_or_else(|| fail(c::SRC_NONE_DOWNLOADABLE, format!("anime {} episode {}", job.anime_id, job.episode)))?;
        let episode = if job.episode.fract() == 0.0 { format!("{:02}", job.episode as i64) } else { format!("{:04.1}", job.episode) };
        let dub = if job.audio == Audio::Dub { " (Dub)" } else { "" };
        let output = folder.join(safe_name(&format!("{title} - E{episode}{dub}")));
        let request = DownloadRequest {
            url: stream.url.clone(),
            format: stream.format.clone(),
            headers: server.headers.clone(),
            transform: transform_for(&server.provider, &stream.url),
            output,
            subtitles: server.subtitles.clone(),
            max_height: job.max_height,
        };
        tracing::info!(id = job.id, server = %server.id, format = %stream.format, height = ?stream.height, ceiling = ?job.max_height, "download starting");
        let started = Instant::now();
        let last_event = Mutex::new(Instant::now());
        let saved = download(&state.http_client(), &request, Arc::clone(&cancel), |p| {
            let mut last = last_event.lock().unwrap();
            if last.elapsed().as_millis() < 400 && p.done < p.total {
                return;
            }
            *last = Instant::now();
            let progress = if p.total > 0 { p.done as f64 / p.total as f64 } else { 0.0 };
            let elapsed = started.elapsed().as_secs_f64();
            let speed = if elapsed > 0.0 { p.bytes as f64 / elapsed } else { 0.0 };
            let eta = (progress > 0.02 && elapsed > 3.0).then(|| elapsed * (1.0 - progress) / progress);
            let _ = state.store.download_update(job.id, "downloading", progress, p.bytes, None, None);
            let _ = app.emit("download-progress", ProgressEvent { id: job.id, progress, bytes: p.bytes, speed, eta });
            downloads.progress.lock().unwrap().insert(job.id, progress);
            taskbar_progress(app);
        })
        .await?;
        let label = if server.label.is_empty() { server.provider.clone() } else { server.label.clone() };
        Ok::<_, crate::app::CommandError>((saved, stream.height, label))
    }
    .await;

    downloads.reserved.fetch_sub(estimate, Ordering::Relaxed);
    downloads.progress.lock().unwrap().remove(&job.id);
    taskbar_progress(app);
    let finished = |error: Option<String>| Finished {
        anime_id: job.anime_id,
        title: title.clone(),
        cover: card.as_ref().and_then(|c| c.cover.clone()),
        episode: job.episode,
        error,
    };
    match result {
        Ok((saved, stream_height, label)) => {
            let bytes = std::fs::metadata(&saved.path).map(|m| m.len()).unwrap_or_default();
            tracing::info!(id = job.id, path = %saved.path.display(), height = ?saved.height, "download finished");
            let _ = state.store.download_update(job.id, "done", 1.0, bytes, Some(&saved.path.to_string_lossy()), None);
            let _ = state.store.download_delivered(job.id, saved.height.or(stream_height), &label);
            app.state::<Downloads>().batch.lock().unwrap().push(finished(None));
        }
        Err(_) if cancel.load(Ordering::Relaxed) => {
            let _ = state.store.download_update(job.id, "cancelled", 0.0, 0, None, None);
        }
        Err(e) => {
            // Already logged under its code; the row shows the message with the code.
            let shown = format!("{} ({})", e.message, e.code);
            let _ = state.store.download_update(job.id, "failed", 0.0, 0, None, Some(&shown));
            app.state::<Downloads>().batch.lock().unwrap().push(finished(Some(e.message)));
        }
    }
    emit_changed(app);
}

/// The server and stream for a job. With a ceiling and `match_quality`, the first server (in
/// preference order) offering something at or under the ceiling; otherwise, or when none does,
/// the preferred server.
async fn choose(state: &AppState, servers: &[Server], audio: Audio, options: &DownloadOptions) -> Option<(Server, Stream)> {
    let settings = state.settings();
    let candidates = downloadable(servers, audio, &settings, options.provider.as_deref(), options.hard_subs);
    if let (Some(max), true) = (options.max_height, options.match_quality) {
        for server in candidates.iter().take(QUALITY_PROBES) {
            let offered = offered_heights(state, server).await;
            // A server that doesn't say what it has can't be ruled out.
            if offered.is_empty() || offered.iter().any(|h| *h <= max) {
                return download_stream(server, options.max_height).map(|s| ((*server).clone(), s.clone()));
            }
        }
    }
    let server = candidates.first()?;
    download_stream(server, options.max_height).map(|s| ((*server).clone(), s.clone()))
}

/// The heights a server offers, tallest first: its streams' own, else its HLS master's
/// variants. Empty when nothing says.
async fn offered_heights(state: &AppState, server: &Server) -> Vec<u32> {
    let mut heights: Vec<u32> = server.streams.iter().filter(|s| crate::watch::is_downloadable(s)).filter_map(|s| s.height).collect();
    if heights.is_empty()
        && let Some(stream) = server.streams.iter().find(|s| s.format == "hls")
    {
        let plain;
        let http = if transform_for(&server.provider, &stream.url) == Transform::MediaIdKey {
            match Http::plain() {
                Ok(client) => {
                    plain = client;
                    &plain
                }
                Err(_) => return heights,
            }
        } else {
            &state.http
        };
        let probe = hls_heights(http, &stream.url, &server.headers);
        heights = tokio::time::timeout(std::time::Duration::from_secs(12), probe).await.ok().and_then(Result::ok).unwrap_or_default();
    }
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    heights
}

/// What a ceiling gets from a list of offered heights: the tallest under it, else the shortest.
fn delivered(offered: &[u32], max_height: Option<u32>) -> Option<u32> {
    match max_height {
        Some(max) => offered.iter().copied().filter(|h| *h <= max).max().or_else(|| offered.iter().copied().min()),
        None => offered.iter().copied().max(),
    }
}

/// The batch the queue just ran, as one notification.
async fn announce_batch<R: Runtime>(app: &AppHandle<R>) {
    let finished: Vec<Finished> = std::mem::take(&mut *app.state::<Downloads>().batch.lock().unwrap());
    let Some(summary) = summarise(&finished) else { return };
    let first = &finished[0];
    let toast = app.state::<AppState>().settings().download_notifications;
    let single = finished.iter().all(|f| f.anime_id == first.anime_id);
    notifications::post(
        app,
        Post {
            kind: "download",
            tag: "batch",
            group: notify::DOWNLOADS,
            title: &summary.0,
            body: &summary.1,
            route: Some("/downloads"),
            cover: first.cover.as_deref(),
            anime_id: single.then_some(first.anime_id),
            toast,
        },
    )
    .await;
}

fn episode_list(numbers: &[f64]) -> String {
    let shown: Vec<String> = numbers.iter().take(6).map(|n| crate::app::episode_number(*n)).collect();
    let more = numbers.len().saturating_sub(6);
    if more > 0 { format!("{} and {more} more", shown.join(", ")) } else { shown.join(", ") }
}

/// (title, body) for a finished batch; `None` when nothing finished.
fn summarise(finished: &[Finished]) -> Option<(String, String)> {
    let first = finished.first()?;
    let ok: Vec<&Finished> = finished.iter().filter(|f| f.error.is_none()).collect();
    let failed: Vec<&Finished> = finished.iter().filter(|f| f.error.is_some()).collect();
    let single = finished.iter().all(|f| f.anime_id == first.anime_id);
    let plural = |n: usize| if n == 1 { "1 episode".to_owned() } else { format!("{n} episodes") };
    if ok.is_empty() {
        let title = if single { first.title.clone() } else { "Downloads failed".to_owned() };
        let body = match failed.as_slice() {
            [one] => format!("Episode {} failed: {}", crate::app::episode_number(one.episode), one.error.clone().unwrap_or_default()),
            _ => format!("{} failed", plural(failed.len())),
        };
        return Some((title, body));
    }
    let failures = if failed.is_empty() { String::new() } else { format!(" · {} failed", failed.len()) };
    if single {
        let numbers: Vec<f64> = ok.iter().map(|f| f.episode).collect();
        let what = if numbers.len() == 1 { format!("Episode {}", episode_list(&numbers)) } else { format!("Episodes {}", episode_list(&numbers)) };
        return Some((first.title.clone(), format!("{what} downloaded{failures}")));
    }
    let mut per_title: Vec<(String, usize)> = Vec::new();
    for f in &ok {
        match per_title.iter_mut().find(|(t, _)| *t == f.title) {
            Some((_, n)) => *n += 1,
            None => per_title.push((f.title.clone(), 1)),
        }
    }
    let body = per_title.iter().map(|(t, n)| format!("{t} ({n})")).collect::<Vec<_>>().join(" · ");
    Some((format!("Downloaded {}", plural(ok.len())), format!("{body}{failures}")))
}

/// Mirrors the running downloads on the taskbar button: their average progress.
fn taskbar_progress<R: Runtime>(app: &AppHandle<R>) {
    use tauri::window::{ProgressBarState, ProgressBarStatus};
    let downloads = app.state::<Downloads>();
    let progress = {
        let running = downloads.progress.lock().unwrap();
        (!running.is_empty()).then(|| running.values().sum::<f64>() / running.len() as f64)
    };
    if let Some(window) = app.get_webview_window("main") {
        let state = match progress {
            Some(p) => ProgressBarState { status: Some(ProgressBarStatus::Normal), progress: Some((p * 100.0) as u64) },
            None => ProgressBarState { status: Some(ProgressBarStatus::None), progress: None },
        };
        let _ = window.set_progress_bar(state);
    }
}

fn download_dir<R: Runtime>(app: &AppHandle<R>, state: &AppState) -> PathBuf {
    if let Some(dir) = state.settings().download_dir.filter(|d| !d.trim().is_empty()) {
        return PathBuf::from(dir);
    }
    app.path().video_dir().or_else(|_| app.path().download_dir()).map(|d| d.join("Anilili")).unwrap_or_else(|_| PathBuf::from("Anilili"))
}

/// Free bytes on the volume `path` is (or will be) on; `None` when it can't be read.
#[cfg(windows)]
fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let existing = path.ancestors().find(|p| p.exists())?;
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: a NUL-terminated path and a valid out pointer; the other outputs are optional.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

#[cfg(unix)]
#[allow(clippy::unnecessary_cast)] // the field types differ between macOS and Linux
fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let path = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    // SAFETY: a NUL-terminated path and a zeroed struct for statvfs to fill.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let ok = unsafe { libc::statvfs(path.as_ptr(), &mut stat) } == 0;
    ok.then(|| stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(not(any(windows, unix)))]
fn free_bytes(_path: &Path) -> Option<u64> {
    None
}

fn set_paused<R: Runtime>(app: &AppHandle<R>, paused: bool) {
    let downloads = app.state::<Downloads>();
    downloads.paused.store(paused, Ordering::Relaxed);
    let _ = app.state::<AppState>().store.set_setting(PAUSED_KEY, &paused);
    if !paused {
        downloads.wake.notify_one();
    }
    let _ = app.emit("downloads-changed", ());
}

// ---- commands -------------------------------------------------------------------------------

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn download_enqueue<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    downloads: State<'_, Downloads>,
    anime: AnimeCard,
    episodes: Vec<f64>,
    audio: Option<Audio>,
    options: Option<DownloadOptions>,
) -> CmdResult<usize> {
    state.store.remember(&anime)?;
    let audio = audio.unwrap_or(state.settings().audio);
    let options = options.unwrap_or_else(|| {
        let settings = state.settings();
        DownloadOptions { max_height: quality_height(&settings.download_quality), hard_subs: settings.download_hard_subs, ..Default::default() }
    });
    let mut added = 0;
    for episode in episodes {
        if state.store.download_insert(anime.id, episode, audio, &anime.title, &options)?.is_some() {
            added += 1;
        }
    }
    downloads.wake.notify_one();
    emit_changed(&app);
    Ok(added)
}

/// Settings' download quality as a ceiling (`best` has none).
pub fn quality_height(quality: &str) -> Option<u32> {
    quality.parse().ok()
}

#[tauri::command(async)]
pub fn downloads_list(state: State<'_, AppState>) -> CmdResult<Vec<DownloadRecord>> {
    Ok(state.store.downloads()?)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueState {
    pub paused: bool,
    /// Queued or running.
    pub pending: i64,
    pub folder: String,
    /// On the folder's volume; `None` when unreadable.
    pub free_bytes: Option<u64>,
    pub headroom_bytes: u64,
}

#[tauri::command(async)]
pub fn downloads_state<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, downloads: State<'_, Downloads>) -> CmdResult<QueueState> {
    let folder = download_dir(&app, &state);
    Ok(QueueState {
        paused: downloads.paused.load(Ordering::Relaxed),
        pending: state.store.downloads_pending()?,
        free_bytes: free_bytes(&folder),
        folder: folder.to_string_lossy().into_owned(),
        headroom_bytes: HEADROOM_BYTES,
    })
}

#[tauri::command]
pub fn downloads_pause<R: Runtime>(app: AppHandle<R>, paused: bool) {
    set_paused(&app, paused);
}

/// Stops the running download and empties the queue.
#[tauri::command(async)]
pub fn downloads_cancel_all<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, downloads: State<'_, Downloads>) -> CmdResult<()> {
    state.store.downloads_cancel_queued()?;
    for flag in downloads.cancels.lock().unwrap().values() {
        flag.store(true, Ordering::Relaxed);
    }
    emit_changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn downloads_retry_failed<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, downloads: State<'_, Downloads>) -> CmdResult<usize> {
    let n = state.store.downloads_retry_failed()?;
    downloads.wake.notify_one();
    emit_changed(&app);
    Ok(n)
}

#[tauri::command(async)]
pub fn downloads_clear_unfinished<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<usize> {
    let n = state.store.downloads_clear_unfinished()?;
    emit_changed(&app);
    Ok(n)
}

/// Opens the download folder (created first, so there is something to open).
#[tauri::command]
pub fn downloads_open_folder<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<()> {
    let folder = download_dir(&app, &state);
    std::fs::create_dir_all(&folder).map_err(|e| fail(c::DL_FOLDER, e.to_string()))?;
    open_path(&folder)
}

fn open_path(path: &Path) -> CmdResult<()> {
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(target_os = "linux")]
    let opener = "xdg-open";
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let opener = "open";
    std::process::Command::new(opener).arg(path).spawn().map_err(|e| fail(c::DL_FOLDER, e.to_string()))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckRequest {
    pub anime_id: i64,
    pub episode: f64,
    pub audio: Audio,
    pub provider: Option<String>,
    pub max_height: Option<u32>,
    #[serde(default)]
    pub hard_subs: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QualityCheck {
    /// The server that would be used, by the name the app shows, and its provider.
    pub server: String,
    pub provider: String,
    /// What it offers, tallest first; empty when it doesn't say.
    pub offered: Vec<u32>,
    pub delivered: Option<u32>,
    /// Whether it has something at or under the ceiling; `None` when it can't be told.
    pub fits: Option<bool>,
    /// Another server that has, when this one hasn't.
    pub alternative: Option<Alternative>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alternative {
    pub server: String,
    pub provider: String,
    pub height: u32,
}

/// What the chosen server really offers for one sample episode (Android's `checkBatchQuality`),
/// said before the download: most servers publish one rendition, and a ceiling can do nothing
/// with one, which afterwards reads as the setting being ignored.
#[tauri::command]
pub async fn download_check(state: State<'_, AppState>, request: CheckRequest) -> CmdResult<QualityCheck> {
    let sources = resolve_sources(&state, request.anime_id, request.episode, false).await?;
    let settings = state.settings();
    let candidates = downloadable(&sources.servers, request.audio, &settings, request.provider.as_deref(), request.hard_subs);
    let first = candidates
        .first()
        .ok_or_else(|| fail(c::SRC_NONE_DOWNLOADABLE, format!("anime {} episode {}", request.anime_id, request.episode)))?;
    let label = |s: &Server| if s.label.is_empty() { s.provider.clone() } else { s.label.clone() };
    let offered = offered_heights(&state, first).await;
    let fits = match (request.max_height, offered.is_empty()) {
        (_, true) => None,
        (None, false) => Some(true),
        (Some(max), false) => Some(offered.iter().any(|h| *h <= max)),
    };
    let mut alternative = None;
    if let (Some(max), Some(false)) = (request.max_height, fits) {
        for server in candidates.iter().skip(1).filter(|s| s.provider != first.provider).take(QUALITY_PROBES) {
            if let Some(height) = offered_heights(&state, server).await.into_iter().find(|h| *h <= max) {
                alternative = Some(Alternative { server: label(server), provider: server.provider.clone(), height });
                break;
            }
        }
    }
    Ok(QualityCheck {
        server: label(first),
        provider: first.provider.clone(),
        delivered: delivered(&offered, request.max_height),
        offered,
        fits,
        alternative,
    })
}

#[tauri::command(async)]
pub fn download_cancel<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    downloads: State<'_, Downloads>,
    id: i64,
) -> CmdResult<()> {
    if let Some(flag) = downloads.cancels.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    } else {
        state.store.download_update(id, "cancelled", 0.0, 0, None, None)?;
    }
    emit_changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn download_retry<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    downloads: State<'_, Downloads>,
    id: i64,
) -> CmdResult<()> {
    state.store.download_update(id, "queued", 0.0, 0, None, None)?;
    downloads.wake.notify_one();
    emit_changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn download_remove<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    downloads: State<'_, Downloads>,
    id: i64,
    delete_file: bool,
) -> CmdResult<()> {
    if let Some(flag) = downloads.cancels.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
    if delete_file
        && let Some(record) = state.store.download(id)?
        && let Some(path) = record.path
    {
        let path = PathBuf::from(path);
        let _ = std::fs::remove_file(&path);
        // Subtitles saved next to it share its stem.
        if let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) {
            for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(&format!("{stem}.")) && !name.ends_with(".part") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    state.store.download_delete(id)?;
    emit_changed(&app);
    Ok(())
}

/// Opens Explorer with the file selected.
#[tauri::command(async)]
pub fn download_reveal(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    let path = state.store.download(id)?.and_then(|r| r.path).ok_or_else(|| fail(c::DL_NO_FILE, format!("download {id}")))?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // `/select,` must be glued to the path as one argument for Explorer to parse it.
        std::process::Command::new("explorer").raw_arg(format!("/select,\"{path}\"")).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    {
        let dir = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        std::process::Command::new("open").arg(dir).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        let dir = std::path::Path::new(&path).parent().map(|p| p.to_path_buf()).unwrap_or_default();
        std::process::Command::new("xdg-open").arg(dir).spawn().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Plays a finished download, tracking progress like a stream.
#[tauri::command(async)]
pub async fn download_play<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, id: i64) -> CmdResult<DownloadRecord> {
    let player = crate::player::get(&app)?;
    let record = state.store.download(id)?.ok_or_else(|| fail(c::DL_MISSING, format!("download {id}")))?;
    let path = record.path.clone().ok_or_else(|| fail(c::DL_UNFINISHED, format!("download {id}")))?;
    if !std::path::Path::new(&path).exists() {
        return Err(fail(c::DL_FILE_GONE, path));
    }
    let card = state.store.card(record.anime_id);
    let start = state
        .store
        .progress_for(record.anime_id)?
        .into_iter()
        .find(|p| p.episode == record.episode && !p.finished())
        .map(|p| (p.position - 3.0).max(0.0));
    let title = card.as_ref().map(|c| c.title.clone()).unwrap_or_else(|| record.title.clone());
    let load = LoadRequest {
        url: path,
        headers: Default::default(),
        start,
        subtitles: vec![],
        title: Some(format!("{title} · Episode {}", record.episode)),
        subtitle_languages: state.settings().subtitle_languages,
        show_subtitles: Some(crate::watch::show_subtitles(&state.settings(), record.audio)),
    };
    let now_playing = NowPlaying {
        anime_id: record.anime_id,
        episode: record.episode,
        title,
        episode_title: None,
        cover: card.as_ref().and_then(|c| c.cover.clone()),
        total: card.as_ref().and_then(|c| c.episodes),
        synced: false,
        loaded: false,
    };
    player.load(&load, Some(now_playing)).await.map_err(|e| fail(c::PLAY_FILE_FAILED, e.to_string()))?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done(anime_id: i64, title: &str, episode: f64, error: Option<&str>) -> Finished {
        Finished { anime_id, title: title.into(), cover: None, episode, error: error.map(Into::into) }
    }

    #[test]
    fn a_batch_is_one_summary() {
        assert_eq!(summarise(&[]), None);
        let one_title = [done(1, "Frieren", 1.0, None), done(1, "Frieren", 2.0, None), done(1, "Frieren", 3.0, Some("x"))];
        assert_eq!(summarise(&one_title), Some(("Frieren".into(), "Episodes 1, 2 downloaded · 1 failed".into())));
        let two_titles = [done(1, "Frieren", 1.0, None), done(2, "Dandadan", 4.0, None), done(2, "Dandadan", 5.0, None)];
        assert_eq!(summarise(&two_titles), Some(("Downloaded 3 episodes".into(), "Frieren (1) · Dandadan (2)".into())));
        let failed = [done(1, "Frieren", 7.0, Some("No server"))];
        assert_eq!(summarise(&failed), Some(("Frieren".into(), "Episode 7 failed: No server".into())));
    }

    #[test]
    fn ceilings_deliver_the_tallest_under_else_the_shortest() {
        assert_eq!(delivered(&[1080, 720, 360], Some(720)), Some(720));
        assert_eq!(delivered(&[1080], Some(480)), Some(1080));
        assert_eq!(delivered(&[1080, 720], None), Some(1080));
        assert_eq!(delivered(&[], Some(720)), None);
        assert_eq!(quality_height("best"), None);
        assert_eq!(quality_height("720"), Some(720));
    }
}
