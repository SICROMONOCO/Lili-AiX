//! Which requests an embedded player's page may make. The webview asks [`Blocker::check`] for
//! each one (`src-tauri/src/adblock.rs`); a refused request gets an empty 200 instead.
//!
//! Two layers, both only for requests to a host other than the embed's own:
//! - **Android's list** ([`ORIGINS`], from `EmbedWebView.kt`): advertising and analytics origins
//!   seen on the embed pages, each confirmed to serve no video. Always on.
//! - **Filter lists** (EasyList, EasyPrivacy, uBlock filters and Badware risks), network rules
//!   only, through Brave's engine. Loaded once the lists are read; until then only the first
//!   layer applies.
//!
//! The rule both apps learned the hard way: a player's own CDN looks like a tracker from here, and
//! refusing it takes the video with it. So nothing on the embed's own host (or its subdomains) is
//! refused, nor media, nor [`NEVER`], nor anything the app itself loads.

use adblock::Engine;
use adblock::lists::{FilterSet, ParseOptions, RuleTypes};
use adblock::request::Request;

/// Android's `EMBED_BLOCKED_ORIGINS`: a host or any subdomain of it. Keep the two in step.
pub const ORIGINS: &[&str] = &[
    "googlesyndication.com",
    "googletagmanager.com",
    "google-analytics.com",
    "doubleclick.net",
    "cloudflareinsights.com",
    "morphify.net",
    // The popunder rotates a random subdomain per page load.
    "sssrr.org",
    "statlytic.net",
    "plausible.io",
    "jwpltx.com",
];

/// Script CDNs the players load from. Android names these as the ones a blocklist must not take.
pub const NEVER: &[&str] = &["cdnjs.cloudflare.com", "iamcdn.net"];

/// What a request is for, as the webview reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Document,
    Subdocument,
    Script,
    Stylesheet,
    Image,
    Font,
    Media,
    Xhr,
    Fetch,
    Websocket,
    Ping,
    Other,
}

impl Kind {
    fn filter_type(self) -> &'static str {
        match self {
            Kind::Document => "document",
            Kind::Subdocument => "subdocument",
            Kind::Script => "script",
            Kind::Stylesheet => "stylesheet",
            Kind::Image => "image",
            Kind::Font => "font",
            Kind::Media => "media",
            Kind::Xhr => "xmlhttprequest",
            Kind::Fetch => "fetch",
            Kind::Websocket => "websocket",
            Kind::Ping => "ping",
            Kind::Other => "other",
        }
    }
}

/// Why a request was refused, for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// On Android's list, under this origin.
    Listed(&'static str),
    /// Matched a filter-list rule (its text, when the engine kept it).
    Filter(Option<String>),
}

#[derive(Default)]
pub struct Blocker {
    engine: Option<Engine>,
    lists: usize,
}

impl Blocker {
    /// Android's list only.
    pub fn new() -> Self {
        Self::default()
    }

    /// Android's list plus the network rules of `lists` (filter-list text).
    pub fn with_lists<'a>(lists: impl IntoIterator<Item = &'a str>) -> Self {
        let mut set = FilterSet::new(false);
        let options = ParseOptions { rule_types: RuleTypes::NetworkOnly, ..ParseOptions::default() };
        let mut count = 0;
        for list in lists {
            set.add_filter_list(list.to_owned(), options);
            count += 1;
        }
        Self { engine: Some(Engine::new_with_filter_set(set)), lists: count }
    }

    /// Filter lists loaded (0 before they are).
    pub fn lists(&self) -> usize {
        self.lists
    }

    /// `None` to let `url` through; `source` is the page asking (its `Referer`, often only the
    /// origin), empty when unknown.
    pub fn check(&self, url: &str, source: &str, kind: Kind) -> Option<Refusal> {
        let host = host_of(url)?;
        if !url.starts_with("http") || is_local(&host) || kind == Kind::Media {
            return None;
        }
        let source_host = host_of(source);
        match &source_host {
            // The app's own page: covers, AniList, the embed's iframe itself.
            Some(s) if is_local(s) => return None,
            Some(s) if within(&host, s) || within(s, &host) => return None,
            _ => {}
        }
        if NEVER.iter().any(|n| within(&host, n)) {
            return None;
        }
        if let Some(origin) = ORIGINS.iter().find(|o| within(&host, o)) {
            return Some(Refusal::Listed(origin));
        }
        let engine = self.engine.as_ref()?;
        // With no source the engine can't tell third party from first; the request's own URL as
        // source keeps it to rules that hold either way.
        let source = if source_host.is_some() { source } else { url };
        let request = Request::new(url, source, kind.filter_type(), "GET").ok()?;
        let result = engine.check_network_request(&request);
        result.should_block().then(|| Refusal::Filter(result.filter.and_then(|f| f.raw_line)))
    }
}

/// The `Referer` an embed's own page must arrive with, for hosts that refuse to serve it to anyone
/// else; `None` leaves the frame's own.
///
/// Kwik (AnimePahe's player, Miruro's `kiwi`) answers its `/e/` page only to its own origin and
/// AnimePahe's: measured 2026-10-22 on one URL, `https://kwik.cx/` and `https://animepahe.pw/` got
/// the player, while `http://tauri.localhost/`, `https://www.miruro.to/` and no Referer at all got
/// Cloudflare's 403 "Attention Required!" page. That page carries `X-Frame-Options: SAMEORIGIN`, so
/// the iframe showed only the browser's blocked-page icon. The real page sets no frame headers.
/// Kwik moves between domains (`kwik.si`, then `kwik.cx`), so it is matched by name, not domain.
pub fn frame_referer(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let host = parsed.host_str()?.to_ascii_lowercase();
    (host.split('.').next() == Some("kwik")).then(|| format!("{}://{host}/", parsed.scheme()))
}

/// Lower-cased host without `www.`.
fn host_of(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_ascii_lowercase();
    Some(host.strip_prefix("www.").map(str::to_owned).unwrap_or(host))
}

/// `host` is `domain` or a subdomain of it.
fn within(host: &str, domain: &str) -> bool {
    host == domain || host.strip_suffix(domain).is_some_and(|rest| rest.ends_with('.'))
}

fn is_local(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost") || host.ends_with(".localhost")
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMBED: &str = "https://megaplay.buzz/";

    #[test]
    fn androids_list_holds_without_filter_lists() {
        let b = Blocker::new();
        assert_eq!(
            b.check("https://pagead2.googlesyndication.com/pagead/js/adsbygoogle.js", EMBED, Kind::Script),
            Some(Refusal::Listed("googlesyndication.com"))
        );
        assert_eq!(b.check("https://k3x9.sssrr.org/pop.js", EMBED, Kind::Script), Some(Refusal::Listed("sssrr.org")));
        assert_eq!(b.check("https://cdn.example.com/player.js", EMBED, Kind::Script), None);
    }

    #[test]
    fn never_refuses_what_the_player_needs() {
        let b = Blocker::with_lists(["||adhost.example^\n||megaplay.buzz/ads^\n||cdnjs.cloudflare.com^\n/banner/*"]);
        // The embed's own host and subdomains, even where a rule names it.
        assert_eq!(b.check("https://megaplay.buzz/ads/x.js", EMBED, Kind::Script), None);
        assert_eq!(b.check("https://cdn.megaplay.buzz/banner/a.png", EMBED, Kind::Image), None);
        // Video, the never list, and the app's own requests.
        assert_eq!(b.check("https://adhost.example/seg-1.ts", EMBED, Kind::Media), None);
        assert_eq!(b.check("https://cdnjs.cloudflare.com/ajax/libs/hls.js/1.5.0/hls.min.js", EMBED, Kind::Script), None);
        assert_eq!(b.check("https://adhost.example/x.js", "http://localhost:5173/", Kind::Script), None);
        assert_eq!(b.check("https://adhost.example/x.js", "http://tauri.localhost/watch/1/2", Kind::Script), None);
        assert_eq!(b.check("http://127.0.0.1:8123/p.m3u8", EMBED, Kind::Xhr), None);
    }

    #[test]
    fn filter_lists_refuse_third_parties() {
        let b = Blocker::with_lists(["! Title: test\n||adhost.example^\n||track.example^$third-party\n@@||adhost.example/ok.js"]);
        assert_eq!(b.lists(), 1);
        assert!(matches!(b.check("https://adhost.example/x.js", EMBED, Kind::Script), Some(Refusal::Filter(_))));
        assert_eq!(b.check("https://adhost.example/ok.js", EMBED, Kind::Script), None, "exceptions apply");
        assert!(b.check("https://track.example/b", EMBED, Kind::Ping).is_some());
        // No source: rules that need to know the party don't fire, the rest do.
        assert_eq!(b.check("https://track.example/b", "", Kind::Ping), None);
        assert!(b.check("https://adhost.example/x.js", "", Kind::Script).is_some());
    }

    #[test]
    fn kwik_pages_carry_their_own_referer() {
        assert_eq!(frame_referer("https://kwik.cx/e/vtTkwnR7f8GR").as_deref(), Some("https://kwik.cx/"));
        assert_eq!(frame_referer("https://KWIK.si/e/abc").as_deref(), Some("https://kwik.si/"));
        assert_eq!(frame_referer("https://megaplay.buzz/stream/s-2/1/sub"), None);
        assert_eq!(frame_referer("https://notkwik.cx/e/x"), None);
    }

    #[test]
    fn matches_whole_labels() {
        assert!(within("a.doubleclick.net", "doubleclick.net"));
        assert!(!within("notdoubleclick.net", "doubleclick.net"));
        assert_eq!(host_of("https://WWW.Example.com/x").as_deref(), Some("example.com"));
    }
}
