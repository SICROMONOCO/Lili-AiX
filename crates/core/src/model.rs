//! Types the UI sees. Serialised camelCase; mirrored by hand in `src/lib/types.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnimeCard {
    pub id: i64,
    pub title: String,
    pub title_romaji: Option<String>,
    pub cover: Option<String>,
    /// AniList's dominant cover colour, `#rrggbb`.
    pub color: Option<String>,
    pub banner: Option<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    pub episodes: Option<i32>,
    pub score: Option<i32>,
    pub year: Option<i32>,
    pub season: Option<String>,
    pub genres: Vec<String>,
    pub next_airing: Option<NextAiring>,
    pub is_adult: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NextAiring {
    pub episode: i32,
    /// Unix seconds.
    pub airing_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnimeDetail {
    #[serde(flatten)]
    pub card: AnimeCard,
    pub mal_id: Option<i64>,
    pub title_native: Option<String>,
    pub synonyms: Vec<String>,
    /// HTML from AniList (`<br>`, `<i>`), sanitised by the UI before display.
    pub description: Option<String>,
    pub duration: Option<i32>,
    pub source: Option<String>,
    pub studios: Vec<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub trailer: Option<Trailer>,
    pub relations: Vec<Relation>,
    pub recommendations: Vec<AnimeCard>,
    pub characters: Vec<Character>,
    pub tags: Vec<String>,
    /// The series' own wordmark (transparent PNG) from the media index; AniList has none.
    #[serde(default)]
    pub logo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trailer {
    pub site: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Relation {
    /// `SEQUEL`, `PREQUEL`, `SIDE_STORY`…
    pub kind: String,
    pub anime: AnimeCard,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Character {
    pub name: String,
    pub image: Option<String>,
    pub role: String,
    pub voice_actor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Home {
    pub trending: Vec<AnimeCard>,
    pub seasonal: Vec<AnimeCard>,
    pub popular: Vec<AnimeCard>,
    pub top_rated: Vec<AnimeCard>,
    pub upcoming: Vec<AnimeCard>,
    /// Android's "New Episodes": titles with an episode just out, newest first. Defaulted (with
    /// `movies`) so a cached Home from before them still loads.
    #[serde(default)]
    pub newest: Vec<AnimeCard>,
    /// Android's "Movies": the most popular films.
    #[serde(default)]
    pub movies: Vec<AnimeCard>,
}

/// One further page of a Home row (`home` opens each row with its first), when its shelf is
/// scrolled to the end.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeRowPage {
    pub cards: Vec<AnimeCard>,
    /// Whether the row has a page after this one.
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub results: Vec<AnimeCard>,
    pub has_next: bool,
    /// The spelling a title search was corrected to, or could be (`catalog::search`). Never cached
    /// with the page: it is worked out after the page is read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction: Option<SearchCorrection>,
}

/// A misspelled search and what it most likely meant (Android's `SearchCorrection`). `applied`
/// when the typed term found nothing and the page holds `suggested`'s results ("Showing results
/// for"); otherwise the page holds the typed term's own few, and `suggested` is only offered
/// ("Did you mean").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchCorrection {
    pub typed: String,
    pub suggested: String,
    pub applied: bool,
}

/// Every field is optional, as the UI's type says: the search palette sends only `query` and
/// `page`, and a required `genres` made each of its requests fail before the command ran.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchFilters {
    pub query: Option<String>,
    pub genres: Vec<String>,
    pub year: Option<i32>,
    pub season: Option<String>,
    pub format: Option<String>,
    pub status: Option<String>,
    /// AniList `MediaSort`, e.g. `POPULARITY_DESC`.
    pub sort: Option<String>,
    pub page: Option<i32>,
    /// The rest of Android's `DiscoverFilters`. Tags as AniList names them.
    pub tags: Vec<String>,
    /// Genres and tags a title must not carry.
    pub excluded_genres: Vec<String>,
    pub excluded_tags: Vec<String>,
    /// An inclusive span of release years (a decade chip); never sent with `year`.
    pub year_from: Option<i32>,
    pub year_to: Option<i32>,
    /// ISO 3166-1 alpha-2: `JP` anime, `CN` donghua, `KR` aeni.
    pub country: Option<String>,
    /// What it was adapted from: AniList's `MediaSource` (`MANGA`, `ORIGINAL`…).
    pub source: Option<String>,
    /// Inclusive bounds on the episode count.
    pub episodes_min: Option<i32>,
    pub episodes_max: Option<i32>,
    /// AniList's average score, 0–100, at least.
    pub minimum_score: Option<i32>,
    /// Only titles the dub index found an English dub for.
    pub dubbed_only: bool,
    /// Leave out what the viewer has watched: `completed`, or `started` (anything begun).
    pub hide_watched: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiringEntry {
    pub airing_at: i64,
    pub episode: i32,
    pub anime: AnimeCard,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Episode {
    pub number: f64,
    pub title: Option<String>,
    pub synopsis: Option<String>,
    pub thumbnail: Option<String>,
    pub aired_on: Option<String>,
    pub duration_seconds: Option<f64>,
    pub filler: bool,
    pub skip: Option<SkipTimes>,
    /// Set on the season's next episode while it is still locked: when it airs (Unix seconds),
    /// for a countdown in its slot. It opens `catalog::WATCHABLE_AFTER_SECS` later.
    #[serde(default)]
    pub airs_at: Option<i64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Window {
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkipTimes {
    pub intro: Option<Window>,
    pub outro: Option<Window>,
    /// The "previously on" block (AniSkip's `recap`); defaulted so cached records without it load.
    #[serde(default)]
    pub recap: Option<Window>,
}

/// Episodes for one title, with what Miruro knows about sub/dub availability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeList {
    pub episodes: Vec<Episode>,
    pub sub_count: Option<i32>,
    pub dub_count: Option<i32>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum Audio {
    Sub,
    Dub,
}

/// One server the user can pick for an episode.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// Stable within one sources response: `provider/track/server`.
    pub id: String,
    pub provider: String,
    /// The name the site shows (`bee`, `kiwi`…), which the app has always used.
    pub label: String,
    pub name: String,
    /// `sub` (hardsub), `ssub` (soft subs) or `dub`.
    pub track: String,
    pub audio: Audio,
    pub headers: BTreeMap<String, String>,
    pub streams: Vec<Stream>,
    /// An iframe player page, for servers only an embed can play.
    pub embed: Option<String>,
    pub subtitles: Vec<Subtitle>,
    /// The seek bar's previews: a WebVTT storyboard of sprite-sheet tiles (`storyboard`), fetched
    /// with `headers`. Absent from sources cached before it existed.
    #[serde(default)]
    pub thumbnails: Option<String>,
}

impl Server {
    pub fn is_native(&self) -> bool {
        !self.streams.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stream {
    pub url: String,
    /// `hls`, `mp4`, `dash`.
    pub format: String,
    pub quality: Option<String>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subtitle {
    pub url: String,
    pub label: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sources {
    pub servers: Vec<Server>,
    pub skip: Option<SkipTimes>,
}

/// A title in the user's local library.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub anime: AnimeCard,
    /// `watching`, `planning`, `completed`, `paused`, `dropped`.
    pub status: String,
    pub updated_at: i64,
    /// The signed-in account has this entry as it is here.
    #[serde(default)]
    pub synced: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub anime_id: i64,
    pub episode: f64,
    pub position: f64,
    pub duration: f64,
    pub updated_at: i64,
}

impl Progress {
    /// Past the point where an episode counts as watched (the credits usually start here).
    pub fn finished(&self) -> bool {
        self.duration > 0.0 && self.position >= self.duration * 0.9
    }
}

/// One queued, running or finished episode download.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    pub id: i64,
    pub anime_id: i64,
    pub episode: f64,
    pub audio: Audio,
    pub title: String,
    pub cover: Option<String>,
    /// `queued`, `downloading`, `done`, `failed`, `cancelled`.
    pub status: String,
    pub progress: f64,
    pub bytes: u64,
    pub path: Option<String>,
    pub error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// The server to try first (a provider key); `None` follows the playback preference.
    pub provider: Option<String>,
    /// The tallest rendition wanted; `None` takes the best there is.
    pub max_height: Option<u32>,
    /// Whether an episode may move to another server when its own has nothing at or below
    /// `max_height`.
    pub match_quality: bool,
    /// Only servers with the subtitles burned into the picture (`DownloadOptions::hard_subs`).
    #[serde(default)]
    pub hard_subs: bool,
    /// What was fetched: the rendition's height (when known) and the server's name.
    pub height: Option<u32>,
    pub server: Option<String>,
}

/// How a batch of episodes is to be fetched (the download dialog's choices).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DownloadOptions {
    pub provider: Option<String>,
    pub max_height: Option<u32>,
    pub match_quality: bool,
    /// "Hard subs only": a sub download takes only a server whose subtitles are in the picture
    /// (track `sub`), so the file shows them in any player. An episode no such server has fails
    /// rather than arriving soft-subbed. Dub is untouched.
    pub hard_subs: bool,
}

/// One entry of the app's own notification log: what it announced, and where a click goes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub id: i64,
    /// `airing`, `reminder`, `download` or `app`.
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub route: Option<String>,
    pub image: Option<String>,
    pub anime_id: Option<i64>,
    pub created_at: i64,
    pub read: bool,
}

/// A "continue watching" row: the title plus where the user left it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContinueEntry {
    pub anime: AnimeCard,
    pub progress: Progress,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_filters_need_no_fields() {
        // What the search palette sends.
        let filters: SearchFilters = serde_json::from_str(r#"{"query":"black clover","page":1}"#).unwrap();
        assert_eq!(filters.query.as_deref(), Some("black clover"));
        assert!(filters.genres.is_empty());
    }
}
