//! UniqueStream (anime.uniquestream.net): a Crunchyroll mirror with an open JSON API.
//!
//! 1. `/search?query=` → series with a `content_id`.
//! 2. `/series/{id}` → seasons, each with `episode_count` and (mostly) a `mal_id` string. A MAL id
//!    match names the exact AniList entry. Otherwise seasons whose MAL id belongs to another entry
//!    are dropped (Frieren's season 1 says `"None"`, season 2 `"59978"`), then a season titled like
//!    the show wins, and failing that the series is one entry split into sagas (One Piece's "East
//!    Blue (1-61)", …) numbered absolutely, read as one list.
//! 3. `/season/{id}/episodes?limit=20&page=N` → episode `content_id`s and their audio locales.
//! 4. `/episode/{id}/media/hls/{ja-JP|en-US}` → a signed HLS master and WebVTT subtitles. When the
//!    Japanese encode has no soft subtitles, the burnt-in English one (`hard_subs`) is used.
//!
//! The segments are AES-128 with a key the app works out from the media id in the playlist URL
//! (the proxy's `MediaIdKey`), and the CDN refuses browser fingerprints; both are the host's job,
//! picked by the provider id. The API black-holes bursts from one IP for a minute, so requests are
//! kept few and lookups cached. Port of Android's `UniqueStreamProvider`.

use provider_kit::{self as kit, DAY, types::*};
use serde_json::{Value, json};

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
    with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
});

const API: &str = "https://anime.uniquestream.net/api/v1";
const PAGE_SIZE: usize = 20;
/// Episode pages read to find one episode.
const PAGE_BUDGET: usize = 15;
/// Series opened for their season lists before giving up on a title.
const PROBE_SERIES: usize = 4;
const MIN_SEASON_TITLE_SCORE: f64 = 0.5;

struct UniqueStream;

fn get(path: &str) -> Result<Value, String> {
    kit::get_json(&format!("{API}/{path}"), &[])
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `[{ "id", "title", "count", "mal" }]` for one series.
fn seasons_of(series: &str) -> Result<Vec<Value>, String> {
    let root = get(&format!("series/{series}"))?;
    Ok(root
        .get("seasons")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| {
            Some(json!({
                "id": text(s, "content_id")?,
                "title": text(s, "title").unwrap_or(""),
                "count": s.get("episode_count").and_then(Value::as_u64),
                "mal": text(s, "mal_id").and_then(|m| m.parse::<u32>().ok()),
            }))
        })
        .collect())
}

/// The seasons that hold this AniList entry, in the API's (chronological) order.
fn select(title: &Title, seasons: Vec<Value>) -> Vec<Value> {
    if let Some(mal) = title.mal_id
        && let Some(exact) = seasons.iter().find(|s| s["mal"].as_u64() == Some(u64::from(mal)))
    {
        return vec![exact.clone()];
    }
    let mine: Vec<Value> = match title.mal_id {
        Some(mal) => seasons.into_iter().filter(|s| s["mal"].as_u64().is_none_or(|m| m == u64::from(mal))).collect(),
        None => seasons,
    };
    if mine.len() <= 1 {
        return mine;
    }
    let score =
        |s: &Value| title.titles.iter().map(|t| kit::title_selection_score(t, s["title"].as_str().unwrap_or(""))).fold(0.0, f64::max);
    match mine.iter().max_by(|a, b| score(a).total_cmp(&score(b))) {
        Some(best) if score(best) >= MIN_SEASON_TITLE_SCORE => vec![best.clone()],
        _ => mine,
    }
}

fn find_seasons(title: &Title) -> Result<Vec<Value>, String> {
    let cached = kit::cached(&format!("seasons:{}", title.anilist_id), 7 * DAY, DAY, || {
        let mut series: Vec<(String, String)> = Vec::new();
        for query in kit::search_queries(&title.titles, 3) {
            let found = get(&format!("search?query={}&limit=20", kit::encode(&query)))?;
            series = found
                .get("series")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|s| Some((text(s, "content_id")?.to_owned(), text(s, "title").unwrap_or("").to_owned())))
                .collect();
            if !series.is_empty() {
                break;
            }
        }
        let score = |name: &str| title.titles.iter().map(|t| kit::title_selection_score(t, name)).fold(0.0, f64::max);
        series.sort_by(|a, b| score(&b.1).total_cmp(&score(&a.1)));
        let mut first: Option<Vec<Value>> = None;
        for (id, _) in series.iter().take(PROBE_SERIES) {
            let seasons = seasons_of(id)?;
            if let Some(mal) = title.mal_id
                && seasons.iter().any(|s| s["mal"].as_u64() == Some(u64::from(mal)))
            {
                return Ok(Some(Value::Array(select(title, seasons)).to_string()));
            }
            first.get_or_insert(seasons);
        }
        // No MAL match anywhere: the best-named series, read as above.
        let chosen = first.map(|s| select(title, s)).unwrap_or_default();
        Ok((!chosen.is_empty()).then(|| Value::Array(chosen).to_string()))
    })?;
    Ok(cached.and_then(|c| serde_json::from_str::<Vec<Value>>(&c).ok()).unwrap_or_default())
}

/// `(content id, audio locales)` of episode `number` on one page of episodes.
fn pick(page: &Value, number: f64) -> Option<(String, Vec<String>)> {
    page.as_array().into_iter().flatten().filter(|e| e.get("is_clip").and_then(Value::as_bool) != Some(true)).find_map(|e| {
        if e.get("episode_number").and_then(Value::as_f64) != Some(number) {
            return None;
        }
        let locales =
            e.get("audio_locales").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect();
        Some((text(e, "content_id")?.to_owned(), locales))
    })
}

/// Episode `number` within `season`, reading pages from `first_page` (a guess) and then from the
/// start, skipping `read` (a page already looked at).
fn find_in_season(
    season: &str,
    number: f64,
    first_page: usize,
    read: Option<usize>,
    budget: &mut usize,
) -> Result<Option<(String, Vec<String>)>, String> {
    let mut order: Vec<usize> = vec![first_page.max(1)];
    order.extend((1..=PAGE_BUDGET).filter(|p| *p != first_page.max(1)));
    for page in order.into_iter().filter(|p| Some(*p) != read) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let episodes = get(&format!("season/{season}/episodes?limit={PAGE_SIZE}&page={page}"))?;
        if let Some(found) = pick(&episodes, number) {
            return Ok(Some(found));
        }
        if episodes.as_array().map_or(0, Vec::len) < PAGE_SIZE {
            break;
        }
    }
    Ok(None)
}

fn find_episode(seasons: &[Value], number: f64) -> Result<Option<(String, Vec<String>)>, String> {
    let mut budget = PAGE_BUDGET;
    let counts: Vec<u64> = seasons.iter().filter_map(|s| s["count"].as_u64().filter(|c| *c > 0)).collect();
    // A saga split with every count known: go straight to the saga holding this absolute number.
    if seasons.len() > 1 && counts.len() == seasons.len() {
        let mut start = 1u64;
        for (season, count) in seasons.iter().zip(&counts) {
            if (number as u64) < start + count {
                let local = (number as u64).saturating_sub(start) as usize;
                return find_in_season(season["id"].as_str().unwrap_or(""), number, local / PAGE_SIZE + 1, None, &mut budget);
            }
            start += count;
        }
    }
    if let [season] = seasons {
        // One AniList entry, which AniList numbers from 1. Crunchyroll often numbers a later season
        // on from the one before (Solo Leveling's second season is 13-25), so count from the
        // season's own first episode.
        let id = season["id"].as_str().unwrap_or("");
        budget -= 1;
        let first_page = get(&format!("season/{id}/episodes?limit={PAGE_SIZE}&page=1"))?;
        let first = first_page
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e.get("is_clip").and_then(Value::as_bool) != Some(true))
            .filter_map(|e| e.get("episode_number").and_then(Value::as_f64))
            .filter(|n| n.fract() == 0.0 && *n >= 1.0)
            .fold(f64::INFINITY, f64::min);
        let target = if first.is_finite() && first > 1.0 { first + number - 1.0 } else { number };
        if let Some(found) = pick(&first_page, target) {
            return Ok(Some(found));
        }
        if first_page.as_array().map_or(0, Vec::len) < PAGE_SIZE {
            return Ok(None);
        }
        let guess = ((target - first.min(target)).max(0.0) as usize) / PAGE_SIZE + 1;
        return find_in_season(id, target, guess, Some(1), &mut budget);
    }
    for season in seasons {
        if let Some(found) = find_in_season(season["id"].as_str().unwrap_or(""), number, 1, None, &mut budget)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// A readable name for a Crunchyroll locale tag.
fn label(tag: &str) -> String {
    let name = match tag {
        "en-US" => "English",
        "es-419" => "Spanish (Latin America)",
        "es-ES" => "Spanish (Spain)",
        "pt-BR" => "Portuguese (Brazil)",
        "pt-PT" => "Portuguese (Portugal)",
        "fr-FR" => "French",
        "de-DE" => "German",
        "it-IT" => "Italian",
        "ru-RU" => "Russian",
        "ar-SA" | "ar-ME" => "Arabic",
        "hi-IN" => "Hindi",
        "id-ID" => "Indonesian",
        "ms-MY" => "Malay",
        "th-TH" => "Thai",
        "vi-VN" => "Vietnamese",
        "tr-TR" => "Turkish",
        "pl-PL" => "Polish",
        "zh-CN" => "Chinese (Simplified)",
        "zh-HK" | "zh-TW" => "Chinese (Traditional)",
        "ja-JP" => "Japanese",
        "ko-KR" => "Korean",
        other => other,
    };
    name.to_owned()
}

impl Guest for UniqueStream {
    fn info() -> Info {
        Info {
            id: "uniquestream".into(),
            name: "unique".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            hosts: vec!["anime.uniquestream.net".into()],
        }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        let seasons = find_seasons(&title)?;
        if seasons.is_empty() {
            return Ok(vec![]);
        }
        let locale = if title.dub { "en-US" } else { "ja-JP" };
        let Some((episode, locales)) = find_episode(&seasons, title.episode)? else { return Ok(vec![]) };
        if !locales.is_empty() && !locales.iter().any(|l| l == locale) {
            return Ok(vec![]);
        }
        let media = get(&format!("episode/{episode}/media/hls/{locale}"))?;
        // Not encoded yet: an error shape (`hls_not_ready`) with no `hls` block.
        let Some(hls) = media.get("hls").filter(|h| h.is_object()) else { return Ok(vec![]) };
        let clean = text(hls, "playlist");
        let hard_en = hls
            .get("hard_subs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|h| text(h, "locale") == Some("en-US"))
            .and_then(|h| text(h, "playlist"));
        let mut subtitles: Vec<Subtitle> = hls
            .get("subtitles")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let url = text(s, "url")?;
                let tag = text(s, "language").unwrap_or("und");
                Some(Subtitle { url: url.into(), label: label(tag), language: tag.split('-').next().unwrap_or("und").to_ascii_lowercase() })
            })
            .collect();
        subtitles.sort_by(|a, b| (b.language == "en").cmp(&(a.language == "en")).then_with(|| a.label.cmp(&b.label)));
        // Japanese audio with no soft subtitles is untranslated: take the burnt-in English encode.
        let (url, subtitles) = match (clean, hard_en) {
            (Some(clean), _) if title.dub || !subtitles.is_empty() => (clean, subtitles),
            (_, Some(hard)) => (hard, vec![]),
            (Some(clean), None) => (clean, subtitles),
            (None, None) => return Ok(vec![]),
        };
        kit::host::log(&format!("uniquestream resolved anilist={} episode={episode} {locale}", title.anilist_id));
        Ok(vec![Server {
            name: "UniqueStream".into(),
            dub: title.dub,
            headers: vec![],
            streams: vec![MediaStream { url: url.into(), format: "hls".into(), quality: None, height: None }],
            subtitles,
            skip_intro: None,
            skip_outro: None,
            thumbnails: None,
        }])
    }
}

export!(UniqueStream);
