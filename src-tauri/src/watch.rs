//! Turning "episode N of title X" into something playing.
//!
//! Server choice follows the user's audio preference, then their preferred provider, then the
//! site's own order. For subs, a soft-sub server with a track in a preferred language beats a
//! hardsub one (selectable, restylable, delayable), which beats a soft-sub server without one.

use std::collections::BTreeMap;

use anilili_core::model::{Audio, Server, SkipTimes, Stream};
use anilili_core::proxy::{Session, Transform, transform_for};
use anilili_core::storyboard::Frame;
use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::app::{AppState, CmdResult, Settings, fail};
use crate::catalog::resolve_sources;
use crate::player::{LoadRequest, NowPlaying, Player, SubtitleTrack};
use anilili_core::catalogue as c;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchRequest {
    pub anime_id: i64,
    pub episode: f64,
    /// A specific server (from a previous `WatchInfo`); otherwise chosen by preference.
    pub server_id: Option<String>,
    pub audio: Option<Audio>,
    /// Seconds; `None` resumes from saved progress when that setting is on.
    pub start: Option<f64>,
    /// Re-resolve sources first (after a playback failure, signed URLs may have expired).
    #[serde(default)]
    pub refresh: bool,
    /// Display title for the player.
    pub title: Option<String>,
    /// The episode's own title, from the episode list (for Discord).
    #[serde(default)]
    pub episode_title: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchInfo {
    pub server: Server,
    pub servers: Vec<Server>,
    pub skip: Option<SkipTimes>,
    pub start: f64,
    pub proxied: bool,
}

fn has_language(server: &Server, languages: &[String]) -> bool {
    server.subtitles.iter().any(|s| {
        let lang = s.language.to_ascii_lowercase();
        let label = s.label.to_ascii_lowercase();
        languages.iter().any(|want| {
            let want = want.to_ascii_lowercase();
            lang.starts_with(&want) || (want == "en" && label.contains("english"))
        })
    })
}

/// Where a provider stands in the viewer's order (Server priority): the listed ones by position,
/// the rest after them. An empty list falls back to the single preferred provider of older
/// settings.
fn priority(provider: &str, settings: &Settings) -> usize {
    let order: &[String] = if settings.server_priority.is_empty() { settings.preferred_provider.as_slice() } else { &settings.server_priority };
    order.iter().position(|p| p == provider).unwrap_or(order.len())
}

/// Not switched off in Settings → Servers.
pub fn is_enabled(server: &Server, settings: &Settings) -> bool {
    !settings.disabled_providers.contains(&server.provider)
}

/// Lower is better. Input order (the site's) breaks ties, as the sort is stable.
fn rank(server: &Server, audio: Audio, settings: &Settings) -> (bool, usize, u8) {
    let audio_miss = server.audio != audio;
    let provider_rank = priority(&server.provider, settings);
    let subtitle_rank = match (audio, server.track.as_str()) {
        (Audio::Sub, "ssub") if has_language(server, &settings.subtitle_languages) => 0,
        (Audio::Sub, "sub") => 1,
        (Audio::Sub, _) => 2,
        _ => 0,
    };
    (audio_miss, provider_rank, subtitle_rank)
}

pub fn pick_server<'a>(servers: &'a [Server], audio: Audio, settings: &Settings) -> Option<&'a Server> {
    let mut native: Vec<&Server> = servers.iter().filter(|s| s.is_native() && is_enabled(s, settings)).collect();
    native.sort_by_key(|s| rank(s, audio, settings));
    native.into_iter().next()
}

/// The servers an episode can be downloaded from (HLS or progressive MP4: not DASH, not an
/// embed) in `audio`, best first by the same preferences as playback, with `provider` (the
/// download dialog's first choice) standing in for the preferred one when given. With
/// `hard_subs`, a sub download keeps only the servers whose subtitles are in the picture.
pub fn downloadable<'a>(servers: &'a [Server], audio: Audio, settings: &Settings, provider: Option<&str>, hard_subs: bool) -> Vec<&'a Server> {
    let mut settings = settings.clone();
    if let Some(provider) = provider {
        settings.server_priority.retain(|p| p != provider);
        settings.server_priority.insert(0, provider.to_owned());
    }
    let mut candidates: Vec<&Server> = servers
        .iter()
        .filter(|s| s.audio == audio && s.streams.iter().any(is_downloadable))
        .filter(|s| is_enabled(s, &settings) || provider == Some(s.provider.as_str()))
        .filter(|s| !hard_subs || audio != Audio::Sub || is_hard_sub(s))
        .collect();
    candidates.sort_by_key(|s| rank(s, audio, &settings));
    candidates
}

/// Subtitles burned into the picture: Miruro's `sub` track, and a plugin server that hands over
/// no subtitle files (`plugin.rs` files those under `sub` too). `ssub` is a clean picture.
pub fn is_hard_sub(server: &Server) -> bool {
    server.track == "sub"
}

pub fn is_downloadable(stream: &Stream) -> bool {
    stream.format == "hls" || stream.format == "mp4"
}

/// A server's stream to download under a height ceiling: the tallest at or under it, else the
/// shortest; without a ceiling, the tallest. Streams of unknown height (an HLS master, which
/// picks its own variant) only when nothing says its height.
pub fn download_stream(server: &Server, max_height: Option<u32>) -> Option<&Stream> {
    let streams: Vec<&Stream> = server.streams.iter().filter(|s| is_downloadable(s)).collect();
    let sized = || streams.iter().copied().filter(|s| s.height.is_some());
    let chosen = match max_height {
        Some(max) => sized().filter(|s| s.height.is_some_and(|h| h <= max)).max_by_key(|s| s.height).or_else(|| sized().min_by_key(|s| s.height)),
        None => sized().max_by_key(|s| s.height),
    };
    chosen.or_else(|| streams.iter().copied().find(|s| s.format == "hls")).or_else(|| streams.first().copied())
}

/// The best rendition: the tallest, else the first (an HLS master picks its own).
fn pick_stream(server: &Server) -> Option<&Stream> {
    server.streams.iter().max_by_key(|s| s.height.unwrap_or(0)).or(server.streams.first())
}

/// AniSkip's timings for an episode, for when the server and the media index had none (Android
/// asks AniSkip the same way). `duration` is the stream's length, which picks among submissions
/// for different encodes. Found answers are kept a week, "none" a day; a failure isn't kept, so
/// an outage is never remembered as "no timings".
#[tauri::command]
pub async fn skip_times(state: State<'_, AppState>, anime_id: i64, episode: f64, duration: f64) -> CmdResult<Option<SkipTimes>> {
    if episode.fract() != 0.0 || episode < 1.0 {
        return Ok(None);
    }
    let mal_key = format!("mal-id:{anime_id}");
    let mal = match state.store.cache_get::<Option<i64>>(&mal_key, 30 * 86_400) {
        Some(mal) => mal,
        None => {
            let mal = state.anilist.mal_id(anime_id).await.map_err(|e| e.to_string())?;
            let _ = state.store.cache_put(&mal_key, &mal);
            mal
        }
    };
    let Some(mal) = mal else { return Ok(None) };
    let key = format!("aniskip:{mal}:{}", episode as i64);
    if let Some(found) = state.store.cache_get::<Option<SkipTimes>>(&key, 86_400) {
        return Ok(found);
    }
    if let Some(Some(found)) = state.store.cache_get::<Option<SkipTimes>>(&key, 7 * 86_400) {
        return Ok(Some(found));
    }
    let found = state.aniskip.skip_times(mal, episode as i64, duration.max(0.0)).await.map_err(|e| e.to_string())?;
    tracing::info!(anime_id, mal, episode, found = found.is_some(), "AniSkip timings");
    let _ = state.store.cache_put(&key, &found);
    Ok(found)
}

#[tauri::command]
pub async fn watch<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, request: WatchRequest) -> CmdResult<WatchInfo> {
    let player = crate::player::get(&app)?;
    let settings = state.settings();
    let sources = resolve_sources(&state, request.anime_id, request.episode, request.refresh).await?;
    let audio = request.audio.unwrap_or(settings.audio);
    let server = match &request.server_id {
        Some(id) => sources.servers.iter().find(|s| &s.id == id),
        None => pick_server(&sources.servers, audio, &settings),
    }
    .ok_or_else(|| fail(c::SRC_NONE_FOR_EPISODE, format!("anime {} episode {}", request.anime_id, request.episode)))?
    .clone();
    let stream = pick_stream(&server).ok_or_else(|| fail(c::SRC_EMPTY_SERVER, server.id.clone()))?.clone();

    let transform = transform_for(&server.provider, &stream.url);
    // mpv looks names up through the system, so a stream from a host the network's DNS blocks (and
    // only secure DNS could find) is read through the proxy, segments and all.
    let host = url::Url::parse(&stream.url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
    // HLS and MP4 only: a DASH manifest's relative segment paths wouldn't survive the detour.
    let blocked = matches!(stream.format.as_str(), "hls" | "mp4") && anilili_core::dns::needs_proxy(&host).await;
    let (url, proxied) = if transform == Transform::None && !blocked {
        (stream.url.clone(), false)
    } else {
        let session = Session { headers: server.headers.clone(), transform, proxy_media: blocked };
        let local = if stream.format == "hls" { state.proxy.register(&stream.url, session) } else { state.proxy.register_media(&stream.url, session) };
        if blocked {
            tracing::info!(%host, "stream host blocked by the network's DNS; playing through the proxy");
        }
        (local, true)
    };

    let start = match request.start {
        Some(start) => start,
        None if settings.resume => state
            .store
            .progress_for(request.anime_id)?
            .into_iter()
            .find(|p| p.episode == request.episode && !p.finished())
            .map(|p| (p.position - 3.0).max(0.0))
            .unwrap_or(0.0),
        None => 0.0,
    };

    let subtitles = server
        .subtitles
        .iter()
        .map(|s| SubtitleTrack { url: s.url.clone(), title: Some(s.label.clone()), lang: Some(s.language.clone()) })
        .collect();
    let load = LoadRequest {
        url,
        headers: server.headers.clone(),
        start: Some(start),
        subtitles,
        title: request.title.clone(),
        subtitle_languages: settings.subtitle_languages.clone(),
        show_subtitles: Some(show_subtitles(&settings, server.audio)),
    };
    tracing::info!(anime = request.anime_id, episode = request.episode, server = %server.id, proxied, start, "watch");
    let card = state.store.card(request.anime_id);
    let now_playing = NowPlaying {
        anime_id: request.anime_id,
        episode: request.episode,
        title: card.as_ref().map(|c| c.title.clone()).or(request.title.clone()).unwrap_or_default(),
        episode_title: request.episode_title.clone(),
        cover: card.as_ref().and_then(|c| c.cover.clone()),
        total: card.as_ref().and_then(|c| c.episodes),
        synced: false,
        loaded: false,
    };
    player.load(&load, Some(now_playing)).await.map_err(|e| fail(c::PLAY_STREAM_FAILED, e.to_string()))?;

    // Switched-off servers stay out of the player's list too; one asked for by id still plays.
    let servers = sources.servers.into_iter().filter(|s| is_enabled(s, &settings) || s.id == server.id).collect();
    Ok(WatchInfo { server, servers, skip: sources.skip, start, proxied })
}

/// An episode in an embedded (iframe) player, and its video's clock once a frame of the page has
/// reported one (`embed-progress.js`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedPresence {
    pub anime_id: i64,
    pub episode: f64,
    pub title: Option<String>,
    pub episode_title: Option<String>,
    /// Seconds into the video, and its length; `None` until the page reports a video.
    #[serde(default)]
    pub position: Option<f64>,
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub paused: bool,
}

/// Discord status for an embedded player: the series, the episode and its cover, with a progress
/// bar and pause once the page reports its video, and a clock counting from when it opened until
/// then. `None` (leaving the embed) takes it down. mpv stopping for the embed leaves it alone
/// (`Discord::clear_player`).
#[tauri::command]
pub fn embed_presence(state: State<'_, AppState>, embed: Option<EmbedPresence>) {
    let Some(embed) = embed else {
        state.discord.clear();
        return;
    };
    if !state.settings().discord_presence {
        return;
    }
    let card = state.store.card(embed.anime_id);
    state.discord.show(crate::discord::Presence {
        anime_id: embed.anime_id,
        title: card.as_ref().map(|c| c.title.clone()).or(embed.title).unwrap_or_default(),
        episode: embed.episode,
        episode_title: embed.episode_title,
        total: card.as_ref().and_then(|c| c.episodes),
        cover: card.as_ref().and_then(|c| c.cover.clone()),
        position: embed.position.unwrap_or_default(),
        duration: embed.duration.unwrap_or_default(),
        paused: embed.paused,
        embedded: true,
    });
}

/// Android's "Show subtitles by default", and "Also on dubbed audio" for a dub.
pub fn show_subtitles(settings: &Settings, audio: Audio) -> bool {
    settings.subtitles_by_default && (audio == Audio::Sub || settings.subtitles_with_dub)
}

#[tauri::command]
pub fn now_playing<R: Runtime>(app: AppHandle<R>) -> Option<NowPlaying> {
    app.try_state::<Player>()?.now_playing()
}

/// A server's storyboard (`Server::thumbnails`), for the seek bar's previews. `headers` are the
/// server's: the CDNs gate the storyboard as they gate the stream.
#[tauri::command]
pub async fn storyboard(url: String, headers: BTreeMap<String, String>) -> CmdResult<Vec<Frame>> {
    Ok(anilili_core::storyboard::frames(&url, &headers).await.map_err(|e| e.to_string())?)
}

/// One sheet a storyboard names, as bytes (the webview can't send the server's headers itself).
#[tauri::command]
pub async fn storyboard_image(url: String, headers: BTreeMap<String, String>) -> CmdResult<Response> {
    let bytes = anilili_core::storyboard::image(&url, &headers).await.map_err(|e| e.to_string())?;
    Ok(Response::new(bytes.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anilili_core::model::Subtitle;
    use std::collections::BTreeMap;

    fn server(id: &str, provider: &str, track: &str, audio: Audio, langs: &[&str]) -> Server {
        Server {
            id: id.into(),
            provider: provider.into(),
            label: provider.into(),
            name: id.into(),
            track: track.into(),
            audio,
            headers: BTreeMap::new(),
            streams: vec![Stream { url: format!("https://{id}/m.m3u8"), format: "hls".into(), quality: None, height: None }],
            embed: None,
            subtitles: langs.iter().map(|l| Subtitle { url: "u".into(), label: (*l).into(), language: (*l).into() }).collect(),
            thumbnails: None,
        }
    }

    #[test]
    fn prefers_soft_subs_in_language_then_hardsub() {
        let servers = vec![
            server("a", "anikoto", "ssub", Audio::Sub, &["es"]),
            server("b", "animepahe", "sub", Audio::Sub, &[]),
            server("c", "icarus", "ssub", Audio::Sub, &["en"]),
            server("d", "anikoto", "dub", Audio::Dub, &[]),
        ];
        let settings = Settings::default();
        assert_eq!(pick_server(&servers, Audio::Sub, &settings).unwrap().id, "c");
        assert_eq!(pick_server(&servers, Audio::Dub, &settings).unwrap().id, "d");
        let pahe_first = Settings { preferred_provider: Some("animepahe".into()), ..Settings::default() };
        assert_eq!(pick_server(&servers, Audio::Sub, &pahe_first).unwrap().id, "b");
    }

    #[test]
    fn hard_subs_only_keeps_burned_in_servers_and_leaves_dub_alone() {
        let servers = vec![
            server("a", "icarus", "ssub", Audio::Sub, &["en"]),
            server("b", "animepahe", "sub", Audio::Sub, &[]),
            server("c", "anikoto", "dub", Audio::Dub, &[]),
        ];
        let settings = Settings::default();
        // Even named as the first choice, a soft-sub server is not taken.
        let ids = |audio, hard| downloadable(&servers, audio, &settings, Some("icarus"), hard).iter().map(|s| s.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(Audio::Sub, true), ["b"]);
        assert_eq!(ids(Audio::Sub, false), ["a", "b"]);
        assert_eq!(ids(Audio::Dub, true), ["c"]);
    }

    #[test]
    fn follows_the_priority_list_and_skips_switched_off_servers() {
        let servers = vec![
            server("a", "anikoto", "ssub", Audio::Sub, &["en"]),
            server("b", "animepahe", "sub", Audio::Sub, &[]),
            server("c", "icarus", "ssub", Audio::Sub, &["en"]),
        ];
        let ordered = Settings { server_priority: vec!["icarus".into(), "animepahe".into()], ..Settings::default() };
        assert_eq!(pick_server(&servers, Audio::Sub, &ordered).unwrap().id, "c");
        // The list wins over the single preferred provider of older settings.
        let both = Settings { preferred_provider: Some("anikoto".into()), ..ordered.clone() };
        assert_eq!(pick_server(&servers, Audio::Sub, &both).unwrap().id, "c");
        let off = Settings { disabled_providers: vec!["icarus".into()], ..ordered };
        assert_eq!(pick_server(&servers, Audio::Sub, &off).unwrap().id, "b");
        let all_off = Settings { disabled_providers: vec!["anikoto".into(), "animepahe".into(), "icarus".into()], ..Settings::default() };
        assert!(pick_server(&servers, Audio::Sub, &all_off).is_none());
    }

    #[test]
    fn subtitles_by_default_and_on_dubs() {
        let on = Settings::default();
        assert!(show_subtitles(&on, Audio::Sub));
        assert!(!show_subtitles(&on, Audio::Dub));
        let with_dub = Settings { subtitles_with_dub: true, ..Settings::default() };
        assert!(show_subtitles(&with_dub, Audio::Dub));
        let off = Settings { subtitles_by_default: false, subtitles_with_dub: true, ..Settings::default() };
        assert!(!show_subtitles(&off, Audio::Sub));
    }
}
