//! AniZone (anizone.to), a Laravel Livewire site that hands its data to its scripts as JSON:
//!
//! 1. `/anime?search=` → `items: JSON.parse('[…]')` in an Alpine `x-data`: each entry's `slug`,
//!    `main_title` and a `title_list` in every language, which is what the match is scored on.
//!    (Android's parser read result links from the HTML; the site moved them into this JSON.)
//! 2. `/anime/{slug}/{episode}` → `vidstackPlayer(JSON.parse('{…}'))`: the HLS master and ASS
//!    subtitles in ~20 languages. Neither needs a Referer (measured 2026-10-04).
//!
//! The master carries nine audio tracks (Japanese by default, English among them), so this could
//! serve dubs once the player can be told which audio language to pick; until then it's sub only.

use provider_kit::{self as kit, Candidate, types::*};
use serde_json::Value;

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
    with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
});

const BASE: &str = "https://anizone.to";
const MIN_TITLE_SCORE: f64 = 0.5;

struct AniZone;

fn search(query: &str) -> Result<Vec<Candidate>, String> {
    let page = kit::get_text(&format!("{BASE}/anime?search={}", kit::encode(query)), &[("Referer", "https://anizone.to/")])?;
    let Some(items) = kit::js_string(&page, "items: JSON.parse(") else { return Ok(vec![]) };
    let items: Value = serde_json::from_str(&items).map_err(|e| format!("AniZone search data: {e}"))?;
    let mut out = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        let Some(slug) = item.get("slug").and_then(Value::as_str) else { continue };
        let names = item
            .get("main_title")
            .and_then(Value::as_str)
            .into_iter()
            .chain(item.get("title_list").and_then(Value::as_object).into_iter().flat_map(|m| m.values().filter_map(Value::as_str)));
        for name in names.filter(|n| n.is_ascii()) {
            out.push(Candidate { id: slug.to_owned(), title: name.replace('`', "'") });
        }
    }
    Ok(out)
}

impl Guest for AniZone {
    fn info() -> Info {
        Info { id: "anizone".into(), name: "zone".into(), version: env!("CARGO_PKG_VERSION").into(), hosts: vec!["anizone.to".into()] }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        if title.dub || title.episode.fract() != 0.0 {
            return Ok(vec![]);
        }
        let Some(slug) = kit::resolve_id(title.anilist_id, &title.titles, MIN_TITLE_SCORE, search)? else { return Ok(vec![]) };
        let episode = title.episode as i64;
        let response = kit::send("GET", &format!("{BASE}/anime/{slug}/{episode}"), &[("Referer", &format!("{BASE}/anime/{slug}"))], None)?;
        if response.status == 404 {
            return Ok(vec![]);
        }
        if response.status >= 400 {
            return Err(format!("anizone.to answered HTTP {}", response.status));
        }
        let page = String::from_utf8_lossy(&response.body);
        let config = kit::js_string(&page, "vidstackPlayer(JSON.parse(").ok_or("AniZone episode page without a player")?;
        let config: Value = serde_json::from_str(&config).map_err(|e| format!("AniZone player data: {e}"))?;
        let src = config.get("src").and_then(Value::as_str).filter(|s| s.starts_with("http")).ok_or("AniZone player without a stream")?;
        let mut subtitles: Vec<Subtitle> = config
            .get("subtitles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let url = s.get("file").and_then(Value::as_str).filter(|u| !u.is_empty())?;
                let label = s.get("title").and_then(Value::as_str).unwrap_or("Subtitles");
                let language = s
                    .get("language")
                    .and_then(Value::as_str)
                    .map(|l| l.split('-').next().unwrap_or(l).to_ascii_lowercase())
                    .unwrap_or_else(|| kit::language(label));
                Some(Subtitle { url: url.into(), label: label.into(), language })
            })
            .collect();
        // English first; the menu otherwise keeps the site's order.
        subtitles.sort_by_key(|s| s.language != "en");
        // Vidstack's seek previews: a `storyboard.vtt` of tiles on one sheet beside the stream.
        let storyboard = config.get("storyboard").and_then(Value::as_str).filter(|u| u.starts_with("http"));
        kit::host::log(&format!("anizone resolved anilist={} {slug} ep={episode}", title.anilist_id));
        Ok(vec![Server {
            name: "AniZone".into(),
            dub: false,
            headers: vec![],
            streams: vec![MediaStream { url: src.into(), format: "hls".into(), quality: None, height: None }],
            subtitles,
            skip_intro: None,
            skip_outro: None,
            thumbnails: storyboard.map(Into::into),
        }])
    }
}

export!(AniZone);
