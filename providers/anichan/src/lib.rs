//! AniChan (`anichan.to`), keyed by AniList id:
//! **Dormant since 2026-10-04:** the session still mints, but `/api/watch/servers` answers
//! `{"servers":[]}` for every title, so it is not bundled. Kept for when the site recovers.
//!
//! 1. `POST /api/watch/session {"token":""}` → `Set-Cookie: anichan_ws=…` (2 h). The token is a
//!    Cloudflare Turnstile token the site currently does not require (`turnstileSiteKey: null`);
//!    if that changes, this POST starts failing and the provider needs revisiting.
//! 2. `GET /api/watch/servers?anilistId=&ep=&category=sub|dub` with the cookie → servers.
//!
//! Stream and subtitle URLs come back site-relative; subtitle paths contain literal spaces that
//! must become `%20` (and only those: `+` is refused). Its `megaplay_*` and `vidhawk` servers are
//! dropped: the same backend the app already reaches through Miruro's `bee`.

use serde_json::Value;

wit_bindgen::generate!({ path: "../../wit", world: "provider" });

use anilili::provider::host::{self, Request};
use anilili::provider::types::{MediaStream, Subtitle};

const BASE: &str = "https://anichan.to";

struct AniChan;

fn absolute(url: &str) -> String {
    let full = if url.starts_with('/') { format!("{BASE}{url}") } else { url.to_owned() };
    full.replace(' ', "%20")
}

fn language(label: &str) -> &'static str {
    let l = label.to_ascii_lowercase();
    for (prefix, code) in [
        ("eng", "en"), ("spa", "es"), ("por", "pt"), ("fre", "fr"), ("fra", "fr"), ("ger", "de"), ("deu", "de"),
        ("ita", "it"), ("ara", "ar"), ("rus", "ru"), ("ind", "id"), ("kor", "ko"), ("jap", "ja"), ("chi", "zh"),
    ] {
        if l.starts_with(prefix) {
            return code;
        }
    }
    "und"
}

fn session() -> Result<String, String> {
    let response = host::http(&Request {
        method: "POST".into(),
        url: format!("{BASE}/api/watch/session"),
        headers: vec![("Content-Type".into(), "application/json".into()), ("Accept".into(), "application/json".into())],
        body: Some(br#"{"token":""}"#.to_vec()),
    })?;
    if response.status >= 400 {
        return Err(format!("session refused with HTTP {}", response.status));
    }
    response
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("set-cookie"))
        .find_map(|(_, v)| v.split(';').next().filter(|c| c.starts_with("anichan_ws=")).map(str::to_owned))
        .ok_or_else(|| "session response carried no cookie".into())
}

impl Guest for AniChan {
    fn info() -> Info {
        Info {
            id: "anichan".into(),
            name: "chan".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            hosts: vec!["anichan.to".into(), "toroplay.click".into()],
        }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        let category = if title.dub { "dub" } else { "sub" };
        let episode = title.episode as i64;
        if title.episode.fract() != 0.0 {
            return Ok(vec![]);
        }
        let cookie = session()?;
        let response = host::http(&Request {
            method: "GET".into(),
            url: format!("{BASE}/api/watch/servers?anilistId={}&ep={episode}&category={category}", title.anilist_id),
            headers: vec![("Cookie".into(), cookie), ("Accept".into(), "application/json".into())],
            body: None,
        })?;
        if response.status == 404 {
            return Ok(vec![]);
        }
        if response.status >= 400 {
            return Err(format!("servers answered HTTP {}", response.status));
        }
        let root: Value = serde_json::from_slice(&response.body).map_err(|e| format!("bad JSON: {e}"))?;
        let mut servers: Vec<&Value> = root
            .get("servers")
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        servers.retain(|s| {
            let name = s.get("name").and_then(Value::as_str).unwrap_or("");
            !name.to_ascii_lowercase().starts_with("megaplay")
                && !name.eq_ignore_ascii_case("vidhawk")
                && s.get("stream").and_then(Value::as_str).is_some_and(|u| !u.is_empty())
        });
        servers.sort_by_key(|s| s.get("rank").and_then(Value::as_i64).unwrap_or(i64::MAX));
        host::log(&format!("anichan anilist={} ep={episode} {category}: {} server(s)", title.anilist_id, servers.len()));

        Ok(servers
            .into_iter()
            .map(|s| {
                let name = s
                    .get("label")
                    .and_then(Value::as_str)
                    .map(|l| l.trim().trim_start_matches('★').trim())
                    .filter(|l| !l.is_empty())
                    .or(s.get("name").and_then(Value::as_str))
                    .unwrap_or("AniChan")
                    .to_owned();
                let mut seen = Vec::new();
                let subtitles = s
                    .get("subtitles")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|t| {
                        let url = absolute(t.get("url").and_then(Value::as_str).filter(|u| !u.is_empty())?);
                        if seen.contains(&url) {
                            return None;
                        }
                        seen.push(url.clone());
                        let label = t.get("lang").and_then(Value::as_str).filter(|l| !l.is_empty()).unwrap_or("Subtitles");
                        Some(Subtitle { url, label: label.into(), language: language(label).into() })
                    })
                    .collect();
                Server {
                    name,
                    dub: title.dub,
                    headers: vec![],
                    streams: vec![MediaStream {
                        url: absolute(s.get("stream").and_then(Value::as_str).unwrap_or_default()),
                        format: "hls".into(),
                        quality: None,
                        height: None,
                    }],
                    subtitles,
                    skip_intro: None,
                    skip_outro: None,
                    thumbnails: None,
                }
            })
            .collect())
    }
}

export!(AniChan);
