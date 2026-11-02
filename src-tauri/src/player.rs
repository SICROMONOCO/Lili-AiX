//! The video player: one mpv core painting the main window under the webview.
//!
//! mpv gets the window's own HWND as `wid`. WebView2 is a child window of that same window and
//! starts transparent, so the HTML controls float over the video. On macOS mpv draws into a layer
//! under the transparent WKWebView instead (`player_mac.rs`), to the same effect. The UI drives mpv through
//! [`player_control`] and receives every state change as a `player` event. Events emitted before
//! the UI subscribed are not lost: [`player_snapshot`] replays the current state as the same
//! events.
//!
//! While an episode plays, its progress is written to the store every few seconds and when it
//! ends, and pushed to AniList once it is finished.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anilili_core::model::Progress;
use anilili_core::store::now;
use anilili_mpv::{EndReason, Event, Format, Mpv, PropertyValue};
#[cfg(any(windows, target_os = "macos"))]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use crate::app::{AppState, CmdResult, Settings, fail};
use anilili_core::catalogue as c;

/// Android's buffer sizes and video cache sizes, by their stored values.
pub const BUFFER_SIZES: &[&str] = &["auto", "small", "medium", "large", "very_large"];
pub const VIDEO_CACHE_SIZES: &[&str] = &["auto", "off", "256", "512", "1024", "2048"];
/// Android's screen sizes, by their stored values.
pub const CONTENT_SCALES: &[&str] = &["fit", "crop", "fill"];

pub struct Player {
    mpv: Mpv,
    /// Where the bundled Anime4K shaders are, if they are.
    shader_dir: Option<PathBuf>,
    snapshot: Arc<Mutex<Snapshot>>,
    now_playing: Arc<Mutex<Option<NowPlaying>>>,
    /// Subtitles mpv refused while the file was loading (mpv before 0.38 adds them only to a
    /// loaded file), with the preferred languages; added on `FileLoaded`.
    pending_subtitles: Mutex<(Vec<SubtitleTrack>, Vec<String>)>,
    /// The stream's own headers (mpv carries them on every request, subtitle fetches included);
    /// kept so a re-picked subtitle track can be checked the same way.
    headers: Mutex<BTreeMap<String, String>>,
    /// Where fetched subtitle files go, so mpv plays them from disk (see `load`); emptied at launch.
    sub_dir: PathBuf,
    /// Numbers the subtitle files, so one load's never overwrite another's.
    sub_files: AtomicU64,
    /// Bumped by every `load`: one overtaken by a newer load while it fetched its subtitles
    /// leaves mpv alone.
    load_turn: AtomicU64,
    /// Held while a load hands mpv its file, so two loads' commands never interleave.
    handing_over: Mutex<()>,
    /// The layer mpv draws into (after `mpv`: dropped once the core has stopped).
    #[cfg(target_os = "macos")]
    _video: Arc<crate::player_mac::Video>,
    /// The GL surface mpv draws into on Linux (kept alive with the player).
    #[cfg(target_os = "linux")]
    _video_linux: Arc<crate::player_linux::Video>,
    /// Linux: mpv calls from plain commands, made on their own thread (see [`Player::off_ui`]).
    #[cfg(target_os = "linux")]
    client: Mutex<std::sync::mpsc::Sender<ClientJob>>,
}

#[cfg(target_os = "linux")]
type ClientJob = Box<dyn FnOnce(&Player) + Send>;

impl Player {
    /// A video is on screen and not paused: the usage ping's minutes watched (`crate::usage`).
    pub fn is_playing(&self) -> bool {
        let s = self.snapshot.lock().unwrap();
        let flag = |name: &str| matches!(s.properties.get(name), Some(PropertyValue::Flag(true)));
        s.active && s.loaded && !flag("pause") && !flag("eof-reached")
    }
}

/// What is playing, for progress tracking.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NowPlaying {
    pub anime_id: i64,
    pub episode: f64,
    pub title: String,
    /// The episode's own title, when the episode list had one (Discord's second line).
    pub episode_title: Option<String>,
    pub cover: Option<String>,
    pub total: Option<i32>,
    /// Set once the episode crossed 90%, so AniList is told once.
    #[serde(skip)]
    pub synced: bool,
    /// Set once mpv has loaded this file. Until then the position mpv reports is the outgoing
    /// file's: its end-of-file event arrives after [`Player::load`] has already named the next
    /// episode, and saving it then marked that episode finished (and told AniList so).
    #[serde(skip)]
    pub loaded: bool,
}

/// The latest state, kept so a freshly mounted UI can catch up.
#[derive(Default)]
struct Snapshot {
    active: bool,
    loaded: bool,
    error: Option<String>,
    properties: BTreeMap<String, PropertyValue>,
}

impl Snapshot {
    fn record(&mut self, event: &Event) {
        match event {
            Event::StartFile => {
                self.active = true;
                self.loaded = false;
                self.error = None;
                // The previous file's clock means nothing for this one.
                self.properties.remove("time-pos");
                self.properties.remove("duration");
            }
            Event::FileLoaded => self.loaded = true,
            Event::EndFile { reason: EndReason::Stop, .. } => {
                self.active = false;
                self.loaded = false;
                self.error = None;
            }
            Event::EndFile { reason: EndReason::Error, error, .. } => {
                self.loaded = false;
                self.error = Some(error.clone().unwrap_or_default());
            }
            Event::PropertyChange { name, value } => {
                self.properties.insert(name.clone(), value.clone());
            }
            _ => {}
        }
    }

    fn replay(&self) -> Vec<Event> {
        let mut events = Vec::with_capacity(self.properties.len() + 3);
        if self.active {
            events.push(Event::StartFile);
        }
        if self.loaded {
            events.push(Event::FileLoaded);
        }
        events.extend(self.properties.iter().map(|(name, value)| Event::PropertyChange { name: name.clone(), value: value.clone() }));
        if let Some(error) = &self.error {
            events.push(Event::EndFile { reason: EndReason::Error, error: Some(error.clone()), code: None });
        }
        events
    }

    fn number(&self, name: &str) -> f64 {
        match self.properties.get(name) {
            Some(PropertyValue::Double(v)) => *v,
            Some(PropertyValue::Int64(v)) => *v as f64,
            _ => 0.0,
        }
    }
}

/// Properties mirrored to the UI. Node-valued ones arrive as JSON strings.
const OBSERVED: &[(&str, Format)] = &[
    ("time-pos", Format::Double),
    ("duration", Format::Double),
    ("pause", Format::Flag),
    ("paused-for-cache", Format::Flag),
    ("cache-buffering-state", Format::Int64),
    ("demuxer-cache-time", Format::Double),
    ("eof-reached", Format::Flag),
    ("volume", Format::Double),
    ("mute", Format::Flag),
    ("speed", Format::Double),
    ("sub-delay", Format::Double),
    ("media-title", Format::String),
    ("track-list", Format::String),
    ("hwdec-current", Format::String),
    ("video-params/w", Format::Int64),
    ("video-params/h", Format::Int64),
];

/// `time-pos` changes every frame; the UI only needs a few updates a second.
const TIME_POS_INTERVAL: Duration = Duration::from_millis(200);
/// A move this far is a seek, not playback, and reaches the UI at once.
const TIME_POS_JUMP: f64 = 1.0;
const SAVE_INTERVAL: Duration = Duration::from_secs(5);

/// Paces `time-pos` for the UI without ever leaving it behind. A throttle that only drops left the
/// UI's clock where playback had been, not where it stopped: a seek lands within 200 ms of the last
/// frame's update, and at the end of a file (Skip Outro, `+85s`) no frame follows it. The Up-next
/// card read that stale clock as "not near the end" and never came.
#[derive(Default)]
struct Clock {
    /// When the UI last heard a position, and which.
    sent: Option<(Instant, f64)>,
    /// The newest position held back since.
    held: Option<f64>,
}

impl Clock {
    /// Whether this position goes to the UI now; one that doesn't is held for [`Clock::flush`].
    fn offer(&mut self, position: f64, now: Instant) -> bool {
        let due = self.sent.is_none_or(|(at, last)| now.duration_since(at) >= TIME_POS_INTERVAL || (position - last).abs() >= TIME_POS_JUMP);
        if due {
            self.sent = Some((now, position));
            self.held = None;
        } else {
            self.held = Some(position);
        }
        due
    }

    /// The position held back, once: sent ahead of an event that says the clock stopped or jumped.
    fn flush(&mut self, now: Instant) -> Option<f64> {
        let position = self.held.take()?;
        self.sent = Some((now, position));
        Some(position)
    }

    /// A new file: what is held is the old file's.
    fn reset(&mut self) {
        *self = Self::default();
    }

    /// Events after which the playhead may not move again for a while: the UI must have its position.
    fn stops(event: &Event) -> bool {
        match event {
            Event::Seek | Event::PlaybackRestart | Event::EndFile { .. } => true,
            Event::PropertyChange { name, .. } => matches!(name.as_str(), "pause" | "paused-for-cache" | "eof-reached"),
            _ => false,
        }
    }
}

/// ffmpeg's HLS demuxer *skips* a segment that fails (`seg_max_retry` defaults to 0), which shows
/// up as a jump. Retry instead, and reconnect dropped connections.
const DEMUXER_LAVF_O: &str = "seg_max_retry=8,reconnect=1,reconnect_streamed=1,reconnect_delay_max=6";

/// The distribution's libmpv, by its soname: the dynamic linker finds it.
#[cfg(target_os = "linux")]
const SYSTEM_LIBMPV: &str = "libmpv.so.2";

/// Why the player didn't start. On macOS and Linux the rest of the app opens without it, and
/// whatever needs it fails with this reason rather than Tauri's "state not managed".
pub struct Unavailable(pub String);

/// The player, or the failure saying why there is none.
pub fn get<R: Runtime>(app: &AppHandle<R>) -> CmdResult<State<'_, Player>> {
    app.try_state::<Player>().ok_or_else(|| {
        let why = app.try_state::<Unavailable>().map(|u| u.0.clone()).unwrap_or_default();
        fail(c::PLAY_UNAVAILABLE, why)
    })
}

pub fn setup<R: Runtime>(app: &mut tauri::App<R>, settings: &Settings) -> Result<(), Box<dyn std::error::Error>> {
    let window = app.get_webview_window("main").ok_or("main window missing")?;
    #[cfg(any(windows, target_os = "macos"))]
    let hwnd = window_id(&window)?;
    #[cfg(not(any(windows, target_os = "macos")))]
    let hwnd: i64 = 0;
    #[cfg(windows)]
    let wid = hwnd.to_string();
    app.manage(crate::media::start(app.handle(), hwnd as isize));
    let library = libmpv_path(app)?;
    tracing::info!(library = %library.display(), hwnd, "starting mpv");

    let volume = settings.volume.to_string();
    #[cfg(windows)]
    let video: &[(&str, &str)] = &[
        ("wid", wid.as_str()),
        // libplacebo renderer over Direct3D 11: full quality, HDR tone-mapping.
        ("vo", "gpu-next"),
        ("gpu-api", "d3d11"),
        // Keep the window painted (black) while idle instead of leaving garbage.
        ("force-window", "yes"),
    ];
    // macOS: the same libplacebo renderer over Vulkan (MoltenVK), presenting into a Metal layer
    // under the webview (player_mac.rs) that it gets as `wid`.
    #[cfg(target_os = "macos")]
    let mac_video = Arc::new(crate::player_mac::attach(&window)?);
    #[cfg(target_os = "macos")]
    let layer = mac_video.wid();
    #[cfg(target_os = "macos")]
    let video: &[(&str, &str)] = &[
        ("wid", layer.as_str()),
        ("vo", "gpu-next"),
        ("gpu-api", "vulkan"),
        ("gpu-context", "moltenvk"),
        // HDR goes to the screen as it is (the compositor fits it to the display) when the
        // screen can show it; `set_hdr_output` turns the hint on for HDR files only, so SDR
        // video keeps the plain sRGB surface everywhere. Off, mpv tone-maps HDR to SDR.
        ("target-colorspace-hint", "no"),
        ("target-colorspace-hint-mode", "source"),
    ];
    #[cfg(target_os = "linux")]
    let video: &[(&str, &str)] = &[("vo", "libmpv")];
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    let video: &[(&str, &str)] = &[("vo", "null")];
    let options = [
        video,
        &[
            ("hwdec", if settings.hardware_decoding { "auto-safe" } else { "no" }),
            ("volume", volume.as_str()),
            ("idle", "yes"),
            ("keep-open", "yes"),
            ("background-color", "#000000"),
            // The webview owns all input and all on-screen UI.
            ("osc", "no"),
            ("osd-level", "0"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("input-cursor", "no"),
            ("cursor-autohide", "no"),
            ("terminal", "no"),
            ("ytdl", "no"),
            ("user-agent", anilili_core::net::USER_AGENT),
            // A/V sync and seeking.
            ("video-sync", "audio"),
            ("hr-seek", "yes"),
            // Streaming buffer, paced: with the cache on, mpv otherwise prefetches the whole
            // episode as fast as it can (`cache-secs` defaults to 1000 hours), which trips CDN
            // rate limits (HTTP 429). Two minutes ahead is plenty.
            ("cache", "yes"),
            ("cache-secs", "120"),
            ("demuxer-max-bytes", "150MiB"),
            ("demuxer-max-back-bytes", "50MiB"),
            ("network-timeout", "30"),
            ("demuxer-lavf-o", DEMUXER_LAVF_O),
            ("stream-lavf-o", "reconnect=1,reconnect_streamed=1,reconnect_delay_max=6"),
            // ASS keeps its own layout but may use the letterbox.
            ("sub-ass-force-margins", "yes"),
            ("sub-use-margins", "yes"),
            ("sub-font-size", "44"),
            ("sub-border-size", "2.5"),
        ],
    ]
    .concat();
    #[cfg(not(target_os = "linux"))]
    let mpv = Mpv::new(&library, &options)?;
    // Linux: the bundled build first. When it won't load (a library it links is missing on this
    // distribution), the distribution's own libmpv, which its package manager keeps loadable.
    #[cfg(target_os = "linux")]
    let mpv = match Mpv::new(&library, &options) {
        Ok(mpv) => mpv,
        Err(bundled) if library.as_os_str() != SYSTEM_LIBMPV => {
            tracing::warn!(error = %bundled, "the bundled libmpv won't load; trying the system's");
            let mpv = Mpv::new(std::path::Path::new(SYSTEM_LIBMPV), &options)
                .map_err(|system| format!("{bundled}; the system's {SYSTEM_LIBMPV}: {system}"))?;
            tracing::info!("playing with the system's libmpv");
            mpv
        }
        Err(e) => return Err(e.into()),
    };

    // ffmpeg before 7 (the Mac's former LGPL build had 6.0) takes the end of a kept-alive HTTPS response
    // for a dropped connection: with `reconnect_streamed` it then splices in the wrong bytes and
    // picture and sound fall apart (measured on Miruro's `.jpg`-named HLS segments); without it,
    // each segment stalls through the retries. A fresh connection per segment avoids both.
    if mpv.get_string("ffmpeg-version").is_ok_and(|v| !anilili_mpv::version_at_least(&v, (7, 0))) {
        mpv.set_property("demuxer-lavf-o", format!("{DEMUXER_LAVF_O},http_persistent=0"))?;
    }

    // Before anything can load: without a render context mpv drops the video track.
    #[cfg(target_os = "linux")]
    let video_linux = Arc::new(crate::player_linux::attach(&window, &mpv)?);

    for (name, format) in OBSERVED {
        mpv.observe(name, *format)?;
    }
    // The video's transfer function: `pq` or `hlg` is HDR (`set_hdr_output`).
    #[cfg(target_os = "macos")]
    mpv.observe("video-params/gamma", Format::String)?;
    // Development aid: `ANILILI_MPV_LOG=v` (or `info`, `debug`) shows what mpv is doing.
    let log_level = std::env::var("ANILILI_MPV_LOG").ok().filter(|_| cfg!(debug_assertions));
    mpv.request_log_messages(log_level.as_deref().unwrap_or("warn"))?;

    let handle = app.handle().clone();
    let snapshot = Arc::new(Mutex::new(Snapshot::default()));
    let now_playing: Arc<Mutex<Option<NowPlaying>>> = Arc::new(Mutex::new(None));
    let recorder = Arc::clone(&snapshot);
    let tracked = Arc::clone(&now_playing);
    let mut clock = Clock::default();
    let mut last_save = Instant::now();
    // The last HTTP refusal ffmpeg logged for the current file: a far better explanation of a
    // failed stream (`h403`) than mpv's own "loading failed".
    let mut last_http_status: Option<u16> = None;
    #[cfg(target_os = "macos")]
    let shown = Arc::clone(&mac_video);
    mpv.start_events(move |event| {
        // The video view shows while a file is open and hides when playback stops, so the
        // window's glass shows through the other pages.
        #[cfg(target_os = "macos")]
        match &event {
            Event::StartFile | Event::EndFile { .. } => {
                let visible = matches!(event, Event::StartFile);
                let shown = Arc::clone(&shown);
                let _ = handle.run_on_main_thread(move || shown.set_visible(visible));
            }
            Event::PropertyChange { name, value } if name == "video-params/gamma" => {
                if let Some(player) = handle.try_state::<Player>() {
                    player.set_hdr_output(matches!(value, PropertyValue::String(g) if g == "pq" || g == "hlg"));
                }
            }
            _ => {}
        }
        match &event {
            Event::StartFile => {
                last_http_status = None;
                clock.reset();
            }
            Event::Log { text, .. } => last_http_status = http_status_in(text).or(last_http_status),
            _ => {}
        }
        // A failure reaches the UI as the catalogue's message and code; mpv's text goes to the log.
        let event = match event {
            Event::EndFile { reason: EndReason::Error, error, code } => {
                let entry = if tracked.lock().unwrap().is_some() { c::PLAY_STREAM_FAILED } else { c::PLAY_FILE_FAILED };
                let mut failure = anilili_core::Failure::new(entry, error.clone().unwrap_or_default());
                failure.suffix = last_http_status.take().map(|s| format!("h{s}")).or(code.map(|n| format!("m{n}")));
                failure.log();
                Event::EndFile { reason: EndReason::Error, error: Some(format!("{} ({})", entry.message, failure.code())), code }
            }
            other => other,
        };
        recorder.lock().unwrap().record(&event);
        match &event {
            Event::PropertyChange { name, value } if name == "time-pos" => {
                if last_save.elapsed() >= SAVE_INTERVAL {
                    last_save = Instant::now();
                    save_progress(&handle, &recorder, &tracked);
                }
                if let PropertyValue::Double(position) = value
                    && !clock.offer(*position, Instant::now())
                {
                    return;
                }
            }
            Event::Log { prefix, level, text } => {
                // ffmpeg's demuxers warn about every damaged packet; only their errors matter.
                if !(prefix.starts_with("ffmpeg") && level == "warn") {
                    tracing::warn!(target: "mpv", "[{prefix}] {level}: {text}");
                }
                return;
            }
            Event::EndFile { reason, error, .. } => {
                tracing::info!(?reason, ?error, "end of file");
                save_progress(&handle, &recorder, &tracked);
                if *reason == EndReason::Stop {
                    handle.state::<AppState>().discord.clear_player();
                    handle.state::<crate::media::Media>().update(crate::media::MediaUpdate::Stopped);
                }
            }
            Event::FileLoaded => {
                if let Some(player) = handle.try_state::<Player>() {
                    player.add_pending_subtitles();
                }
                if let Some(playing) = tracked.lock().unwrap().as_mut() {
                    playing.loaded = true;
                }
                update_presence(&handle, &recorder, &tracked);
            }
            Event::PlaybackRestart => update_presence(&handle, &recorder, &tracked),
            Event::PropertyChange { name, .. } if name == "pause" => update_presence(&handle, &recorder, &tracked),
            Event::PropertyChange { name, value } if name == "eof-reached" && *value == PropertyValue::Flag(true) => {
                save_progress(&handle, &recorder, &tracked);
            }
            _ => {}
        }
        if Clock::stops(&event)
            && let Some(position) = clock.flush(Instant::now())
        {
            let _ = handle.emit("player", &Event::PropertyChange { name: "time-pos".into(), value: PropertyValue::Double(position) });
        }
        let _ = handle.emit("player", &event);
    });

    // Development aid: start playing a stream on launch, so the compositing can be checked
    // without clicking through the UI.
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var("ANILILI_SPIKE_URL") {
        tracing::info!(%url, "autoplay (ANILILI_SPIKE_URL)");
        #[cfg(not(target_os = "linux"))]
        let options = std::env::var("ANILILI_SPIKE_SUB").map(|path| format!("sub-files={path}")).unwrap_or_default();
        #[cfg(not(target_os = "linux"))]
        mpv.loadfile(&url, &options)?;
        #[cfg(target_os = "linux")]
        tracing::info!("spike load deferred until the render context is ready");
    }

    let shader_dir = bundled_dir(app, "shaders", "../vendor/shaders");
    let sub_dir = app.path().app_cache_dir().map(|dir| dir.join("subtitles")).unwrap_or_default();
    if !sub_dir.as_os_str().is_empty() {
        let _ = std::fs::remove_dir_all(&sub_dir);
    }
    let player = Player {
        mpv,
        shader_dir,
        snapshot,
        now_playing,
        pending_subtitles: Mutex::default(),
        headers: Mutex::default(),
        sub_dir,
        sub_files: AtomicU64::new(0),
        load_turn: AtomicU64::new(0),
        handing_over: Mutex::default(),
        #[cfg(target_os = "macos")]
        _video: mac_video,
        #[cfg(target_os = "linux")]
        _video_linux: video_linux,
        #[cfg(target_os = "linux")]
        client: Mutex::new(client_thread(app.handle())),
    };
    player.apply_settings(settings);
    app.manage(player);
    #[cfg(all(debug_assertions, target_os = "linux"))]
    if let Ok(url) = std::env::var("ANILILI_SPIKE_URL") {
        let options = std::env::var("ANILILI_SPIKE_SUB").map(|path| format!("sub-files={path}")).unwrap_or_default();
        let handle = app.handle().clone();
        tauri::async_runtime::spawn(async move {
            for _ in 0..100 {
                if handle.state::<Player>()._video_linux.is_ready() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let player = handle.state::<Player>();
            if let Err(e) = player.mpv.loadfile(&url, &options) {
                tracing::warn!(error = %e, "Linux spike load failed");
            }
        });
    }
    Ok(())
}

/// The thread [`Player::off_ui`] hands its calls to: one, so they keep the order they were made in.
#[cfg(target_os = "linux")]
fn client_thread<R: Runtime>(app: &AppHandle<R>) -> std::sync::mpsc::Sender<ClientJob> {
    let (sender, jobs) = std::sync::mpsc::channel::<ClientJob>();
    let app = app.clone();
    std::thread::Builder::new()
        .name("mpv-client".into())
        .spawn(move || {
            for job in jobs {
                if let Some(player) = app.try_state::<Player>() {
                    job(&player);
                }
            }
        })
        .expect("spawning the mpv client thread");
    sender
}

/// Writes the current position for whatever is playing; pushes to AniList once it is finished.
fn save_progress<R: Runtime>(app: &AppHandle<R>, snapshot: &Mutex<Snapshot>, now_playing: &Mutex<Option<NowPlaying>>) {
    let (position, duration) = {
        let s = snapshot.lock().unwrap();
        (s.number("time-pos"), s.number("duration"))
    };
    let Some(playing) = now_playing.lock().unwrap().clone().filter(|p| p.loaded) else { return };
    if position < 5.0 || duration <= 0.0 {
        return;
    }
    let progress = Progress { anime_id: playing.anime_id, episode: playing.episode, position, duration, updated_at: now() };
    let state = app.state::<AppState>();
    if let Err(e) = state.store.save_progress(&progress) {
        tracing::warn!(error = %e, "saving progress failed");
    }
    if progress.finished() && !playing.synced {
        if let Some(current) = now_playing.lock().unwrap().as_mut() {
            current.synced = true;
        }
        let _ = app.emit("progress-finished", &playing);
        if state.settings().sync_anilist {
            crate::simkl::track_progress(app, playing.anime_id, playing.episode.floor() as i32);
            crate::progress_queue::push(app, playing.anime_id, playing.episode.floor() as i32);
        }
    }
}

/// Mirrors what is playing to Discord and the OS media controls (position-based, so seeks and
/// pauses keep them accurate).
fn update_presence<R: Runtime>(app: &AppHandle<R>, snapshot: &Mutex<Snapshot>, now_playing: &Mutex<Option<NowPlaying>>) {
    let state = app.state::<AppState>();
    let (position, duration, paused, title) = {
        let s = snapshot.lock().unwrap();
        let paused = matches!(s.properties.get("pause"), Some(PropertyValue::Flag(true)));
        let title = match s.properties.get("media-title") {
            Some(PropertyValue::String(t)) => t.clone(),
            _ => String::new(),
        };
        (s.number("time-pos"), s.number("duration"), paused, title)
    };
    let playing = now_playing.lock().unwrap().clone();
    let media = app.state::<crate::media::Media>();
    match &playing {
        Some(p) => media.update(crate::media::MediaUpdate::Metadata {
            title: p.title.clone(),
            subtitle: format!("Episode {}", crate::discord::episode_text(p.episode)),
            cover: p.cover.clone(),
            duration,
        }),
        None if !title.is_empty() => {
            media.update(crate::media::MediaUpdate::Metadata { title, subtitle: String::new(), cover: None, duration })
        }
        None => {}
    }
    media.update(if paused { crate::media::MediaUpdate::Paused { position } } else { crate::media::MediaUpdate::Playing { position } });

    if !state.settings().discord_presence {
        return;
    }
    let Some(playing) = playing else { return };
    state.discord.show(crate::discord::Presence {
        anime_id: playing.anime_id,
        title: playing.title,
        episode: playing.episode,
        episode_title: playing.episode_title,
        total: playing.total,
        cover: playing.cover,
        position,
        duration,
        paused,
        embedded: false,
    });
}

#[cfg(any(windows, target_os = "macos"))]
fn window_id<R: Runtime>(window: &tauri::WebviewWindow<R>) -> Result<i64, Box<dyn std::error::Error>> {
    let handle = window.window_handle()?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get() as i64),
        // Stock macOS libmpv ignores `wid`; macOS needs the render API path.
        RawWindowHandle::AppKit(h) => Ok(h.ns_view.as_ptr() as i64),
        other => Err(format!("unsupported window handle: {other:?}").into()),
    }
}

/// A bundled resource directory, or (in development) the fetched copy under `vendor/`.
fn bundled_dir<R: Runtime>(app: &tauri::App<R>, name: &str, dev: &str) -> Option<PathBuf> {
    if let Ok(dir) = app.path().resource_dir()
        && dir.join(name).is_dir()
    {
        return Some(dir.join(name));
    }
    #[cfg(debug_assertions)]
    {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(dev);
        if dev.is_dir() {
            return Some(dev);
        }
    }
    let _ = dev;
    None
}

/// The bundled DLL in a packaged build, the fetched one in development. On macOS libmpv comes
/// with the dylibs it links (ffmpeg, libass…) in their own folder, `Resources/libmpv/`, where its
/// `@loader_path` rpath finds them.
fn libmpv_path<R: Runtime>(app: &tauri::App<R>) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let name = if cfg!(windows) { "libmpv-2.dll" } else if cfg!(target_os = "linux") { "libmpv.so.2" } else { "libmpv.dylib" };
    if let Ok(dir) = app.path().resource_dir() {
        let bundled = if cfg!(any(target_os = "macos", target_os = "linux")) { dir.join("libmpv").join(name) } else { dir.join(name) };
        if bundled.exists() {
            return Ok(bundled);
        }
    }
    // The fetched copy, in development only: a release build must never look at the build
    // machine's paths.
    #[cfg(debug_assertions)]
    {
        let platform = format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
            // Node's names, as scripts/fetch-libmpv.mjs files them: win32-x64, darwin-arm64.
            .replace("windows", "win32")
            .replace("macos", "darwin")
            .replace("x86_64", "x64")
            .replace("aarch64", "arm64");
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/libmpv").join(platform).join(name);
        if dev.exists() {
            return Ok(dev);
        }
    }
    // Linux, with none bundled: the distribution's own, wherever its dynamic linker keeps it.
    #[cfg(target_os = "linux")]
    return Ok(PathBuf::from(SYSTEM_LIBMPV));
    #[cfg(not(target_os = "linux"))]
    Err(format!(
        "{}: {name} is missing next to the app; reinstall Anilili (in development: node scripts/fetch-libmpv.mjs)",
        c::PLAY_NO_LIBMPV.code()
    )
    .into())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleTrack {
    pub url: String,
    pub title: Option<String>,
    pub lang: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadRequest {
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Seconds to start from.
    pub start: Option<f64>,
    /// External subtitle files or URLs to attach.
    #[serde(default)]
    pub subtitles: Vec<SubtitleTrack>,
    pub title: Option<String>,
    /// Preferred subtitle languages, for mpv's own track selection.
    #[serde(default)]
    pub subtitle_languages: Vec<String>,
    /// Whether a subtitle track is chosen as the file opens (Show subtitles by default); unset is
    /// yes. The tracks are added either way, for the controls to pick from.
    #[serde(default)]
    pub show_subtitles: Option<bool>,
}

impl Player {
    /// mpv's version string, for the diagnostics file.
    pub fn mpv_version(&self) -> Option<String> {
        self.mpv.get_string("mpv-version").ok()
    }

    pub async fn load(&self, request: &LoadRequest, now_playing: Option<NowPlaying>) -> anilili_mpv::Result<()> {
        let turn = self.load_turn.fetch_add(1, Ordering::SeqCst) + 1;
        // External subtitles go to mpv as local files. mpv reads a subtitle file whole when it
        // opens it, and over a CDN that refuses range requests (Icarus's answers a plain 200 to
        // them) that read comes up short: the episode's first seconds carry cues, then nothing,
        // however healthy the file. A local copy reads whole. They are fetched together and
        // before the file is handed over, as mpv itself fetched them while loading: the tracks
        // are there when mpv picks one by `slang`, which it does once, at load.
        let subtitles = self.local_subtitles(&request.subtitles, &request.headers, turn).await;

        // Linux: mpv drops a file loaded before the GL render context exists.
        #[cfg(target_os = "linux")]
        {
            for _ in 0..100 {
                if self._video_linux.is_ready() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        // No await from here on: the hand-over is one piece.
        let _handing_over = self.handing_over.lock().unwrap();
        if self.load_turn.load(Ordering::SeqCst) != turn {
            // Another episode was asked for while these subtitles were on their way; it plays.
            tracing::info!("load overtaken by a newer one");
            return Ok(());
        }
        let mpv = &self.mpv;
        *self.now_playing.lock().unwrap() = now_playing;

        // Headers are per stream: clear the previous stream's first. `change-list append` takes
        // one item verbatim, so values may contain commas.
        mpv.command(&["change-list", "http-header-fields", "clr", ""])?;
        for (name, value) in &request.headers {
            if name.eq_ignore_ascii_case("user-agent") {
                continue;
            }
            mpv.command(&["change-list", "http-header-fields", "append", &format!("{name}: {value}")])?;
        }
        *self.headers.lock().unwrap() = request.headers.clone();
        if !request.subtitle_languages.is_empty() {
            mpv.set_property("slang", request.subtitle_languages.join(","))?;
        }
        // Set for every file: a track picked by hand in the last one would otherwise carry over.
        let show = request.show_subtitles.unwrap_or(true);
        mpv.set_property("sid", if show { "auto" } else { "no" })?;

        let mut options = Vec::new();
        if let Some(start) = request.start.filter(|s| *s > 0.0) {
            options.push(format!("start={start}"));
        }
        if let Some(title) = &request.title {
            // Quoted so a comma in the title does not split the option list.
            options.push(format!("force-media-title=\"{}\"", title.replace('"', "'")));
        }
        mpv.loadfile(&request.url, &options.join(","))?;

        let mut pending = Vec::new();
        for subtitle in &subtitles {
            let title = subtitle.title.clone().unwrap_or_default();
            let lang = subtitle.lang.clone().unwrap_or_default();
            // `auto` adds the track and lets `slang` decide selection.
            match mpv.command(&["sub-add", &subtitle.url, "auto", &title, &lang]) {
                // mpv before 0.38 has no file to add it to yet: after loading, then.
                Err(anilili_mpv::Error::Mpv { code: anilili_mpv::ERROR_COMMAND, .. }) => pending.push(subtitle.clone()),
                result => result?,
            }
        }
        // No languages to choose by when subtitles start off: the pending ones are only added.
        let languages = if show { request.subtitle_languages.clone() } else { Vec::new() };
        *self.pending_subtitles.lock().unwrap() = (pending, languages);
        Ok(())
    }

    /// The load's subtitles with every remote one fetched to disk, all at once (a plain client
    /// carrying the stream's headers, as mpv would ask). A link that doesn't answer with a file
    /// keeps its remote URL: the track is still offered, and picking it in the panel tries again
    /// (`reopen_subtitle`). Local files stay where they are.
    async fn local_subtitles(&self, tracks: &[SubtitleTrack], headers: &BTreeMap<String, String>, turn: u64) -> Vec<SubtitleTrack> {
        if tracks.is_empty() {
            return Vec::new();
        }
        self.sweep_subtitle_files(turn);
        let client = anilili_core::net::Http::plain().ok().map(|http| http.client().clone());
        let fetches: Vec<_> = tracks
            .iter()
            .map(|track| {
                let client = client.clone().filter(|_| is_remote(&track.url))?;
                let (url, headers) = (track.url.clone(), headers.clone());
                Some(tauri::async_runtime::spawn(async move { subtitle_bytes(&client, &url, &headers).await }))
            })
            .collect();
        let mut local = Vec::with_capacity(tracks.len());
        for (track, fetch) in tracks.iter().zip(fetches) {
            let bytes = match fetch {
                Some(fetch) => fetch.await.ok().flatten(),
                None => None,
            };
            local.push(match bytes.and_then(|bytes| self.save_subtitle(turn, &track.url, bytes)) {
                Some(path) => SubtitleTrack { url: path.to_string_lossy().into_owned(), ..track.clone() },
                None => track.clone(),
            });
        }
        local
    }

    /// Writes a fetched subtitle file where mpv will read it. WebVTT goes as SRT (text and timing
    /// kept, markup dropped); anything else as it came, under the link's own extension.
    fn save_subtitle(&self, turn: u64, url: &str, bytes: Vec<u8>) -> Option<PathBuf> {
        if self.sub_dir.as_os_str().is_empty() {
            return None;
        }
        std::fs::create_dir_all(&self.sub_dir).ok()?;
        let (contents, extension) = match srt_from_webvtt(&bytes) {
            Some(srt) => (srt, "srt"),
            None => (bytes, subtitle_extension(url)),
        };
        let number = self.sub_files.fetch_add(1, Ordering::Relaxed);
        let path = self.sub_dir.join(format!("sub-{turn}-{number}.{extension}"));
        std::fs::write(&path, &contents).ok()?;
        tracing::info!(turn, bytes = contents.len(), extension, "subtitle fetched to disk");
        Some(path)
    }

    /// Deletes the subtitle files of loads before the last one (that one may still be playing
    /// while this load fetches).
    fn sweep_subtitle_files(&self, turn: u64) {
        for entry in std::fs::read_dir(&self.sub_dir).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let file_turn = name.strip_prefix("sub-").and_then(|rest| rest.split('-').next()).and_then(|t| t.parse::<u64>().ok());
            if file_turn.is_some_and(|t| t + 1 < turn) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    /// macOS: signals HDR video to the screen as HDR (`auto`: only where the screen's EDR headroom
    /// says it can show it, `player_mac.rs`), and everything else as plain SDR. Per file, because
    /// with the hint on mpv would tag SDR video BT.709 on an HDR screen and sRGB elsewhere.
    #[cfg(target_os = "macos")]
    fn set_hdr_output(&self, hdr: bool) {
        if let Err(e) = self.mpv.set_property("target-colorspace-hint", if hdr { "auto" } else { "no" }) {
            tracing::warn!(error = %e, "target-colorspace-hint");
        }
    }

    /// Adds the subtitles mpv refused during loading. Track selection has run by now, so `auto`
    /// would no longer let `slang` pick one: when nothing is selected, the first track in the
    /// most preferred language is selected here instead.
    fn add_pending_subtitles(&self) {
        let (tracks, languages) = std::mem::take(&mut *self.pending_subtitles.lock().unwrap());
        if tracks.is_empty() {
            return;
        }
        let none_selected = self.mpv.get_string("sid").is_ok_and(|sid| sid == "no");
        let chosen = languages
            .iter()
            .filter(|_| none_selected)
            .find_map(|wanted| tracks.iter().position(|t| t.lang.as_deref().is_some_and(|lang| lang.eq_ignore_ascii_case(wanted))));
        for (index, subtitle) in tracks.iter().enumerate() {
            let flag = if chosen == Some(index) { "select" } else { "auto" };
            let title = subtitle.title.clone().unwrap_or_default();
            let lang = subtitle.lang.clone().unwrap_or_default();
            if let Err(e) = self.mpv.command(&["sub-add", &subtitle.url, flag, &title, &lang]) {
                tracing::warn!(error = %e, "subtitle not added");
            }
        }
    }

    /// Picking an external subtitle track in the panel fetches its file anew. A server's
    /// subtitle link can answer 403 one minute and work the next, and mpv swallows the
    /// failure — the track stays selected over an error page that parses as no cues
    /// (Icarus's, 2026-10-10). A link that answers now is fetched to disk like the ones at load,
    /// and the track replaced by the local copy; the old track is removed either way, and `false`
    /// says the link was dead, for the panel to say so. Local tracks (fetched at load) are simply
    /// re-added.
    pub async fn reopen_subtitle(&self, url: String, title: Option<String>, lang: Option<String>) -> bool {
        let turn = self.load_turn.load(Ordering::SeqCst);
        let local = if is_remote(&url) {
            let headers = self.headers.lock().unwrap().clone();
            let bytes = match anilili_core::net::Http::plain() {
                Ok(http) => subtitle_bytes(http.client(), &url, &headers).await,
                Err(_) => None,
            };
            bytes.map(|bytes| self.save_subtitle(turn, &url, bytes).map_or_else(|| url.clone(), |path| path.to_string_lossy().into_owned()))
        } else {
            Some(url.clone())
        };
        tracing::info!(alive = local.is_some(), "subtitle re-pick");
        if self.load_turn.load(Ordering::SeqCst) != turn {
            // Another episode started meanwhile: this track belonged to the last one.
            return true;
        }
        if let Some(id) = self.external_track_id(&url) {
            let _ = self.mpv.command(&["sub-remove", &id]);
        }
        let Some(local) = local else { return false };
        let _ = self.mpv.command(&["sub-add", &local, "select", title.as_deref().unwrap_or_default(), lang.as_deref().unwrap_or_default()]);
        true
    }

    /// The id of the external subtitle track playing from this URL, if one is loaded.
    fn external_track_id(&self, url: &str) -> Option<String> {
        let list = self.mpv.get_string("track-list").ok()?;
        let tracks = serde_json::from_str::<serde_json::Value>(&list).ok()?;
        tracks
            .as_array()?
            .iter()
            .filter_map(|track| {
                let external = track.get("external").and_then(serde_json::Value::as_bool).unwrap_or(false);
                let from_here = track.get("external-filename").and_then(serde_json::Value::as_str) == Some(url);
                (external && from_here)
                    .then(|| track.get("id").and_then(serde_json::Value::as_u64).map(|id| id.to_string()))
                    .flatten()
            })
            .next()
    }

    pub fn set_pause(&self, paused: bool) {
        let _ = self.mpv.set_property("pause", paused);
    }

    pub fn toggle_pause(&self) {
        let _ = self.mpv.command(&["cycle", "pause"]);
    }

    pub fn seek_absolute(&self, seconds: f64) {
        let _ = self.mpv.command(&["seek", &seconds.to_string(), "absolute+exact"]);
    }

    pub fn seek_relative(&self, seconds: f64) {
        let _ = self.mpv.command(&["seek", &seconds.to_string(), "relative+exact"]);
    }

    pub fn now_playing(&self) -> Option<NowPlaying> {
        self.now_playing.lock().unwrap().clone()
    }

    /// Runs mpv calls made from a plain command (one on the main thread) where they can't stall it.
    /// On Linux the main thread is also the one drawing the video (`player_linux.rs`), and mpv's
    /// render API forbids synchronous client calls there: one waits on the core, which can be
    /// waiting for a frame only that thread draws, and the window stalls until mpv gives up.
    /// Elsewhere mpv draws on its own thread, and the call runs here and now.
    pub fn off_ui(&self, job: impl FnOnce(&Player) + Send + 'static) {
        #[cfg(target_os = "linux")]
        {
            if let Err(std::sync::mpsc::SendError(job)) = self.client.lock().unwrap().send(Box::new(job)) {
                tracing::warn!("the mpv client thread is gone; calling mpv here");
                job(self);
            }
        }
        #[cfg(not(target_os = "linux"))]
        job(self);
    }

    pub fn apply_settings(&self, settings: &Settings) {
        let hwdec = if settings.hardware_decoding { "auto-safe" } else { "no" };
        if let Err(e) = self.mpv.set_property("hwdec", hwdec) {
            tracing::warn!(error = %e, "hwdec");
        }
        let _ = self.mpv.set_property("sub-scale", settings.subtitle_scale.clamp(0.5, 2.5));
        self.apply_captions(&settings.captions);
        self.apply_upscaler(&settings.upscaler);
        self.apply_buffer(&settings.playback_buffer_size, &settings.video_cache_size);
        self.apply_content_scale(&settings.player_content_scale);
    }

    /// Android's Caption appearance on mpv's text subtitles (styled ASS keeps its own). The box is
    /// libass's `background-box` where mpv has it (0.38 on), which keeps the outline; older mpv
    /// draws `opaque-box` instead, coloured by the outline colour, so the edge goes.
    /// With a box, its padding takes the shadow's offset, so Shadow draws no edge there.
    fn apply_captions(&self, c: &crate::app::Captions) {
        let mpv = &self.mpv;
        let text = caption_colour(caption_text_rgb(&c.text_color), c.text_opacity);
        let edge = caption_colour(0x000000, c.text_opacity);
        let boxed = c.background_opacity > 0;
        let mut results = vec![
            mpv.set_property("sub-font", caption_font(&c.font)),
            mpv.set_property("sub-bold", c.bold),
            mpv.set_property("sub-color", text.as_str()),
            mpv.set_property("sub-align-y", if c.position == "top" { "top" } else { "bottom" }),
            mpv.set_property("sub-margin-y", caption_margin(c.edge_margin)),
        ];
        let (align, margin_x) = caption_across(c.horizontal_anchor);
        results.extend([mpv.set_property("sub-align-x", align), mpv.set_property("sub-margin-x", margin_x)]);
        if boxed {
            let fill = caption_colour(caption_background_rgb(&c.background_color), c.background_opacity);
            if mpv.set_property("sub-border-style", "background-box").is_ok() {
                results.extend([
                    mpv.set_property("sub-back-color", fill.as_str()),
                    mpv.set_property("sub-shadow-offset", 6.0),
                    mpv.set_property("sub-outline-color", edge.as_str()),
                    mpv.set_property("sub-outline-size", if c.edge_style == "outline" { 2.5 } else { 0.0 }),
                ]);
            } else {
                results.extend([
                    mpv.set_property("sub-border-style", "opaque-box"),
                    mpv.set_property("sub-outline-color", fill.as_str()),
                    mpv.set_property("sub-outline-size", 4.0),
                    mpv.set_property("sub-shadow-offset", 0.0),
                ]);
            }
        } else {
            results.extend([
                mpv.set_property("sub-border-style", "outline-and-shadow"),
                mpv.set_property("sub-outline-color", edge.as_str()),
                mpv.set_property("sub-outline-size", if c.edge_style == "outline" { 2.5 } else { 0.0 }),
                mpv.set_property("sub-back-color", caption_colour(0x000000, c.text_opacity.saturating_mul(4) / 5).as_str()),
                mpv.set_property("sub-shadow-offset", if c.edge_style == "shadow" { 2.0 } else { 0.0 }),
            ]);
        }
        if let Some(e) = results.into_iter().find_map(Result::err) {
            tracing::warn!(error = %e, "caption style");
        }
    }

    /// Android's Screen size. Fit letterboxes; Crop zooms until the window is covered (`panscan`
    /// cuts the overflow); Fill stretches the picture to the window's shape.
    fn apply_content_scale(&self, scale: &str) {
        let (keepaspect, panscan) = match scale {
            "crop" => ("yes", 1.0),
            "fill" => ("no", 0.0),
            _ => ("yes", 0.0),
        };
        if let Err(e) = self.mpv.set_property("keepaspect", keepaspect).and_then(|()| self.mpv.set_property("panscan", panscan)) {
            tracing::warn!(error = %e, scale, "screen size");
        }
    }

    /// Buffer size and video cache (Android's, under Playback and Data). Ahead: how far mpv reads
    /// and how long it waits after a stall before resuming (a longer wait keeps one stall from
    /// becoming a stutter). Behind: watched video kept for seeking back, on disk once sized.
    /// Automatic keeps the tuning this app shipped with (two minutes ahead: mpv's own default reads
    /// the whole episode at once and trips CDN rate limits). A change applies from the next file.
    fn apply_buffer(&self, size: &str, cache: &str) {
        let (secs, bytes, wait) = match size {
            "small" => (15, "64MiB", 1),
            "medium" => (40, "150MiB", 2),
            "large" => (90, "300MiB", 3),
            "very_large" => (240, "600MiB", 5),
            _ => (120, "150MiB", 1),
        };
        let (on_disk, back) = match cache {
            "off" => ("no", "0"),
            "256" => ("yes", "256MiB"),
            "512" => ("yes", "512MiB"),
            "1024" => ("yes", "1024MiB"),
            "2048" => ("yes", "2048MiB"),
            _ => ("no", "50MiB"),
        };
        for (name, value) in [
            ("cache-secs", secs.to_string()),
            ("demuxer-max-bytes", bytes.to_owned()),
            ("cache-pause-wait", wait.to_string()),
            ("cache-on-disk", on_disk.to_owned()),
            ("demuxer-max-back-bytes", back.to_owned()),
        ] {
            if let Err(e) = self.mpv.set_property(name, value.as_str()) {
                tracing::warn!(error = %e, name, "buffer setting");
            }
        }
    }

    /// Anime4K's "Mode A" chains (for 1080p sources; the auto-downscale passes keep them correct
    /// at other sizes). Paths join with the platform's list separator.
    fn apply_upscaler(&self, mode: &str) {
        const FAST: &[&str] = &[
            "Anime4K_Clamp_Highlights",
            "Anime4K_Restore_CNN_M",
            "Anime4K_Upscale_CNN_x2_M",
            "Anime4K_AutoDownscalePre_x2",
            "Anime4K_AutoDownscalePre_x4",
            "Anime4K_Upscale_CNN_x2_S",
        ];
        const QUALITY: &[&str] = &[
            "Anime4K_Clamp_Highlights",
            "Anime4K_Restore_CNN_VL",
            "Anime4K_Upscale_CNN_x2_VL",
            "Anime4K_AutoDownscalePre_x2",
            "Anime4K_AutoDownscalePre_x4",
            "Anime4K_Upscale_CNN_x2_M",
        ];
        let chain = match mode {
            "fast" => FAST,
            "quality" => QUALITY,
            _ => &[],
        };
        let value = match &self.shader_dir {
            Some(dir) if !chain.is_empty() => chain
                .iter()
                .map(|name| dir.join(format!("{name}.glsl")).to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(if cfg!(windows) { ";" } else { ":" }),
            _ => String::new(),
        };
        if let Err(e) = self.mpv.set_property("glsl-shaders", value) {
            tracing::warn!(error = %e, mode, "applying upscaler failed");
        }
    }

    pub fn stop(&self) {
        *self.now_playing.lock().unwrap() = None;
        // On Linux mpv hears of it a moment later (`off_ui`): a load begun since keeps its file.
        let turn = self.load_turn.load(Ordering::SeqCst);
        self.off_ui(move |player| {
            if player.load_turn.load(Ordering::SeqCst) == turn {
                let _ = player.mpv.command(&["stop"]);
            }
        });
    }
}

/// Current player state, as the events that would have produced it.
#[tauri::command]
pub fn player_snapshot<R: Runtime>(app: AppHandle<R>) -> Vec<Event> {
    app.try_state::<Player>().map(|player| player.snapshot.lock().unwrap().replay()).unwrap_or_default()
}

/// Loads an arbitrary URL or file (drag and drop, the developer panel).
#[tauri::command(async)]
pub async fn player_load<R: Runtime>(app: AppHandle<R>, request: LoadRequest) -> CmdResult<()> {
    get(&app)?.load(&request, None).await.map_err(|e| fail(c::PLAY_FILE_FAILED, e.to_string()))
}

/// Re-picks an external subtitle track, fetching its file anew. `false` says the file
/// wouldn't load — the panel says so.
#[tauri::command(async)]
pub async fn subtitle_reopen<R: Runtime>(app: AppHandle<R>, url: String, title: Option<String>, lang: Option<String>) -> CmdResult<bool> {
    Ok(get(&app)?.reopen_subtitle(url, title, lang).await)
}

/// The subtitle file, fetched the way mpv fetches it: a plain client carrying the stream's
/// headers — the emulated browser fingerprint is refused outright. `None` when it doesn't
/// answer with one: some CDNs serve an HTML error page in its place (a 403 for a link gone
/// stale), which mpv would parse as no cues and say nothing about. Every track of a load is
/// fetched before the episode starts, so a server that hangs is given up on soon.
async fn subtitle_bytes(client: &wreq::Client, url: &str, headers: &BTreeMap<String, String>) -> Option<Vec<u8>> {
    let mut request = client.get(url).timeout(Duration::from_secs(10)).header("User-Agent", anilili_core::net::USER_AGENT);
    for (name, value) in headers {
        // mpv sends its own user agent, never the stream's (see `load`); so does this.
        if name.eq_ignore_ascii_case("user-agent") {
            continue;
        }
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().await.ok()?;
    (!bytes.starts_with(b"<")).then(|| bytes.to_vec())
}

/// A subtitle track's URL is a link to fetch, not a file on this computer.
fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// The extension a fetched subtitle file is saved under: the link's own when it names a subtitle
/// format, else `vtt`, what the servers send.
fn subtitle_extension(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    ["srt", "ass", "ssa", "vtt"].into_iter().find(|ext| path.ends_with(&format!(".{ext}"))).unwrap_or("vtt")
}

/// WebVTT cues as SRT. Timing converts (`MM:SS.mmm` or `HH:MM:SS.mmm` to SRT's comma form),
/// cue settings go, markup tags are stripped and character references (`&amp;`) decoded, and
/// the header, `STYLE`, `REGION` and `NOTE` blocks fall away — a cue whose body turns out empty
/// is dropped. `None` when the file isn't WebVTT or holds no cue at all.
fn srt_from_webvtt(bytes: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !text.starts_with("WEBVTT") {
        return None;
    }
    let stamp = |raw: &str| -> Option<String> {
        let mut parts = Vec::new();
        for part in raw.trim().split(':') {
            parts.push(part.parse::<f64>().ok()?);
        }
        let seconds = match parts.as_slice() {
            [minutes, secs] => minutes * 60.0 + secs,
            [hours, minutes, secs] => hours * 3600.0 + minutes * 60.0 + secs,
            _ => return None,
        };
        let millis = (seconds * 1000.0).round() as u64;
        Some(format!("{:02}:{:02}:{:02},{:03}", millis / 3_600_000, millis % 3_600_000 / 60_000, millis % 60_000 / 1000, millis % 1000))
    };

    let mut srt = Vec::new();
    let mut number: u64 = 0;
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let Some((from, rest)) = line.split_once("-->") else { continue };
        let Some(from) = stamp(from) else { continue };
        // Cue settings may follow the end timestamp; they are WebVTT's, not SRT's.
        let Some(to) = rest.split_whitespace().next().and_then(stamp) else { continue };
        let mut body = Vec::new();
        while let Some(next) = lines.peek() {
            if next.trim().is_empty() {
                break;
            }
            let mut plain = String::new();
            let mut in_tag = false;
            for ch in next.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' => in_tag = false,
                    _ if !in_tag => plain.push(ch),
                    _ => {}
                }
            }
            if !plain.trim().is_empty() {
                body.push(
                    plain
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&nbsp;", "\u{a0}")
                        .replace("&quot;", "\"")
                        .replace("&apos;", "'")
                        .replace("&#39;", "'")
                        .replace("&amp;", "&"),
                );
            }
            lines.next();
        }
        if body.is_empty() {
            continue;
        }
        number += 1;
        srt.extend_from_slice(format!("{number}\n{from} --> {to}\n{}\n\n", body.join("\n")).as_bytes());
    }
    (number > 0).then_some(srt)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Control {
    TogglePause,
    SetPause {
        paused: bool,
    },
    Seek {
        seconds: f64,
        relative: bool,
    },
    SetVolume {
        volume: f64,
    },
    SetMute {
        muted: bool,
    },
    SetSpeed {
        speed: f64,
    },
    SetSubDelay {
        seconds: f64,
    },
    /// `kind` is `sub`, `audio` or `video`; `id` is a track id, or `no`.
    SelectTrack {
        kind: String,
        id: String,
    },
    AddSubtitle {
        url: String,
    },
    Stop,
    /// Fractions of the window kept free of video (e.g. for a side panel).
    SetMargins {
        left: f64,
        right: f64,
        top: f64,
        bottom: f64,
    },
    /// Bottom margin for text subtitles, in mpv's 720-line OSD units, so they clear the control
    /// bar while it is shown. Styled ASS keeps its own layout.
    SetSubtitleMargin {
        margin: i64,
    },
    /// Saves a screenshot (with subtitles) to Pictures/Anilili.
    Screenshot,
}

#[tauri::command]
pub fn player_control<R: Runtime>(app: AppHandle<R>, control: Control) -> CmdResult<()> {
    let player = get(&app)?;
    // What has to happen here, in order with the other plain commands: the volume is a setting
    // (a read-modify-write of the store), and a bad track kind is the caller's mistake.
    let control = match control {
        Control::SetVolume { volume } => {
            let volume = volume.clamp(0.0, 150.0);
            let state = app.state::<AppState>();
            let mut settings = state.settings();
            settings.volume = volume;
            let _ = state.save_settings(settings);
            Control::SetVolume { volume }
        }
        Control::SelectTrack { kind, .. } if !matches!(kind.as_str(), "sub" | "audio" | "video") => {
            return Err(fail(c::PLAY_NO_TRACK, format!("unknown track kind {kind}")));
        }
        Control::Stop => {
            player.stop();
            return Ok(());
        }
        Control::Screenshot => {
            let dir = app.path().picture_dir().map(|d| d.join("Anilili")).unwrap_or_default();
            let _ = std::fs::create_dir_all(&dir);
            // Linux runs the control after this returns (`off_ui`), so its failure is only logged.
            #[cfg(target_os = "linux")]
            {
                player.off_ui(move |player| {
                    if let Err(e) = screenshot(&player.mpv, &dir) {
                        tracing::warn!(error = %e, "screenshot");
                    }
                });
                return Ok(());
            }
            #[cfg(not(target_os = "linux"))]
            return Ok(screenshot(&player.mpv, &dir).map_err(|e| e.to_string())?);
        }
        other => other,
    };
    player.off_ui(move |player| {
        if let Err(e) = control_mpv(player, control) {
            tracing::warn!(error = %e, "player control");
        }
    });
    Ok(())
}

fn screenshot(mpv: &Mpv, dir: &std::path::Path) -> anilili_mpv::Result<()> {
    mpv.set_property("screenshot-directory", dir.to_string_lossy().into_owned()).and_then(|()| mpv.command(&["screenshot", "subtitles"]))
}

/// A control's mpv calls (`player_control` has dealt with the rest).
fn control_mpv(player: &Player, control: Control) -> anilili_mpv::Result<()> {
    let mpv = &player.mpv;
    match control {
        Control::TogglePause => mpv.command(&["cycle", "pause"]),
        Control::SetPause { paused } => mpv.set_property("pause", paused),
        Control::Seek { seconds, relative } => {
            let mode = if relative { "relative+exact" } else { "absolute+exact" };
            mpv.command(&["seek", &seconds.to_string(), mode])
        }
        Control::SetVolume { volume } => mpv.set_property("volume", volume),
        Control::SetMute { muted } => mpv.set_property("mute", muted),
        Control::SetSpeed { speed } => mpv.set_property("speed", speed.clamp(0.25, 4.0)),
        Control::SetSubDelay { seconds } => mpv.set_property("sub-delay", seconds),
        Control::SelectTrack { kind, id } => {
            let property = match kind.as_str() {
                "sub" => "sid",
                "audio" => "aid",
                _ => "vid",
            };
            mpv.set_property(property, id)
        }
        Control::AddSubtitle { url } => mpv.command(&["sub-add", &url, "select"]),
        Control::SetMargins { left, right, top, bottom } => [
            ("video-margin-ratio-left", left),
            ("video-margin-ratio-right", right),
            ("video-margin-ratio-top", top),
            ("video-margin-ratio-bottom", bottom),
        ]
        .into_iter()
        .try_for_each(|(name, value)| mpv.set_property(name, value.clamp(0.0, 1.0))),
        Control::SetSubtitleMargin { margin } => mpv.set_property("sub-margin-y", margin.clamp(0, 600)),
        // Taken by `player_control`.
        Control::Stop | Control::Screenshot => Ok(()),
    }
}

/// mpv's `#AARRGGBB` for a colour at a 0..=100 opacity.
fn caption_colour(rgb: u32, opacity: u8) -> String {
    let alpha = u32::from(opacity.min(100)) * 255 / 100;
    format!("#{alpha:02X}{:06X}", rgb & 0xFF_FFFF)
}

/// Android's caption text colours (`CaptionTextColor`).
fn caption_text_rgb(name: &str) -> u32 {
    match name {
        "yellow" => 0xFFEB3B,
        "cyan" => 0x4DD0E1,
        "green" => 0x81C784,
        "magenta" => 0xFF80AB,
        _ => 0xFFFFFF,
    }
}

/// Android's caption backgrounds (`CaptionBackgroundColor`).
fn caption_background_rgb(name: &str) -> u32 {
    match name {
        "gray" => 0x4A4A4A,
        "navy" => 0x102A43,
        "white" => 0xFFFFFF,
        _ => 0x000000,
    }
}

/// Android's caption faces as families each system carries; libass falls back to its default for a
/// missing one. Keep in step with `CAPTION_FONTS` in src/lib/captions.ts, which draws the preview.
fn caption_font(name: &str) -> &'static str {
    #[cfg(target_os = "macos")]
    let face = match name {
        "serif" => "Georgia",
        "mono" => "Menlo",
        "condensed" => "Avenir Next Condensed",
        "casual" => "Comic Sans MS",
        "cursive" => "Snell Roundhand",
        _ => "Helvetica Neue",
    };
    #[cfg(target_os = "linux")]
    let face = match name {
        "serif" => "DejaVu Serif",
        "mono" => "DejaVu Sans Mono",
        "condensed" => "DejaVu Sans Condensed",
        "casual" => "DejaVu Sans",
        "cursive" => "DejaVu Sans",
        _ => "DejaVu Sans",
    };
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let face = match name {
        "serif" => "Georgia",
        "mono" => "Consolas",
        "condensed" => "Bahnschrift Condensed",
        "casual" => "Comic Sans MS",
        "cursive" => "Segoe Script",
        _ => "Arial",
    };
    face
}

/// Android's horizontal anchor as mpv can place text: aligned left, centred or right, with a margin.
/// mpv can't centre a line on a point, so the anchor lands on the nearest of Android's five steps:
/// against an edge, pushed in from it (about where a short line centred at a quarter would start),
/// or centred. The margin is in mpv's scaled pixels (720 to the picture's height; 1280 across 16:9).
pub fn caption_across(anchor: u8) -> (&'static str, f64) {
    const EDGE: f64 = 19.0; // mpv's own default
    const IN: f64 = 150.0;
    match anchor.min(100) {
        0..=12 => ("left", EDGE),
        13..=37 => ("left", IN),
        38..=62 => ("center", EDGE),
        63..=87 => ("right", IN),
        _ => ("right", EDGE),
    }
}

/// The caption's gap to its edge, a percentage of the picture's height, in mpv's 720-line units.
/// The player adds the control bar's height to it while the bar is up (`setSubtitleMargin`).
pub fn caption_margin(percent: u8) -> i64 {
    (i64::from(percent.min(100)) * 720 / 100).max(0)
}

/// The HTTP status in an ffmpeg log line about a refused request ("HTTP error 403 Forbidden",
/// "Server returned 404 Not Found").
fn http_status_in(text: &str) -> Option<u16> {
    ["HTTP error ", "Server returned "].iter().find_map(|marker| {
        let rest = &text[text.find(marker)? + marker.len()..];
        rest.get(..3)?.parse().ok().filter(|status| (400..600).contains(status))
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use anilili_mpv::{EndReason, Event, PropertyValue};

    use super::{Clock, caption_across, caption_margin, http_status_in, srt_from_webvtt, subtitle_extension};

    #[test]
    fn reads_http_refusals_from_ffmpeg_logs() {
        assert_eq!(http_status_in("HTTP error 403 Forbidden"), Some(403));
        assert_eq!(http_status_in("Server returned 404 Not Found"), Some(404));
        assert_eq!(http_status_in("Opening 'https://x/seg-1.ts' for reading"), None);
    }

    #[test]
    fn webvtt_becomes_srt() {
        let file = b"WEBVTT\n\n00:00.000 --> 00:06.780\n<b>Second Princess</b>\n\n01:02:03.270 --> 01:02:09.920\nTwo lines\nlike this\n\nNOTE a comment\n\n00:08.550 --> 00:10.300 settings align:start\n\n";
        let srt = String::from_utf8(srt_from_webvtt(file).unwrap()).unwrap();
        assert_eq!(
            srt,
            "1\n00:00:00,000 --> 00:00:06,780\nSecond Princess\n\n2\n01:02:03,270 --> 01:02:09,920\nTwo lines\nlike this\n\n"
        );
    }

    #[test]
    fn a_byte_order_mark_and_character_references_are_read_through() {
        let file = "\u{feff}WEBVTT\n\n00:01.000 --> 00:02.000\nTom &amp; Jerry &lt;3\n\n".as_bytes();
        let srt = String::from_utf8(srt_from_webvtt(file).unwrap()).unwrap();
        assert_eq!(srt, "1\n00:00:01,000 --> 00:00:02,000\nTom & Jerry <3\n\n");
    }

    #[test]
    fn a_fetched_subtitle_keeps_its_links_extension() {
        assert_eq!(subtitle_extension("https://x/subs/en.ASS?token=1"), "ass");
        assert_eq!(subtitle_extension("https://x/subs/en.srt#t"), "srt");
        assert_eq!(subtitle_extension("https://x/subs/12345"), "vtt");
    }

    #[test]
    fn non_webvtt_and_cueless_files_stay_as_they_are() {
        assert_eq!(srt_from_webvtt(b"<html>403</html>"), None);
        assert_eq!(srt_from_webvtt(b"WEBVTT\n\nNOTE nothing but a comment\n"), None);
    }

    #[test]
    fn the_anchor_lands_on_the_nearest_of_five_places() {
        assert_eq!(caption_across(0), ("left", 19.0));
        assert_eq!(caption_across(25), ("left", 150.0));
        assert_eq!(caption_across(50), ("center", 19.0));
        assert_eq!(caption_across(55), ("center", 19.0));
        assert_eq!(caption_across(75), ("right", 150.0));
        assert_eq!(caption_across(100), ("right", 19.0));
        assert_eq!(caption_margin(3), 21);
    }

    #[test]
    fn the_clock_paces_positions_and_never_loses_the_last() {
        let start = Instant::now();
        let at = |ms: u64| start + Duration::from_millis(ms);
        let mut clock = Clock::default();
        assert!(clock.offer(1300.0, at(0)));
        // Frames between two updates are held, not sent.
        assert!(!clock.offer(1300.04, at(40)));
        assert!(!clock.offer(1300.08, at(80)));
        assert!(clock.offer(1300.2, at(200)), "a fifth of a second on, playback is sent again");
        assert_eq!(clock.flush(at(210)), None, "nothing held once the newest was sent");
        // Skip Outro, 40 ms after the last update: a seek is sent at once.
        assert!(clock.offer(1418.0, at(240)));
        // mpv settles on the last frame and stops there; the stop sends it.
        assert!(!clock.offer(1417.96, at(300)));
        assert_eq!(clock.flush(at(320)), Some(1417.96));
        assert_eq!(clock.flush(at(330)), None, "once");
    }

    #[test]
    fn a_new_file_drops_the_old_files_position() {
        let start = Instant::now();
        let mut clock = Clock::default();
        assert!(clock.offer(10.0, start));
        assert!(!clock.offer(10.1, start + Duration::from_millis(50)));
        clock.reset();
        assert_eq!(clock.flush(start + Duration::from_millis(60)), None);
        assert!(clock.offer(0.0, start + Duration::from_millis(70)), "the new file's first position goes straight out");
    }

    #[test]
    fn the_clock_is_flushed_when_playback_stops_or_jumps() {
        let property = |name: &str, value| Event::PropertyChange { name: name.into(), value };
        assert!(Clock::stops(&property("eof-reached", PropertyValue::Flag(true))));
        assert!(Clock::stops(&property("pause", PropertyValue::Flag(true))));
        assert!(Clock::stops(&property("paused-for-cache", PropertyValue::Flag(true))));
        assert!(Clock::stops(&Event::PlaybackRestart));
        assert!(Clock::stops(&Event::Seek));
        assert!(Clock::stops(&Event::EndFile { reason: EndReason::Eof, error: None, code: None }));
        // Not the ones that stream in while it plays, or the throttle would be no throttle.
        assert!(!Clock::stops(&property("demuxer-cache-time", PropertyValue::Double(30.0))));
        assert!(!Clock::stops(&property("time-pos", PropertyValue::Double(30.0))));
    }
}
