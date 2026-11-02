//! Seek-bar previews: a server's storyboard, the WebVTT file players read thumbnails from. Each
//! cue names a time range and a picture, usually a tile of a sprite sheet given as a
//! `#xywh=x,y,w,h` fragment:
//!
//! ```text
//! 00:00:03.000 --> 00:00:06.000
//! storyboard.webp#xywh=180,0,180,101
//! ```
//!
//! Sheet paths are relative to the VTT (AniZone, KickAssAnime) or to its host (Icarus). Both the
//! VTT and the sheets are fetched with the server's headers, from a plain client: the CDNs gate on
//! them as they do for the stream, and like mpv's own requests, a plain client passes where the
//! Chrome fingerprint is refused (Icarus's CDN answered it 403, 2026-10-06).

use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::Duration;

use serde::Serialize;
use url::Url;

use crate::net::{Http, USER_AGENT};
use crate::{Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub start: f64,
    pub end: f64,
    /// Absolute URL of the picture (a sprite sheet, or a picture of its own).
    pub image: String,
    /// The tile within the sheet; absent when the picture is the frame.
    pub rect: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// The storyboard at `url`, its pictures made absolute. Not a storyboard (an error page, a
/// subtitle file): no frames, not an error.
pub async fn frames(url: &str, headers: &BTreeMap<String, String>) -> Result<Vec<Frame>> {
    let base = Url::parse(url).map_err(|e| Error::Other(format!("storyboard URL: {e}")))?;
    let body = get(url, headers).await?;
    Ok(parse(&String::from_utf8_lossy(&body), &base))
}

/// One sheet (or picture) named by [`frames`].
pub async fn image(url: &str, headers: &BTreeMap<String, String>) -> Result<bytes::Bytes> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Error::Other("storyboard picture is not a web address".into()));
    }
    get(url, headers).await
}

async fn get(url: &str, headers: &BTreeMap<String, String>) -> Result<bytes::Bytes> {
    static PLAIN: LazyLock<std::result::Result<Http, String>> = LazyLock::new(|| Http::plain().map_err(|e| e.to_string()));
    let http = PLAIN.as_ref().map_err(|e| Error::Other(e.clone()))?;
    let mut pairs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    if !headers.keys().any(|k| k.eq_ignore_ascii_case("user-agent")) {
        pairs.push(("User-Agent", USER_AGENT));
    }
    Ok(http.get_bytes("storyboard", url, &pairs, Duration::from_secs(20)).await?.0)
}

pub fn parse(text: &str, base: &Url) -> Vec<Frame> {
    let text = text.trim_start_matches('\u{feff}');
    if !text.starts_with("WEBVTT") {
        return Vec::new();
    }
    let mut frames = Vec::new();
    let mut lines = text.lines().map(str::trim);
    while let Some(line) = lines.next() {
        let Some((from, to)) = line.split_once("-->") else { continue };
        // Cue settings may follow the end time.
        let to = to.split_whitespace().next().unwrap_or_default();
        let (Some(start), Some(end)) = (timestamp(from.trim()), timestamp(to)) else { continue };
        let Some(payload) = lines.next().filter(|p| !p.is_empty()) else { continue };
        let (path, fragment) = payload.split_once('#').unwrap_or((payload, ""));
        let Ok(image) = base.join(path) else { continue };
        if !matches!(image.scheme(), "http" | "https") {
            continue;
        }
        frames.push(Frame { start, end, image: image.into(), rect: rect(fragment) });
    }
    frames
}

/// `hh:mm:ss.mmm` or `mm:ss.mmm`, in seconds.
fn timestamp(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    let parts: Vec<&str> = text.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    for part in parts {
        seconds = seconds * 60.0 + part.replace(',', ".").parse::<f64>().ok()?;
    }
    Some(seconds)
}

/// `xywh=x,y,w,h` (Media Fragments; `pixel:` is the only unit players use).
fn rect(fragment: &str) -> Option<Rect> {
    let value = fragment.split('&').find_map(|p| p.strip_prefix("xywh="))?;
    let value = value.strip_prefix("pixel:").unwrap_or(value);
    let n: Vec<u32> = value.split(',').map(|v| v.trim().parse().ok()).collect::<Option<_>>()?;
    match n[..] {
        [x, y, w, h] if w > 0 && h > 0 => Some(Rect { x, y, w, h }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sprite_tiles_relative_to_the_vtt() {
        // AniZone's, as served (2026-10-06).
        let text = "WEBVTT\n\n00:00:00.000 --> 00:00:03.000\nstoryboard.webp#xywh=0,0,180,101\n\n\
                    00:25:57.000 --> 00:26:00.000\nstoryboard.webp#xywh=1260,3232,180,101\n";
        let base = Url::parse("https://cdn.example/a7305a5d/storyboard.vtt").unwrap();
        let frames = parse(text, &base);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].image, "https://cdn.example/a7305a5d/storyboard.webp");
        assert_eq!(frames[0].rect, Some(Rect { x: 0, y: 0, w: 180, h: 101 }));
        assert_eq!((frames[1].start, frames[1].end), (1557.0, 1560.0));
    }

    #[test]
    fn reads_numbered_cues_root_paths_and_whole_pictures() {
        // Icarus numbers its cues and roots its paths at the host; some players give one picture
        // per cue, without a tile.
        let text = "\u{feff}WEBVTT\r\n\r\n1\r\n00:05.000 --> 00:10.000 align:start\r\n/a/sheet.jpeg#xywh=pixel:320,0,320,180\r\n\r\n\
                    2\r\n00:10.000 --> 00:15.000\r\nframes/0003.jpg\r\n";
        let base = Url::parse("https://vault.example/a/board.jpeg").unwrap();
        let frames = parse(text, &base);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].image, "https://vault.example/a/sheet.jpeg");
        assert_eq!((frames[0].start, frames[0].rect), (5.0, Some(Rect { x: 320, y: 0, w: 320, h: 180 })));
        assert_eq!(frames[1].image, "https://vault.example/a/frames/0003.jpg");
        assert_eq!(frames[1].rect, None);
    }

    #[test]
    fn anything_else_has_no_frames() {
        let base = Url::parse("https://x.example/p.vtt").unwrap();
        assert!(parse("<html><title>403 Forbidden</title></html>", &base).is_empty());
        assert!(parse("WEBVTT\n\n00:00.000 --> 00:03.000\njavascript:alert(1)\n", &base).is_empty());
    }
}
