//! Zoko (zokoanime.video), keyed by MAL id, so no title matching:
//!
//! `GET /stream/mal/{malId}/{episode}/{sub|dub}` → a ~2 KB page carrying `window.__P`: base64 of
//! the player config's UTF-8 bytes XOR'd with a fixed key from the site's `core/obfuscate.js`.
//! Inside: an HLS master, VTT subtitles and intro/outro windows. A missing episode is a 200 with
//! no `__P`. (Their `/stream/anilist/…` route is documented but 404s for every id.)
//!
//! Playlists and segments answer 403 without `Referer: https://zokoanime.video/`. Its uncached
//! segments are slow (Android demoted it from the fast list on 2026-09-26 for that), so it's an
//! alternative, not a first choice. Port of Android's `ZokoProvider`.

use provider_kit::{self as kit, types::*};
use serde_json::Value;

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
    with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
});

const BASE: &str = "https://zokoanime.video";
/// The XOR key. If the site changes it, `payload` stops decoding and episodes read as missing
/// instead of playing garbage.
const KEY: &[u8] = b"otaku-embed-v1";

struct Zoko;

fn payload(page: &str) -> Option<Value> {
    let start = page.find("window.__P")?;
    let blob = page[start..].split('"').nth(1)?;
    let mut bytes = kit::base64_decode(blob)?;
    for (i, b) in bytes.iter_mut().enumerate() {
        *b ^= KEY[i % KEY.len()];
    }
    serde_json::from_slice(&bytes).ok()
}

fn window(skip: &Value, key: &str) -> Option<(f64, f64)> {
    let w = skip.get(key)?;
    let (start, end) = (w.get("start")?.as_f64()?, w.get("end")?.as_f64()?);
    (end > start).then_some((start, end))
}

impl Guest for Zoko {
    fn info() -> Info {
        Info { id: "zoko".into(), name: "zoko".into(), version: env!("CARGO_PKG_VERSION").into(), hosts: vec!["zokoanime.video".into()] }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        let Some(mal_id) = title.mal_id.filter(|id| *id > 0) else { return Ok(vec![]) };
        let track = if title.dub { "dub" } else { "sub" };
        let episode = kit::episode_label(title.episode);
        let page = kit::get_text(&format!("{BASE}/stream/mal/{mal_id}/{episode}/{track}"), &[])?;
        let Some(config) = payload(&page) else { return Ok(vec![]) };
        let src = config.get("src").and_then(Value::as_str).filter(|s| s.starts_with("http")).ok_or("Zoko config without a stream")?;

        // Every track is published as `lang: "en"`, so the label is the only honest language. The
        // default track goes first.
        let mut tracks: Vec<&Value> = config.get("subtitles").and_then(Value::as_array).into_iter().flatten().collect();
        tracks.sort_by_key(|t| t.get("default").and_then(Value::as_bool) != Some(true));
        let mut subtitles: Vec<Subtitle> = Vec::new();
        for t in tracks {
            let Some(url) = t.get("src").and_then(Value::as_str).filter(|u| !u.is_empty()) else { continue };
            if subtitles.iter().any(|s| s.url == url) {
                continue;
            }
            let label = t.get("label").and_then(Value::as_str).map(str::trim).filter(|l| !l.is_empty()).unwrap_or("Subtitles");
            subtitles.push(Subtitle { url: url.into(), label: label.into(), language: kit::language(label) });
        }
        let skip = config.get("skip").cloned().unwrap_or(Value::Null);
        // The player's seek previews; empty on every title tried (2026-10-06), but the field is there.
        let sprites = config
            .get("sprite_vtt")
            .and_then(Value::as_str)
            .filter(|u| !u.is_empty())
            .map(|u| if u.starts_with('/') { format!("{BASE}{u}") } else { u.to_owned() });
        kit::host::log(&format!("zoko resolved mal={mal_id} ep={episode} {track}"));
        Ok(vec![Server {
            name: "Zoko".into(),
            dub: title.dub,
            headers: vec![("Referer".into(), format!("{BASE}/")), ("Origin".into(), BASE.into())],
            streams: vec![MediaStream { url: src.into(), format: "hls".into(), quality: None, height: None }],
            subtitles,
            skip_intro: window(&skip, "intro"),
            skip_outro: window(&skip, "outro"),
            thumbnails: sprites.filter(|u| u.starts_with("http")),
        }])
    }
}

export!(Zoko);
