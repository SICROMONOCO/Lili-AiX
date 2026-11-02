//! The project's media index: static JSON on jsDelivr, built from AniList, TMDB and AniSkip by the
//! `pommieyewear/media-index` repository, shared with the Android app (which calls it Konoha).
//!
//! No keys and no rate limits, so it fills what the live sources leave empty:
//!
//! - `anime/{id/1000}/{id}/episodes.json`: episode titles, synopses and TMDB stills, which Miruro
//!   lacks for anything new (Black Clover season 2 had none the day after it aired).
//! - `anime/{id/1000}/{id}/index.json`: the title's record, with its transparent **logo** (from
//!   TMDB; AniList has no such art), a banner, and a full AniList snapshot for when AniList refuses.
//! - `skip/{id/1000}/{id}.json`: a mirror of AniSkip's intro/outro submissions.
//! - `filler-index.json`, `dub-episodes.json`, `backdrops.json`: per-title filler episodes, the
//!   latest dubbed episode, and wide TMDB backdrops for titles without a banner.
//! - `schedule/{yyyy-mm}.json`: the month's airing schedule (a week either side), as AniList has it.
//!
//! A 404 is the ordinary answer for a title the index hasn't covered, and reads as "nothing". The
//! app caches what it reads (see `catalog.rs`); jsDelivr caches the branch for up to 12 hours.

use std::collections::HashMap;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{AiringEntry, AnimeCard, Episode, SkipTimes, Window};
use crate::net::Http;
use crate::{Error, Result};

const SERVICE: &str = "Media index";
pub const BASE: &str = "https://cdn.jsdelivr.net/gh/pommieyewear/media-index@main/data";

#[derive(Clone)]
pub struct MediaIndex {
    http: Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEpisode {
    pub number: Option<f64>,
    pub title: Option<String>,
    pub overview: Option<String>,
    pub air_date: Option<String>,
    pub still: Option<String>,
    pub runtime: Option<i32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexImages {
    pub poster: Option<String>,
    pub banner: Option<String>,
    /// The series' own wordmark, a transparent PNG.
    pub logo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDetail {
    #[serde(default)]
    pub images: IndexImages,
    /// AniList's media record as the index captured it (`parse_detail` reads it).
    pub anilist: Option<Value>,
    /// The title's season chain, as the generator walked it (AniList ids, airing order).
    #[serde(default)]
    pub series: Vec<i64>,
}

/// One row of the index's title list (`index.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexTitle {
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub popularity: Option<i64>,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// AniList's cover, the `medium` size.
    #[serde(default)]
    pub poster: Option<String>,
    #[serde(default)]
    pub poster_color: Option<String>,
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub episodes: Option<i32>,
    #[serde(default)]
    pub score: Option<i32>,
}

impl IndexTitle {
    /// The list carries no adult flag; AniList's Hentai genre marks the same titles (as Android
    /// reads its copy).
    pub fn is_adult(&self) -> bool {
        self.genres.iter().any(|g| g == "Hentai")
    }

    /// The row as a card, for search results AniList didn't give. The poster is asked for at the
    /// `large` size the grid draws at when AniList's CDN has one: its `bx`/`nx` uploads come in
    /// every size at the same path, older files only in the size listed (`large` is a 404).
    pub fn card(&self) -> AnimeCard {
        let cover = self.poster.as_ref().map(|p| match p.split_once("/cover/medium/") {
            Some((base, file)) if file.starts_with("bx") || file.starts_with("nx") => format!("{base}/cover/large/{file}"),
            _ => p.clone(),
        });
        AnimeCard {
            id: self.id,
            title: self.title.clone(),
            title_romaji: None,
            cover,
            color: self.poster_color.clone(),
            banner: None,
            format: self.format.clone(),
            status: self.status.clone(),
            episodes: self.episodes,
            score: self.score,
            year: self.year,
            season: None,
            genres: self.genres.clone(),
            next_airing: None,
            is_adult: self.is_adult(),
        }
    }
}

/// A title search answered from the index's title list (Android's `LocalCatalog`): the first page,
/// for when AniList answers a title search thinly or not at all. AniList matches whole words only,
/// so a half-typed "one pie" finds little there, and a reply that came back empty or failed would
/// otherwise end on "Nothing found".
///
/// Titles holding the typed text, ranked as Android's `searchIn` ranks them (the whole title, then
/// one starting with it, then one containing it, popularity breaking ties) for the default sort,
/// and by the chosen sort otherwise. The list carries genres, year, status, format, episodes and
/// score, so those filters are honoured; one it has nothing for (tags, season, country, source)
/// returns nothing rather than results that contradict the chips on screen. `only` is the dub
/// index for "Dubbed only"; `excluded` is "Hide watched".
pub fn search_titles(
    titles: &[IndexTitle],
    filters: &crate::model::SearchFilters,
    show_adult: bool,
    only: Option<&[i64]>,
    excluded: &[i64],
) -> Vec<AnimeCard> {
    const PAGE: usize = 30;
    let needle = filters.query.as_deref().map(|q| q.trim().to_lowercase()).unwrap_or_default();
    let unsupported = !filters.tags.is_empty()
        || !filters.excluded_tags.is_empty()
        || filters.season.is_some()
        || filters.country.is_some()
        || filters.source.is_some();
    if needle.is_empty() || unsupported {
        return Vec::new();
    }
    let has = |v: &Option<String>, want: &Option<String>| want.as_deref().is_none_or(|w| v.as_deref().is_some_and(|v| v.eq_ignore_ascii_case(w)));
    let mut ranked: Vec<(u8, &IndexTitle)> = titles
        .iter()
        .filter(|t| show_adult || !t.is_adult())
        .filter(|t| filters.genres.is_empty() || t.genres.iter().any(|g| filters.genres.contains(g)))
        .filter(|t| !t.genres.iter().any(|g| filters.excluded_genres.contains(g)))
        .filter(|t| filters.year.is_none_or(|y| t.year == Some(y)))
        .filter(|t| filters.year_from.is_none_or(|y| t.year.is_some_and(|ty| ty >= y)))
        .filter(|t| filters.year_to.is_none_or(|y| t.year.is_some_and(|ty| ty <= y)))
        .filter(|t| has(&t.status, &filters.status) && has(&t.format, &filters.format))
        .filter(|t| filters.episodes_min.is_none_or(|m| t.episodes.is_some_and(|e| e >= m)))
        .filter(|t| filters.episodes_max.is_none_or(|m| t.episodes.is_some_and(|e| e <= m)))
        .filter(|t| filters.minimum_score.is_none_or(|m| t.score.unwrap_or(0) >= m))
        .filter(|t| only.is_none_or(|ids| ids.binary_search(&t.id).is_ok()))
        .filter(|t| !excluded.contains(&t.id))
        .filter_map(|t| {
            let title = t.title.to_lowercase();
            let rank = if title == needle {
                0
            } else if title.starts_with(&needle) {
                1
            } else if title.contains(&needle) {
                2
            } else {
                return None;
            };
            Some((rank, t))
        })
        .collect();
    let popularity = |t: &IndexTitle| std::cmp::Reverse(t.popularity.unwrap_or(0));
    match filters.sort.as_deref().unwrap_or("SEARCH_MATCH") {
        s if s.starts_with("SCORE") => ranked.sort_by_key(|(_, t)| (std::cmp::Reverse(t.score.unwrap_or(0)), popularity(t))),
        s if s.starts_with("START_DATE") => ranked.sort_by_key(|(_, t)| (std::cmp::Reverse(t.year.unwrap_or(0)), popularity(t))),
        "SEARCH_MATCH" => ranked.sort_by_key(|(rank, t)| (*rank, popularity(t))),
        // A snapshot has no idea what is trending this week; popularity is the honest stand-in.
        _ => ranked.sort_by_key(|(_, t)| popularity(t)),
    }
    ranked.into_iter().take(PAGE).map(|(_, t)| t.card()).collect()
}

/// AniSkip rows per episode: `[kind, start, end, episode length, skip id]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexSkip {
    pub mal: Option<i64>,
    #[serde(default)]
    pub episodes: HashMap<String, Vec<Vec<Value>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexAiring {
    id: i64,
    episode: i32,
    #[serde(rename = "airingAt")]
    airing_at: i64,
    english: Option<String>,
    romaji: Option<String>,
    cover: Option<String>,
    format: Option<String>,
    year: Option<i32>,
    #[serde(default)]
    adult: bool,
}

#[derive(Deserialize)]
struct Titled<T> {
    titles: HashMap<String, T>,
}

impl MediaIndex {
    pub fn new(http: Http) -> Self {
        Self { http }
    }

    /// The file at `path`, or `None` when the index has no such file.
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<Option<T>> {
        let url = format!("{BASE}/{path}");
        match self.http.get_bytes(SERVICE, &url, &[("Accept", "application/json")], Duration::from_secs(15)).await {
            Ok((body, _)) => serde_json::from_slice(&body).map(Some).map_err(|e| Error::decode(SERVICE, e)),
            Err(Error::Http { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub async fn episodes(&self, anilist_id: i64) -> Result<Vec<IndexEpisode>> {
        Ok(self.get(&format!("anime/{}/{anilist_id}/episodes.json", anilist_id / 1000)).await?.unwrap_or_default())
    }

    pub async fn detail(&self, anilist_id: i64) -> Result<Option<IndexDetail>> {
        self.get(&format!("anime/{}/{anilist_id}/index.json", anilist_id / 1000)).await
    }

    pub async fn skip(&self, anilist_id: i64) -> Result<Option<IndexSkip>> {
        self.get(&format!("skip/{}/{anilist_id}.json", anilist_id / 1000)).await
    }

    /// Filler episode numbers per AniList id.
    pub async fn fillers(&self) -> Result<HashMap<i64, Vec<f64>>> {
        Ok(keyed(self.get::<Titled<Vec<f64>>>("filler-index.json").await?))
    }

    /// The latest dubbed episode per AniList id.
    pub async fn dub_episodes(&self) -> Result<HashMap<i64, i32>> {
        Ok(keyed(self.get::<Titled<i32>>("dub-episodes.json").await?))
    }

    /// Every AniList id a crawl positively found an English dub for (Android's `DubIndex`). Titles
    /// that are absent are unknown, not undubbed.
    pub async fn dub_index(&self) -> Result<Vec<i64>> {
        Ok(self.get::<Vec<i64>>("dub-index.json").await?.unwrap_or_default())
    }

    /// Every title the index holds (the list Android ships as its offline catalogue), trimmed to
    /// what search spelling reads. Music videos are left out, as Android's bundle leaves them out.
    pub async fn titles(&self) -> Result<Vec<IndexTitle>> {
        let rows = self.get::<Vec<IndexTitle>>("index.json").await?.unwrap_or_default();
        Ok(rows.into_iter().filter(|t| t.format.as_deref() != Some("MUSIC")).collect())
    }

    /// A wide backdrop (TMDB, 1280 px) per AniList id.
    pub async fn backdrops(&self) -> Result<HashMap<i64, String>> {
        Ok(keyed(self.get::<Titled<String>>("backdrops.json").await?))
    }

    /// Episodes airing in `[from, to)` (unix seconds), from each month file the range touches.
    pub async fn schedule(&self, from: i64, to: i64, show_adult: bool) -> Result<Vec<AiringEntry>> {
        let mut months: Vec<String> = Vec::new();
        let mut at = from;
        while at < to + 31 * 86_400 {
            let (year, month) = year_month(at.min(to));
            let name = format!("{year:04}-{month:02}");
            if !months.contains(&name) {
                months.push(name);
            }
            at += 28 * 86_400;
        }
        let mut files = Vec::new();
        for month in months {
            if let Some(rows) = self.get::<Vec<IndexAiring>>(&format!("schedule/{month}.json")).await? {
                files.push((month, rows));
            }
        }
        Ok(merge_schedule(files, from, to, show_adult))
    }
}

/// The months' rows in `from..to`, one per episode. A month's file overlaps its neighbours by a
/// week, and an older file keeps times that have since changed: September's still had Aoashi
/// Season 2 ep 1 at 10:00 on Oct 4 (October's and AniList: 4:00) and ONE PIECE ep 1181 on Oct 4
/// (moved to January; October's leaves it out). So a month's own file, when it was loaded, alone
/// decides that month; a neighbour's rows for it count only when it wasn't (not published yet).
fn merge_schedule(files: Vec<(String, Vec<IndexAiring>)>, from: i64, to: i64, show_adult: bool) -> Vec<AiringEntry> {
    let month_of = |unix: i64| {
        let (year, month) = year_month(unix);
        format!("{year:04}-{month:02}")
    };
    let loaded: std::collections::HashSet<String> = files.iter().map(|(month, _)| month.clone()).collect();
    let mut chosen: HashMap<(i64, i32), IndexAiring> = HashMap::new();
    for (month, rows) in files {
        for row in rows {
            let airs_in = month_of(row.airing_at);
            if airs_in != month && loaded.contains(&airs_in) {
                continue;
            }
            chosen.entry((row.id, row.episode)).or_insert(row);
        }
    }
    let mut entries: Vec<AiringEntry> = chosen
        .into_values()
        .filter(|row| row.airing_at >= from && row.airing_at < to && (show_adult || !row.adult))
        .map(airing_entry)
        .collect();
    entries.sort_by(|a, b| a.airing_at.cmp(&b.airing_at).then(a.anime.id.cmp(&b.anime.id)));
    entries
}

fn keyed<T>(file: Option<Titled<T>>) -> HashMap<i64, T> {
    file.map(|f| f.titles.into_iter().filter_map(|(k, v)| Some((k.parse().ok()?, v))).collect()).unwrap_or_default()
}

fn airing_entry(row: IndexAiring) -> AiringEntry {
    AiringEntry {
        airing_at: row.airing_at,
        episode: row.episode,
        anime: AnimeCard {
            id: row.id,
            title: row.english.clone().or_else(|| row.romaji.clone()).unwrap_or_else(|| format!("#{}", row.id)),
            title_romaji: row.romaji,
            cover: row.cover,
            color: None,
            banner: None,
            format: row.format,
            status: None,
            episodes: None,
            score: None,
            year: row.year,
            season: None,
            genres: vec![],
            next_airing: None,
            is_adult: row.adult,
        },
    }
}

/// (year, month) for a unix time (UTC).
fn year_month(unix: i64) -> (i64, i64) {
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month)
}

/// "Episode 3" and the like: a placeholder, not a title.
fn is_placeholder(title: &str) -> bool {
    let t = title.trim().to_ascii_lowercase();
    t.is_empty() || t.strip_prefix("episode").is_some_and(|rest| rest.trim().chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// Fills what the live list lacks (stills, synopses, real titles, air dates, runtimes) from the
/// index, matched by episode number. What the live list has, it keeps.
pub fn merge_episodes(episodes: &mut [Episode], index: &[IndexEpisode]) {
    for episode in episodes {
        let Some(row) = index.iter().find(|r| r.number == Some(episode.number)) else { continue };
        if episode.thumbnail.is_none() {
            episode.thumbnail = row.still.clone().filter(|s| !s.is_empty());
        }
        if episode.synopsis.is_none() {
            episode.synopsis = row.overview.clone().filter(|s| !s.is_empty());
        }
        if episode.title.as_deref().is_none_or(is_placeholder)
            && let Some(title) = row.title.as_deref().filter(|t| !is_placeholder(t))
        {
            episode.title = Some(title.to_owned());
        }
        if episode.aired_on.is_none() {
            episode.aired_on = row.air_date.clone();
        }
        if episode.duration_seconds.is_none() {
            episode.duration_seconds = row.runtime.filter(|r| *r > 0).map(|r| f64::from(r) * 60.0);
        }
    }
}

/// An episode list built from the index alone, for a title Miruro doesn't know.
pub fn episodes_from_index(index: &[IndexEpisode]) -> Vec<Episode> {
    let mut episodes: Vec<Episode> = index
        .iter()
        .filter_map(|r| {
            Some(Episode {
                number: r.number?,
                title: None,
                synopsis: None,
                thumbnail: None,
                aired_on: None,
                duration_seconds: None,
                filler: false,
                skip: None,
                airs_at: None,
            })
        })
        .collect();
    merge_episodes(&mut episodes, index);
    episodes
}

/// Intro and outro for one episode from the AniSkip mirror: the plain `op`/`ed` rows over the
/// `mixed-` ones (those overlap the episode's own content), and among several submissions the one
/// whose episode length is nearest `duration` (seconds), when known.
pub fn skip_for(file: &IndexSkip, episode: f64, duration: Option<f64>) -> Option<SkipTimes> {
    if episode.fract() != 0.0 {
        return None;
    }
    let rows = file.episodes.get(&format!("{}", episode as i64))?;
    let pick = |kinds: &[&str]| -> Option<Window> {
        kinds.iter().find_map(|kind| {
            rows.iter()
                .filter(|r| r.first().and_then(Value::as_str) == Some(*kind))
                .filter_map(|r| {
                    let (start, end) = (r.get(1)?.as_f64()?, r.get(2)?.as_f64()?);
                    let length = r.get(3).and_then(Value::as_f64).unwrap_or(0.0);
                    (end > start).then_some((Window { start, end }, length))
                })
                .min_by(|a, b| {
                    let off = |l: f64| duration.map_or(0.0, |d| (l - d).abs());
                    off(a.1).total_cmp(&off(b.1))
                })
                .map(|(w, _)| w)
        })
    };
    let skip = SkipTimes { intro: pick(&["op", "mixed-op"]), outro: pick(&["ed", "mixed-ed"]), recap: pick(&["recap"]) };
    (skip.intro.is_some() || skip.outro.is_some() || skip.recap.is_some()).then_some(skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn episode(number: f64, title: Option<&str>) -> Episode {
        Episode {
            number,
            title: title.map(str::to_owned),
            synopsis: None,
            thumbnail: None,
            aired_on: None,
            duration_seconds: None,
            filler: false,
            skip: None,
            airs_at: None,
        }
    }

    #[test]
    fn merges_what_the_live_list_lacks() {
        let index: Vec<IndexEpisode> = serde_json::from_str(
            r#"[{"number":1.0,"title":"The Battle Begins","overview":"Asta trains.","air_date":"2026-10-03","still":"https://i/1.jpg","runtime":24},
                {"number":2.0,"title":"Episode 2","overview":null,"air_date":"2026-10-10","still":null,"runtime":null}]"#,
        )
        .unwrap();
        let mut live = vec![episode(1.0, Some("Live title")), episode(2.0, Some("Episode 2")), episode(3.0, None)];
        merge_episodes(&mut live, &index);
        assert_eq!(live[0].thumbnail.as_deref(), Some("https://i/1.jpg"));
        assert_eq!(live[0].title.as_deref(), Some("Live title"), "a real live title stays");
        assert_eq!(live[0].duration_seconds, Some(1440.0));
        assert_eq!(live[1].title.as_deref(), Some("Episode 2"), "a placeholder is not an upgrade");
        assert!(live[1].thumbnail.is_none());
        assert!(live[2].thumbnail.is_none());
        let mut generic = vec![episode(1.0, Some("Episode 1"))];
        merge_episodes(&mut generic, &index);
        assert_eq!(generic[0].title.as_deref(), Some("The Battle Begins"));
    }

    #[test]
    fn picks_skip_rows() {
        let file: IndexSkip = serde_json::from_str(
            r#"{"mal":52991,"episodes":{"1":[["mixed-op",58.7,148.7,1470.0,"a"],["op",3.2,93.2,1559.9,"b"],["op",5.0,95.0,1420.0,"c"],["ed",1417.1,1507.1,1510.1,"d"]]}}"#,
        )
        .unwrap();
        let skip = skip_for(&file, 1.0, Some(1425.0)).unwrap();
        assert_eq!(skip.intro, Some(Window { start: 5.0, end: 95.0 }), "nearest episode length wins");
        assert_eq!(skip.outro, Some(Window { start: 1417.1, end: 1507.1 }));
        assert!(skip_for(&file, 2.0, None).is_none());
        assert!(skip_for(&file, 1.5, None).is_none());
    }

    #[test]
    fn spans_months() {
        assert_eq!(year_month(1_791_138_592), (2026, 10));
        assert_eq!(year_month(0), (1970, 1));
        assert!(is_placeholder("Episode 12") && is_placeholder("episode 6.5") && !is_placeholder("Episode of Luffy"));
    }

    #[test]
    fn a_months_own_file_wins_for_its_episodes() {
        let row = |airing_at: i64| IndexAiring {
            id: 191788,
            episode: 1,
            airing_at,
            english: Some("Aoashi Season 2".into()),
            romaji: None,
            cover: None,
            format: None,
            year: None,
            adult: false,
        };
        // 2026-10-04 10:00 and 04:00 UTC: September's stale time, then October's.
        let (stale, moved) = (1_791_108_000, 1_791_086_400);
        let files = vec![("2026-09".to_owned(), vec![row(stale)]), ("2026-10".to_owned(), vec![row(moved)])];
        let merged = merge_schedule(files, 1_790_985_600, 1_791_590_400, false);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].airing_at, moved);
    }

    #[test]
    fn an_episode_its_own_month_dropped_stays_dropped() {
        let row = one_piece_1181();
        // Only September's file lists it (on Oct 4): October's, loaded too, has it elsewhere.
        let files = vec![("2026-09".to_owned(), vec![row]), ("2026-10".to_owned(), vec![])];
        assert!(merge_schedule(files, 1_790_985_600, 1_791_590_400, false).is_empty());
        // With October's file not published yet, September's overlap is all there is.
        let row = one_piece_1181();
        let files = vec![("2026-09".to_owned(), vec![row])];
        assert_eq!(merge_schedule(files, 1_790_985_600, 1_791_590_400, false).len(), 1);
    }

    fn one_piece_1181() -> IndexAiring {
        IndexAiring {
            id: 21,
            episode: 1181,
            airing_at: 1_791_108_000,
            english: None,
            romaji: None,
            cover: None,
            format: None,
            year: None,
            adult: false,
        }
    }

    fn title(id: i64, title: &str, popularity: i64, year: i32, genres: &[&str]) -> IndexTitle {
        IndexTitle {
            id,
            title: title.into(),
            popularity: Some(popularity),
            format: Some("TV".into()),
            genres: genres.iter().map(|g| (*g).to_owned()).collect(),
            poster: Some(format!("https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/{}{id}.jpg", if id == 4 { "bx" } else { "" })),
            poster_color: None,
            year: Some(year),
            status: Some("FINISHED".into()),
            episodes: Some(12),
            score: Some(80),
        }
    }

    fn search(titles: &[IndexTitle], query: &str) -> Vec<i64> {
        let filters = crate::model::SearchFilters { query: Some(query.into()), ..Default::default() };
        search_titles(titles, &filters, false, None, &[]).into_iter().map(|c| c.id).collect()
    }

    #[test]
    fn a_title_search_is_answered_from_the_title_list() {
        let titles = [
            title(1, "Koisuru ONE PIECE", 900, 2025, &[]),
            title(2, "ONE PIECE", 750_000, 1999, &["Action"]),
            title(3, "ONE PIECE Film: Red", 100_000, 2022, &["Action"]),
            title(4, "Naruto", 600_000, 2002, &[]),
            title(5, "One Piece Hentai", 999_999, 2020, &["Hentai"]),
        ];
        // The whole title, then one starting with it, then one holding it; adult titles left out.
        assert_eq!(search(&titles, "One Piece"), [2, 3, 1]);
        // A half-typed word AniList finds nothing for.
        assert_eq!(search(&titles, "one pie"), [2, 3, 1]);
        assert!(search(&titles, "").is_empty());
        let card = search_titles(&titles, &crate::model::SearchFilters { query: Some("naruto".into()), ..Default::default() }, false, None, &[]);
        assert_eq!(card[0].cover.as_deref(), Some("https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx4.jpg"));
        // An older upload has no `large` copy, so it stays as listed.
        let older = search_titles(&titles, &crate::model::SearchFilters { query: Some("film: red".into()), ..Default::default() }, false, None, &[]);
        assert_eq!(older[0].cover.as_deref(), Some("https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/3.jpg"));
    }

    #[test]
    fn the_title_list_honours_the_filters_it_can() {
        let titles = [title(2, "ONE PIECE", 750_000, 1999, &["Action"]), title(3, "ONE PIECE Film: Red", 100_000, 2022, &["Action"])];
        let with = |f: crate::model::SearchFilters| -> Vec<i64> {
            let f = crate::model::SearchFilters { query: Some("one piece".into()), ..f };
            search_titles(&titles, &f, false, None, &[]).into_iter().map(|c| c.id).collect()
        };
        assert_eq!(with(crate::model::SearchFilters { year: Some(2022), ..Default::default() }), [3]);
        assert_eq!(with(crate::model::SearchFilters { year_to: Some(2009), ..Default::default() }), [2]);
        assert_eq!(with(crate::model::SearchFilters { sort: Some("START_DATE_DESC".into()), ..Default::default() }), [3, 2]);
        // Nothing to answer a season or a tag with: no results rather than wrong ones.
        assert!(with(crate::model::SearchFilters { season: Some("FALL".into()), ..Default::default() }).is_empty());
        assert!(with(crate::model::SearchFilters { tags: vec!["Pirates".into()], ..Default::default() }).is_empty());
        // Dubbed only, and Hide watched.
        let only = search_titles(&titles, &crate::model::SearchFilters { query: Some("one piece".into()), ..Default::default() }, false, Some(&[3]), &[]);
        assert_eq!(only.iter().map(|c| c.id).collect::<Vec<_>>(), [3]);
        let hidden = search_titles(&titles, &crate::model::SearchFilters { query: Some("one piece".into()), ..Default::default() }, false, None, &[2]);
        assert_eq!(hidden.iter().map(|c| c.id).collect::<Vec<_>>(), [3]);
    }
}
