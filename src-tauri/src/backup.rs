//! Backup and restore, in the file format the Android app reads and writes (`anilili_core::backup`).
//!
//! What travels: settings (the ones both apps have, under Android's key names; the desktop's own
//! under `desktop.`), the library (as Android's saved list, each entry also carrying its status),
//! Continue Watching (one row per title: the episode in progress, the ones finished, and where each
//! part-watched episode was left), and, with a passphrase, the AniList login.
//!
//! A restore replaces the settings it carries and **merges** the rest: a title saved on either
//! device stays saved (the local status wins when both have one), and a progress row only replaces
//! an older one. Logins are opened before anything is written, so a wrong passphrase changes
//! nothing. Downloads never travel: they are files on this disk.
//!
//! Not carried yet: a MyAnimeList hand-off *from* the desktop (it needs a sign-in at export time;
//! one made on Android is redeemed here), search history and custom lists (the desktop has
//! neither), and OpenSubtitles (the desktop doesn't use it).

use std::collections::BTreeMap;
use std::path::PathBuf;

use anilili_core::backup::{self, Credentials, Document, EpisodeResume, HistoryEntry, WatchlistEntry};
use anilili_core::catalogue as c;
use anilili_core::model::{AnimeCard, Audio, Progress};
use serde::Serialize;
use serde_json::{Map, Value, json};
use tauri::{AppHandle, Manager, Runtime, State};

use crate::app::{AppState, Appearance, Captions, CmdResult, Settings, TOKEN_KEY, fail};
use crate::player::Player;

/// What a backup file holds, shown before restoring it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// Unix milliseconds.
    pub created_at: i64,
    pub app_version: String,
    pub summary: String,
    /// Logins sealed in the file (`anilist`, `mal`, `opensubtitles`).
    pub services: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub summary: String,
    /// Parts that didn't land while the rest did (a MAL hand-off MAL refused, say).
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    pub path: String,
    pub summary: String,
}

#[tauri::command]
pub fn backup_suggest_passphrase() -> String {
    backup::suggest_passphrase()
}

/// Writes a backup to Downloads. With a passphrase, the AniList login is sealed into it.
#[tauri::command]
pub async fn backup_export<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, passphrase: Option<String>) -> CmdResult<Exported> {
    let mut document = Document {
        created_at: anilili_core::store::now() * 1000,
        app_version: format!("desktop {}", app.package_info().version),
        settings: settings_out(&state.settings()),
        watchlist: watchlist_out(&state)?,
        history: history_out(&state)?,
        ..Document::default()
    };
    if let Some(passphrase) = passphrase.filter(|p| !p.is_empty()) {
        // The OpenSubtitles login travels as Android's does (its username and password, sealed).
        let open_subtitles = crate::opensubtitles::saved_login(&state);
        let credentials = Credentials {
            anilist_token: state.store.setting::<String>(TOKEN_KEY),
            open_subtitles_username: open_subtitles.as_ref().map(|(user, _)| user.clone()),
            open_subtitles_password: open_subtitles.map(|(_, password)| password),
            ..Credentials::default()
        };
        // Sealing derives the key with 210,000 HMAC rounds: off the async threads.
        document = tokio::task::spawn_blocking(move || {
            let mut document = document;
            document.seal(&passphrase, &credentials).map(|()| document)
        })
        .await
        .map_err(|e| fail(c::BAK_WRITE, e.to_string()))??;
    }
    let dir = app.path().download_dir().map_err(|e| fail(c::BAK_WRITE, e.to_string()))?;
    let path = unique_path(&dir, &format!("Anilili-backup-{}", date_stamp(anilili_core::store::now())));
    std::fs::write(&path, backup::encode(&document)).map_err(|e| fail(c::BAK_WRITE, format!("{}: {e}", path.display())))?;
    tracing::info!(path = %path.display(), summary = %document.describe(), "backup written");
    Ok(Exported { path: path.to_string_lossy().into_owned(), summary: document.describe() })
}

/// Reads a backup's header and counts, without applying anything.
#[tauri::command(async)]
pub fn backup_inspect(text: String) -> CmdResult<BackupInfo> {
    let document = backup::decode(text.as_bytes())?;
    Ok(BackupInfo {
        created_at: document.created_at,
        app_version: document.app_version.clone(),
        summary: document.describe(),
        services: document.credentials.as_ref().map(|c| c.services.clone()).unwrap_or_default(),
    })
}

/// Applies a backup: settings replaced, library and progress merged, logins adopted.
#[tauri::command]
pub async fn backup_restore<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    text: String,
    passphrase: Option<String>,
) -> CmdResult<Restored> {
    let document = backup::decode(text.as_bytes())?;
    let mut warnings = Vec::new();
    // Opened before anything is written: a wrong passphrase must not leave a half-restore.
    let credentials = match passphrase.filter(|p| !p.is_empty()) {
        Some(passphrase) => {
            let sealed = document.clone();
            tokio::task::spawn_blocking(move || sealed.open(&passphrase)).await.map_err(|e| fail(c::BAK_UNREADABLE, e.to_string()))??
        }
        None => {
            if document.credentials.is_some() {
                warnings.push("The logins in this backup stayed sealed: no passphrase was given.".to_owned());
            }
            None
        }
    };

    let (settings, settings_count) = settings_in(&document.settings, state.settings());
    if settings_count > 0 {
        if let Some(player) = app.try_state::<Player>() {
            player.apply_settings(&settings);
        }
        if !settings.discord_presence {
            state.discord.clear();
        }
        state.save_settings(settings)?;
    }
    let saved = watchlist_in(&state, &document.watchlist)?;
    let history = history_in(&state, &document.history)?;

    let mut logins = 0;
    if let Some(credentials) = credentials {
        if credentials.has_anilist() {
            crate::library::adopt_anilist_token(&app, credentials.anilist_token.clone().unwrap_or_default()).await;
            logins += 1;
        }
        if credentials.has_mal_handoff() {
            let (code, verifier) = (credentials.mal_code.clone().unwrap_or_default(), credentials.mal_verifier.clone().unwrap_or_default());
            match state.mal.exchange_code(&code, &verifier).await {
                Ok(tokens) => {
                    crate::library::adopt_mal_tokens(&app, tokens).await;
                    logins += 1;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "MAL hand-off from a backup was refused");
                    // MAL never says whether a code was spent or expired; its age answers that.
                    let age = if credentials.mal_issued_at > 0 {
                        format!(" It was made {}.", ago(credentials.mal_issued_at / 1000))
                    } else {
                        String::new()
                    };
                    warnings.push(format!(
                        "MyAnimeList refused the sign-in in this backup.{age} It works once and expires quickly: sign in from Settings instead."
                    ));
                }
            }
        }
        if credentials.has_open_subtitles() {
            let (user, password) =
                (credentials.open_subtitles_username.clone().unwrap_or_default(), credentials.open_subtitles_password.clone().unwrap_or_default());
            // Kept as given; the next subtitle download signs in with it, as Android adopts it.
            match crate::opensubtitles::remember(&state, &user, &password) {
                Ok(()) => logins += 1,
                Err(e) => warnings.push(format!("The OpenSubtitles login couldn't be saved: {}", e.message)),
            }
        }
    }
    if !document.custom_lists.is_empty() {
        warnings.push(format!("{} custom list(s) were left out: the desktop app has no custom lists yet.", document.custom_lists.len()));
    }
    let summary = backup::describe_counts(&[
        (settings_count, "setting", "settings"),
        (saved, "saved title", "saved titles"),
        (history, "title's progress", "titles' progress"),
        (logins, "login", "logins"),
    ]);
    tracing::info!(%summary, from = %document.app_version, "backup restored");
    Ok(Restored { summary, warnings })
}

// ---- settings -----------------------------------------------------------------------------

/// ISO 639-1 (the desktop's) to 639-2 (Android's stored subtitle language), for the languages
/// Android offers.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", "eng"),
    ("ja", "jpn"),
    ("es", "spa"),
    ("pt", "por"),
    ("ar", "ara"),
    ("fr", "fra"),
    ("de", "deu"),
    ("it", "ita"),
    ("tr", "tur"),
    ("ru", "rus"),
    ("pl", "pol"),
    ("id", "ind"),
    ("hi", "hin"),
    ("zh", "zho"),
    ("ko", "kor"),
];

fn settings_out(s: &Settings) -> Map<String, Value> {
    let to_android = |code: &String| LANGUAGES.iter().find(|(one, _)| one == code).map(|(_, two)| *two);
    let mut out = Map::new();
    out.insert("autoplay".into(), json!(s.autoplay_next));
    out.insert("prefer_dub".into(), json!(s.audio == Audio::Dub));
    out.insert("auto_skip_intro_outro".into(), json!(s.auto_skip_intro));
    out.insert("hide_adult_content".into(), json!(!s.show_adult));
    out.insert("auto_sync_anilist".into(), json!(s.sync_anilist));
    out.insert("discord_rich_presence".into(), json!(s.discord_presence));
    out.insert("release_notifications".into(), json!(s.release_notifications));
    out.insert("show_skip_button".into(), json!(s.show_skip_button));
    out.insert("skip_button_seconds".into(), json!(s.skip_button_seconds));
    out.insert("playback_buffer_size".into(), json!(s.playback_buffer_size));
    out.insert("video_cache_size".into(), json!(s.video_cache_size));
    out.insert("download_quality".into(), json!(s.download_quality));
    out.insert("download_hard_subs_only".into(), json!(s.download_hard_subs));
    out.insert("pause_details_overlay".into(), json!(s.pause_details_overlay));
    out.insert("player_clock".into(), json!(s.player_clock));
    out.insert("update_check_on_launch".into(), json!(s.check_updates_on_launch));
    out.insert("player_content_scale".into(), json!(s.player_content_scale));
    if let Some(first) = s.subtitle_languages.first().and_then(to_android) {
        out.insert("preferred_subtitle_language".into(), json!(first));
    }
    if let Some(second) = s.subtitle_languages.get(1).and_then(to_android) {
        out.insert("secondary_subtitle_language".into(), json!(second));
    }
    // The desktop's own, which Android doesn't read. The download folder is a path on this
    // computer, so it stays behind.
    out.insert("desktop.subtitle_languages".into(), json!(s.subtitle_languages));
    out.insert("desktop.auto_skip_outro".into(), json!(s.auto_skip_outro));
    out.insert("desktop.resume".into(), json!(s.resume));
    out.insert("desktop.preferred_provider".into(), json!(s.preferred_provider));
    out.insert("desktop.volume".into(), json!(s.volume));
    out.insert("desktop.hardware_decoding".into(), json!(s.hardware_decoding));
    out.insert("desktop.subtitle_scale".into(), json!(s.subtitle_scale));
    out.insert("desktop.upscaler".into(), json!(s.upscaler));
    out.insert("desktop.download_notifications".into(), json!(s.download_notifications));
    out.insert("desktop.server_priority".into(), json!(s.server_priority));
    out.insert("desktop.disabled_providers".into(), json!(s.disabled_providers));
    // Android's own key and encoding: its codes, comma-separated.
    out.insert("preferred_source_languages".into(), json!(s.preferred_source_languages.join(",")));
    out.insert("doh_provider".into(), json!(s.doh_provider));
    out.insert("subtitles_by_default".into(), json!(s.subtitles_by_default));
    out.insert("subtitles_with_dub".into(), json!(s.subtitles_with_dub));
    captions_out(&s.captions, s.subtitle_scale, &mut out);
    appearance_out(&s.appearance, &mut out);
    out
}

/// Android's caption keys (`caption_*`, `CaptionStyle.kt`'s values). The size is the desktop's
/// subtitle scale as Android's percentage. The margin stays the desktop's own: Android measures it
/// against a phone's caption view, and its default would lift these well up the picture.
fn captions_out(c: &Captions, scale: f64, out: &mut Map<String, Value>) {
    out.insert("caption_font".into(), json!(c.font));
    out.insert("caption_bold_text".into(), json!(c.bold));
    out.insert("caption_text_color".into(), json!(c.text_color));
    out.insert("caption_text_opacity".into(), json!(c.text_opacity));
    out.insert("caption_background_color".into(), json!(c.background_color));
    out.insert("caption_background_opacity".into(), json!(c.background_opacity));
    out.insert("caption_edge_style".into(), json!(c.edge_style));
    out.insert("caption_position".into(), json!(c.position));
    out.insert("caption_horizontal_anchor".into(), json!(c.horizontal_anchor));
    out.insert("caption_text_scale".into(), json!((scale * 100.0).round().clamp(50.0, 200.0) as i64));
    out.insert("desktop.caption_margin".into(), json!(c.edge_margin));
}

/// [`captions_out`] backwards; a value Android doesn't offer is skipped.
fn captions_in(map: &Map<String, Value>, c: &mut Captions) -> usize {
    let mut applied = 0;
    let mut choice = |key: &str, allowed: &[&str], set: &mut dyn FnMut(String)| {
        if let Some(v) = map.get(key).and_then(Value::as_str).filter(|v| allowed.contains(v)) {
            set(v.to_owned());
            applied += 1;
        }
    };
    choice("caption_font", CAPTION_FONTS, &mut |v| c.font = v);
    choice("caption_text_color", &["white", "yellow", "cyan", "green", "magenta"], &mut |v| c.text_color = v);
    choice("caption_background_color", &["black", "gray", "navy", "white"], &mut |v| c.background_color = v);
    choice("caption_edge_style", &["none", "outline", "shadow"], &mut |v| c.edge_style = v);
    choice("caption_position", &["bottom", "top"], &mut |v| c.position = v);
    let mut percent = |key: &str, set: &mut dyn FnMut(u8)| {
        if let Some(v) = map.get(key).and_then(Value::as_u64) {
            set(v.min(100) as u8);
            applied += 1;
        }
    };
    percent("caption_text_opacity", &mut |v| c.text_opacity = v);
    percent("caption_background_opacity", &mut |v| c.background_opacity = v);
    percent("desktop.caption_margin", &mut |v| c.edge_margin = v);
    percent("caption_horizontal_anchor", &mut |v| c.horizontal_anchor = v);
    if let Some(v) = map.get("caption_bold_text").and_then(Value::as_bool) {
        c.bold = v;
        applied += 1;
    }
    applied
}

const CAPTION_FONTS: &[&str] = &["sans", "serif", "mono", "condensed", "casual", "cursive"];

/// Android's glass parts (`GlassSection`, stored as `glass_<name lowercased>`).
fn glass_parts(a: &Appearance) -> [(&'static str, bool); 6] {
    [
        ("navigation", a.glass_navigation),
        ("home", a.glass_home),
        ("details", a.glass_details),
        ("browse", a.glass_browse),
        ("popups", a.glass_popups),
        ("refresh", a.glass_refresh),
    ]
}

/// The Appearance keys, as Android stores them. Its own background picture is a file on this
/// computer, so it stays behind (as Android's does).
fn appearance_out(a: &Appearance, out: &mut Map<String, Value>) {
    out.insert("theme_background".into(), json!(a.background));
    out.insert("theme_accent".into(), json!(a.accent));
    if let Some(argb) = seed_to_argb(&a.custom_seed) {
        out.insert("theme_custom_seed".into(), json!(argb));
    }
    out.insert("theme_style".into(), json!(a.style));
    out.insert("theme_contrast".into(), json!(a.contrast));
    out.insert("player_accent".into(), json!(a.player_accent));
    out.insert("show_colour_pages".into(), json!(a.show_colour_pages));
    let source = if a.backdrop_source == "image" { "none" } else { a.backdrop_source.as_str() };
    out.insert("app_backdrop_source".into(), json!(source));
    if let Some(url) = &a.backdrop_anime_url {
        out.insert("app_backdrop_anime_url".into(), json!(url));
    }
    if let Some(title) = &a.backdrop_anime_title {
        out.insert("app_backdrop_anime_title".into(), json!(title));
    }
    out.insert("app_backdrop_dim".into(), json!(a.backdrop_dim));
    out.insert("app_backdrop_blur".into(), json!(a.backdrop_blur));
    out.insert("ui_scale".into(), json!(a.ui_scale));
    out.insert("clock_format".into(), json!(a.clock_format));
    out.insert("hidden_poster_elements".into(), json!(a.hidden_poster_elements.join(",")));
    out.insert("blur_episode_images".into(), json!(a.blur_episode_images));
    out.insert("show_series_logos".into(), json!(a.show_series_logos));
    for (part, on) in glass_parts(a) {
        out.insert(format!("glass_{part}"), json!(on));
    }
    out.insert("desktop.glass_pages".into(), json!(a.glass_pages));
    if let Some(layout) = &a.home_layout {
        out.insert("home_layout".into(), json!(layout));
    }
}

/// `#8b5cf6` as Android's opaque ARGB int (signed, as Kotlin's Int stores it).
fn seed_to_argb(hex: &str) -> Option<i64> {
    let rgb = u32::from_str_radix(hex.strip_prefix('#')?, 16).ok().filter(|_| hex.len() == 7)?;
    Some(i64::from((0xFF00_0000 | rgb) as i32))
}

fn argb_to_seed(argb: i64) -> String {
    format!("#{:06x}", argb as u32 & 0x00FF_FFFF)
}

/// The Appearance keys a backup carries; returns how many applied. Values outside what each
/// option offers are skipped, as Android skips them.
fn appearance_in(map: &Map<String, Value>, a: &mut Appearance) -> usize {
    let mut applied = 0;
    let mut choice = |key: &str, allowed: &[&str], set: &mut dyn FnMut(String)| {
        if let Some(v) = map.get(key).and_then(Value::as_str).filter(|v| allowed.contains(v)) {
            set(v.to_owned());
            applied += 1;
        }
    };
    choice("theme_background", &["amoled", "dark"], &mut |v| a.background = v);
    // "From wallpaper" is Android 12's; the desktop has no wallpaper colour to read.
    let accents = [
        "monochrome",
        "custom",
        "purple",
        "neon_green",
        "blood_red",
        "ocean_blue",
        "cyan",
        "sakura_pink",
        "sunset_orange",
        "gold",
        "emerald",
    ];
    choice("theme_accent", &accents, &mut |v| a.accent = v);
    choice("theme_style", &["vivid", "tonal", "neutral", "vibrant"], &mut |v| a.style = v);
    choice("theme_contrast", &["standard", "medium", "high"], &mut |v| a.contrast = v);
    choice("app_backdrop_source", &["none", "anime"], &mut |v| a.backdrop_source = v);
    choice("app_backdrop_dim", &["light", "medium", "strong"], &mut |v| a.backdrop_dim = v);
    choice("ui_scale", &["small", "medium", "large"], &mut |v| a.ui_scale = v);
    choice("clock_format", &["system", "12", "24"], &mut |v| a.clock_format = v);
    let mut flag = |key: &str, set: &mut dyn FnMut(bool)| {
        if let Some(v) = map.get(key).and_then(Value::as_bool) {
            set(v);
            applied += 1;
        }
    };
    flag("player_accent", &mut |v| a.player_accent = v);
    flag("show_colour_pages", &mut |v| a.show_colour_pages = v);
    flag("app_backdrop_blur", &mut |v| a.backdrop_blur = v);
    flag("blur_episode_images", &mut |v| a.blur_episode_images = v);
    flag("show_series_logos", &mut |v| a.show_series_logos = v);
    flag("glass_navigation", &mut |v| a.glass_navigation = v);
    flag("glass_home", &mut |v| a.glass_home = v);
    flag("glass_details", &mut |v| a.glass_details = v);
    flag("glass_browse", &mut |v| a.glass_browse = v);
    flag("glass_popups", &mut |v| a.glass_popups = v);
    flag("glass_refresh", &mut |v| a.glass_refresh = v);
    flag("desktop.glass_pages", &mut |v| a.glass_pages = v);
    if let Some(argb) = map.get("theme_custom_seed").and_then(Value::as_i64) {
        a.custom_seed = argb_to_seed(argb);
        applied += 1;
    }
    let text = |key: &str| map.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()).map(str::to_owned);
    if let Some(url) = text("app_backdrop_anime_url") {
        a.backdrop_anime_url = Some(url);
        a.backdrop_anime_title = text("app_backdrop_anime_title");
        applied += 1;
    }
    if let Some(hidden) = map.get("hidden_poster_elements").and_then(Value::as_str) {
        a.hidden_poster_elements = hidden.split(',').map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned).collect();
        applied += 1;
    }
    if let Some(layout) = text("home_layout") {
        a.home_layout = Some(layout);
        applied += 1;
    }
    applied
}

/// The backup's settings over `current`, and how many applied. A key of the wrong type is skipped,
/// as Android skips it; keys the file doesn't carry keep this computer's value.
fn settings_in(map: &Map<String, Value>, mut s: Settings) -> (Settings, usize) {
    let mut applied = 0;
    let mut bool_of = |key: &str, set: &mut dyn FnMut(bool)| {
        if let Some(v) = map.get(key).and_then(Value::as_bool) {
            set(v);
            applied += 1;
        }
    };
    bool_of("autoplay", &mut |v| s.autoplay_next = v);
    bool_of("prefer_dub", &mut |v| s.audio = if v { Audio::Dub } else { Audio::Sub });
    bool_of("auto_skip_intro_outro", &mut |v| {
        s.auto_skip_intro = v;
        s.auto_skip_outro = v;
    });
    bool_of("hide_adult_content", &mut |v| s.show_adult = !v);
    bool_of("auto_sync_anilist", &mut |v| s.sync_anilist = v);
    bool_of("discord_rich_presence", &mut |v| s.discord_presence = v);
    bool_of("desktop.auto_skip_outro", &mut |v| s.auto_skip_outro = v);
    bool_of("desktop.resume", &mut |v| s.resume = v);
    bool_of("desktop.hardware_decoding", &mut |v| s.hardware_decoding = v);
    bool_of("release_notifications", &mut |v| s.release_notifications = v);
    bool_of("show_skip_button", &mut |v| s.show_skip_button = v);
    bool_of("pause_details_overlay", &mut |v| s.pause_details_overlay = v);
    bool_of("player_clock", &mut |v| s.player_clock = v);
    bool_of("update_check_on_launch", &mut |v| s.check_updates_on_launch = v);
    bool_of("desktop.download_notifications", &mut |v| s.download_notifications = v);

    let to_desktop = |code: &str| LANGUAGES.iter().find(|(_, two)| *two == code).map(|(one, _)| one.to_string());
    if let Some(list) = map.get("desktop.subtitle_languages").and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok()) {
        s.subtitle_languages = list;
        applied += 1;
    } else {
        let first = map.get("preferred_subtitle_language").and_then(Value::as_str).and_then(to_desktop);
        let second = map.get("secondary_subtitle_language").and_then(Value::as_str).and_then(to_desktop);
        if first.is_some() || second.is_some() {
            s.subtitle_languages = first.into_iter().chain(second).collect();
            applied += 1;
        }
    }
    if let Some(v) = map.get("desktop.preferred_provider").filter(|v| v.is_null() || v.is_string()) {
        s.preferred_provider = v.as_str().map(str::to_owned);
        applied += 1;
    }
    if let Some(v) = map.get("desktop.volume").and_then(Value::as_f64) {
        s.volume = v.clamp(0.0, 130.0);
        applied += 1;
    }
    if let Some(v) = map.get("desktop.subtitle_scale").and_then(Value::as_f64) {
        s.subtitle_scale = v.clamp(0.5, 3.0);
        applied += 1;
    }
    if let Some(v) = map.get("desktop.upscaler").and_then(Value::as_str).filter(|v| matches!(*v, "off" | "fast" | "quality")) {
        s.upscaler = v.to_owned();
        applied += 1;
    }
    if let Some(v) = map.get("player_content_scale").and_then(Value::as_str).filter(|v| crate::player::CONTENT_SCALES.contains(v)) {
        s.player_content_scale = v.to_owned();
        applied += 1;
    }
    if let Some(v) = map.get("skip_button_seconds").and_then(Value::as_u64) {
        s.skip_button_seconds = v.clamp(5, 180) as u32;
        applied += 1;
    }
    if let Some(v) = map.get("playback_buffer_size").and_then(Value::as_str).filter(|v| crate::player::BUFFER_SIZES.contains(v)) {
        s.playback_buffer_size = v.to_owned();
        applied += 1;
    }
    if let Some(v) = map.get("video_cache_size").and_then(Value::as_str).filter(|v| crate::player::VIDEO_CACHE_SIZES.contains(v)) {
        s.video_cache_size = v.to_owned();
        applied += 1;
    }
    if let Some(v) = map.get("download_quality").and_then(Value::as_str).filter(|v| matches!(*v, "best" | "1080" | "720" | "480" | "360")) {
        s.download_quality = v.to_owned();
        applied += 1;
    }
    for (key, field) in [
        ("subtitles_by_default", &mut s.subtitles_by_default),
        ("subtitles_with_dub", &mut s.subtitles_with_dub),
        ("download_hard_subs_only", &mut s.download_hard_subs),
    ] {
        if let Some(v) = map.get(key).and_then(Value::as_bool) {
            *field = v;
            applied += 1;
        }
    }
    if !map.contains_key("desktop.subtitle_scale")
        && let Some(v) = map.get("caption_text_scale").and_then(Value::as_f64)
    {
        s.subtitle_scale = (v / 100.0).clamp(0.5, 2.0);
        applied += 1;
    }
    let list = |key: &str| map.get(key).and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok());
    if let Some(order) = list("desktop.server_priority") {
        s.server_priority = order;
        applied += 1;
    }
    if let Some(off) = list("desktop.disabled_providers") {
        s.disabled_providers = off;
        applied += 1;
    }
    // Android's values, all known to the desktop; an unknown one would read as Automatic anyway.
    if let Some(v) = map.get("doh_provider").and_then(Value::as_str).filter(|v| *v == "auto" || *v == "none" || anilili_core::dns::PROVIDERS.iter().any(|p| p.id == *v)) {
        s.doh_provider = v.to_owned();
        applied += 1;
    }
    if let Some(languages) = map.get("preferred_source_languages").and_then(Value::as_str) {
        s.preferred_source_languages = languages
            .split(',')
            .map(str::trim)
            .filter(|code| code.len() == 3 && code.chars().all(|c| c.is_ascii_lowercase()))
            .map(str::to_owned)
            .collect();
        applied += 1;
    }
    applied += captions_in(map, &mut s.captions);
    applied += appearance_in(map, &mut s.appearance);
    (s, applied)
}

// ---- library --------------------------------------------------------------------------------

fn watchlist_out(state: &AppState) -> CmdResult<Vec<WatchlistEntry>> {
    Ok(state
        .store
        .library()?
        .into_iter()
        .map(|entry| WatchlistEntry {
            anilist_id: entry.anime.id,
            title: entry.anime.title.clone(),
            cover: entry.anime.cover.clone(),
            format: entry.anime.format.clone(),
            average_score: entry.anime.score,
            added_at: entry.updated_at * 1000,
            status: Some(entry.status),
        })
        .collect())
}

/// Saved titles this computer doesn't have yet; a title saved on both keeps its local status.
fn watchlist_in(state: &AppState, entries: &[WatchlistEntry]) -> CmdResult<usize> {
    let mut added = 0;
    for entry in entries.iter().filter(|e| e.anilist_id > 0) {
        if state.store.library_status(entry.anilist_id).is_some() {
            continue;
        }
        let card = card_for(state, entry.anilist_id, &entry.title, entry.cover.clone(), entry.format.clone(), entry.average_score);
        let status = entry.status.as_deref().filter(|s| crate::library::to_anilist_status(s).is_some()).unwrap_or("planning");
        state.store.library_set(&card, Some(status))?;
        added += 1;
    }
    Ok(added)
}

/// The stored card, or one built from what the backup says about the title.
fn card_for(state: &AppState, id: i64, title: &str, cover: Option<String>, format: Option<String>, score: Option<i32>) -> AnimeCard {
    state.store.card(id).unwrap_or(AnimeCard {
        id,
        title: if title.is_empty() { format!("#{id}") } else { title.to_owned() },
        title_romaji: None,
        cover,
        color: None,
        banner: None,
        format,
        status: None,
        episodes: None,
        score,
        year: None,
        season: None,
        genres: vec![],
        next_airing: None,
        is_adult: false,
    })
}

// ---- progress -------------------------------------------------------------------------------

fn history_out(state: &AppState) -> CmdResult<Vec<HistoryEntry>> {
    let mut by_title: BTreeMap<i64, Vec<Progress>> = BTreeMap::new();
    for row in state.store.all_progress()? {
        by_title.entry(row.anime_id).or_default().push(row);
    }
    let category = if state.settings().audio == Audio::Dub { "dub" } else { "sub" };
    Ok(by_title
        .into_iter()
        .filter_map(|(id, rows)| {
            let current = rows.iter().max_by_key(|r| r.updated_at)?;
            let card = state.store.card(id);
            let ms = |seconds: f64| (seconds * 1000.0).round() as i64;
            Some(HistoryEntry {
                anilist_id: id,
                title: card.as_ref().map(|c| c.title.clone()).unwrap_or_default(),
                cover: card.as_ref().and_then(|c| c.cover.clone()),
                episode_number: current.episode,
                episode_title: None,
                provider: String::new(),
                category: category.into(),
                position_ms: ms(current.position),
                duration_ms: ms(current.duration),
                updated_at: current.updated_at * 1000,
                watched_episodes: rows.iter().filter(|r| r.finished()).map(|r| r.episode).collect(),
                from_remote: false,
                resume_points: rows
                    .iter()
                    .filter(|r| !r.finished() && r.episode != current.episode && r.position > 0.0)
                    .map(|r| (backup::episode_key(r.episode), EpisodeResume { position_ms: ms(r.position), duration_ms: ms(r.duration) }))
                    .collect(),
            })
        })
        .collect())
}

/// Progress rows from the backup that are newer than (or missing from) this computer's.
fn history_in(state: &AppState, entries: &[HistoryEntry]) -> CmdResult<usize> {
    let mut titles = 0;
    for entry in entries.iter().filter(|e| e.anilist_id > 0 && !e.from_remote) {
        let local = state.store.progress_for(entry.anilist_id)?;
        let at = (entry.updated_at / 1000).max(1);
        let seconds = |ms: i64| ms as f64 / 1000.0;
        let mut rows: Vec<Progress> = Vec::new();
        // A finished episode with no known length is stored as 1 of 1: finished, nothing to resume.
        for &episode in &entry.watched_episodes {
            rows.push(Progress { anime_id: entry.anilist_id, episode, position: 1.0, duration: 1.0, updated_at: at - 1 });
        }
        for (key, point) in &entry.resume_points {
            if let Ok(episode) = key.parse::<f64>() {
                rows.push(Progress {
                    anime_id: entry.anilist_id,
                    episode,
                    position: seconds(point.position_ms),
                    duration: seconds(point.duration_ms),
                    updated_at: at - 1,
                });
            }
        }
        rows.push(Progress {
            anime_id: entry.anilist_id,
            episode: entry.episode_number,
            position: seconds(entry.position_ms),
            duration: seconds(entry.duration_ms),
            updated_at: at,
        });
        let mut wrote = false;
        for row in rows {
            let newer = local.iter().find(|l| l.episode == row.episode).is_none_or(|l| l.updated_at < row.updated_at);
            if newer {
                state.store.save_progress(&row)?;
                wrote = true;
            }
        }
        if wrote {
            if state.store.card(entry.anilist_id).is_none() {
                state.store.remember(&card_for(state, entry.anilist_id, &entry.title, entry.cover.clone(), None, None))?;
            }
            titles += 1;
        }
    }
    Ok(titles)
}

// ---- files and dates ------------------------------------------------------------------------

/// `Anilili-backup-20261004.json`, then `-2`, `-3`… so a second backup that day keeps the first.
fn unique_path(dir: &std::path::Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.json"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem}-{n}.json"))).find(|p| !p.exists()).unwrap_or(first)
}

/// `20261004` (UTC) for unix seconds.
fn date_stamp(secs: i64) -> String {
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}{month:02}{day:02}", yoe + era * 400 + i64::from(month <= 2))
}

/// "3 days ago", roughly, for unix seconds.
fn ago(secs: i64) -> String {
    let d = (anilili_core::store::now() - secs).max(0);
    match d {
        0..=89 => "moments ago".into(),
        90..=5399 => format!("{} minutes ago", (d + 30) / 60),
        5400..=129_599 => format!("{} hours ago", (d + 1800) / 3600),
        _ => format!("{} days ago", (d + 43_200) / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_travel_both_ways() {
        let s = Settings {
            subtitle_languages: vec!["en".into(), "ja".into()],
            audio: Audio::Dub,
            show_adult: true,
            upscaler: "fast".into(),
            player_clock: false,
            player_content_scale: "crop".into(),
            check_updates_on_launch: false,
            ..Settings::default()
        };
        let map = settings_out(&s);
        assert_eq!(map["preferred_subtitle_language"], "eng");
        assert_eq!(map["secondary_subtitle_language"], "jpn");
        assert_eq!(map["hide_adult_content"], false);
        let (back, applied) = settings_in(&map, Settings::default());
        assert_eq!(back.subtitle_languages, ["en", "ja"]);
        assert_eq!(back.audio, Audio::Dub);
        assert!(back.show_adult);
        assert_eq!(back.upscaler, "fast");
        assert!(!back.player_clock);
        assert!(!back.check_updates_on_launch);
        assert_eq!(back.player_content_scale, "crop");
        assert!(applied >= 10);
    }

    #[test]
    fn captions_and_servers_travel_both_ways() {
        let captions = Captions {
            font: "serif".into(),
            bold: true,
            text_color: "yellow".into(),
            text_opacity: 90,
            background_color: "navy".into(),
            background_opacity: 60,
            edge_style: "shadow".into(),
            position: "top".into(),
            edge_margin: 8,
            horizontal_anchor: 25,
        };
        let s = Settings {
            captions: captions.clone(),
            subtitle_scale: 1.25,
            subtitles_by_default: false,
            subtitles_with_dub: true,
            server_priority: vec!["icarus".into(), "megaplay".into()],
            disabled_providers: vec!["animegg".into()],
            preferred_source_languages: vec!["eng".into(), "ara".into()],
            doh_provider: "quad9".into(),
            ..Settings::default()
        };
        let map = settings_out(&s);
        // Android's own keys and values, so the phone reads them too.
        assert_eq!(map["caption_font"], "serif");
        assert_eq!(map["caption_text_scale"], 125);
        assert_eq!(map["subtitles_by_default"], false);
        let (back, _) = settings_in(&map, Settings::default());
        assert_eq!(back.captions, captions);
        assert_eq!(back.subtitle_scale, 1.25);
        assert!(!back.subtitles_by_default && back.subtitles_with_dub);
        assert_eq!(back.server_priority, ["icarus", "megaplay"]);
        assert_eq!(back.disabled_providers, ["animegg"]);
        assert_eq!(map["preferred_source_languages"], "eng,ara");
        assert_eq!(back.preferred_source_languages, ["eng", "ara"]);
        assert_eq!(map["doh_provider"], "quad9");
        assert_eq!(back.doh_provider, "quad9");

        // From Android: its scale, and a value it may offer that the desktop doesn't is skipped.
        let android: Map<String, Value> =
            serde_json::from_str(r#"{"caption_text_scale": 150, "caption_font": "comic", "caption_edge_style": "outline"}"#).unwrap();
        let (phone, _) = settings_in(&android, Settings::default());
        assert_eq!(phone.subtitle_scale, 1.5);
        assert_eq!(phone.captions.font, "sans");
        assert_eq!(phone.captions.edge_style, "outline");
    }

    #[test]
    fn reads_android_settings() {
        let map: Map<String, Value> = serde_json::from_str(
            r#"{"autoplay": false, "prefer_dub": true, "auto_skip_intro_outro": true, "preferred_subtitle_language": "spa",
                "theme_accent": "blue", "hide_adult_content": "yes", "player_content_scale": "zoom", "pause_details_overlay": false}"#,
        )
        .unwrap();
        let (s, applied) = settings_in(&map, Settings::default());
        assert!(!s.autoplay_next && s.audio == Audio::Dub && s.auto_skip_intro && s.auto_skip_outro);
        assert_eq!(s.subtitle_languages, ["es"]);
        assert!(!s.show_adult, "a wrongly typed value is skipped, not applied");
        assert_eq!(s.player_content_scale, "fit", "an unknown screen size is skipped");
        assert!(!s.pause_details_overlay);
        assert_eq!(applied, 5);
    }

    #[test]
    fn appearance_travels_under_androids_keys() {
        let mut s = Settings::default();
        s.appearance.accent = "custom".into();
        s.appearance.custom_seed = "#d1102b".into();
        s.appearance.hidden_poster_elements = vec!["score".into(), "title".into()];
        s.appearance.backdrop_source = "image".into();
        let map = settings_out(&s);
        assert_eq!(map["theme_custom_seed"], json!(0xFFD1102Bu32 as i32));
        assert_eq!(map["hidden_poster_elements"], "score,title");
        assert_eq!(map["app_backdrop_source"], "none", "a picture on this computer doesn't travel");
        let (back, _) = settings_in(&map, Settings::default());
        assert_eq!(back.appearance.custom_seed, "#d1102b");
        assert_eq!(back.appearance.hidden_poster_elements, ["score", "title"]);
        // Android's wallpaper accent has no desktop meaning, and is skipped.
        let android: Map<String, Value> = serde_json::from_str(r#"{"theme_accent": "wallpaper", "ui_scale": "large"}"#).unwrap();
        let (back, applied) = settings_in(&android, Settings::default());
        assert_eq!((back.appearance.accent.as_str(), back.appearance.ui_scale.as_str(), applied), ("monochrome", "large", 1));
    }

    #[test]
    fn stamps_dates() {
        assert_eq!(date_stamp(1_791_138_592), "20261004");
    }
}
