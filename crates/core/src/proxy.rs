//! Localhost HLS proxy, for streams mpv cannot play as served.
//!
//! mpv sends the server's headers on every request itself (`http-header-fields`), so most streams
//! go straight to the CDN. A stream comes through here only when its playlist has to change:
//!
//! - **Kwik** derives each segment's AES-128 IV from the number in its filename
//!   (`segment-135-…` → IV 135), while a playlist without `IV=` tells every compliant player to use
//!   the media sequence number. They agree only for the first segment. The wrong IV corrupts byte
//!   15 of each segment, the PAT's PMT PID, so playback dies after the first segment or on any
//!   resume. Writing an explicit `IV=` per segment (RFC 8216 §4.3.2.4) fixes it without us doing
//!   any decryption.
//! - **UniqueStream** (`mediacache.cc`) serves its AES-128 key base64-wrapped, and only to requests
//!   carrying its own player's header. Unwrapped, the key is the media id in the playlist's path,
//!   read as hex, so the key URI is pointed at `/s/{id}/key`, which answers those 16 bytes. Its CDN
//!   refuses browser fingerprints, so these playlists are fetched with [`Http::plain`]; mpv's own
//!   requests (ffmpeg, a plain client) pass.
//!
//! A session is registered per stream and lives until replaced; playlists are re-fetched on every
//! request (live playlists change), and URIs inside them are made absolute. Variant and rendition
//! playlists route back through the proxy; segments and keys go direct unless the session asks.

use std::collections::{BTreeMap, HashMap};
use std::net::SocketAddr;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::StreamExt;
use regex::Regex;
use url::Url;

use crate::net::{Http, USER_AGENT};
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transform {
    None,
    /// Make Kwik's filename-derived AES IVs explicit.
    KwikIv,
    /// Answer the key from the media id in the URL (UniqueStream), fetching with a plain client.
    MediaIdKey,
}

#[derive(Debug, Clone)]
pub struct Session {
    pub headers: BTreeMap<String, String>,
    pub transform: Transform,
    /// Route segments and keys through the proxy too (for CDNs that check something mpv can't send).
    pub proxy_media: bool,
}

struct ProxyState {
    http: Http,
    /// For [`Transform::MediaIdKey`] streams.
    plain: Http,
    /// The most recent request headers seen by `/echo`.
    last_echo: std::sync::Mutex<Vec<(String, String)>>,
    /// Results a webview handed back through `/bridge`, by request id.
    bridge: std::sync::Mutex<HashMap<String, String>>,
    bridge_ready: tokio::sync::Notify,
    sessions: RwLock<HashMap<String, Session>>,
    port: u16,
}

#[derive(Clone)]
pub struct Proxy {
    state: Arc<ProxyState>,
}

impl Proxy {
    /// Binds 127.0.0.1 on a free port and serves on the current tokio runtime.
    pub async fn start(http: Http) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0)))
            .await
            .map_err(|e| Error::Other(format!("proxy bind: {e}")))?;
        let port = listener.local_addr().map_err(|e| Error::Other(e.to_string()))?.port();
        let state = Arc::new(ProxyState {
            http,
            plain: Http::plain()?,
            last_echo: std::sync::Mutex::new(Vec::new()),
            bridge: std::sync::Mutex::new(HashMap::new()),
            bridge_ready: tokio::sync::Notify::new(),
            sessions: RwLock::new(HashMap::new()),
            port,
        });
        let app = Router::new()
            .route("/s/{id}/playlist", get(playlist))
            .route("/s/{id}/media", get(media))
            .route("/s/{id}/key", get(key))
            .route("/echo", get(echo))
            .route("/bridge", get(bridge))
            .with_state(Arc::clone(&state));
        tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(error = %e, "stream proxy stopped");
            }
        });
        tracing::info!(port, "stream proxy listening");
        Ok(Self { state })
    }

    /// Registers `session` for a stream and returns the local URL to give the player.
    pub fn register(&self, url: &str, session: Session) -> String {
        let id: String = (0..16).map(|_| format!("{:x}", rand::random::<u8>() & 0xf)).collect();
        let mut sessions = self.state.sessions.write().unwrap();
        // One stream plays at a time; old sessions only matter for a few seconds after a switch.
        if sessions.len() > 16 {
            sessions.clear();
        }
        sessions.insert(id.clone(), session);
        local_url(self.state.port, &id, "playlist", url)
    }

    /// [`Proxy::register`] for a stream that isn't a playlist (a progressive MP4): mpv reads it
    /// through the media route, ranges and all.
    pub fn register_media(&self, url: &str, session: Session) -> String {
        let local = self.register(url, session);
        local.replacen("/playlist?", "/media?", 1)
    }
}

fn encode(url: &str) -> String {
    url::form_urlencoded::byte_serialize(url.as_bytes()).collect()
}

fn local_url(port: u16, id: &str, route: &str, target: &str) -> String {
    format!("http://127.0.0.1:{port}/s/{id}/{route}?u={}", encode(target))
}

fn session_of(state: &ProxyState, id: &str) -> Option<Session> {
    state.sessions.read().unwrap().get(id).cloned()
}

async fn upstream(
    state: &ProxyState,
    session: &Session,
    url: &str,
    range: Option<&HeaderValue>,
) -> std::result::Result<wreq::Response, Box<Response>> {
    let http = if session.transform == Transform::MediaIdKey { &state.plain } else { &state.http };
    let mut request = http.client().get(url).timeout(Duration::from_secs(30)).header("User-Agent", USER_AGENT);
    for (name, value) in &session.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    if let Some(range) = range.and_then(|r| r.to_str().ok()) {
        request = request.header("Range", range);
    }
    request.send().await.map_err(|e| {
        tracing::warn!(%url, error = %e, "proxy upstream failed");
        Box::new((StatusCode::BAD_GATEWAY, e.to_string()).into_response())
    })
}

async fn playlist(State(state): State<Arc<ProxyState>>, Path(id): Path<String>, Query(query): Query<HashMap<String, String>>) -> Response {
    let (Some(session), Some(target)) = (session_of(&state, &id), query.get("u")) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let response = match upstream(&state, &session, target, None).await {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let status = response.status();
    // Redirects are followed, so relative URIs resolve against where the playlist really lives.
    let base = Url::parse(&response.uri().to_string()).or_else(|_| Url::parse(target));
    let body = match response.bytes().await {
        Ok(b) => b,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };
    if !status.is_success() {
        tracing::warn!(%target, status = status.as_u16(), "proxy playlist refused");
        return (StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY), body).into_response();
    }
    let Ok(base) = base else {
        return (StatusCode::BAD_REQUEST, "bad playlist url").into_response();
    };
    let text = String::from_utf8_lossy(&body);
    let text = match session.transform {
        Transform::KwikIv => rewrite_kwik_ivs(&text).unwrap_or_else(|| text.into_owned()),
        Transform::None | Transform::MediaIdKey => text.into_owned(),
    };
    let port = state.port;
    let rewritten = rewrite_uris(&text, &base, |url, kind| match kind {
        UriKind::Playlist => local_url(port, &id, "playlist", url),
        UriKind::Key if session.transform == Transform::MediaIdKey => local_url(port, &id, "key", url),
        UriKind::Media | UriKind::Key if session.proxy_media => local_url(port, &id, "media", url),
        _ => url.to_owned(),
    });
    ([(header::CONTENT_TYPE, "application/vnd.apple.mpegurl")], rewritten).into_response()
}

async fn media(
    State(state): State<Arc<ProxyState>>,
    Path(id): Path<String>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let (Some(session), Some(target)) = (session_of(&state, &id), query.get("u")) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let response = match upstream(&state, &session, target, headers.get(header::RANGE)).await {
        Ok(r) => r,
        Err(e) => return *e,
    };
    let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out = Response::builder().status(status);
    for name in [header::CONTENT_TYPE, header::CONTENT_LENGTH, header::CONTENT_RANGE, header::ACCEPT_RANGES] {
        if let Some(value) = response.headers().get(name.as_str()) {
            out = out.header(name, value.as_bytes());
        }
    }
    let stream = response.bytes_stream().map(|chunk| chunk.map_err(std::io::Error::other));
    out.body(Body::from_stream(stream)).unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// The key for a [`Transform::MediaIdKey`] stream, worked out from its URL; nothing is fetched.
async fn key(State(state): State<Arc<ProxyState>>, Path(id): Path<String>, Query(query): Query<HashMap<String, String>>) -> Response {
    let (Some(session), Some(target)) = (session_of(&state, &id), query.get("u")) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    match media_id_key(target).filter(|_| session.transform == Transform::MediaIdKey) {
        Some(key) => ([(header::CONTENT_TYPE, "application/octet-stream")], key.to_vec()).into_response(),
        None => {
            tracing::warn!(%target, "no media id in key URL");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// UniqueStream's AES key: the 32-hex-digit media id that names the stream's folder
/// (`…/f53adb363d089d22bb717b25da8d5f9d_ja-JP/keys/key.bin`), as bytes.
pub fn media_id_key(url: &str) -> Option<[u8; 16]> {
    let url = Url::parse(url).ok()?;
    let id = url.path_segments()?.find_map(|segment| {
        let id = segment.get(..32)?;
        let rest = &segment[32..];
        (id.bytes().all(|b| b.is_ascii_hexdigit()) && (rest.is_empty() || rest.starts_with('_'))).then_some(id)
    })?;
    let mut key = [0u8; 16];
    for (i, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&id[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(key)
}

/// Diagnostic: answers with the request's headers, in arrival order (used to see exactly what a
/// webview sends, e.g. its client hints, without involving any remote server).
async fn echo(State(state): State<Arc<ProxyState>>, uri: axum::http::Uri, headers: HeaderMap) -> Response {
    let mut pairs: Vec<(String, String)> =
        headers.iter().map(|(k, v)| (k.as_str().to_owned(), v.to_str().unwrap_or("").to_owned())).collect();
    if let Some(query) = uri.query() {
        pairs.push(("x-echo-query".into(), query.to_owned()));
    }
    tracing::debug!(?pairs, "echo");
    *state.last_echo.lock().unwrap() = pairs.clone();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}: {v}"))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
        .into_response()
}

impl Proxy {
    pub fn port(&self) -> u16 {
        self.state.port
    }

    /// Where a webview hands a result back: `GET {url}?id=<id>&d=<data>`.
    pub fn bridge_url(&self) -> String {
        format!("http://127.0.0.1:{}/bridge", self.state.port)
    }

    /// Waits for the result a webview sends to [`Self::bridge_url`] for `id`.
    pub async fn wait_bridge(&self, id: &str, timeout: Duration) -> Option<String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.state.bridge_ready.notified();
            if let Some(value) = self.state.bridge.lock().unwrap().remove(id) {
                return Some(value);
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return None;
            }
        }
    }

    /// Headers of the last request `/echo` received.
    pub fn last_echo(&self) -> Vec<(String, String)> {
        self.state.last_echo.lock().unwrap().clone()
    }
}

/// A webview's answer to a bridged request, delivered by top-level navigation (subresource
/// requests from a public page to localhost are restricted by Local Network Access; navigations
/// are not).
async fn bridge(State(state): State<Arc<ProxyState>>, Query(query): Query<HashMap<String, String>>) -> Response {
    if let (Some(id), Some(data)) = (query.get("id"), query.get("d")) {
        state.bridge.lock().unwrap().insert(id.clone(), data.clone());
        state.bridge_ready.notify_waiters();
    }
    ([(header::CONTENT_TYPE, "text/html")], "<!doctype html><title>ok</title>").into_response()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UriKind {
    /// A variant or rendition playlist.
    Playlist,
    /// A segment or init section.
    Media,
    Key,
}

/// Makes every URI in a playlist absolute (against `base`) and passes it through `route`.
pub fn rewrite_uris(text: &str, base: &Url, route: impl Fn(&str, UriKind) -> String) -> String {
    static URI_ATTR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"URI="([^"]*)""#).unwrap());
    let master = text.contains("#EXT-X-STREAM-INF");
    let absolute = |uri: &str| base.join(uri.trim()).map(|u| u.to_string()).unwrap_or_else(|_| uri.to_owned());
    text.split('\n')
        .map(|raw| {
            let (line, cr) = raw.strip_suffix('\r').map_or((raw, ""), |l| (l, "\r"));
            if line.starts_with('#') {
                if !line.contains("URI=\"") {
                    return raw.to_owned();
                }
                let kind = if line.starts_with("#EXT-X-MEDIA") || line.starts_with("#EXT-X-I-FRAME-STREAM-INF") {
                    UriKind::Playlist
                } else if line.starts_with("#EXT-X-KEY") || line.starts_with("#EXT-X-SESSION-KEY") {
                    UriKind::Key
                } else {
                    UriKind::Media
                };
                let replaced = URI_ATTR.replace_all(line, |c: &regex::Captures<'_>| {
                    let uri = &c[1];
                    // Data URIs and key schemes like skd:// are not fetchable over HTTP.
                    if uri.starts_with("data:") || uri.starts_with("skd:") {
                        return c[0].to_owned();
                    }
                    format!("URI=\"{}\"", route(&absolute(uri), kind))
                });
                format!("{replaced}{cr}")
            } else if line.trim().is_empty() {
                raw.to_owned()
            } else {
                let kind = if master { UriKind::Playlist } else { UriKind::Media };
                format!("{}{cr}", route(&absolute(line), kind))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Spells out Kwik's filename-derived IVs, or `None` when the playlist is not that shape.
pub fn rewrite_kwik_ivs(text: &str) -> Option<String> {
    static SEGMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:^|/)segment-(\d+)-").unwrap());
    if !text.starts_with("#EXTM3U") {
        return None;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    let key = lines.iter().map(|l| l.trim()).find(|l| l.starts_with("#EXT-X-KEY:"))?;
    if !key.to_ascii_uppercase().contains("METHOD=AES-128") || key.to_ascii_uppercase().contains(",IV=") {
        return None;
    }
    let mut numbers = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        let uri = line.trim();
        if uri.is_empty() || uri.starts_with('#') {
            continue;
        }
        numbers.insert(index, SEGMENT.captures(uri)?[1].parse::<u128>().ok()?);
    }
    if numbers.is_empty() {
        return None;
    }
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    Some(
        lines
            .iter()
            .enumerate()
            .map(|(index, line)| match numbers.get(&index) {
                Some(n) => format!("{key},IV=0x{n:032x}{eol}{line}"),
                None => (*line).to_owned(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

/// The transform a stream needs, judged from its URL and the server it came from.
pub fn transform_for(provider: &str, url: &str) -> Transform {
    let host = Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
    if provider == "animepahe" || host.contains("uwucdn") || host.contains("owocdn") || host.contains("kwik") {
        Transform::KwikIv
    } else if provider == "uniquestream" || host == "mediacache.cc" || host.ends_with(".mediacache.cc") {
        Transform::MediaIdKey
    } else {
        Transform::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KWIK: &str = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://k/mon.key\"\n#EXTINF:5,\nhttps://v/segment-1-v1-a1.jpg\n#EXTINF:5,\nhttps://v/segment-5-v1-a1.jpg\n#EXT-X-ENDLIST\n";

    #[test]
    fn media_id_keys_come_from_the_path() {
        let url =
            "https://get4.mediacache.cc/episode/z5T4zQKO/xo1VaaSy/NKtEl1kD/f53adb363d089d22bb717b25da8d5f9d_ja-JP/keys/key.bin?sign=x";
        let key = media_id_key(url).unwrap();
        assert_eq!(key[0], 0xf5);
        assert_eq!(key[15], 0x9d);
        assert!(media_id_key("https://get4.mediacache.cc/episode/a/b/keys/key.bin").is_none());
        assert_eq!(transform_for("uniquestream", url), Transform::MediaIdKey);
        assert_eq!(transform_for("zoko", "https://hls.dramahot.top/p/master.m3u8"), Transform::None);
    }

    #[test]
    fn kwik_ivs_follow_filenames() {
        let out = rewrite_kwik_ivs(KWIK).unwrap();
        assert!(out.contains("IV=0x00000000000000000000000000000001\nhttps://v/segment-1-"));
        assert!(out.contains("IV=0x00000000000000000000000000000005\nhttps://v/segment-5-"));
    }

    #[test]
    fn kwik_rewrite_skips_explicit_iv_and_other_shapes() {
        assert!(rewrite_kwik_ivs(&KWIK.replace("mon.key\"", "mon.key\",IV=0x1")).is_none());
        assert!(rewrite_kwik_ivs("#EXTM3U\n#EXTINF:5,\nseg0.ts\n").is_none());
    }

    #[test]
    fn master_variants_route_through_proxy_and_segments_go_direct() {
        let base = Url::parse("https://cdn.example/path/master.m3u8").unwrap();
        let master = "#EXTM3U\n#EXT-X-MEDIA:TYPE=AUDIO,URI=\"audio/en.m3u8\"\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv/720.m3u8\n";
        let out = rewrite_uris(master, &base, |u, k| format!("{k:?}:{u}"));
        assert!(out.contains("URI=\"Playlist:https://cdn.example/path/audio/en.m3u8\""));
        assert!(out.contains("\nPlaylist:https://cdn.example/path/v/720.m3u8"));

        let media = "#EXTM3U\r\n#EXT-X-KEY:METHOD=AES-128,URI=\"/k.key\"\r\n#EXTINF:4,\r\nseg1.ts\r\n";
        let out = rewrite_uris(media, &base, |u, k| format!("{k:?}:{u}"));
        assert!(out.contains("URI=\"Key:https://cdn.example/k.key\"\r\n"));
        assert!(out.contains("Media:https://cdn.example/path/seg1.ts\r\n"));
    }
}
