//! AnimeOnsen (animeonsen.xyz): a JSON API, no HTML beyond one token in the landing page.
//!
//! 1. `www.animeonsen.xyz/` → `<meta name="ao-search-token">`, the search index key. Cached a day.
//! 2. `auth.animeonsen.xyz/oauth/token` → the API bearer, a `client_credentials` grant with the
//!    site's own public reader client (from its browser bundle; not an account). Cached until it
//!    expires (a week).
//! 3. `search.animeonsen.xyz/indexes/content/search {q}` → hits with a `content_id`.
//! 4. `api…/v4/content/{id}/extensive` → `mal_id`: a candidate is *confirmed* against AniList's MAL
//!    id, not trusted on its title (a strict title score only when there's no MAL id).
//! 5. `api…/v4/content/{id}/video/{ep}` → a DASH manifest, ASS subtitles per locale (served from
//!    extension-less URLs; mpv reads the format from the content) and the intro window.
//!
//! Sub only: the manifest carries one audio set, `lang="und"`. Every host, the CDN and subtitle
//! host included, gates on `Referer: https://www.animeonsen.xyz/` and nothing else. A small
//! library (~1,100 titles), but nine subtitle languages, which nothing else offers.
//! Port of Android's `AnimeOnsenProvider`.

use provider_kit::{self as kit, Candidate, DAY, types::*};
use serde_json::{Value, json};

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
    with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
});

const BASE: &str = "https://www.animeonsen.xyz";
const API: &str = "https://api.animeonsen.xyz/v4";
const SEARCH: &str = "https://search.animeonsen.xyz/indexes/content/search";
const TOKEN: &str = "https://auth.animeonsen.xyz/oauth/token";
/// The site's own public reader credentials, as its browser bundle sends them.
const CLIENT_ID: &str = "f296be26-28b5-4358-b5a1-6259575e23b7";
const CLIENT_SECRET: &str = "349038c4157d0480784753841217270c3c5b35f4281eaee029de21cb04084235";
/// Relevance-ordered hits worth a confirming detail request: a real match is near the top.
const CONFIRM_LIMIT: usize = 3;
/// Only when AniList has no MAL id to confirm against: a wrong show is worse than none.
const TITLE_FLOOR: f64 = 0.6;

const REFERER: (&str, &str) = ("Referer", "https://www.animeonsen.xyz/");
const ORIGIN: (&str, &str) = ("Origin", BASE);

struct AnimeOnsen;

fn search_token() -> Result<String, String> {
    kit::cached("search-token", DAY, 60, || {
        let html = kit::get_text(&format!("{BASE}/"), &[REFERER])?;
        Ok(kit::tags(&html, "meta")
            .find(|t| kit::attr(t, "name") == "ao-search-token")
            .map(|t| kit::attr(t, "content"))
            .filter(|t| !t.is_empty()))
    })?
    .ok_or_else(|| "AnimeOnsen published no search token".into())
}

fn api_token() -> Result<String, String> {
    if let Some(token) = kit::host::cache_get("api-token") {
        return Ok(token);
    }
    let form = format!("client_id={CLIENT_ID}&client_secret={CLIENT_SECRET}&grant_type=client_credentials");
    let response =
        kit::send("POST", TOKEN, &[("Content-Type", "application/x-www-form-urlencoded"), REFERER, ORIGIN], Some(form.into_bytes()))?;
    if response.status >= 400 {
        return Err(format!("AnimeOnsen token answered HTTP {}", response.status));
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|e| format!("AnimeOnsen token: {e}"))?;
    let token = body.get("access_token").and_then(Value::as_str).ok_or("AnimeOnsen token without access_token")?.to_owned();
    let lifetime = body.get("expires_in").and_then(Value::as_u64).unwrap_or(3600).saturating_sub(60) as u32;
    kit::host::cache_set("api-token", &token, lifetime.max(60));
    Ok(token)
}

fn api(path: &str) -> Result<Value, String> {
    let bearer = format!("Bearer {}", api_token()?);
    kit::get_json(&format!("{API}/{path}"), &[("Authorization", &bearer), REFERER, ORIGIN])
}

/// `(content id, every title spelling)` for each hit.
fn search(query: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let bearer = format!("Bearer {}", search_token()?);
    let found = kit::post_json(SEARCH, &[("Authorization", &bearer), REFERER, ORIGIN], &json!({ "q": query }))?;
    Ok(found
        .get("hits")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|hit| {
            let id = hit.get("content_id").and_then(Value::as_str).filter(|i| !i.is_empty())?;
            let titles = ["content_title", "content_title_en", "content_title_jp"]
                .iter()
                .filter_map(|k| hit.get(*k).and_then(Value::as_str))
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .collect();
            Some((id.to_owned(), titles))
        })
        .collect())
}

/// The content id for the title, confirmed by MAL id when AniList has one.
fn content_id(title: &Title) -> Result<Option<String>, String> {
    kit::cached(&format!("id:{}", title.anilist_id), 7 * DAY, DAY, || {
        let mut hits: Vec<(String, Vec<String>)> = Vec::new();
        for query in kit::search_queries(&title.titles, 3) {
            for hit in search(&query)? {
                if !hits.iter().any(|(id, _)| *id == hit.0) {
                    hits.push(hit);
                }
            }
        }
        if let Some(mal_id) = title.mal_id {
            for (id, _) in hits.iter().take(CONFIRM_LIMIT) {
                let detail = api(&format!("content/{id}/extensive"))?;
                if detail.get("mal_id").and_then(Value::as_u64) == Some(u64::from(mal_id)) {
                    return Ok(Some(id.clone()));
                }
            }
            return Ok(None);
        }
        let candidates: Vec<Candidate> =
            hits.iter().flat_map(|(id, names)| names.iter().map(|n| Candidate { id: id.clone(), title: n.clone() })).collect();
        Ok(kit::best_match(&title.titles, &candidates, TITLE_FLOOR).map(|c| c.id.clone()))
    })
}

/// A display name for the API's closed set of locale tags.
fn label(tag: &str) -> &str {
    match tag {
        "ar-ME" => "Arabic",
        "de-DE" => "German",
        "en-US" => "English",
        "es-ES" => "Spanish",
        "es-LA" => "Spanish (Latin America)",
        "fr-FR" => "French",
        "it-IT" => "Italian",
        "pt-BR" => "Portuguese (Brazil)",
        "ru-RU" => "Russian",
        other => other,
    }
}

/// Timings arrive as quoted whole seconds (`"55"`).
fn seconds(v: &Value, key: &str) -> Option<f64> {
    let v = v.get(key)?;
    v.as_f64().or_else(|| v.as_str()?.trim().parse().ok())
}

impl Guest for AnimeOnsen {
    fn info() -> Info {
        Info {
            id: "animeonsen".into(),
            name: "onsen".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            hosts: vec!["animeonsen.xyz".into()],
        }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        if title.dub || title.episode.fract() != 0.0 {
            return Ok(vec![]);
        }
        let Some(id) = content_id(&title)? else { return Ok(vec![]) };
        let episode = title.episode as i64;
        let video = match api(&format!("content/{id}/video/{episode}")) {
            Ok(v) => v,
            // An episode past what the site has.
            Err(e) if e.contains("HTTP 404") => return Ok(vec![]),
            Err(e) => return Err(e),
        };
        let uri = video.get("uri").ok_or("AnimeOnsen video without a uri")?;
        let stream = uri.get("stream").and_then(Value::as_str).filter(|s| !s.is_empty()).ok_or("AnimeOnsen video without a stream")?;
        let mut subtitles: Vec<Subtitle> = uri
            .get("subtitles")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .filter_map(|(tag, url)| {
                let url = url.as_str().filter(|u| !u.is_empty())?;
                let language = tag.split('-').next().unwrap_or("und").to_ascii_lowercase();
                Some(Subtitle { url: url.into(), label: label(tag).into(), language })
            })
            .collect();
        subtitles.sort_by(|a, b| (b.language == "en").cmp(&(a.language == "en")).then_with(|| a.label.cmp(&b.label)));
        // `metadata.episode` is [number, this episode, every title]: the middle one has timings.
        // `nextEpisode_s` is a cue, not an outro window with an end, so it isn't mapped.
        let current = video.pointer("/metadata/episode/1").cloned().unwrap_or(Value::Null);
        let intro = seconds(&current, "skipIntro_s").zip(seconds(&current, "skipIntro_e")).filter(|(s, e)| e > s);
        kit::host::log(&format!("animeonsen resolved anilist={} content={id} ep={episode}", title.anilist_id));
        Ok(vec![Server {
            name: "AnimeOnsen".into(),
            dub: false,
            headers: vec![(REFERER.0.into(), REFERER.1.into()), (ORIGIN.0.into(), ORIGIN.1.into())],
            streams: vec![MediaStream { url: stream.into(), format: "dash".into(), quality: None, height: None }],
            subtitles,
            skip_intro: intro,
            skip_outro: None,
            thumbnails: None,
        }])
    }
}

export!(AnimeOnsen);
