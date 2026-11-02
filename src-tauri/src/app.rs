//! Shared state: the core services, settings, and in-memory caches for things that expire fast.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use anilili_core::anilist::{AniList, Viewer};
use anilili_core::mal::{Mal, MalTokens, MalUser};
use anilili_core::media_index::MediaIndex;
use anilili_core::miruro::{Miruro, Title};
use anilili_core::model::{Audio, Sources};
use anilili_core::net::Http;
use anilili_core::plugin::Plugins;
use anilili_core::proxy::Proxy;
use anilili_core::store::Store;
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::discord::Discord;

/// How long one episode's server list stays usable: its stream URLs are signed and expire.
pub const SOURCES_TTL: Duration = Duration::from_secs(8 * 60);

/// (AniList id, episode bits) → when resolved, and the sources.
pub type SourcesCache = HashMap<(i64, u64), (Instant, Sources)>;

pub struct AppState {
    pub http: Http,
    pub anilist: AniList,
    pub miruro: Miruro,
    /// The project's static media index: episode art, logos, skip times, schedule.
    pub media_index: MediaIndex,
    /// AniSkip's live intro/outro/recap timings (`watch::skip_times`).
    pub aniskip: anilili_core::aniskip::AniSkip,
    pub store: Arc<Store>,
    pub proxy: Proxy,
    /// WebAssembly providers; replaced wholesale when modules are updated.
    pub plugins: RwLock<Arc<Plugins>>,
    pub browser: Arc<dyn anilili_core::plugin::BrowserFetch>,
    pub discord: Discord,
    pub settings: RwLock<Settings>,
    pub viewer: RwLock<Option<Viewer>>,
    pub mal: Mal,
    pub mal_tokens: tokio::sync::Mutex<Option<MalTokens>>,
    pub mal_user: RwLock<Option<MalUser>>,
    /// PKCE material for a MAL login in progress: (state, verifier).
    pub mal_pending: Mutex<Option<(String, String)>>,
    /// The Simkl tracker, beside whichever list service is signed in (`crate::simkl`).
    pub simkl: anilili_core::simkl::Simkl,
    pub simkl_tokens: tokio::sync::Mutex<Option<anilili_core::simkl::SimklTokens>>,
    /// The sign-in polling for approval, so a second one (or Cancel) can stop it.
    pub simkl_login: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    /// AniList id → Miruro title, for the session.
    pub titles: Mutex<HashMap<i64, Title>>,
    /// (AniList id, episode bits) → sources, briefly.
    pub sources: Arc<Mutex<SourcesCache>>,
    /// Search's "Did you mean", built from the media index's title list on the first search that
    /// needs it, and rebuilt when a day old (`catalog::speller`).
    pub speller: tokio::sync::Mutex<Option<(Instant, Arc<anilili_core::spelling::TitleSpeller>)>>,
    /// The media index's title list, which the speller is built from and which answers a title
    /// search AniList answered thinly (`catalog::index_titles`); kept a day.
    pub index_titles: tokio::sync::Mutex<Option<(Instant, Arc<Vec<anilili_core::media_index::IndexTitle>>)>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub audio: Audio,
    /// ISO 639-1 codes in order of preference, for soft subtitles.
    pub subtitle_languages: Vec<String>,
    pub autoplay_next: bool,
    pub auto_skip_intro: bool,
    pub auto_skip_outro: bool,
    pub resume: bool,
    pub show_adult: bool,
    /// Provider key to try first (`animepahe`, `anikoto`…), when it has the episode: the first of
    /// `server_priority`, kept for older backups and the download dialog's first choice.
    pub preferred_provider: Option<String>,
    /// Android's "Server priority": provider keys in the order they are tried. Providers not named
    /// follow in the catalog's own order; empty is Automatic.
    pub server_priority: Vec<String>,
    /// Android's "Enabled servers": provider keys switched off, never picked or offered.
    pub disabled_providers: Vec<String>,
    /// Android's "Preferred server languages", as its stored values (`eng`, `ara`…): the language
    /// tags and filters the player's server menu shows. Empty shows every language.
    pub preferred_source_languages: Vec<String>,
    /// Android's "DNS over HTTPS" (`anilili_core::dns`), as its stored values: `auto` (the network's
    /// DNS, a secure one where it blocks a source), `none`, or a provider (`cloudflare`, `quad9`…).
    pub doh_provider: String,
    pub volume: f64,
    /// Push progress to AniList when an episode is finished (signed in only).
    pub sync_anilist: bool,
    pub hardware_decoding: bool,
    /// Show what is playing on the user's Discord profile.
    pub discord_presence: bool,
    /// Where downloads go; `None` means Videos/Anilili.
    pub download_dir: Option<String>,
    /// The download dialog's starting quality, as Android stores it: `best`, `1080`, `720`, `480`
    /// or `360`.
    pub download_quality: String,
    /// The download dialog's "Hard subs only" (Android's `download_hard_subs_only`): remembered,
    /// since wanting subtitles that show in any player is a taste rather than a per-batch choice.
    pub download_hard_subs: bool,
    /// "New episode alerts" (Android's `release_notifications`): the AniList feed and the local
    /// release alerts for library titles, as Windows notifications.
    pub release_notifications: bool,
    /// A notification when a batch of downloads finishes.
    pub download_notifications: bool,
    /// Android's "Skip button": a one-press jump on the controls for shows without skip timings,
    /// `skip_button_seconds` long (85 by default, a TV opening).
    pub show_skip_button: bool,
    pub skip_button_seconds: u32,
    /// Android's "Buffer size" (`auto`, `small`, `medium`, `large`, `very_large`): how far ahead
    /// mpv reads, and how long it waits after a stall before resuming.
    pub playback_buffer_size: String,
    /// Android's "Video cache" (`auto`, `off`, `256`, `512`, `1024`, `2048` MB): watched video kept
    /// for seeking back, on disk when sized.
    pub video_cache_size: String,
    /// Multiplier for text subtitles (mpv `sub-scale`).
    pub subtitle_scale: f64,
    /// Android's "Show subtitles by default": a subtitle track is chosen as an episode opens.
    pub subtitles_by_default: bool,
    /// Android's "Also on dubbed audio": the same when the audio is dubbed.
    pub subtitles_with_dub: bool,
    /// Android's "Caption appearance", for text subtitles; styled (ASS) ones keep their own look.
    pub captions: Captions,
    /// Anime4K upscaling: `off`, `fast` or `quality`.
    pub upscaler: String,
    /// Android's "Pause details": the episode's billing (logo, title, synopsis) over a pause that
    /// has outlived the controls.
    pub pause_details_overlay: bool,
    /// Android's "Clock and finishing time": the time now and when the episode ends, top right.
    pub player_clock: bool,
    /// Android's "Screen size": `fit`, `crop` (fills the window, cuts the edges) or `fill`
    /// (stretches).
    pub player_content_scale: String,
    /// Android's "Check for updates on launch": look for a new version at startup and notify.
    pub check_updates_on_launch: bool,
    /// How the app looks: Android's Appearance section, under its stored values.
    pub appearance: Appearance,
}

/// Android's caption style (`CaptionStyle.kt`), under its stored values, applied to mpv's text
/// subtitles (`player.rs`, `apply_captions`). The defaults are the look the desktop always had: white
/// text with a black outline and no box.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Captions {
    /// `sans`, `serif`, `mono`, `condensed`, `casual` or `cursive`.
    pub font: String,
    pub bold: bool,
    /// `white`, `yellow`, `cyan`, `green` or `magenta`.
    pub text_color: String,
    /// 0..=100.
    pub text_opacity: u8,
    /// `black`, `gray`, `navy` or `white`.
    pub background_color: String,
    /// 0..=100; 0 draws no box.
    pub background_opacity: u8,
    /// `none`, `outline` or `shadow`.
    pub edge_style: String,
    /// `bottom` or `top`.
    pub position: String,
    /// The gap to that edge, as a percentage of the picture's height.
    pub edge_margin: u8,
    /// Where across the picture: 0 the left edge, 50 centred, 100 the right (Android's anchor).
    /// mpv aligns text left, centre or right, so this lands on the nearest of five places.
    pub horizontal_anchor: u8,
}

impl Default for Captions {
    fn default() -> Self {
        Self {
            font: "sans".into(),
            bold: false,
            text_color: "white".into(),
            text_opacity: 100,
            background_color: "black".into(),
            background_opacity: 0,
            edge_style: "outline".into(),
            position: "bottom".into(),
            edge_margin: 3,
            horizontal_anchor: 50,
        }
    }
}

/// Android's Appearance choices (`ThemeSettings.kt`, `SettingsStore.kt`), as the values it stores,
/// so a backup carries them between the apps. What each looks like is the frontend's business
/// (`src/lib/theme.ts`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// `amoled` or `dark`.
    pub background: String,
    /// `monochrome`, `custom`, or a named colour (`purple`, `neon_green`, `blood_red`, `ocean_blue`,
    /// `cyan`, `sakura_pink`, `sunset_orange`, `gold`, `emerald`).
    pub accent: String,
    /// The custom accent, `#rrggbb`.
    pub custom_seed: String,
    /// `vivid`, `tonal`, `neutral` or `vibrant`.
    pub style: String,
    /// `standard`, `medium` or `high`.
    pub contrast: String,
    /// The accent on the player's controls (else white).
    pub player_accent: bool,
    /// A title's page takes its colours from the show's artwork.
    pub show_colour_pages: bool,
    /// `none`, `anime` or `image`.
    pub backdrop_source: String,
    pub backdrop_anime_url: Option<String>,
    pub backdrop_anime_title: Option<String>,
    /// The viewer's own picture, copied into the app's data folder.
    pub backdrop_image: Option<String>,
    /// `light`, `medium` or `strong`.
    pub backdrop_dim: String,
    pub backdrop_blur: bool,
    /// `small`, `medium` or `large`.
    pub ui_scale: String,
    /// `system`, `12` or `24`.
    pub clock_format: String,
    /// Poster elements switched off: `score`, `dub`, `new_episodes`, `progress`, `adult`, `title`,
    /// `details`.
    pub hidden_poster_elements: Vec<String>,
    pub blur_episode_images: bool,
    pub show_series_logos: bool,
    /// Liquid glass per part of the interface, Android's `GlassSection` under its `glass_<part>`
    /// keys (all on by default): the sidebar, Home, title pages, Browse, popups, and Android's
    /// pull-to-refresh spinner (kept for backups; the desktop has none). `glass_pages` is the
    /// desktop's own: every other page. Off, a part's surfaces are a solid fill.
    pub glass_navigation: bool,
    pub glass_home: bool,
    pub glass_details: bool,
    pub glass_browse: bool,
    pub glass_popups: bool,
    pub glass_refresh: bool,
    pub glass_pages: bool,
    /// Home's rows in order, hidden ones prefixed `-` (Android's encoding); `None` is the default.
    pub home_layout: Option<String>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            background: "amoled".into(),
            accent: "monochrome".into(),
            custom_seed: "#8b5cf6".into(),
            style: "vivid".into(),
            contrast: "standard".into(),
            player_accent: false,
            show_colour_pages: true,
            backdrop_source: "none".into(),
            backdrop_anime_url: None,
            backdrop_anime_title: None,
            backdrop_image: None,
            backdrop_dim: "medium".into(),
            backdrop_blur: true,
            ui_scale: "medium".into(),
            clock_format: "system".into(),
            hidden_poster_elements: Vec::new(),
            blur_episode_images: false,
            show_series_logos: true,
            glass_navigation: true,
            glass_home: true,
            glass_details: true,
            glass_browse: true,
            glass_popups: true,
            glass_refresh: true,
            glass_pages: true,
            home_layout: None,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: Audio::Sub,
            subtitle_languages: vec!["en".into()],
            autoplay_next: true,
            auto_skip_intro: false,
            auto_skip_outro: false,
            resume: true,
            show_adult: false,
            preferred_provider: None,
            server_priority: Vec::new(),
            disabled_providers: Vec::new(),
            preferred_source_languages: Vec::new(),
            doh_provider: "auto".into(),
            volume: 100.0,
            sync_anilist: true,
            hardware_decoding: true,
            discord_presence: true,
            download_dir: None,
            download_quality: "best".into(),
            download_hard_subs: false,
            release_notifications: true,
            download_notifications: true,
            show_skip_button: true,
            skip_button_seconds: 85,
            playback_buffer_size: "auto".into(),
            video_cache_size: "auto".into(),
            subtitle_scale: 1.0,
            subtitles_by_default: true,
            subtitles_with_dub: false,
            captions: Captions::default(),
            upscaler: "off".into(),
            pause_details_overlay: true,
            player_clock: true,
            player_content_scale: "fit".into(),
            check_updates_on_launch: true,
            appearance: Appearance::default(),
        }
    }
}

/// An episode number as people write it: `5`, `12.5`.
pub fn episode_number(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }
}

const SETTINGS_KEY: &str = "settings";
pub const TOKEN_KEY: &str = "anilist_token";
pub const MAL_TOKENS_KEY: &str = "mal_tokens";

impl AppState {
    pub fn new<R: tauri::Runtime>(app: &tauri::App<R>) -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir = app.path().app_data_dir()?;
        let store = Arc::new(Store::open(&data_dir.join("anilili.db"))?);
        let http = Http::new()?;
        let proxy = tauri::async_runtime::block_on(Proxy::start(http.clone()))?;
        let browser: Arc<dyn anilili_core::plugin::BrowserFetch> = Arc::new(crate::clearance::AppBrowser { app: app.handle().clone() });
        let plugins = Arc::new(tauri::async_runtime::block_on(Plugins::load(
            http.clone(),
            &plugin_dirs(app),
            Some(browser.clone()),
            Some(store.clone()),
            wasm_cache_dir(app).as_deref(),
        ))?);
        let anilist = AniList::new(http.clone());
        let token: Option<String> = store.setting(TOKEN_KEY);
        anilist.set_token(token.filter(|t| !token_expired(t)));
        let settings: Settings = store.setting(SETTINGS_KEY).unwrap_or_default();
        anilili_core::dns::set_mode(anilili_core::dns::Mode::from_stored(&settings.doh_provider));
        let mal = Mal::new(http.clone());
        let mal_tokens: Option<MalTokens> = store.setting(MAL_TOKENS_KEY);
        let simkl = anilili_core::simkl::Simkl::new(http.clone(), app.package_info().version.to_string());
        let simkl_tokens = store.setting(crate::simkl::SIMKL_TOKENS_KEY);
        tracing::info!(data = %data_dir.display(), "app state ready");
        Ok(Self {
            miruro: Miruro::new(http.clone()),
            media_index: MediaIndex::new(http.clone()),
            aniskip: anilili_core::aniskip::AniSkip::new(http.clone()),
            anilist,
            http,
            store,
            proxy,
            plugins: RwLock::new(plugins),
            browser,
            discord: Discord::start(),
            settings: RwLock::new(settings),
            viewer: RwLock::new(None),
            mal,
            mal_tokens: tokio::sync::Mutex::new(mal_tokens),
            mal_user: RwLock::new(None),
            mal_pending: Mutex::new(None),
            simkl,
            simkl_tokens: tokio::sync::Mutex::new(simkl_tokens),
            simkl_login: Mutex::new(None),
            titles: Mutex::new(HashMap::new()),
            sources: Arc::new(Mutex::new(HashMap::new())),
            speller: tokio::sync::Mutex::new(None),
            index_titles: tokio::sync::Mutex::new(None),
        })
    }

    /// A valid MAL access token, refreshing (and storing the rotated pair) when near expiry.
    /// A failed refresh keeps the session unless the old token is already dead.
    pub async fn mal_access_token(&self) -> Option<String> {
        let mut guard = self.mal_tokens.lock().await;
        let tokens = guard.clone()?;
        if !tokens.needs_refresh() {
            return Some(tokens.access_token);
        }
        match self.mal.refresh(&tokens).await {
            Ok(fresh) => {
                let _ = self.store.set_setting(MAL_TOKENS_KEY, &fresh);
                let token = fresh.access_token.clone();
                *guard = Some(fresh);
                Some(token)
            }
            Err(e) => {
                tracing::warn!(error = %e, "MAL token refresh failed");
                if tokens.expired() {
                    *guard = None;
                    let _ = self.store.delete_setting(MAL_TOKENS_KEY);
                    None
                } else {
                    Some(tokens.access_token)
                }
            }
        }
    }

    /// Whether a list service is signed in to take progress.
    pub async fn signed_in(&self) -> bool {
        self.anilist.has_token() || self.mal_tokens.lock().await.is_some()
    }

    /// Pushes "watched up to `episode`" to whichever account is signed in (AniList first). Only
    /// [`crate::progress_queue`] calls this, which keeps the push until it succeeds.
    ///
    /// A `deferred` push waited in the queue, so the account may have moved past it since (the
    /// episode after it watched on the phone): AniList is asked first, and an entry already at or
    /// beyond it is left alone rather than wound back.
    pub async fn deliver_progress(&self, anime_id: i64, episode: i32, deferred: bool) -> anilili_core::Result<()> {
        let total = self.store.card(anime_id).and_then(|c| c.episodes);
        let finished = total == Some(episode);
        if self.anilist.has_token() {
            if deferred && self.anilist.entry_progress(anime_id).await?.is_some_and(|p| p >= episode) {
                tracing::info!(anime_id, episode, "AniList already past queued progress");
                return Ok(());
            }
            let status = if finished { "COMPLETED" } else { "CURRENT" };
            self.anilist.save_progress(anime_id, episode, Some(status)).await?;
            tracing::info!(anime_id, episode, deferred, "AniList progress saved");
        } else if let Some(token) = self.mal_access_token().await {
            let Some(mal_id) = self.anilist.mal_id(anime_id).await? else { return Ok(()) };
            let status = if finished { "completed" } else { "watching" };
            self.mal.update(&token, mal_id, Some(status), Some(episode)).await?;
            tracing::info!(anime_id, mal_id, episode, deferred, "MAL progress saved");
        }
        Ok(())
    }

    pub fn http_client(&self) -> Http {
        self.http.clone()
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn save_settings(&self, settings: Settings) -> anilili_core::Result<()> {
        self.store.set_setting(SETTINGS_KEY, &settings)?;
        anilili_core::dns::set_mode(anilili_core::dns::Mode::from_stored(&settings.doh_provider));
        *self.settings.write().unwrap() = settings;
        Ok(())
    }
}

/// Where provider modules load from: bundled first, then the user's (updates win when newer).
pub fn plugin_dirs<R: tauri::Runtime, M: Manager<R>>(app: &M) -> Vec<std::path::PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        dirs.push(dir.join("providers"));
    }
    #[cfg(debug_assertions)]
    dirs.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/providers"));
    if let Ok(dir) = app.path().app_data_dir() {
        dirs.push(dir.join("providers"));
    }
    dirs
}

/// Where the providers' compiled code is kept between launches (`Plugins::load`).
pub fn wasm_cache_dir<R: tauri::Runtime, M: Manager<R>>(app: &M) -> Option<std::path::PathBuf> {
    app.path().app_cache_dir().ok().map(|dir| dir.join("wasm"))
}

/// AniList tokens are JWTs; `exp` says when to stop sending one.
pub fn token_expired(token: &str) -> bool {
    use base64_lite::decode_url;
    let Some(payload) = token.split('.').nth(1) else { return false };
    let Some(bytes) = decode_url(payload) else { return false };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return false };
    value.get("exp").and_then(|e| e.as_f64()).is_some_and(|exp| (exp as i64) < anilili_core::store::now())
}

/// Just enough base64url for reading a JWT payload, rather than a dependency.
mod base64_lite {
    pub fn decode_url(input: &str) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(input.len() * 3 / 4);
        let mut buffer = 0u32;
        let mut bits = 0;
        for c in input.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'-' | b'+' => 62,
                b'_' | b'/' => 63,
                b'=' => break,
                _ => return None,
            } as u32;
            buffer = (buffer << 6) | v;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((buffer >> bits) as u8);
                buffer &= (1 << bits) - 1;
            }
        }
        Some(out)
    }
}

/// A failure as the UI receives it: the catalogue code and message, the next step the UI offers,
/// and the technical detail it shows behind **Details**. Building one logs it under its code.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: String,
    pub message: String,
    pub detail: String,
    pub next: anilili_core::catalogue::Next,
}

impl From<anilili_core::Failure> for CommandError {
    fn from(failure: anilili_core::Failure) -> Self {
        failure.log();
        Self { code: failure.code(), message: failure.entry.message.to_owned(), detail: failure.detail, next: failure.entry.next }
    }
}

impl From<anilili_core::Error> for CommandError {
    fn from(e: anilili_core::Error) -> Self {
        e.failure().into()
    }
}

/// A catalogue entry with no further detail.
impl From<&'static anilili_core::catalogue::Entry> for CommandError {
    fn from(entry: &'static anilili_core::catalogue::Entry) -> Self {
        anilili_core::Failure::new(entry, "").into()
    }
}

/// An unexpected failure with only a technical description: the generic entry, with the text
/// as its detail. Prefer a catalogue entry ([`fail`]) wherever the failure is foreseeable.
impl From<String> for CommandError {
    fn from(detail: String) -> Self {
        anilili_core::Failure::new(anilili_core::catalogue::APP_GENERIC, detail).into()
    }
}

impl From<&str> for CommandError {
    fn from(detail: &str) -> Self {
        detail.to_owned().into()
    }
}

/// A catalogue failure with technical detail: `Err(fail(DL_FILE_GONE, path))`.
pub fn fail(entry: &'static anilili_core::catalogue::Entry, detail: impl Into<String>) -> CommandError {
    anilili_core::Failure::new(entry, detail).into()
}

/// Re-labels a failure with the entry that says *what* failed (`NET_HOME`), keeping the lower
/// layer's code and detail, except where the failure itself is more telling (a rate limit, a
/// browser check, an unreadable response).
pub fn as_entry(entry: &'static anilili_core::catalogue::Entry) -> impl Fn(anilili_core::Error) -> CommandError {
    move |e| {
        use anilili_core::catalogue as c;
        let mut failure = e.failure();
        if ![c::NET_RATE_LIMITED, c::NET_CHALLENGE, c::NET_CERTIFICATE, c::API_UNREADABLE, c::DB_FAILED].contains(&failure.entry) {
            failure.entry = entry;
        }
        failure.into()
    }
}

pub type CmdResult<T> = Result<T, CommandError>;

/// A failure with no command to hand it back to (a sign-in window, a background sync, a
/// download): already logged when it was built; this shows it to the viewer through the
/// `failure` event, which the UI turns into a toast.
pub fn report<R: tauri::Runtime>(app: &tauri::AppHandle<R>, failure: CommandError) {
    use tauri::Emitter;
    let _ = app.emit("failure", &failure);
}
