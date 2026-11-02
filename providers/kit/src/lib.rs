//! What every provider needs and none should write twice: requests through the host, the private
//! cache, HTML scraping helpers, and matching an AniList title against a site's search results.
//!
//! Ported in spirit from the Android app's `NativeProviderParsers` (title scores, season markers,
//! attribute reading), so a title matches the same site entry on both apps.
//!
//! Providers reuse this crate's bindings instead of generating their own:
//!
//! ```ignore
//! wit_bindgen::generate!({
//!     path: "../../wit",
//!     world: "provider",
//!     with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
//! });
//! ```

wit_bindgen::generate!({ path: "../../wit", world: "kit" });

pub use anilili::provider::{host, types};

use host::{Request, Response};
use serde_json::Value;

// ---- Requests ------------------------------------------------------------------------------

/// A request through the app's client. Fails on transport errors only; see [`get_text`].
pub fn send(method: &str, url: &str, headers: &[(&str, &str)], body: Option<Vec<u8>>) -> Result<Response, String> {
    host::http(&Request {
        method: method.into(),
        url: url.into(),
        headers: headers.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
        body,
    })
}

fn checked(url: &str, response: Response) -> Result<Vec<u8>, String> {
    if response.status >= 400 {
        return Err(format!("{} answered HTTP {}", host_of(url), response.status));
    }
    Ok(response.body)
}

/// GET, failing on an HTTP error status.
pub fn get_bytes(url: &str, headers: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    checked(url, send("GET", url, headers, None)?)
}

pub fn get_text(url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&get_bytes(url, headers)?).into_owned())
}

pub fn get_json(url: &str, headers: &[(&str, &str)]) -> Result<Value, String> {
    let mut all = vec![("Accept", "application/json, */*")];
    all.extend_from_slice(headers);
    parse_json(url, &get_bytes(url, &all)?)
}

pub fn post_json(url: &str, headers: &[(&str, &str)], body: &Value) -> Result<Value, String> {
    let mut all = vec![("Accept", "application/json, */*"), ("Content-Type", "application/json")];
    all.extend_from_slice(headers);
    let response = send("POST", url, &all, Some(body.to_string().into_bytes()))?;
    parse_json(url, &checked(url, response)?)
}

fn parse_json(url: &str, body: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(body).map_err(|e| format!("bad JSON from {}: {e}", host_of(url)))
}

/// GET made inside a browser page on `origin`, for sites behind a JavaScript challenge.
pub fn browser_text(origin: &str, url: &str, headers: &[(&str, &str)]) -> Result<String, String> {
    let request = Request {
        method: "GET".into(),
        url: url.into(),
        headers: headers.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
        body: None,
    };
    Ok(String::from_utf8_lossy(&checked(url, host::browser_fetch(origin, &request)?)?).into_owned())
}

// ---- Cache ---------------------------------------------------------------------------------

/// `key` from the provider's cache, or `fill()` stored for `ttl` seconds. An empty string is a
/// remembered miss, kept for `miss_ttl` seconds so a title a site lacks costs no requests.
pub fn cached(key: &str, ttl: u32, miss_ttl: u32, fill: impl FnOnce() -> Result<Option<String>, String>) -> Result<Option<String>, String> {
    if let Some(hit) = host::cache_get(key) {
        return Ok((!hit.is_empty()).then_some(hit));
    }
    let value = fill()?;
    match &value {
        Some(v) => host::cache_set(key, v, ttl),
        None => host::cache_set(key, "", miss_ttl),
    }
    Ok(value)
}

pub const DAY: u32 = 24 * 3600;

// ---- Text and HTML -------------------------------------------------------------------------

/// Percent-encoding for a query value.
pub fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn host_of(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split(['/', '?', '#']).next()).unwrap_or(url)
}

/// `value` resolved against the page it came from (absolute, protocol-relative or root-relative).
pub fn absolute_url(base: &str, value: &str) -> String {
    if value.starts_with("http://") || value.starts_with("https://") {
        return value.to_owned();
    }
    let scheme = base.split("://").next().unwrap_or("https");
    if let Some(rest) = value.strip_prefix("//") {
        return format!("{scheme}://{rest}");
    }
    let origin = format!("{scheme}://{}", host_of(base));
    if value.starts_with('/') {
        return format!("{origin}{value}");
    }
    let dir = base.rsplit_once('/').map(|(d, _)| d).filter(|d| d.len() > origin.len()).unwrap_or(&origin);
    format!("{dir}/{value}")
}

/// HTML entities and JSON's escaped slash.
pub fn decode_entities(value: &str) -> String {
    if !value.contains('&') && !value.contains('\\') {
        return value.to_owned();
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(i) = rest.find(['&', '\\']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if let Some(after) = rest.strip_prefix("\\/") {
            out.push('/');
            rest = after;
            continue;
        }
        let entity = rest.find(';').filter(|&end| end <= 10).map(|end| (&rest[1..end], end));
        let decoded = entity.and_then(|(name, end)| {
            let c = match name {
                "quot" => '"',
                "apos" | "#39" => '\'',
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "nbsp" => ' ',
                _ => {
                    let code = match name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => name.strip_prefix('#')?.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push_str(&rest[..1]);
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// An attribute's value from one tag, quoted or not, entities decoded. Empty when absent.
pub fn attr(tag: &str, name: &str) -> String {
    let lower = tag.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(&name).map(|i| i + from) {
        from = i + name.len();
        // Whole attribute name: not the tail of a longer one (`src` in `data-src`).
        if i > 0 && !lower.as_bytes()[i - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = tag[from..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let raw = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[1..].split(q).next().unwrap_or(""),
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        return decode_entities(raw);
    }
    String::new()
}

/// Each `<name …>` opening tag in `html`, as text.
pub fn tags<'a>(html: &'a str, name: &str) -> impl Iterator<Item = &'a str> + 'a {
    let open = format!("<{}", name.to_ascii_lowercase());
    let lower = html.to_ascii_lowercase();
    let starts: Vec<usize> = lower
        .match_indices(&open)
        .map(|(i, _)| i)
        .filter(|&i| lower.as_bytes().get(i + open.len()).is_some_and(|b| b.is_ascii_whitespace() || *b == b'>' || *b == b'/'))
        .collect();
    starts.into_iter().filter_map(move |i| html[i..].find('>').map(|end| &html[i..=i + end]))
}

/// Text with tags removed and whitespace collapsed.
pub fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    decode_entities(&out.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// Every HLS playlist URL written anywhere in a page or script, escaped slashes undone.
pub fn hls_urls(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let stop = |c: char| matches!(c, '"' | '\'' | '<' | '>' | '`') || c.is_whitespace();
    let mut urls: Vec<String> = Vec::new();
    for (i, _) in lower.match_indices(".m3u8") {
        let start = text[..i].rfind(stop).map_or(0, |s| s + 1);
        let end = text[i..].find(stop).map_or(text.len(), |e| i + e);
        let url = decode_entities(&text[start..end]);
        let url = if url.starts_with("//") { format!("https:{url}") } else { url };
        if url.starts_with("http") && !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls
}

/// The text between `start` and the next `end` after it.
pub fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    Some(&text[from..from + text[from..].find(end)?])
}

/// The JavaScript string literal that starts right after `marker` (quoted with `'` or `"`),
/// unescaped: for data a page hands its scripts as `JSON.parse('…')`.
pub fn js_string(text: &str, marker: &str) -> Option<String> {
    let rest = &text[text.find(marker)? + marker.len()..];
    let quote = rest.chars().next().filter(|c| matches!(c, '\'' | '"'))?;
    let mut out = String::new();
    let mut chars = rest[1..].chars();
    while let Some(c) = chars.next() {
        match c {
            c if c == quote => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let unit = u32::from_str_radix(&hex, 16).ok()?;
                    // A surrogate pair is two escapes; anything unpaired becomes U+FFFD.
                    if (0xD800..0xDC00).contains(&unit) {
                        let low: String = chars.clone().take(6).collect();
                        if let Some(low) =
                            low.strip_prefix("\\u").and_then(|h| u32::from_str_radix(h, 16).ok()).filter(|l| (0xDC00..0xE000).contains(l))
                        {
                            chars.by_ref().take(6).for_each(drop);
                            out.push(char::from_u32(0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)).unwrap_or('\u{FFFD}'));
                            continue;
                        }
                    }
                    out.push(char::from_u32(unit).unwrap_or('\u{FFFD}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// Standard base64 (padding optional, whitespace ignored).
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    };
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').collect();
    let mut out = Vec::with_capacity(digits.len() * 3 / 4);
    for chunk in digits.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= value(c)? << (18 - 6 * i);
        }
        let bytes = n.to_be_bytes();
        out.extend_from_slice(&bytes[1..chunk.len()]);
    }
    Some(out)
}

/// `1` for whole episodes, `6.5` for half ones.
pub fn episode_label(episode: f64) -> String {
    if episode.fract() == 0.0 { format!("{}", episode as i64) } else { episode.to_string() }
}

/// The ISO 639-1 code for a subtitle label like "Portuguese (Brazil)"; `und` when unknown.
pub fn language(label: &str) -> String {
    let first = label.trim().split([' ', '(', '-', '[']).next().unwrap_or("").to_ascii_lowercase();
    let code = match first.as_str() {
        "english" | "eng" | "en" => "en",
        "spanish" | "español" | "espanol" | "spa" | "es" => "es",
        "portuguese" | "português" | "por" | "pt" => "pt",
        "french" | "français" | "fre" | "fr" => "fr",
        "german" | "deutsch" | "ger" | "de" => "de",
        "italian" | "italiano" | "ita" | "it" => "it",
        "arabic" | "ara" | "ar" => "ar",
        "russian" | "rus" | "ru" => "ru",
        "indonesian" | "ind" | "id" => "id",
        "korean" | "kor" | "ko" => "ko",
        "japanese" | "jpn" | "ja" => "ja",
        "chinese" | "chi" | "zh" => "zh",
        "danish" => "da",
        "dutch" => "nl",
        "finnish" => "fi",
        "norwegian" => "no",
        "polish" => "pl",
        "swedish" => "sv",
        "turkish" | "tur" | "tr" => "tr",
        "thai" | "th" => "th",
        "vietnamese" | "vie" | "vi" => "vi",
        "hindi" | "hin" | "hi" => "hi",
        "malay" | "may" | "ms" => "ms",
        _ => "und",
    };
    code.into()
}

// ---- Title matching ------------------------------------------------------------------------

/// Lowercase ASCII letters and digits only.
pub fn normalize(value: &str) -> String {
    value.chars().flat_map(char::to_lowercase).filter(char::is_ascii_alphanumeric).collect()
}

/// Dice similarity over letter pairs; containment scores 0.86, as on Android.
pub fn title_score(query: &str, candidate: &str) -> f64 {
    normalized_score(&normalize(query), &normalize(candidate))
}

fn normalized_score(a: &str, b: &str) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    if a == b {
        return 1.0;
    }
    if a.contains(b) || b.contains(a) {
        return 0.86;
    }
    if a.len() < 2 || b.len() < 2 {
        return 0.0;
    }
    let pairs = |s: &str| -> Vec<[u8; 2]> { s.as_bytes().windows(2).map(|w| [w[0], w[1]]).collect() };
    let mut left = pairs(a);
    let right = pairs(b);
    let mut hits = 0;
    for pair in &right {
        if let Some(i) = left.iter().position(|p| p == pair) {
            left.swap_remove(i);
            hits += 1;
        }
    }
    2.0 * hits as f64 / (a.len() - 1 + b.len() - 1) as f64
}

/// [`title_score`] with a length tie-breaker, so the base series beats a similarly named spinoff.
pub fn title_selection_score(query: &str, candidate: &str) -> f64 {
    let (a, b) = (normalize(query), normalize(candidate));
    let similarity = normalized_score(&a, &b);
    if a.is_empty() || b.is_empty() || a == b {
        return similarity;
    }
    let ratio = a.len().min(b.len()) as f64 / a.len().max(b.len()) as f64;
    similarity * (0.6 + 0.4 * ratio)
}

/// The season a title claims: "Season 2", "2nd Season", "II", a trailing "2" (not "No. 8").
pub fn season_number(title: &str) -> Option<u32> {
    let lower = title.trim().to_ascii_lowercase();
    let words: Vec<&str> =
        lower.split(|c: char| c.is_whitespace() || matches!(c, ':' | '-' | '(' | ')' | '[' | ']')).filter(|w| !w.is_empty()).collect();
    for (i, word) in words.iter().enumerate() {
        if *word == "season" {
            if let Some(n) = words.get(i + 1).and_then(|n| n.parse().ok()) {
                return Some(n);
            }
            if let Some(n) = i.checked_sub(1).and_then(|p| words[p].trim_end_matches(['s', 't', 'n', 'd', 'r', 'h']).parse().ok()) {
                return Some(n);
            }
        }
    }
    let last = *words.last()?;
    match last {
        "ii" => return Some(2),
        "iii" => return Some(3),
        "iv" => return Some(4),
        "v" => return Some(5),
        "vi" => return Some(6),
        _ => {}
    }
    let previous = words.len().checked_sub(2).map(|i| words[i]);
    let after_marker = previous.is_some_and(|p| p.ends_with('.') || p.ends_with('#'));
    match last.parse::<u32>() {
        Ok(n @ 2..=9) if !after_marker && words.len() > 1 => Some(n),
        _ => None,
    }
}

/// An AniList sequel name cut back to the series a site files it under: "Solo Leveling Season 2
/// -Arise from the Shadow-" → "Solo Leveling". Unchanged when there's no season marker.
pub fn base_series_title(title: &str) -> String {
    let lower = title.to_ascii_lowercase();
    let Some(at) = lower.match_indices("season").map(|(i, _)| i).find(|&i| i == 0 || !lower.as_bytes()[i - 1].is_ascii_alphanumeric())
    else {
        return title.trim().to_owned();
    };
    let mut head = title[..at].trim_end().to_owned();
    loop {
        let lower = head.to_ascii_lowercase();
        let last = lower.rsplit(' ').next().unwrap_or("");
        let ordinal =
            last.len() > 2 && last[..last.len() - 2].parse::<u32>().is_ok() && ["st", "nd", "rd", "th"].contains(&&last[last.len() - 2..]);
        if ordinal || last == "the" || last == "final" {
            head.truncate(head.len() - last.len());
            head = head.trim_end().to_owned();
        } else {
            break;
        }
    }
    head.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '–' | '—' | ':' | '|' | '(' | '[')).to_owned()
}

/// Search queries for a title: its names, then each sequel name cut back to its series.
pub fn search_queries(titles: &[String], max: usize) -> Vec<String> {
    let mut queries: Vec<String> = Vec::new();
    let mut push = |q: String| {
        if !q.trim().is_empty() && !queries.iter().any(|x| normalize(x) == normalize(&q)) {
            queries.push(q);
        }
    };
    for t in titles.iter().filter(|t| t.is_ascii()) {
        push(t.clone());
    }
    for t in titles.iter().filter(|t| t.is_ascii()) {
        push(base_series_title(t));
    }
    queries.truncate(max);
    queries
}

/// One entry from a site's search.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// What the site calls it in URLs (slug or id).
    pub id: String,
    pub title: String,
}

/// How well `candidate` fits the AniList names, 0–1: the best name score, halved when the
/// candidate claims a different season than the names do.
pub fn candidate_score(titles: &[String], candidate: &Candidate) -> f64 {
    let best = titles
        .iter()
        .flat_map(|t| [title_selection_score(t, &candidate.title), title_selection_score(t, &candidate.id.replace('-', " "))])
        .fold(0.0, f64::max);
    let wanted = titles.iter().find_map(|t| season_number(t)).unwrap_or(1);
    let offered = season_number(&candidate.title).unwrap_or(1);
    if wanted == offered { best } else { best * 0.5 }
}

/// The best-scoring candidate, if it scores at least `threshold`.
pub fn best_match<'a>(titles: &[String], candidates: &'a [Candidate], threshold: f64) -> Option<&'a Candidate> {
    candidates
        .iter()
        .map(|c| (candidate_score(titles, c), c))
        .filter(|(score, _)| *score >= threshold)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, c)| c)
}

/// The site's id for an AniList title: cached for a week (a miss for a day); otherwise each search
/// query in turn until one gives a match scoring at least `threshold`.
pub fn resolve_id(
    anilist_id: u32,
    titles: &[String],
    threshold: f64,
    mut search: impl FnMut(&str) -> Result<Vec<Candidate>, String>,
) -> Result<Option<String>, String> {
    cached(&format!("id:{anilist_id}"), 7 * DAY, DAY, || {
        let mut seen: Vec<Candidate> = Vec::new();
        let mut last_error = None;
        for query in search_queries(titles, 4) {
            match search(&query) {
                Ok(found) => seen.extend(found.into_iter().filter(|c| !c.id.is_empty())),
                Err(e) => last_error = Some(e),
            }
            if let Some(best) = best_match(titles, &seen, threshold.max(0.9)) {
                return Ok(Some(best.id.clone()));
            }
        }
        if seen.is_empty()
            && let Some(e) = last_error
        {
            return Err(e);
        }
        Ok(best_match(titles, &seen, threshold).map(|c| c.id.clone()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_attributes() {
        let tag = r#"<track kind=subtitles src="/s/en.vtt?a=1&amp;b=2" data-src='x' label="English">"#;
        assert_eq!(attr(tag, "src"), "/s/en.vtt?a=1&b=2");
        assert_eq!(attr(tag, "data-src"), "x");
        assert_eq!(attr(tag, "kind"), "subtitles");
        assert_eq!(attr(tag, "missing"), "");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("a &amp; b &#39;c&#x27; https:\\/\\/x"), "a & b 'c' https://x");
        assert_eq!(decode_entities("AT&T"), "AT&T");
    }

    #[test]
    fn finds_playlists() {
        let page = r#"x = "https:\/\/cdn.example\/a\/master.m3u8?t=1"; y='//h.example/b.m3u8'"#;
        assert_eq!(hls_urls(page), ["https://cdn.example/a/master.m3u8?t=1", "https://h.example/b.m3u8"]);
    }

    #[test]
    fn resolves_urls() {
        assert_eq!(absolute_url("https://a.to/anime/x/1", "/s.vtt"), "https://a.to/s.vtt");
        assert_eq!(absolute_url("https://a.to/anime/x/1", "//b.to/s.vtt"), "https://b.to/s.vtt");
        assert_eq!(absolute_url("https://a.to/anime/x/1", "s.vtt"), "https://a.to/anime/x/s.vtt");
    }

    #[test]
    fn reads_seasons() {
        assert_eq!(season_number("Solo Leveling Season 2 -Arise from the Shadow-"), Some(2));
        assert_eq!(season_number("Mushoku Tensei 2nd Season"), Some(2));
        assert_eq!(season_number("Saga of Tanya the Evil II"), Some(2));
        assert_eq!(season_number("Gintama 2"), Some(2));
        assert_eq!(season_number("Kaiju No. 8"), None);
        assert_eq!(season_number("Frieren"), None);
        assert_eq!(base_series_title("Solo Leveling Season 2 -Arise from the Shadow-"), "Solo Leveling");
        assert_eq!(base_series_title("Attack on Titan Final Season"), "Attack on Titan");
        assert_eq!(base_series_title("Mushoku Tensei 2nd Season"), "Mushoku Tensei");
        assert_eq!(base_series_title("Frieren"), "Frieren");
    }

    #[test]
    fn matches_titles() {
        let titles = vec!["Frieren: Beyond Journey's End".to_owned(), "Sousou no Frieren".to_owned()];
        let candidates = vec![
            Candidate { id: "sousou-no-frieren-2".into(), title: "Sousou no Frieren 2nd Season".into() },
            Candidate { id: "sousou-no-frieren".into(), title: "Sousou no Frieren".into() },
            Candidate { id: "frieren-mini".into(), title: "Sousou no Frieren: Marumaru no Mahou".into() },
        ];
        assert_eq!(best_match(&titles, &candidates, 0.5).map(|c| c.id.as_str()), Some("sousou-no-frieren"));
        assert!(best_match(&titles, &[Candidate { id: "naruto".into(), title: "Naruto".into() }], 0.5).is_none());
    }

    #[test]
    fn reads_js_strings() {
        let page = r#"x-data="{ items: JSON.parse('[{"slug":"ab","url":"http:\\\/\\\/a.to","t":"\\u846c it\'s"}]') }""#;
        let json = js_string(page, "JSON.parse(").unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value[0]["url"], "http://a.to");
        assert_eq!(value[0]["t"], "葬 it's");
    }

    #[test]
    fn decodes_base64() {
        assert_eq!(base64_decode("aGVsbG8gd29ybGQ=").unwrap(), b"hello world");
        assert_eq!(base64_decode("aGk").unwrap(), b"hi");
    }

    /// The cases both apps must answer alike (fixtures/parity/titles.json, Android's answers).
    #[test]
    fn matches_titles_as_android_does() {
        let corpus: Value = serde_json::from_str(include_str!("../../../fixtures/parity/titles.json")).unwrap();
        let text = |c: &Value, k: &str| c[k].as_str().unwrap().to_owned();
        for case in corpus["seasonNumber"].as_array().unwrap() {
            let expected = case["expected"].as_u64().map(|n| n as u32);
            assert_eq!(season_number(&text(case, "title")), expected, "seasonNumber {case}");
        }
        for case in corpus["baseSeriesTitle"].as_array().unwrap() {
            assert_eq!(base_series_title(&text(case, "title")), text(case, "expected"), "baseSeriesTitle {case}");
        }
        for case in corpus["titleScore"].as_array().unwrap() {
            let score = title_score(&text(case, "query"), &text(case, "candidate"));
            if let Some(exactly) = case["exactly"].as_f64() {
                assert!((score - exactly).abs() < 1e-9, "titleScore {case}: {score}");
            }
            if let Some(above) = case["above"].as_f64() {
                assert!(score > above, "titleScore {case}: {score}");
            }
        }
        for case in corpus["selectionOrder"].as_array().unwrap() {
            let query = text(case, "query");
            let (better, worse) =
                (title_selection_score(&query, &text(case, "better")), title_selection_score(&query, &text(case, "worse")));
            assert!(better > worse, "selectionOrder {case}: {better} <= {worse}");
        }
    }
}
