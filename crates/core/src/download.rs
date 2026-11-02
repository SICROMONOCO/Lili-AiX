//! Episode downloads: HLS (TS or fMP4) and progressive MP4, to a single local file.
//!
//! HLS is fetched segment by segment with the server's headers, decrypted here (AES-128-CBC, with
//! Kwik's filename-derived IVs), and appended in order: TS segments concatenate into a valid
//! `.ts`, and fMP4 segments after their init section into a valid fragmented `.mp4`. Both play
//! directly in the app (and in VLC/mpv). Subtitles are saved next to the video as
//! `<name>.<lang>.vtt`, which mpv picks up automatically.
//!
//! Segments are fetched a few at a time but written strictly in order. A failed segment is retried
//! with backoff (CDNs answer bursts with 429), never skipped: a skipped segment is a jump in the
//! episode that nobody notices until they watch it.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use futures_util::{StreamExt, stream};
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::model::Subtitle;
use crate::net::{Http, USER_AGENT};
use crate::proxy::Transform;
use crate::{Error, Result};

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

const PARALLEL: usize = 4;
const RETRIES: u32 = 6;

#[derive(Debug, Clone)]
pub struct DownloadRequest {
    pub url: String,
    /// `hls` or `mp4`.
    pub format: String,
    pub headers: BTreeMap<String, String>,
    pub transform: Transform,
    /// Output path without extension; the extension follows the container.
    pub output: PathBuf,
    pub subtitles: Vec<Subtitle>,
    /// The tallest rendition wanted from an HLS master playlist; `None` takes the best. When
    /// nothing is that short, the shortest there is.
    pub max_height: Option<u32>,
}

/// A finished download: the file, and the rendition's height when the playlist said it.
#[derive(Debug, Clone)]
pub struct Downloaded {
    pub path: PathBuf,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DownloadProgress {
    pub done: u64,
    pub total: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone)]
struct Segment {
    url: String,
    key: Option<Key>,
}

#[derive(Debug, Clone, PartialEq)]
struct Key {
    url: String,
    iv: [u8; 16],
}

#[derive(Debug, Default)]
struct MediaPlaylist {
    init: Option<String>,
    segments: Vec<Segment>,
}

/// Downloads to `request.output` + extension.
pub async fn download(
    http: &Http,
    request: &DownloadRequest,
    cancel: Arc<AtomicBool>,
    on_progress: impl Fn(DownloadProgress) + Send + Sync,
) -> Result<Downloaded> {
    if let Some(dir) = request.output.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(io)?;
    }
    // A CDN that refuses browser fingerprints: UniqueStream's key by rule (see
    // `Transform::MediaIdKey`), and, after a 403 below, any CDN that answers the Chrome
    // fingerprint with one (Icarus's did; mpv's own requests are plain and pass).
    let plain;
    // Whether the first attempt wrote anything: progress is reported only after bytes land.
    let wrote = AtomicBool::new(false);
    let tracked = |progress: DownloadProgress| {
        wrote.store(true, Ordering::Relaxed);
        on_progress(progress)
    };
    // Subtitles go with whichever client fetched the stream.
    let (path, height, http) = if request.transform == Transform::MediaIdKey {
        plain = Http::plain()?;
        let (path, height) = run(&plain, request, &cancel, &on_progress).await?;
        (path, height, &plain)
    } else {
        match run(http, request, &cancel, &tracked).await {
            Ok((path, height)) => (path, height, http),
            // Only a refusal before any bytes were written (the playlist, the key, the first
            // response or segment) is the fingerprint's: starting over from a plain client then
            // loses nothing. A 403 partway through is a link gone stale, which a plain client
            // would only meet again after re-fetching everything already saved.
            Err(Error::Http { status: 403, .. }) if !wrote.load(Ordering::Relaxed) => {
                plain = Http::plain()?;
                let (path, height) = run(&plain, request, &cancel, &on_progress).await?;
                (path, height, &plain)
            }
            Err(e) => return Err(e),
        }
    };
    for subtitle in &request.subtitles {
        if let Err(e) = download_subtitle(http, request, subtitle).await {
            tracing::warn!(url = %subtitle.url, error = %e, "subtitle download failed");
        }
    }
    Ok(Downloaded { path, height })
}

/// One whole-download attempt, whatever the format.
async fn run(
    http: &Http,
    request: &DownloadRequest,
    cancel: &AtomicBool,
    on_progress: &(impl Fn(DownloadProgress) + Send + Sync),
) -> Result<(PathBuf, Option<u32>)> {
    match request.format.as_str() {
        "mp4" => download_file(http, request, cancel, on_progress).await.map(|p| (p, None)),
        "hls" => download_hls(http, request, cancel, on_progress).await,
        other => Err(Error::Other(format!("downloading {other} streams is not supported"))),
    }
}

/// The heights an HLS master playlist offers, tallest first; empty for a media playlist (one
/// rendition, its height unstated) or a master that doesn't say.
pub async fn hls_heights(http: &Http, url: &str, headers: &BTreeMap<String, String>) -> Result<Vec<u32>> {
    let text = String::from_utf8_lossy(&fetch(http, url, headers).await?).into_owned();
    let mut heights: Vec<u32> = variants(&text).into_iter().filter_map(|v| v.height).collect();
    heights.sort_unstable_by(|a, b| b.cmp(a));
    heights.dedup();
    Ok(heights)
}

fn io(e: std::io::Error) -> Error {
    Error::Storage(e.to_string())
}

async fn fetch(http: &Http, url: &str, headers: &BTreeMap<String, String>) -> Result<bytes::Bytes> {
    let mut attempt = 0;
    loop {
        let mut request = http.client().get(url).timeout(Duration::from_secs(60)).header("User-Agent", USER_AGENT);
        for (k, v) in headers {
            request = request.header(k.as_str(), v.as_str());
        }
        let outcome = match request.send().await {
            Ok(r) if r.status().is_success() => r.bytes().await.map_err(Error::from),
            Ok(r) => Err(Error::Http { service: "CDN", status: r.status().as_u16(), detail: url.chars().take(80).collect() }),
            Err(e) => Err(e.into()),
        };
        match outcome {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                let retryable = !matches!(e, Error::Http { status, .. } if status == 403 || status == 404 || status == 410);
                attempt += 1;
                if !retryable || attempt >= RETRIES {
                    return Err(e);
                }
                // 429s want real backoff: 1, 2, 4, 8, 16s.
                tokio::time::sleep(Duration::from_secs(1 << (attempt - 1))).await;
            }
        }
    }
}

async fn download_file(
    http: &Http,
    request: &DownloadRequest,
    cancel: &AtomicBool,
    on_progress: &(impl Fn(DownloadProgress) + Send + Sync),
) -> Result<PathBuf> {
    let path = request.output.with_extension("mp4");
    let part = request.output.with_extension("mp4.part");
    let mut builder = http.client().get(&request.url).timeout(Duration::from_secs(60 * 60)).header("User-Agent", USER_AGENT);
    for (k, v) in &request.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let response = builder.send().await?;
    if !response.status().is_success() {
        return Err(Error::Http { service: "CDN", status: response.status().as_u16(), detail: "download refused".into() });
    }
    let total = response.content_length().unwrap_or(0);
    let mut file = tokio::fs::File::create(&part).await.map_err(io)?;
    let mut body = response.bytes_stream();
    let mut bytes = 0u64;
    while let Some(chunk) = body.next().await {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Error::Other("cancelled".into()));
        }
        let chunk = chunk?;
        file.write_all(&chunk).await.map_err(io)?;
        bytes += chunk.len() as u64;
        on_progress(DownloadProgress { done: bytes, total, bytes });
    }
    file.flush().await.map_err(io)?;
    drop(file);
    tokio::fs::rename(&part, &path).await.map_err(io)?;
    Ok(path)
}

async fn download_hls(
    http: &Http,
    request: &DownloadRequest,
    cancel: &AtomicBool,
    on_progress: &(impl Fn(DownloadProgress) + Send + Sync),
) -> Result<(PathBuf, Option<u32>)> {
    let (playlist_url, text, height) = resolve_media_playlist(http, &request.url, &request.headers, request.max_height).await?;
    let base = Url::parse(&playlist_url).map_err(|e| Error::decode("HLS", e))?;
    let playlist = parse_media_playlist(&text, &base, request.transform)?;
    if playlist.segments.is_empty() {
        return Err(Error::decode("HLS", "playlist has no segments"));
    }

    let extension = if playlist.init.is_some() { "mp4" } else { "ts" };
    let path = request.output.with_extension(extension);
    let part = request.output.with_extension(format!("{extension}.part"));
    let mut file = tokio::fs::File::create(&part).await.map_err(io)?;
    if let Some(init) = &playlist.init {
        file.write_all(&fetch(http, init, &request.headers).await?).await.map_err(io)?;
    }

    // Keys are fetched once each.
    let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
    for segment in &playlist.segments {
        if let Some(key) = &segment.key
            && !keys.contains_key(&key.url)
        {
            let key_bytes: [u8; 16] = if request.transform == Transform::MediaIdKey {
                crate::proxy::media_id_key(&key.url).ok_or_else(|| Error::decode("HLS", "no media id in the key URL"))?
            } else {
                let bytes = fetch(http, &key.url, &request.headers).await?;
                normalise_key(&bytes).ok_or_else(|| Error::decode("HLS", "AES key is not 16 bytes"))?
            };
            keys.insert(key.url.clone(), key_bytes);
        }
    }
    let keys = Arc::new(keys);

    let total = playlist.segments.len() as u64;
    let mut done = 0u64;
    let mut bytes = 0u64;
    let mut results = stream::iter(playlist.segments.into_iter().map(|segment| {
        let keys = Arc::clone(&keys);
        let headers = request.headers.clone();
        async move {
            let data = fetch(http, &segment.url, &headers).await?;
            match &segment.key {
                Some(key) => {
                    let key_bytes = keys.get(&key.url).copied().ok_or_else(|| Error::decode("HLS", "missing key"))?;
                    decrypt(&data, &key_bytes, &key.iv)
                }
                None => Ok(data.to_vec()),
            }
        }
    }))
    .buffered(PARALLEL);

    while let Some(result) = results.next().await {
        if cancel.load(Ordering::Relaxed) {
            drop(file);
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Error::Other("cancelled".into()));
        }
        let data = result?;
        file.write_all(&data).await.map_err(io)?;
        done += 1;
        bytes += data.len() as u64;
        on_progress(DownloadProgress { done, total, bytes });
    }
    file.flush().await.map_err(io)?;
    drop(file);
    tokio::fs::rename(&part, &path).await.map_err(io)?;
    Ok((path, height))
}

/// Follows a master playlist to the variant wanted. Returns (playlist URL, playlist text, the
/// variant's height when stated).
async fn resolve_media_playlist(
    http: &Http,
    url: &str,
    headers: &BTreeMap<String, String>,
    max_height: Option<u32>,
) -> Result<(String, String, Option<u32>)> {
    let text = String::from_utf8_lossy(&fetch(http, url, headers).await?).into_owned();
    if !text.contains("#EXT-X-STREAM-INF") {
        return Ok((url.to_owned(), text, None));
    }
    let base = Url::parse(url).map_err(|e| Error::decode("HLS", e))?;
    let all = variants(&text);
    let variant = pick_variant(&all, max_height).ok_or_else(|| Error::decode("HLS", "master playlist without variants"))?;
    let variant_url = base.join(&variant.uri).map_err(|e| Error::decode("HLS", e))?.to_string();
    let text = String::from_utf8_lossy(&fetch(http, &variant_url, headers).await?).into_owned();
    Ok((variant_url, text, variant.height))
}

#[derive(Debug, Clone, PartialEq)]
struct Variant {
    uri: String,
    bandwidth: u64,
    height: Option<u32>,
}

fn variants(text: &str) -> Vec<Variant> {
    let mut out = Vec::new();
    let mut pending: Option<(u64, Option<u32>)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
            let bandwidth = attribute(attrs, "BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0);
            let height = attribute(attrs, "RESOLUTION").and_then(|r| r.split_once(['x', 'X']).and_then(|(_, h)| h.trim().parse().ok()));
            pending = Some((bandwidth, height));
        } else if !line.is_empty()
            && !line.starts_with('#')
            && let Some((bandwidth, height)) = pending.take()
        {
            out.push(Variant { uri: line.to_owned(), bandwidth, height });
        }
    }
    out
}

/// The variant to download: the best (by height, then bandwidth) at or under `max_height`; with
/// nothing that short, the shortest. Without a ceiling, or heights to judge by, the highest
/// bandwidth.
fn pick_variant(variants: &[Variant], max_height: Option<u32>) -> Option<&Variant> {
    match max_height {
        Some(max) if variants.iter().any(|v| v.height.is_some()) => variants
            .iter()
            .filter(|v| v.height.is_some_and(|h| h <= max))
            .max_by_key(|v| (v.height, v.bandwidth))
            .or_else(|| variants.iter().filter(|v| v.height.is_some()).min_by_key(|v| (v.height, v.bandwidth))),
        _ => variants.iter().max_by_key(|v| v.bandwidth),
    }
}

fn attribute(attrs: &str, name: &str) -> Option<String> {
    // Attribute lists are comma-separated, but quoted values may contain commas.
    let mut rest = attrs;
    while !rest.is_empty() {
        let (key, after) = rest.split_once('=')?;
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let end = quoted.find('"')?;
            (&quoted[..end], quoted[end + 1..].trim_start_matches(','))
        } else {
            match after.split_once(',') {
                Some((v, n)) => (v, n),
                None => (after, ""),
            }
        };
        if key.trim().eq_ignore_ascii_case(name) {
            return Some(value.to_owned());
        }
        rest = next;
    }
    None
}

fn parse_media_playlist(text: &str, base: &Url, transform: Transform) -> Result<MediaPlaylist> {
    let mut out = MediaPlaylist::default();
    let mut sequence: u128 = 0;
    let mut key: Option<(String, Option<[u8; 16]>)> = None;
    for line in text.lines().map(str::trim) {
        if let Some(v) = line.strip_prefix("#EXT-X-MEDIA-SEQUENCE:") {
            sequence = v.trim().parse().unwrap_or(0);
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-KEY:") {
            let method = attribute(attrs, "METHOD").unwrap_or_default();
            key = match method.as_str() {
                "NONE" => None,
                "AES-128" => {
                    let uri = attribute(attrs, "URI").ok_or_else(|| Error::decode("HLS", "key without URI"))?;
                    let url = base.join(&uri).map_err(|e| Error::decode("HLS", e))?.to_string();
                    let iv = attribute(attrs, "IV").and_then(|iv| parse_iv(&iv));
                    Some((url, iv))
                }
                other => return Err(Error::Other(format!("{other} encryption is not supported for download"))),
            };
        } else if let Some(attrs) = line.strip_prefix("#EXT-X-MAP:") {
            if let Some(uri) = attribute(attrs, "URI") {
                out.init = Some(base.join(&uri).map_err(|e| Error::decode("HLS", e))?.to_string());
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            let url = base.join(line).map_err(|e| Error::decode("HLS", e))?.to_string();
            let segment_key = key.as_ref().map(|(key_url, explicit)| {
                let iv = explicit.unwrap_or_else(|| {
                    let n = match transform {
                        Transform::KwikIv => kwik_segment_number(line).unwrap_or(sequence),
                        Transform::None | Transform::MediaIdKey => sequence,
                    };
                    n.to_be_bytes()
                });
                Key { url: key_url.clone(), iv }
            });
            out.segments.push(Segment { url, key: segment_key });
            sequence += 1;
        }
    }
    Ok(out)
}

fn kwik_segment_number(uri: &str) -> Option<u128> {
    let lower = uri.to_ascii_lowercase();
    let start = lower.find("segment-")? + "segment-".len();
    let digits: String = lower[start..].chars().take_while(char::is_ascii_digit).collect();
    if lower[start + digits.len()..].starts_with('-') { digits.parse().ok() } else { None }
}

fn parse_iv(text: &str) -> Option<[u8; 16]> {
    let hex = text.trim().trim_start_matches("0x").trim_start_matches("0X");
    let value = u128::from_str_radix(hex, 16).ok()?;
    Some(value.to_be_bytes())
}

/// Some CDNs serve the key as base64 text rather than 16 raw bytes.
fn normalise_key(bytes: &[u8]) -> Option<[u8; 16]> {
    if bytes.len() == 16 {
        return bytes.try_into().ok();
    }
    let text = std::str::from_utf8(bytes).ok()?.trim();
    let decoded = base64_decode(text)?;
    decoded.as_slice().try_into().ok()
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0);
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
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

fn decrypt(data: &[u8], key: &[u8; 16], iv: &[u8; 16]) -> Result<Vec<u8>> {
    let mut buffer = data.to_vec();
    let plain = Aes128CbcDec::new(key.into(), iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|_| Error::decode("HLS", "segment decryption failed (wrong key or IV)"))?;
    Ok(plain.to_vec())
}

async fn download_subtitle(http: &Http, request: &DownloadRequest, subtitle: &Subtitle) -> Result<()> {
    let bytes = fetch(http, &subtitle.url, &request.headers).await?;
    let extension = Path::new(Url::parse(&subtitle.url).map(|u| u.path().to_owned()).unwrap_or_default().as_str())
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| matches!(*e, "vtt" | "srt" | "ass" | "ssa"))
        .unwrap_or("vtt")
        .to_owned();
    let lang: String = subtitle.language.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let name = format!(
        "{}.{}.{extension}",
        request.output.file_name().and_then(|n| n.to_str()).unwrap_or("episode"),
        if lang.is_empty() { "und" } else { lang.as_str() }
    );
    let path = request.output.with_file_name(name);
    tokio::fs::write(path, bytes).await.map_err(io)
}

/// A file-system-safe version of a title for folder and file names.
pub fn safe_name(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c.is_control() { ' ' } else { c })
        .collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_end_matches(['.', ' ']).to_owned();
    if trimmed.is_empty() { "Untitled".into() } else { trimmed.chars().take(120).collect() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360\n360.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=2500000,RESOLUTION=1280x720,CODECS=\"avc1.640028,mp4a.40.2\"\n720.m3u8\n\
        #EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080\n1080.m3u8\n";

    #[test]
    fn picks_the_variant_under_the_ceiling() {
        let all = variants(MASTER);
        assert_eq!(all.len(), 3);
        assert_eq!(pick_variant(&all, None).unwrap().uri, "1080.m3u8");
        assert_eq!(pick_variant(&all, Some(1080)).unwrap().uri, "1080.m3u8");
        assert_eq!(pick_variant(&all, Some(720)).unwrap().uri, "720.m3u8");
        assert_eq!(pick_variant(&all, Some(480)).unwrap().uri, "360.m3u8");
        // Nothing that short: the shortest there is.
        assert_eq!(pick_variant(&all, Some(240)).unwrap().uri, "360.m3u8");
    }

    #[test]
    fn without_heights_the_highest_bandwidth_wins() {
        let text = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=5000000\nhigh.m3u8\n";
        assert_eq!(pick_variant(&variants(text), Some(480)).unwrap().uri, "high.m3u8");
    }

    #[test]
    fn kwik_ivs_come_from_filenames_and_others_from_sequence() {
        let base = Url::parse("https://v/p.m3u8").unwrap();
        let text = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:1\n#EXT-X-KEY:METHOD=AES-128,URI=\"k.key\"\n#EXTINF:5,\nsegment-1-v1-a1.jpg\n#EXTINF:5,\nsegment-5-v1-a1.jpg\n";
        let kwik = parse_media_playlist(text, &base, Transform::KwikIv).unwrap();
        assert_eq!(kwik.segments[1].key.as_ref().unwrap().iv, 5u128.to_be_bytes());
        let plain = parse_media_playlist(text, &base, Transform::None).unwrap();
        assert_eq!(plain.segments[1].key.as_ref().unwrap().iv, 2u128.to_be_bytes());
        assert_eq!(plain.segments[0].key.as_ref().unwrap().url, "https://v/k.key");
    }

    #[test]
    fn decrypts_aes_cbc() {
        use aes::cipher::BlockEncryptMut;
        type Enc = cbc::Encryptor<aes::Aes128>;
        let (key, iv) = ([7u8; 16], [9u8; 16]);
        let plain = b"segment payload of some length".to_vec();
        let mut buffer = plain.clone();
        buffer.resize(plain.len() + 16, 0);
        let cipher = Enc::new(&key.into(), &iv.into()).encrypt_padded_mut::<Pkcs7>(&mut buffer, plain.len()).unwrap().to_vec();
        assert_eq!(decrypt(&cipher, &key, &iv).unwrap(), plain);
    }

    #[test]
    fn keys_may_be_base64() {
        assert_eq!(normalise_key(b"AAECAwQFBgcICQoLDA0ODw=="), Some([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]));
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("Re:Zero / Season 2?"), "Re Zero Season 2");
        assert_eq!(safe_name("..."), "Untitled");
    }
}
