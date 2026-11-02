//! AnimePahe, direct (not through Miruro), so its hardsub/dub Kwik streams survive a Miruro outage.
//!
//! `animepahe.pw` sits behind Cloudflare's JavaScript challenge, which no plain client passes, so
//! its requests go through `browser-fetch` (made inside the app's webview). Kwik itself answers
//! plain requests with a Referer.
//!
//! 1. `/api?m=search&q=` → candidate series (session ids). Cached.
//! 2. `/anime/<session>` → the page links `anilist.co/anime/<id>`: an exact match. Cached 7 days.
//! 3. `/api?m=release&id=<session>&sort=episode_asc&page=N` → episode sessions. Pahe numbers some
//!    sequels absolutely, so episode N is "first episode on page 1, plus N-1". Cached 6 hours.
//! 4. `/play/<session>/<episode session>` → Kwik embeds per resolution and audio (`jpn`/`eng`).
//! 5. Kwik embed → a Dean Edwards packed script → the HLS URL (on `uwucdn`). The app applies
//!    Kwik's filename-derived AES IVs to these streams itself.

use serde_json::Value;

wit_bindgen::generate!({ path: "../../wit", world: "provider" });

use anilili::provider::host::{self, Request};
use anilili::provider::types::MediaStream;

const ORIGIN: &str = "https://animepahe.pw/";

struct AnimePahe;

fn browser_get(url: &str) -> Result<Vec<u8>, String> {
    let response = host::browser_fetch(
        ORIGIN,
        &Request {
            method: "GET".into(),
            url: url.into(),
            headers: vec![("Accept".into(), "application/json, text/html".into())],
            body: None,
        },
    )?;
    if response.status >= 400 {
        return Err(format!("animepahe answered HTTP {} for {url}", response.status));
    }
    Ok(response.body)
}

fn browser_json(url: &str) -> Result<Value, String> {
    serde_json::from_slice(&browser_get(url)?).map_err(|e| format!("bad JSON: {e}"))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The series session whose page links this AniList id.
fn find_series(title: &Title) -> Result<Option<String>, String> {
    let key = format!("series:{}", title.anilist_id);
    if let Some(hit) = host::cache_get(&key) {
        return Ok((!hit.is_empty()).then_some(hit));
    }
    let needle = format!("anilist.co/anime/{}", title.anilist_id);
    for query in title.titles.iter().take(3) {
        let found = browser_json(&format!("{ORIGIN}api?m=search&l=8&q={}", encode(query)))?;
        for candidate in found.get("data").and_then(Value::as_array).into_iter().flatten().take(4) {
            let Some(session) = candidate.get("session").and_then(Value::as_str) else { continue };
            let page = String::from_utf8_lossy(&browser_get(&format!("{ORIGIN}anime/{session}"))?).into_owned();
            // Exact id match, not a prefix (anime/1545 must not match anime/154587).
            if page.match_indices(&needle).any(|(i, _)| !page[i + needle.len()..].starts_with(|c: char| c.is_ascii_digit())) {
                host::cache_set(&key, session, 7 * 24 * 3600);
                return Ok(Some(session.to_owned()));
            }
        }
    }
    // Remember a miss for a day, so titles AnimePahe lacks do not cost browser requests every time.
    host::cache_set(&key, "", 24 * 3600);
    Ok(None)
}

/// The episode session for app episode `episode` (1-based within the AniList title).
fn find_episode(series: &str, episode: i64) -> Result<Option<String>, String> {
    let key = format!("episode:{series}:{episode}");
    if let Some(hit) = host::cache_get(&key) {
        return Ok(Some(hit));
    }
    let first_page = browser_json(&format!("{ORIGIN}api?m=release&id={series}&sort=episode_asc&page=1"))?;
    let per_page = first_page.get("per_page").and_then(Value::as_i64).unwrap_or(30).max(1);
    let first = first_page
        .get("data")
        .and_then(Value::as_array)
        .and_then(|d| d.first())
        .and_then(|e| e.get("episode"))
        .and_then(Value::as_i64)
        .unwrap_or(1);
    let target = first + episode - 1;
    let page_no = (episode - 1) / per_page + 1;
    let page = if page_no == 1 {
        first_page
    } else {
        browser_json(&format!("{ORIGIN}api?m=release&id={series}&sort=episode_asc&page={page_no}"))?
    };
    for item in page.get("data").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(n), Some(session)) = (item.get("episode").and_then(Value::as_i64), item.get("session").and_then(Value::as_str)) {
            host::cache_set(&format!("episode:{series}:{}", n - first + 1), session, 6 * 3600);
            if n == target {
                return Ok(Some(session.to_owned()));
            }
        }
    }
    Ok(None)
}

/// `(kwik embed URL, height, audio)` from a play page.
fn embeds(html: &str) -> Vec<(String, u32, String)> {
    let attr = |tag: &str, name: &str| -> Option<String> {
        let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
        Some(tag[start..].split('"').next()?.to_owned())
    };
    html.split("<button")
        .filter(|tag| tag.contains("kwik"))
        .filter_map(|tag| {
            let tag = tag.split('>').next()?;
            Some((
                attr(tag, "data-src")?,
                attr(tag, "data-resolution")?.parse().ok()?,
                attr(tag, "data-audio").unwrap_or_else(|| "jpn".into()),
            ))
        })
        .collect()
}

/// Dean Edwards' packer: `}('<payload>',<radix>,<count>,'<words>'.split('|')`, every packed
/// script in the page. Returns the unpacked sources.
fn unpack_all(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (start, _) in html.match_indices("}('") {
        let rest = &html[start + 3..];
        let Some(payload_end) = find_unescaped_quote(rest) else { continue };
        let payload = &rest[..payload_end];
        let mut tail = rest[payload_end + 1..].trim_start_matches(',').splitn(3, ',');
        let (Some(radix), Some(count), Some(words)) = (tail.next(), tail.next(), tail.next()) else { continue };
        let (Ok(radix), Ok(count)) = (radix.trim().parse::<u32>(), count.trim().parse::<usize>()) else { continue };
        let Some(words) = words.trim_start_matches('\'').split('\'').next() else { continue };
        let words: Vec<&str> = words.split('|').collect();
        out.push(substitute(payload, radix, count, &words));
    }
    out
}

fn find_unescaped_quote(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\'' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn to_radix(mut n: usize, radix: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut s = Vec::new();
    loop {
        s.push(DIGITS[n % radix as usize]);
        n /= radix as usize;
        if n == 0 {
            break;
        }
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn substitute(payload: &str, radix: u32, count: usize, words: &[&str]) -> String {
    let table: std::collections::HashMap<String, &str> =
        (0..count).map(|i| (to_radix(i, radix), words.get(i).copied().filter(|w| !w.is_empty()).unwrap_or(""))).collect();
    let mut out = String::with_capacity(payload.len() * 2);
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if !word.is_empty() {
            match table.get(word.as_str()) {
                Some(replacement) if !replacement.is_empty() => out.push_str(replacement),
                _ => out.push_str(word),
            }
            word.clear();
        }
    };
    for c in payload.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            flush(&mut word, &mut out);
            out.push(c);
        }
    }
    flush(&mut word, &mut out);
    out
}

fn kwik_stream(embed: &str) -> Result<Option<String>, String> {
    let response =
        host::http(&Request { method: "GET".into(), url: embed.into(), headers: vec![("Referer".into(), ORIGIN.into())], body: None })?;
    if response.status >= 400 {
        return Err(format!("kwik answered HTTP {}", response.status));
    }
    let html = String::from_utf8_lossy(&response.body);
    for script in unpack_all(&html) {
        if let Some(start) = script.find("https://")
            && let Some(url) = script[start..].split(['\'', '"', '\\', ';', ' ']).find(|u| u.contains(".m3u8"))
        {
            return Ok(Some(url.to_owned()));
        }
        // The URL is not always the first one in the script; scan them all.
        for (i, _) in script.match_indices("https://") {
            let candidate: String = script[i..].chars().take_while(|c| !matches!(c, '\'' | '"' | '\\' | ';' | ' ')).collect();
            if candidate.contains(".m3u8") {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
}

impl Guest for AnimePahe {
    fn info() -> Info {
        Info {
            id: "pahe-direct".into(),
            name: "pahe".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            hosts: vec!["animepahe.pw".into(), "kwik.cx".into()],
        }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        if title.episode.fract() != 0.0 {
            return Ok(vec![]);
        }
        let Some(series) = find_series(&title)? else { return Ok(vec![]) };
        let Some(episode) = find_episode(&series, title.episode as i64)? else { return Ok(vec![]) };
        let page = String::from_utf8_lossy(&browser_get(&format!("{ORIGIN}play/{series}/{episode}"))?).into_owned();
        let audio = if title.dub { "eng" } else { "jpn" };
        let mut wanted: Vec<(String, u32, String)> = embeds(&page).into_iter().filter(|(_, _, a)| a == audio).collect();
        wanted.sort_by_key(|(_, h, _)| std::cmp::Reverse(*h));
        let mut streams = Vec::new();
        for (embed, height, _) in wanted.into_iter().take(2) {
            if let Some(url) = kwik_stream(&embed)? {
                streams.push(MediaStream { url, format: "hls".into(), quality: Some(format!("{height}p")), height: Some(height) });
            }
        }
        host::log(&format!("animepahe anilist={} ep={} {audio}: {} stream(s)", title.anilist_id, title.episode, streams.len()));
        if streams.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![Server {
            name: "Kwik".into(),
            dub: title.dub,
            headers: vec![("Referer".into(), "https://kwik.cx/".into())],
            streams,
            subtitles: vec![],
            skip_intro: None,
            skip_outro: None,
            thumbnails: None,
        }])
    }
}

export!(AnimePahe);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unpacks_dean_edwards() {
        // eval(function(p,a,c,k,e,d){...}('0 1=\'2://3.4/5.6\'',10,7,'const|source|https|vault|cdn|uwu|m3u8'.split('|'),0,{}))
        let html = r"}('0 1=\'2://3.4/5.6\'',10,7,'const|source|https|vault|cdn|uwu|m3u8'.split('|'),0,{}))";
        let scripts = unpack_all(html);
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].contains("const source=\\'https://vault.cdn/uwu.m3u8\\'"), "{}", scripts[0]);
    }

    #[test]
    fn reads_play_page_embeds() {
        let html = r#"<button data-src="https://kwik.cx/e/a" data-url="x" data-fansub="SEV" data-resolution="1080" data-audio="jpn" class="dropdown-item active">"#;
        assert_eq!(embeds(html), vec![("https://kwik.cx/e/a".to_owned(), 1080, "jpn".to_owned())]);
    }
}
