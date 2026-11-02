//! `anilili://` links: the format is written down in `docs/LINKS.md`, for the Android app to share.
//!
//! ```text
//! anilili://anime/<anilist id>                        the title's page
//! anilili://watch/<anilist id>/<episode>[?t=<secs>]   play an episode, from `t` if given
//! anilili://search?q=<text>                           search
//! ```
//!
//! Ids are AniList's, as everywhere both apps meet. Episodes are the numbers AniList counts
//! (`6.5` for a recap). Unknown query parameters are ignored, so a newer app can add some.

use url::Url;

use crate::catalogue as c;
use crate::error::Failure;

pub const SCHEME: &str = "anilili";

#[derive(Debug, Clone, PartialEq)]
pub enum Link {
    Anime { id: i64 },
    Watch { id: i64, episode: f64, start: Option<f64> },
    Search { query: String },
}

impl Link {
    /// Reads a link. Anything that isn't one of the forms above is `DLINK-01`.
    pub fn parse(text: &str) -> Result<Self, Failure> {
        let unknown = |why: &str| Failure::new(c::LINK_UNKNOWN, format!("{why}: {}", shorten(text)));
        let url = Url::parse(text.trim()).map_err(|_| unknown("not a link"))?;
        if url.scheme() != SCHEME {
            return Err(unknown("not an anilili:// link"));
        }
        // `anilili://anime/1` has host `anime`; `anilili:anime/1` and `anilili:///anime/1` don't.
        let path = url.path().trim_matches('/');
        let parts: Vec<&str> = url.host_str().into_iter().chain(path.split('/')).filter(|p| !p.is_empty()).collect();
        let query = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned());
        let id = |part: Option<&&str>| part.and_then(|p| p.parse::<i64>().ok()).filter(|id| *id > 0);
        match parts.as_slice() {
            ["anime", rest @ ..] if rest.len() == 1 => Ok(Link::Anime { id: id(rest.first()).ok_or_else(|| unknown("no AniList id"))? }),
            ["watch", rest @ ..] if rest.len() == 2 => {
                let id = id(rest.first()).ok_or_else(|| unknown("no AniList id"))?;
                let episode = rest[1].parse::<f64>().ok().filter(|e| e.is_finite() && *e >= 0.0).ok_or_else(|| unknown("no episode"))?;
                let start = query("t").and_then(|t| t.parse::<f64>().ok()).filter(|t| t.is_finite() && *t > 0.0);
                Ok(Link::Watch { id, episode, start })
            }
            ["search"] => Ok(Link::Search {
                query: query("q").map(|q| q.trim().to_owned()).filter(|q| !q.is_empty()).ok_or_else(|| unknown("nothing to search for"))?,
            }),
            _ => Err(unknown("not a page this app has")),
        }
    }

    /// The canonical form of the link.
    pub fn to_url(&self) -> String {
        match self {
            Link::Anime { id } => format!("{SCHEME}://anime/{id}"),
            Link::Watch { id, episode, start } => {
                let base = format!("{SCHEME}://watch/{id}/{}", number(*episode));
                match start {
                    Some(t) => format!("{base}?t={}", t.floor() as i64),
                    None => base,
                }
            }
            Link::Search { query } => {
                let mut url = Url::parse(&format!("{SCHEME}://search")).expect("a fixed URL parses");
                url.query_pairs_mut().append_pair("q", query);
                url.into()
            }
        }
    }

    /// The desktop app's route for the link (TanStack Router paths in `src/router.tsx`).
    pub fn to_route(&self) -> String {
        match self {
            Link::Anime { id } => format!("/anime/{id}"),
            Link::Watch { id, episode, start } => {
                let base = format!("/watch/{id}/{}", number(*episode));
                match start {
                    Some(t) => format!("{base}?t={}", t.floor() as i64),
                    None => base,
                }
            }
            Link::Search { query } => {
                let mut url = Url::parse("app://x/search").expect("a fixed URL parses");
                url.query_pairs_mut().append_pair("q", query);
                format!("/search?{}", url.query().unwrap_or_default())
            }
        }
    }
}

/// The first `anilili://` link among command-line arguments (how Windows hands one over).
pub fn from_args<I: IntoIterator<Item = String>>(args: I) -> Option<String> {
    args.into_iter().find(|a| a.to_ascii_lowercase().starts_with("anilili:"))
}

fn number(n: f64) -> String {
    if n.fract() == 0.0 { format!("{}", n as i64) } else { n.to_string() }
}

/// Links end up in logs and diagnostics: keep a pasted wall of text short.
fn shorten(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(120) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cases both apps must answer alike (fixtures/parity/links.json).
    fn corpus() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../fixtures/parity/links.json")).unwrap()
    }

    fn link_of(case: &serde_json::Value) -> Link {
        let id = case["id"].as_i64().unwrap_or_default();
        match case["page"].as_str().unwrap() {
            "anime" => Link::Anime { id },
            "watch" => Link::Watch { id, episode: case["episode"].as_f64().unwrap(), start: case["start"].as_f64() },
            "search" => Link::Search { query: case["query"].as_str().unwrap().to_owned() },
            other => panic!("unknown page {other}"),
        }
    }

    #[test]
    fn reads_every_form() {
        for case in corpus()["read"].as_array().unwrap() {
            let text = case["link"].as_str().unwrap();
            assert_eq!(Link::parse(text).unwrap(), link_of(case), "{text:?}");
        }
    }

    #[test]
    fn refuses_what_it_cannot_open() {
        for text in corpus()["refuse"].as_array().unwrap() {
            let text = text.as_str().unwrap();
            assert_eq!(Link::parse(text).unwrap_err().entry, c::LINK_UNKNOWN, "{text:?}");
        }
    }

    #[test]
    fn writes_the_canonical_form() {
        for case in corpus()["write"].as_array().unwrap() {
            let link = link_of(case);
            assert_eq!(link.to_url(), case["link"].as_str().unwrap());
            if !matches!(link, Link::Watch { start: Some(_), .. }) {
                assert_eq!(Link::parse(&link.to_url()).unwrap(), link, "reads back what it writes");
            }
        }
    }

    #[test]
    fn writes_and_routes() {
        let watch = Link::Watch { id: 21, episode: 6.5, start: Some(754.9) };
        assert_eq!(watch.to_url(), "anilili://watch/21/6.5?t=754");
        assert_eq!(watch.to_route(), "/watch/21/6.5?t=754");
        let search = Link::Search { query: "Re:Zero & friends".into() };
        assert_eq!(search.to_url(), "anilili://search?q=Re%3AZero+%26+friends");
        assert_eq!(Link::parse(&search.to_url()).unwrap(), search);
        assert_eq!(search.to_route(), "/search?q=Re%3AZero+%26+friends");
        assert_eq!(Link::Anime { id: 7 }.to_route(), "/anime/7");
    }

    #[test]
    fn finds_the_link_among_arguments() {
        let args = ["C:\\Anilili\\anilili-desktop.exe", "anilili://anime/1"].map(String::from);
        assert_eq!(from_args(args).as_deref(), Some("anilili://anime/1"));
        assert_eq!(from_args(["app.exe".to_owned()]), None);
    }
}
