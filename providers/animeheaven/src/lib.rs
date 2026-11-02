//! AnimeHeaven (animeheaven.me): three plain GETs and no player page.
//!
//! 1. `search.php?s=` → `similarimg` cards linking `anime.php?<id>`. The search is literal and
//!    files sequels under its own numbering ("… II", "… 2"), so the season-stripped name is asked
//!    first, and candidates are ranked on season before title score: on title score alone, "Saga of
//!    Tanya the Evil" beats "Saga of Tanya the Evil II" for "… Season 2" (0.764 vs 0.727).
//! 2. `anime.php?<id>` → one `gatea("<hash>")` link per episode, its number in a `watch2` div.
//! 3. `gate.php` with cookie `key=<hash>` → `<source>` tags: the same MP4 on up to three hosts, in
//!    failover order. The file URL carries a short-lived token, so this is asked at play time.
//!
//! Progressive MP4 that needs no Referer and has no Cloudflare in front. Sub only, subtitles burned
//! in; one quality (720p on titles measured). Port of Android's `AnimeHeavenProvider`.

use provider_kit::{self as kit, DAY, types::*};

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
    with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
});

const BASE: &str = "https://animeheaven.me";
const MIN_TITLE_SCORE: f64 = 0.28;
const MAX_PROBE_PAGES: usize = 3;

struct AnimeHeaven;

fn get(url: &str, referer: &str, cookie: Option<&str>) -> Result<String, String> {
    let mut headers = vec![("Referer", referer), ("Accept", "text/html,application/xhtml+xml,*/*")];
    if let Some(cookie) = cookie {
        headers.push(("Cookie", cookie));
    }
    kit::get_text(url, &headers)
}

/// `(anime.php?… path, title)` per search card.
fn parse_search(html: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for card in html.split("class='similarimg'").skip(1).chain(html.split("class='chart'").skip(1)) {
        let Some(href) = kit::tags(card, "a").map(|a| kit::attr(a, "href")).find(|h| h.contains("anime.php?")) else { continue };
        let path = href.trim_start_matches('/').to_owned();
        let caption = ["similarname", "charttitle"]
            .iter()
            .find_map(|class| card.split(class).nth(1))
            .and_then(|rest| kit::between(rest, "<a", "</a>"))
            .and_then(|a| a.split_once('>').map(|(_, text)| kit::strip_tags(text)))
            .filter(|t| !t.is_empty());
        let title = caption.or_else(|| kit::tags(card, "img").map(|i| kit::attr(i, "alt")).find(|a| !a.is_empty()));
        if let Some(title) = title
            && !out.iter().any(|(p, _)| *p == path)
        {
            out.push((path, title));
        }
    }
    out
}

/// `(episode number, gate key)`, ascending. A number is looked for only up to the next link, so a
/// row without one can't borrow the following episode's number.
fn parse_episodes(html: &str) -> Vec<(u32, String)> {
    let starts: Vec<usize> = html.match_indices("gatea(").map(|(i, _)| i).collect();
    let mut out: Vec<(u32, String)> = Vec::new();
    for (n, &start) in starts.iter().enumerate() {
        let row = &html[start + "gatea(".len()..starts.get(n + 1).copied().unwrap_or(html.len())];
        let key: String = row
            .trim_start()
            .trim_start_matches(['"', '\''])
            .trim_start_matches("&quot;")
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .collect();
        let number = row
            .split("watch2")
            .nth(1)
            .and_then(|rest| rest.split_once('>'))
            .and_then(|(_, rest)| rest.split('<').next())
            .and_then(|text| text.trim().parse::<f64>().ok());
        if let (false, Some(number)) = (key.is_empty(), number)
            && !out.iter().any(|(e, _)| *e == number as u32)
        {
            out.push((number as u32, key));
        }
    }
    out.sort_by_key(|(e, _)| *e);
    out
}

/// The anime page for the title, by season fit first, then title score.
fn find_page(title: &Title) -> Result<Option<String>, String> {
    kit::cached(&format!("page:{}", title.anilist_id), 7 * DAY, 6 * 3600, || {
        let mut queries: Vec<String> = Vec::new();
        for t in title.titles.iter().filter(|t| t.is_ascii()) {
            for q in [kit::base_series_title(t), t.split(':').next().unwrap_or(t).trim().to_owned(), t.clone()] {
                if q.len() >= 3 && !queries.iter().any(|x| x.eq_ignore_ascii_case(&q)) {
                    queries.push(q);
                }
            }
        }
        let mut found = Vec::new();
        for query in queries.iter().take(3) {
            found = parse_search(&get(&format!("{BASE}/search.php?s={}", kit::encode(query)), &format!("{BASE}/"), None)?);
            if !found.is_empty() {
                break;
            }
        }
        let wanted = title.titles.iter().find_map(|t| kit::season_number(t));
        let names: Vec<String> = title.titles.iter().flat_map(|t| [t.clone(), kit::base_series_title(t)]).collect();
        let mut ranked: Vec<(u8, f64, String)> = found
            .into_iter()
            .map(|(path, name)| {
                let score = names.iter().map(|n| kit::title_selection_score(n, &name)).fold(0.0, f64::max);
                let season = match wanted {
                    None => 1,
                    Some(w) if kit::season_number(&name).unwrap_or(1) == w => 2,
                    Some(_) => 0,
                };
                (season, score, path)
            })
            .filter(|(_, score, _)| *score >= MIN_TITLE_SCORE)
            .collect();
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)));
        for (_, _, path) in ranked.into_iter().take(MAX_PROBE_PAGES) {
            if !parse_episodes(&get(&format!("{BASE}/{path}"), &format!("{BASE}/"), None)?).is_empty() {
                return Ok(Some(path));
            }
        }
        Ok(None)
    })
}

impl Guest for AnimeHeaven {
    fn info() -> Info {
        Info {
            id: "animeheaven".into(),
            name: "heaven".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            hosts: vec!["animeheaven.me".into()],
        }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        if title.dub {
            return Ok(vec![]);
        }
        let Some(path) = find_page(&title)? else { return Ok(vec![]) };
        let page_url = format!("{BASE}/{path}");
        let episodes = parse_episodes(&get(&page_url, &format!("{BASE}/"), None)?);
        let Some((_, key)) = episodes.iter().find(|(n, _)| f64::from(*n) == title.episode) else { return Ok(vec![]) };
        let gate = get(&format!("{BASE}/gate.php"), &page_url, Some(&format!("key={key}")))?;
        let mut urls: Vec<String> = Vec::new();
        for src in kit::tags(&gate, "source").map(|t| kit::attr(t, "src").trim().to_owned()) {
            if src.to_ascii_lowercase().contains(".mp4") && !urls.contains(&src) {
                urls.push(src);
            }
        }
        kit::host::log(&format!("animeheaven resolved anilist={} {path} ep={} mirrors={}", title.anilist_id, title.episode, urls.len()));
        // One server per mirror, in the page's failover order: the same file on different hosts,
        // named by host, not a quality ladder.
        Ok(urls
            .into_iter()
            .map(|url| {
                let host = kit::host_of(&url).split('.').next().unwrap_or("mirror").to_owned();
                Server {
                    name: host,
                    dub: false,
                    headers: vec![],
                    streams: vec![MediaStream { url, format: "mp4".into(), quality: None, height: None }],
                    subtitles: vec![],
                    skip_intro: None,
                    skip_outro: None,
                    thumbnails: None,
                }
            })
            .collect())
    }
}

export!(AnimeHeaven);
