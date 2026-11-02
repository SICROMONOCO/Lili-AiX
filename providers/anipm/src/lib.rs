//! ani.pm, resolved through Settlar. Keyed by AniList id, so no title matching:
//!
//! 1. `ani.pm/api/anime/playback-bootstrap/anilist/{id}?ep=N&lang=sub|dub` → `settlarSelection`,
//!    the language actually served, and skip times.
//! 2. `ani.pm/api/anime/settlar/session?selection=…` → `embedUrl` with a session token `t`.
//! 3. `embed.settlar.io/api/embed/session?t=…` → the HLS `source` and VTT `subtitles`.
//!
//! All three are behind a Cloudflare TLS-fingerprint gate, which the app's `http` passes. A missing
//! dub is answered with `effectiveLanguage: "sub"`, so that is compared against what was asked.

use serde_json::Value;

wit_bindgen::generate!({ path: "../../wit", world: "provider" });

use anilili::provider::host::{self, Request};
use anilili::provider::types::{MediaStream, Subtitle};

struct AniPm;

const UA_ACCEPT: (&str, &str) = ("Accept", "application/json");

fn get_json(url: &str) -> Result<Value, String> {
    let response = host::http(&Request {
        method: "GET".into(),
        url: url.into(),
        headers: vec![(UA_ACCEPT.0.into(), UA_ACCEPT.1.into()), ("Referer".into(), "https://ani.pm/".into())],
        body: None,
    })?;
    if response.status >= 400 {
        return Err(format!("{} answered HTTP {}", host_of(url), response.status));
    }
    serde_json::from_slice(&response.body).map_err(|e| format!("bad JSON from {}: {e}", host_of(url)))
}

fn host_of(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn window(skip: &Value, key: &str) -> Option<(f64, f64)> {
    let w = skip.get(key)?;
    let (start, end) = (w.get("start")?.as_f64()?, w.get("end")?.as_f64()?);
    (end > start).then_some((start, end))
}

impl Guest for AniPm {
    fn info() -> Info {
        Info {
            id: "anipm".into(),
            name: "pm".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            hosts: vec!["ani.pm".into(), "settlar.io".into()],
        }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        let lang = if title.dub { "dub" } else { "sub" };
        let episode = if title.episode.fract() == 0.0 { format!("{}", title.episode as i64) } else { title.episode.to_string() };
        let boot =
            get_json(&format!("https://ani.pm/api/anime/playback-bootstrap/anilist/{}?ep={episode}&lang={lang}&core=0", title.anilist_id))?;
        if boot.get("effectiveLanguage").and_then(Value::as_str) != Some(lang) {
            return Ok(vec![]);
        }
        let selection = boot.get("settlarSelection").and_then(Value::as_str).ok_or("no selection for this episode")?;
        let session = get_json(&format!(
            "https://ani.pm/api/anime/settlar/session?selection={}&provider=anipm&ep={episode}&channel={lang}&telemetry=0",
            encode(selection)
        ))?;
        let embed = session.get("embedUrl").and_then(Value::as_str).ok_or("no embed session")?;
        let token = embed.split("t=").nth(1).map(|t| t.split('&').next().unwrap_or(t)).ok_or("embed URL without a token")?;
        let player = get_json(&format!("https://embed.settlar.io/api/embed/session?t={token}"))?;
        let source = player.get("source").and_then(Value::as_str).ok_or("no stream in the session")?;
        host::log(&format!("ani.pm resolved anilist={} ep={episode} {lang}", title.anilist_id));

        let subtitles = player
            .get("subtitles")
            .and_then(Value::as_array)
            .map(|subs| {
                subs.iter()
                    .filter_map(|s| {
                        let url = s.get("url").or(s.get("file")).and_then(Value::as_str)?;
                        let language = s.get("lang").or(s.get("language")).and_then(Value::as_str).unwrap_or("en");
                        Some(Subtitle {
                            url: url.into(),
                            label: s.get("label").and_then(Value::as_str).unwrap_or(language).into(),
                            language: language.into(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let skip = boot.get("skip").cloned().unwrap_or(Value::Null);
        Ok(vec![Server {
            name: "Settlar".into(),
            dub: title.dub,
            headers: vec![("Referer".into(), "https://embed.settlar.io/".into())],
            streams: vec![MediaStream { url: source.into(), format: "hls".into(), quality: None, height: None }],
            subtitles,
            skip_intro: window(&skip, "op"),
            skip_outro: window(&skip, "ed"),
            thumbnails: None,
        }])
    }
}

export!(AniPm);
