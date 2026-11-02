//! AniList GraphQL: browse, search, details, schedule, and the signed-in user's list.
//!
//! Rate limit: 90 requests/minute normally, 30 when AniList is degraded; a 429 means a one-minute
//! timeout. Callers cache through [`crate::store::Store`] so browsing does not spend the budget.

use std::sync::RwLock;
use std::time::Duration;

use serde_json::{Value, json};

use crate::model::{
    AiringEntry, AnimeCard, AnimeDetail, Character, Home, HomeRowPage, NextAiring, Relation, SearchFilters, SearchPage, Trailer,
};
use crate::net::Http;
use crate::{Error, Result};

const SERVICE: &str = "AniList";
const ENDPOINT: &str = "https://graphql.anilist.co";

const CARD_FIELDS: &str = r#"
  id isAdult format status episodes averageScore seasonYear season genres
  title { romaji english userPreferred }
  coverImage { extraLarge large color }
  bannerImage
  nextAiringEpisode { episode airingAt }
"#;

pub struct AniList {
    http: Http,
    token: RwLock<Option<String>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewer {
    pub id: i64,
    pub name: String,
    pub avatar: Option<String>,
    pub show_adult: bool,
}

/// One row of the signed-in user's AniList anime list.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListEntry {
    pub anime: AnimeCard,
    /// AniList `MediaListStatus`: CURRENT, PLANNING, COMPLETED, PAUSED, DROPPED, REPEATING.
    pub status: String,
    pub progress: i32,
    pub score: f64,
    pub updated_at: i64,
    /// The title's main animation studios, for the profile's stats.
    #[serde(default)]
    pub studios: Vec<String>,
}

/// A title with its start date and franchise relations, for the watch guide (`franchise`).
#[derive(Debug, Clone)]
pub struct Member {
    pub card: AnimeCard,
    /// (year, month, day), 9999 where unknown, so undated entries sort last.
    pub start: (i32, i32, i32),
    /// (relation type, the related title) for anime relations.
    pub relations: Vec<(String, Member)>,
}

impl Member {
    /// Android's SERIES_AIRING_ORDER: start date, then id.
    pub fn airing_key(&self) -> ((i32, i32, i32), i64) {
        (self.start, self.card.id)
    }

    fn parse(v: &Value) -> Option<Self> {
        let card = parse_card(v)?;
        let date = |key: &str| v.pointer(&format!("/startDate/{key}")).and_then(Value::as_i64).map(|n| n as i32);
        let start = (date("year").or(card.year).unwrap_or(9999), date("month").unwrap_or(99), date("day").unwrap_or(99));
        let relations = v
            .pointer("/relations/edges")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|e| e.pointer("/node/type").and_then(Value::as_str) == Some("ANIME"))
            .filter_map(|e| Some((e.get("relationType")?.as_str()?.to_owned(), Member::parse(e.get("node")?)?)))
            .collect();
        Some(Member { card, start, relations })
    }
}

/// A card with its synopsis (AniList's HTML), for the dice and recommendations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    #[serde(flatten)]
    pub card: AnimeCard,
    pub description: Option<String>,
}

/// One item of the signed-in account's notification feed (Android's `AppNotification`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedItem {
    pub id: i64,
    /// `airing`, `media` or `social`.
    pub kind: String,
    /// Unix seconds.
    pub created_at: i64,
    /// The anime, else the user, else "AniList".
    pub title: String,
    /// `EP 5`, `NEW RELATED`, `MEDIA UPDATE`.
    pub badge: Option<String>,
    /// For social items: who did what.
    pub detail: Option<String>,
    /// Airing items only.
    pub episode: Option<i64>,
    pub media_id: Option<i64>,
    pub image: Option<String>,
    pub banner: Option<String>,
    pub unread: bool,
}

/// The dice's pool, as Android's: popular enough that a pick is worth a look, released, and only
/// the formats people sit down to watch. 70 pages of 50.
pub const RANDOM_POOL_PAGES: u32 = 70;
const RANDOM_POOL_PAGE_SIZE: u32 = 50;
const RANDOM_POOL_MIN_POPULARITY: u32 = 10_000;

/// The signed-in account as its profile page shows it.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: i64,
    pub name: String,
    pub avatar: Option<String>,
    pub banner: Option<String>,
    /// Unix seconds; `None` when the service doesn't say.
    pub joined: Option<i64>,
    pub url: String,
    pub titles: i64,
    pub episodes: i64,
    pub minutes: i64,
    /// 0..10; `None` before anything is scored.
    pub mean_score: Option<f64>,
    /// List activity per day (unix seconds at the day's start, UTC), as the service's own
    /// heatmap counts it. Empty when the service keeps none.
    pub activity: Vec<ActivityDay>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityDay {
    pub day: i64,
    pub amount: i64,
}

impl AniList {
    pub fn new(http: Http) -> Self {
        Self { http, token: RwLock::new(None) }
    }

    pub fn set_token(&self, token: Option<String>) {
        *self.token.write().unwrap() = token;
    }

    pub fn has_token(&self) -> bool {
        self.token.read().unwrap().is_some()
    }

    async fn query(&self, query: &str, variables: Value) -> Result<Value> {
        let body = json!({ "query": query, "variables": variables });
        let mut request = self
            .http
            .client()
            .post(ENDPOINT)
            .timeout(Duration::from_secs(20))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(serde_json::to_vec(&body)?);
        if let Some(token) = self.token.read().unwrap().as_deref() {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        let response = request.send().await?;
        let status = response.status().as_u16();
        if status == 429 {
            let retry_after =
                response.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(60);
            return Err(Error::RateLimited { service: SERVICE, retry_after });
        }
        let bytes = response.bytes().await?;
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| Error::decode(SERVICE, e))?;
        if let Some(errors) = value.get("errors").and_then(Value::as_array).filter(|e| !e.is_empty()) {
            let message = errors[0].get("message").and_then(Value::as_str).unwrap_or("error").to_owned();
            let code = errors[0].get("status").and_then(Value::as_u64).unwrap_or(status as u64) as u16;
            if code == 404 {
                return Err(Error::NotFound(message));
            }
            return Err(Error::Http { service: SERVICE, status: code, detail: message });
        }
        if status >= 400 {
            return Err(Error::Http { service: SERVICE, status, detail: "request failed".into() });
        }
        value.get("data").cloned().ok_or_else(|| Error::decode(SERVICE, "no data"))
    }

    pub async fn home(&self, season: &str, year: i32, show_adult: bool) -> Result<Home> {
        let adult = if show_adult { "" } else { ", isAdult: false" };
        let query = format!(
            r#"query ($season: MediaSeason, $year: Int, $airedBefore: Int) {{
              trending: Page(perPage: 20) {{ media(type: ANIME, sort: TRENDING_DESC{adult}) {{ {CARD_FIELDS} }} }}
              seasonal: Page(perPage: 20) {{ media(type: ANIME, season: $season, seasonYear: $year, sort: POPULARITY_DESC{adult}) {{ {CARD_FIELDS} }} }}
              popular: Page(perPage: 20) {{ media(type: ANIME, sort: POPULARITY_DESC{adult}) {{ {CARD_FIELDS} }} }}
              topRated: Page(perPage: 20) {{ media(type: ANIME, sort: SCORE_DESC{adult}) {{ {CARD_FIELDS} }} }}
              upcoming: Page(perPage: 20) {{ media(type: ANIME, status: NOT_YET_RELEASED, sort: POPULARITY_DESC{adult}) {{ {CARD_FIELDS} }} }}
              movies: Page(perPage: 20) {{ media(type: ANIME, format: MOVIE, sort: POPULARITY_DESC{adult}) {{ {CARD_FIELDS} }} }}
              newest: Page(perPage: 50) {{ airingSchedules(airingAt_lesser: $airedBefore, sort: TIME_DESC) {{ media {{ {CARD_FIELDS} }} }} }}
            }}"#
        );
        // As Android: a little before now, so an episode listed as out has had time to reach the
        // sources.
        let aired_before = crate::store::now() - NEWEST_AIRED_BUFFER_SECS;
        let data = self.query(&query, json!({ "season": season, "year": year, "airedBefore": aired_before })).await?;
        let list = |key: &str| page_cards(data.get(key));
        Ok(Home {
            trending: list("trending"),
            seasonal: list("seasonal"),
            popular: list("popular"),
            top_rated: list("topRated"),
            upcoming: list("upcoming"),
            movies: list("movies"),
            newest: newest_cards(data.pointer("/newest/airingSchedules"), 20, show_adult),
        })
    }

    /// One further page of a Home row, in the same order and with the same filters [`Self::home`]
    /// opens it with, so the pages line up. `newest` pages through `airingSchedules`; its window is
    /// "now", re-derived on every call, so a page can repeat a title an earlier one had — the UI
    /// drops ids its row already shows. An unknown row simply has nothing more.
    pub async fn home_row(&self, row: &str, page: u32, season: &str, year: i32, show_adult: bool) -> Result<HomeRowPage> {
        let adult = if show_adult { "" } else { ", isAdult: false" };
        let sorted = |filter: &str, declares: &str| {
            format!(
                "query ($page: Int{declares}) {{ row: Page(page: $page, perPage: 20) {{ pageInfo {{ hasNextPage }} media({filter}{adult}) {{ {CARD_FIELDS} }} }} }}"
            )
        };
        let (query, variables) = match row {
            "trending" => (sorted("type: ANIME, sort: TRENDING_DESC", ""), json!({ "page": page })),
            "seasonal" => (
                sorted("type: ANIME, season: $season, seasonYear: $year, sort: POPULARITY_DESC", ", $season: MediaSeason, $year: Int"),
                json!({ "page": page, "season": season, "year": year }),
            ),
            "popular" => (sorted("type: ANIME, sort: POPULARITY_DESC", ""), json!({ "page": page })),
            "top_rated" => (sorted("type: ANIME, sort: SCORE_DESC", ""), json!({ "page": page })),
            "upcoming" => (sorted("type: ANIME, status: NOT_YET_RELEASED, sort: POPULARITY_DESC", ""), json!({ "page": page })),
            "movies" => (sorted("type: ANIME, format: MOVIE, sort: POPULARITY_DESC", ""), json!({ "page": page })),
            "newest" => (
                format!(
                    "query ($page: Int, $airedBefore: Int) {{ row: Page(page: $page, perPage: 50) {{ pageInfo {{ hasNextPage }} \
                     airingSchedules(airingAt_lesser: $airedBefore, sort: TIME_DESC) {{ media {{ {CARD_FIELDS} }} }} }} }}"
                ),
                json!({ "page": page, "airedBefore": crate::store::now() - NEWEST_AIRED_BUFFER_SECS }),
            ),
            _ => return Ok(HomeRowPage { cards: Vec::new(), has_more: false }),
        };
        let data = self.query(&query, variables).await?;
        let page_obj = data.get("row");
        Ok(HomeRowPage {
            cards: if row == "newest" {
                newest_cards(page_obj.and_then(|p| p.get("airingSchedules")), 50, show_adult)
            } else {
                page_cards(page_obj)
            },
            has_more: page_obj.and_then(|p| p.pointer("/pageInfo/hasNextPage")).and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// One page of Browse. `ids_in` narrows to those titles (Dubbed only), `ids_not_in` leaves
    /// some out (Hide watched): the two filters AniList can't answer itself, sent as id lists so
    /// paging stays honest, as Android does.
    pub async fn search(&self, filters: &SearchFilters, show_adult: bool, ids_in: Option<&[i64]>, ids_not_in: &[i64]) -> Result<SearchPage> {
        let query = format!(
            r#"query ($page: Int, $search: String, $genres: [String], $genresNot: [String], $tags: [String], $tagsNot: [String],
                      $year: Int, $season: MediaSeason, $from: FuzzyDateInt, $to: FuzzyDateInt, $format: MediaFormat,
                      $status: MediaStatus, $country: CountryCode, $source: MediaSource, $episodesMore: Int, $episodesLess: Int,
                      $scoreMore: Int, $ids: [Int], $idsNot: [Int], $sort: [MediaSort], $isAdult: Boolean) {{
              Page(page: $page, perPage: 30) {{
                pageInfo {{ hasNextPage }}
                media(type: ANIME, search: $search, genre_in: $genres, genre_not_in: $genresNot, tag_in: $tags,
                      tag_not_in: $tagsNot, seasonYear: $year, season: $season, startDate_greater: $from,
                      startDate_lesser: $to, format: $format, status: $status, countryOfOrigin: $country,
                      source: $source, episodes_greater: $episodesMore, episodes_lesser: $episodesLess,
                      averageScore_greater: $scoreMore, id_in: $ids, id_not_in: $idsNot, sort: $sort,
                      isAdult: $isAdult) {{ {CARD_FIELDS} }}
              }}
            }}"#
        );
        let data = self.query(&query, search_variables(filters, show_adult, ids_in, ids_not_in)).await?;
        let page = data.get("Page");
        Ok(SearchPage {
            results: page_cards(page),
            has_next: page.and_then(|p| p.pointer("/pageInfo/hasNextPage")).and_then(Value::as_bool).unwrap_or(false),
            correction: None,
        })
    }

    pub async fn detail(&self, id: i64) -> Result<AnimeDetail> {
        let query = format!(
            r#"query ($id: Int) {{
              Media(id: $id, type: ANIME) {{
                {CARD_FIELDS}
                idMal duration source synonyms description(asHtml: true)
                title {{ native }}
                startDate {{ year month day }} endDate {{ year month day }}
                studios(isMain: true) {{ nodes {{ name }} }}
                trailer {{ site id }}
                tags {{ name rank isMediaSpoiler }}
                relations {{ edges {{ relationType(version: 2) node {{ type {CARD_FIELDS} }} }} }}
                recommendations(perPage: 12, sort: RATING_DESC) {{ nodes {{ mediaRecommendation {{ type {CARD_FIELDS} }} }} }}
                characters(perPage: 12, sort: [ROLE, RELEVANCE]) {{
                  edges {{ role node {{ name {{ userPreferred }} image {{ large }} }}
                          voiceActors(language: JAPANESE) {{ name {{ userPreferred }} }} }}
                }}
              }}
            }}"#
        );
        let data = self.query(&query, json!({ "id": id })).await?;
        let media = data.get("Media").ok_or_else(|| Error::NotFound(format!("anime {id}")))?;
        parse_detail(media).ok_or_else(|| Error::decode(SERVICE, "media without id"))
    }

    /// Episodes airing between two unix times, oldest first.
    pub async fn schedule(&self, from: i64, to: i64, show_adult: bool) -> Result<Vec<AiringEntry>> {
        let query = format!(
            r#"query ($page: Int, $from: Int, $to: Int) {{
              Page(page: $page, perPage: 50) {{
                pageInfo {{ hasNextPage }}
                airingSchedules(airingAt_greater: $from, airingAt_lesser: $to, sort: TIME) {{
                  airingAt episode media {{ {CARD_FIELDS} }}
                }}
              }}
            }}"#
        );
        let mut entries = Vec::new();
        for page in 1..=6 {
            let data = self.query(&query, json!({ "page": page, "from": from, "to": to })).await?;
            let page_value = data.get("Page");
            for item in page_value.and_then(|p| p.get("airingSchedules")).and_then(Value::as_array).into_iter().flatten() {
                let Some(anime) = item.get("media").and_then(parse_card) else { continue };
                if anime.is_adult && !show_adult {
                    continue;
                }
                entries.push(AiringEntry {
                    airing_at: item.get("airingAt").and_then(Value::as_i64).unwrap_or_default(),
                    episode: item.get("episode").and_then(Value::as_i64).unwrap_or_default() as i32,
                    anime,
                });
            }
            let more = page_value.and_then(|p| p.pointer("/pageInfo/hasNextPage")).and_then(Value::as_bool).unwrap_or(false);
            if !more {
                break;
            }
        }
        Ok(entries)
    }

    pub async fn viewer(&self) -> Result<Viewer> {
        let data = self.query("query { Viewer { id name avatar { large } options { displayAdultContent } } }", json!({})).await?;
        let v = data.get("Viewer").ok_or_else(|| Error::decode(SERVICE, "no viewer"))?;
        Ok(Viewer {
            id: v.get("id").and_then(Value::as_i64).unwrap_or_default(),
            name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
            avatar: v.pointer("/avatar/large").and_then(Value::as_str).map(str::to_owned),
            show_adult: v.pointer("/options/displayAdultContent").and_then(Value::as_bool).unwrap_or(false),
        })
    }

    /// The signed-in account's profile: picture, banner, totals and day-by-day activity.
    pub async fn account(&self) -> Result<Account> {
        let data = self
            .query(
                "query { Viewer { id name siteUrl createdAt bannerImage avatar { large }
                   statistics { anime { count episodesWatched minutesWatched meanScore } } } }",
                json!({}),
            )
            .await?;
        let v = data.get("Viewer").ok_or_else(|| Error::decode(SERVICE, "no viewer"))?;
        let id = v.get("id").and_then(Value::as_i64).unwrap_or_default();
        let stats = v.pointer("/statistics/anime");
        let number = |key: &str| stats.and_then(|s| s.get(key)).and_then(Value::as_i64).unwrap_or_default();
        let name = v.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
        Ok(Account {
            id,
            url: v.get("siteUrl").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| format!("https://anilist.co/user/{name}/")),
            name,
            avatar: v.pointer("/avatar/large").and_then(Value::as_str).map(str::to_owned),
            banner: v.get("bannerImage").and_then(Value::as_str).map(str::to_owned),
            joined: v.get("createdAt").and_then(Value::as_i64).filter(|t| *t > 0),
            titles: number("count"),
            episodes: number("episodesWatched"),
            minutes: number("minutesWatched"),
            // AniList's mean is on the account's own scale; ours is 0..10.
            mean_score: stats
                .and_then(|s| s.get("meanScore"))
                .and_then(Value::as_f64)
                .filter(|m| *m > 0.0)
                .map(|m| if m > 10.0 { m / 10.0 } else { m }),
            // `User.stats` is deprecated but is the only source of the daily history; a failure
            // there costs the heatmap, not the page.
            activity: self.activity_history(id).await.unwrap_or_default(),
        })
    }

    async fn activity_history(&self, user_id: i64) -> Result<Vec<ActivityDay>> {
        let data = self
            .query("query ($id: Int) { User(id: $id) { stats { activityHistory { date amount } } } }", json!({ "id": user_id }))
            .await?;
        Ok(data
            .pointer("/User/stats/activityHistory")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|d| {
                let day = d.get("date").and_then(Value::as_i64)?;
                let amount = d.get("amount").and_then(Value::as_i64).filter(|a| *a > 0)?;
                Some(ActivityDay { day: day - day.rem_euclid(86_400), amount })
            })
            .collect())
    }

    /// The signed-in user's whole anime list.
    pub async fn list(&self, user_id: i64) -> Result<Vec<ListEntry>> {
        let query = format!(
            r#"query ($userId: Int) {{
              MediaListCollection(userId: $userId, type: ANIME) {{
                lists {{ entries {{ status progress score(format: POINT_10_DECIMAL) updatedAt media {{ {CARD_FIELDS} studios(isMain: true) {{ nodes {{ name isAnimationStudio }} }} }} }} }}
              }}
            }}"#
        );
        let data = self.query(&query, json!({ "userId": user_id })).await?;
        let mut entries = Vec::new();
        for list in data.pointer("/MediaListCollection/lists").and_then(Value::as_array).into_iter().flatten() {
            for e in list.get("entries").and_then(Value::as_array).into_iter().flatten() {
                let Some(anime) = e.get("media").and_then(parse_card) else { continue };
                entries.push(ListEntry {
                    anime,
                    status: e.get("status").and_then(Value::as_str).unwrap_or("CURRENT").to_owned(),
                    progress: e.get("progress").and_then(Value::as_i64).unwrap_or_default() as i32,
                    score: e.get("score").and_then(Value::as_f64).unwrap_or_default(),
                    updated_at: e.get("updatedAt").and_then(Value::as_i64).unwrap_or_default(),
                    studios: e
                        .pointer("/media/studios/nodes")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter(|n| n.get("isAnimationStudio").and_then(Value::as_bool).unwrap_or(true))
                        .filter_map(|n| n.get("name").and_then(Value::as_str).map(str::to_owned))
                        .collect(),
                });
            }
        }
        Ok(entries)
    }

    /// AniList cards for MAL ids, in batches (MAL has no usable metadata of its own).
    pub async fn cards_by_mal_ids(&self, ids: &[i64]) -> Result<Vec<(i64, AnimeCard)>> {
        let query = format!(
            r#"query ($ids: [Int], $page: Int) {{
              Page(page: $page, perPage: 50) {{ media(idMal_in: $ids, type: ANIME) {{ idMal {CARD_FIELDS} }} }}
            }}"#
        );
        let mut out = Vec::new();
        for chunk in ids.chunks(50) {
            let data = self.query(&query, json!({ "ids": chunk, "page": 1 })).await?;
            for media in data.pointer("/Page/media").and_then(Value::as_array).into_iter().flatten() {
                if let (Some(mal), Some(card)) = (media.get("idMal").and_then(Value::as_i64), parse_card(media)) {
                    out.push((mal, card));
                }
            }
        }
        Ok(out)
    }

    /// Cards (with synopses) for AniList ids, in the order asked, 50 to a request. Ids AniList
    /// doesn't return are left out.
    pub async fn cards_by_ids(&self, ids: &[i64]) -> Result<Vec<Pick>> {
        let query = format!(
            r#"query ($ids: [Int]) {{
              Page(page: 1, perPage: 50) {{ media(id_in: $ids, type: ANIME) {{ {CARD_FIELDS} description(asHtml: false) }} }}
            }}"#
        );
        let mut found: std::collections::HashMap<i64, Pick> = std::collections::HashMap::new();
        for chunk in ids.chunks(50) {
            let data = self.query(&query, json!({ "ids": chunk })).await?;
            for media in data.pointer("/Page/media").and_then(Value::as_array).into_iter().flatten() {
                if let Some(card) = parse_card(media) {
                    let description = media.get("description").and_then(Value::as_str).map(str::to_owned);
                    found.insert(card.id, Pick { card, description });
                }
            }
        }
        Ok(ids.iter().filter_map(|id| found.remove(id)).collect())
    }

    /// One page (1..=[`RANDOM_POOL_PAGES`]) of the dice's pool, most popular first.
    pub async fn random_pool(&self, page: u32, hide_adult: bool) -> Result<Vec<Pick>> {
        let adult = if hide_adult { ", isAdult: false" } else { "" };
        let query = format!(
            r#"query ($page: Int, $perPage: Int, $minPopularity: Int) {{
              Page(page: $page, perPage: $perPage) {{
                media(type: ANIME, sort: [POPULARITY_DESC], popularity_greater: $minPopularity,
                      format_in: [TV, MOVIE, ONA, OVA], status_not: NOT_YET_RELEASED{adult}) {{
                  {CARD_FIELDS} description(asHtml: false)
                }}
              }}
            }}"#
        );
        let data = self
            .query(&query, json!({ "page": page, "perPage": RANDOM_POOL_PAGE_SIZE, "minPopularity": RANDOM_POOL_MIN_POPULARITY }))
            .await?;
        Ok(data
            .pointer("/Page/media")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|m| {
                Some(Pick { card: parse_card(m)?, description: m.get("description").and_then(Value::as_str).map(str::to_owned) })
            })
            .collect())
    }

    /// Titles with their start dates and anime relations (the watch guide's members), 50 a request.
    pub async fn members(&self, ids: &[i64]) -> Result<Vec<Member>> {
        let query = format!(
            r#"query ($ids: [Int]) {{
              Page(page: 1, perPage: 50) {{ media(id_in: $ids, type: ANIME) {{
                {CARD_FIELDS} startDate {{ year month day }}
                relations {{ edges {{ relationType node {{ type {CARD_FIELDS} startDate {{ year month day }} }} }} }}
              }} }}
            }}"#
        );
        let mut out = Vec::new();
        for chunk in ids.chunks(50) {
            let data = self.query(&query, json!({ "ids": chunk })).await?;
            out.extend(data.pointer("/Page/media").and_then(Value::as_array).into_iter().flatten().filter_map(Member::parse));
        }
        Ok(out)
    }

    /// The MAL id of an AniList title, if it has one.
    pub async fn mal_id(&self, id: i64) -> Result<Option<i64>> {
        let data = self.query("query ($id: Int) { Media(id: $id, type: ANIME) { idMal } }", json!({ "id": id })).await?;
        Ok(data.pointer("/Media/idMal").and_then(Value::as_i64))
    }

    /// Sets progress (and optionally status) for one title on the user's AniList.
    pub async fn save_progress(&self, anime_id: i64, progress: i32, status: Option<&str>) -> Result<()> {
        let query = "mutation ($mediaId: Int, $progress: Int, $status: MediaListStatus) {
            SaveMediaListEntry(mediaId: $mediaId, progress: $progress, status: $status) { id }
        }";
        let mut variables = json!({ "mediaId": anime_id, "progress": progress });
        if let Some(status) = status {
            variables["status"] = json!(status);
        }
        self.query(query, variables).await.map(|_| ())
    }

    /// How far the signed-in account's entry for a title has got, if it has one.
    pub async fn entry_progress(&self, anime_id: i64) -> Result<Option<i32>> {
        let data = self
            .query("query ($id: Int) { Media(id: $id, type: ANIME) { mediaListEntry { progress } } }", json!({ "id": anime_id }))
            .await?;
        Ok(data.pointer("/Media/mediaListEntry/progress").and_then(Value::as_i64).map(|p| p as i32))
    }

    /// Puts a title on one of the standard lists, keeping its progress and score. Choosing a status
    /// is choosing to have it on that list, so a title kept in custom lists only comes out onto it
    /// (as Android's `updateMediaListStatus`).
    pub async fn save_status(&self, anime_id: i64, status: &str) -> Result<()> {
        let query = "mutation ($mediaId: Int, $status: MediaListStatus) {
            SaveMediaListEntry(mediaId: $mediaId, status: $status, hiddenFromStatusLists: false) { id }
        }";
        self.query(query, json!({ "mediaId": anime_id, "status": status })).await.map(|_| ())
    }

    /// The account's notification feed, newest first, and how many are unread. AniList keeps no
    /// per-item read flag: the first `unread` entries are the unread ones. `mark_read` resets the
    /// count afterwards, in a separate request (sibling root fields have no order).
    pub async fn notifications(&self, mark_read: bool) -> Result<(Vec<FeedItem>, i64)> {
        const MEDIA: &str = "media { id type format title { romaji english userPreferred } coverImage { large } bannerImage }";
        const USER: &str = "user { id name avatar { large } }";
        let query = format!(
            r#"query {{
              Viewer {{ unreadNotificationCount }}
              Page(page: 1, perPage: 50) {{
                notifications(resetNotificationCount: false) {{
                  __typename
                  ... on AiringNotification {{ id createdAt episode {MEDIA} }}
                  ... on RelatedMediaAdditionNotification {{ id createdAt context {MEDIA} }}
                  ... on MediaDataChangeNotification {{ id createdAt context {MEDIA} }}
                  ... on MediaMergeNotification {{ id createdAt reason {MEDIA} }}
                  ... on FollowingNotification {{ id createdAt context {USER} }}
                  ... on ActivityMessageNotification {{ id createdAt context {USER} }}
                  ... on ActivityMentionNotification {{ id createdAt context {USER} }}
                  ... on ActivityReplyNotification {{ id createdAt context {USER} }}
                  ... on ActivityLikeNotification {{ id createdAt context {USER} }}
                  ... on ActivityReplyLikeNotification {{ id createdAt context {USER} }}
                }}
              }}
            }}"#
        );
        let data = self.query(&query, json!({})).await?;
        let unread = data.pointer("/Viewer/unreadNotificationCount").and_then(Value::as_i64).unwrap_or_default();
        let mut items = Vec::new();
        let mut hidden_unread = 0;
        for (index, n) in data.pointer("/Page/notifications").and_then(Value::as_array).into_iter().flatten().enumerate() {
            let is_unread = (index as i64) < unread;
            if !feed_media_wanted(n) {
                hidden_unread += i64::from(is_unread);
                continue;
            }
            items.extend(parse_feed_item(n, is_unread));
        }
        // The bell counts what the page shows.
        let unread = (unread - hidden_unread).max(0);
        if mark_read && unread > 0 {
            self.query("query { Page(page: 1, perPage: 1) { notifications(resetNotificationCount: true) { __typename } } }", json!({}))
                .await?;
        }
        Ok((items, unread))
    }

    /// Every tag AniList files anime under, with its category.
    pub async fn tags(&self) -> Result<Vec<Tag>> {
        let data = self.query("query { MediaTagCollection { name description category isAdult } }", json!({})).await?;
        Ok(data
            .get("MediaTagCollection")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|t| serde_json::from_value(t.clone()).ok())
            .collect())
    }

    /// Takes a title off the list, progress and score included (AniList keeps no undo). `false`
    /// when there was no entry to delete.
    pub async fn delete_entry(&self, anime_id: i64) -> Result<bool> {
        let data =
            self.query("query ($id: Int) { Media(id: $id, type: ANIME) { mediaListEntry { id } } }", json!({ "id": anime_id })).await?;
        let Some(entry) = data.pointer("/Media/mediaListEntry/id").and_then(Value::as_i64) else { return Ok(false) };
        let data = self.query("mutation ($id: Int) { DeleteMediaListEntry(id: $id) { deleted } }", json!({ "id": entry })).await?;
        Ok(data.pointer("/DeleteMediaListEntry/deleted").and_then(Value::as_bool).unwrap_or(false))
    }
}

/// One of AniList's tags, for Browse's tag filter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tag {
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub is_adult: bool,
}

/// How long before now an episode has to have aired to count as new (Android's
/// `NEWEST_AIRED_BUFFER_SEC`).
const NEWEST_AIRED_BUFFER_SECS: i64 = 90 * 60;

/// Recently aired episodes' titles, newest first: a show airing several times in the window
/// appears once, and adult titles (which `airingSchedules` can't filter) follow the setting. Home's own
/// row shows `limit` of them; a page asked for by scrolling shows all it holds.
fn newest_cards(schedules: Option<&Value>, limit: usize, show_adult: bool) -> Vec<AnimeCard> {
    let mut seen = std::collections::HashSet::new();
    schedules
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.get("media").and_then(parse_card))
        .filter(|c| (show_adult || !c.is_adult) && seen.insert(c.id))
        .take(limit)
        .collect()
}

fn page_cards(page: Option<&Value>) -> Vec<AnimeCard> {
    page.and_then(|p| p.get("media"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_card).collect())
        .unwrap_or_default()
}

fn s(v: &Value, pointer: &str) -> Option<String> {
    v.pointer(pointer).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
}

fn i(v: &Value, pointer: &str) -> Option<i64> {
    v.pointer(pointer).and_then(Value::as_i64)
}

/// Whether a notification's title belongs on the page: AniList also announces music videos and
/// manga added to a franchise, which this app has nothing to do with. Social items carry no media.
fn feed_media_wanted(n: &Value) -> bool {
    let Some(media) = n.get("media").filter(|m| !m.is_null()) else { return true };
    let field = |key: &str| media.get(key).and_then(Value::as_str);
    field("type").is_none_or(|t| t == "ANIME") && field("format") != Some("MUSIC")
}

fn parse_feed_item(n: &Value, unread: bool) -> Option<FeedItem> {
    let s = |path: &str| n.pointer(path).and_then(Value::as_str).map(str::to_owned);
    let typename = s("/__typename")?;
    let id = n.get("id").and_then(Value::as_i64)?;
    let media_title = s("/media/title/english").or_else(|| s("/media/title/userPreferred")).or_else(|| s("/media/title/romaji"));
    let user = s("/user/name");
    let context = s("/context").or_else(|| s("/reason"));
    let kind = match typename.as_str() {
        "AiringNotification" => "airing",
        "RelatedMediaAdditionNotification" | "MediaDataChangeNotification" | "MediaMergeNotification" => "media",
        _ => "social",
    };
    let episode = n.get("episode").and_then(Value::as_i64);
    let badge = match typename.as_str() {
        "AiringNotification" => episode.map(|e| format!("EP {e}")),
        "RelatedMediaAdditionNotification" => Some("NEW RELATED".into()),
        "MediaDataChangeNotification" | "MediaMergeNotification" => Some("MEDIA UPDATE".into()),
        _ => None,
    };
    let detail = (kind == "social")
        .then(|| [user.clone(), context.map(|c| c.trim().to_owned())].into_iter().flatten().collect::<Vec<_>>().join(" "))
        .filter(|d| !d.trim().is_empty());
    Some(FeedItem {
        id,
        kind: kind.into(),
        created_at: n.get("createdAt").and_then(Value::as_i64).unwrap_or_default(),
        title: media_title.or(user).unwrap_or_else(|| "AniList".into()),
        badge,
        detail,
        episode: if kind == "airing" { episode } else { None },
        media_id: n.pointer("/media/id").and_then(Value::as_i64),
        image: s("/media/coverImage/large").or_else(|| s("/user/avatar/large")),
        banner: s("/media/bannerImage"),
        unread,
    })
}

pub fn parse_card(m: &Value) -> Option<AnimeCard> {
    let id = i(m, "/id")?;
    let title = s(m, "/title/english")
        .or_else(|| s(m, "/title/userPreferred"))
        .or_else(|| s(m, "/title/romaji"))
        .unwrap_or_else(|| format!("#{id}"));
    Some(AnimeCard {
        id,
        title,
        title_romaji: s(m, "/title/romaji"),
        cover: s(m, "/coverImage/extraLarge").or_else(|| s(m, "/coverImage/large")),
        color: s(m, "/coverImage/color"),
        banner: s(m, "/bannerImage"),
        format: s(m, "/format"),
        status: s(m, "/status"),
        episodes: i(m, "/episodes").map(|n| n as i32),
        score: i(m, "/averageScore").map(|n| n as i32),
        year: i(m, "/seasonYear").map(|n| n as i32),
        season: s(m, "/season"),
        genres: m
            .get("genres")
            .and_then(Value::as_array)
            .map(|g| g.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default(),
        next_airing: m
            .get("nextAiringEpisode")
            .filter(|v| v.is_object())
            .map(|n| NextAiring { episode: i(n, "/episode").unwrap_or_default() as i32, airing_at: i(n, "/airingAt").unwrap_or_default() }),
        is_adult: m.get("isAdult").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn date(v: &Value, pointer: &str) -> Option<String> {
    let d = v.pointer(pointer)?;
    let year = d.get("year").and_then(Value::as_i64)?;
    Some(match (d.get("month").and_then(Value::as_i64), d.get("day").and_then(Value::as_i64)) {
        (Some(m), Some(day)) => format!("{year}-{m:02}-{day:02}"),
        (Some(m), None) => format!("{year}-{m:02}"),
        _ => year.to_string(),
    })
}

pub fn parse_detail(m: &Value) -> Option<AnimeDetail> {
    let card = parse_card(m)?;
    let is_anime = |node: &Value| node.get("type").and_then(Value::as_str).is_none_or(|t| t == "ANIME");
    Some(AnimeDetail {
        mal_id: i(m, "/idMal"),
        title_native: s(m, "/title/native"),
        synonyms: m
            .get("synonyms")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default(),
        description: s(m, "/description"),
        duration: i(m, "/duration").map(|n| n as i32),
        source: s(m, "/source"),
        studios: m
            .pointer("/studios/nodes")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|n| s(n, "/name")).collect())
            .unwrap_or_default(),
        start_date: date(m, "/startDate"),
        end_date: date(m, "/endDate"),
        trailer: match (s(m, "/trailer/site"), s(m, "/trailer/id")) {
            (Some(site), Some(id)) => Some(Trailer { site, id }),
            _ => None,
        },
        relations: m
            .pointer("/relations/edges")
            .and_then(Value::as_array)
            .map(|edges| {
                edges
                    .iter()
                    .filter_map(|e| {
                        let node = e.get("node")?;
                        if !is_anime(node) {
                            return None;
                        }
                        Some(Relation { kind: s(e, "/relationType")?, anime: parse_card(node)? })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        recommendations: m
            .pointer("/recommendations/nodes")
            .and_then(Value::as_array)
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|n| n.get("mediaRecommendation").filter(|r| r.is_object() && is_anime(r)))
                    .filter_map(parse_card)
                    .collect()
            })
            .unwrap_or_default(),
        characters: m
            .pointer("/characters/edges")
            .and_then(Value::as_array)
            .map(|edges| {
                edges
                    .iter()
                    .filter_map(|e| {
                        Some(Character {
                            name: s(e, "/node/name/userPreferred")?,
                            image: s(e, "/node/image/large"),
                            role: s(e, "/role").unwrap_or_default(),
                            voice_actor: e
                                .get("voiceActors")
                                .and_then(Value::as_array)
                                .and_then(|v| v.first())
                                .and_then(|v| s(v, "/name/userPreferred")),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        tags: m
            .get("tags")
            .and_then(Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter(|t| !t.get("isMediaSpoiler").and_then(Value::as_bool).unwrap_or(false))
                    .filter_map(|t| s(t, "/name"))
                    .take(12)
                    .collect()
            })
            .unwrap_or_default(),
        logo: None,
        card,
    })
}

/// Browse's variables. Only what is set is sent: AniList reads a variable sent as `null` as a
/// filter, so `isAdult: null` (adult titles shown) matched nothing at all, and every Browse page
/// without a title came back empty. The others are left out the same way.
fn search_variables(filters: &SearchFilters, show_adult: bool, ids_in: Option<&[i64]>, ids_not_in: &[i64]) -> Value {
    let search = filters.query.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let sort = filters.sort.clone().unwrap_or_else(|| if search.is_some() { "SEARCH_MATCH".into() } else { "POPULARITY_DESC".into() });
    let mut variables = json!({ "page": filters.page.unwrap_or(1), "sort": [sort] });
    let set = |v: &mut Value, k: &str, x: Value| {
        v[k] = x;
    };
    if !show_adult {
        set(&mut variables, "isAdult", json!(false));
    }
    if let Some(q) = search {
        set(&mut variables, "search", json!(q));
    }
    for (key, list) in [
        ("genres", &filters.genres),
        ("genresNot", &filters.excluded_genres),
        ("tags", &filters.tags),
        ("tagsNot", &filters.excluded_tags),
    ] {
        if !list.is_empty() {
            set(&mut variables, key, json!(list));
        }
    }
    if let Some(y) = filters.year {
        set(&mut variables, "year", json!(y));
    } else {
        // AniList's date bounds are exclusive and FuzzyDateInt (YYYYMMDD): a year span becomes
        // "after the last day before it, before the first day after it".
        if let Some(from) = filters.year_from {
            set(&mut variables, "from", json!((from - 1) * 10000 + 1231));
        }
        if let Some(to) = filters.year_to {
            set(&mut variables, "to", json!((to + 1) * 10000 + 101));
        }
    }
    for (key, value) in [
        ("season", &filters.season),
        ("format", &filters.format),
        ("status", &filters.status),
        ("country", &filters.country),
        ("source", &filters.source),
    ] {
        if let Some(v) = value.as_deref().filter(|v| !v.is_empty()) {
            set(&mut variables, key, json!(v));
        }
    }
    // Inclusive bounds, sent as AniList's exclusive ones.
    if let Some(min) = filters.episodes_min {
        set(&mut variables, "episodesMore", json!(min - 1));
    }
    if let Some(max) = filters.episodes_max {
        set(&mut variables, "episodesLess", json!(max + 1));
    }
    if let Some(score) = filters.minimum_score {
        set(&mut variables, "scoreMore", json!(score - 1));
    }
    if let Some(ids) = ids_in {
        set(&mut variables, "ids", json!(ids));
    }
    if !ids_not_in.is_empty() {
        set(&mut variables, "idsNot", json!(ids_not_in));
    }
    variables
}

#[cfg(test)]
mod tests {
    #[test]
    fn drops_music_videos_and_manga_from_the_feed() {
        let n = |media: Value| json!({ "__typename": "RelatedMediaAdditionNotification", "id": 1, "media": media });
        assert!(!feed_media_wanted(&n(json!({ "id": 2, "type": "ANIME", "format": "MUSIC" }))));
        assert!(!feed_media_wanted(&n(json!({ "id": 2, "type": "MANGA", "format": "MANGA" }))));
        assert!(feed_media_wanted(&n(json!({ "id": 2, "type": "ANIME", "format": "OVA" }))));
        assert!(feed_media_wanted(&json!({ "__typename": "FollowingNotification", "id": 1, "user": { "name": "x" } })));
    }

    #[test]
    fn reads_feed_items_as_android_does() {
        let airing = json!({"__typename": "AiringNotification", "id": 7, "createdAt": 100, "episode": 5,
            "media": {"id": 1, "title": {"romaji": "Sousou no Frieren", "english": "Frieren"}, "coverImage": {"large": "c"}}});
        let item = super::parse_feed_item(&airing, true).unwrap();
        assert_eq!((item.kind.as_str(), item.title.as_str(), item.badge.as_deref(), item.episode), ("airing", "Frieren", Some("EP 5"), Some(5)));
        let like = json!({"__typename": "ActivityLikeNotification", "id": 8, "createdAt": 90, "context": " liked your activity.",
            "user": {"id": 2, "name": "Someone", "avatar": {"large": "a"}}});
        let item = super::parse_feed_item(&like, false).unwrap();
        assert_eq!((item.kind.as_str(), item.title.as_str(), item.detail.as_deref(), item.image.as_deref()), ("social", "Someone", Some("Someone liked your activity."), Some("a")));
    }

    use super::*;

    #[test]
    fn newest_follows_visibility_without_repeating_titles() {
        let schedules = json!([
            { "media": { "id": 1, "title": { "romaji": "General A" }, "isAdult": false } },
            { "media": { "id": 2, "title": { "romaji": "Restricted" }, "isAdult": true } },
            { "media": { "id": 2, "title": { "romaji": "Restricted" }, "isAdult": true } },
            { "media": { "id": 3, "title": { "romaji": "General B" }, "isAdult": false } }
        ]);
        let ids = |show_adult, limit| newest_cards(Some(&schedules), limit, show_adult).into_iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(ids(false, 20), [1, 3]);
        assert_eq!(ids(true, 20), [1, 2, 3]);
        assert_eq!(ids(false, 2), [1, 3]);
        assert_eq!(ids(true, 2), [1, 2]);
    }

    #[test]
    fn browse_sends_no_null_variables() {
        // AniList answered `isAdult: null` with an empty page: Browse showed nothing until a title
        // was typed, and a genre alone found nothing.
        let action = SearchFilters { genres: vec!["Action".into()], ..Default::default() };
        let shown = search_variables(&action, true, None, &[]);
        assert!(shown.get("isAdult").is_none());
        assert!(shown.as_object().unwrap().values().all(|v| !v.is_null()), "{shown}");
        assert_eq!(shown["genres"], json!(["Action"]));
        assert_eq!(shown["sort"], json!(["POPULARITY_DESC"]));
        assert_eq!(search_variables(&action, false, None, &[])["isAdult"], json!(false));
    }

    #[test]
    fn card_prefers_english_title() {
        let card = parse_card(&json!({
            "id": 154587, "title": {"romaji": "Sousou no Frieren", "english": "Frieren"},
            "coverImage": {"large": "https://c/l.jpg", "color": "#e4a15d"}, "genres": ["Adventure"],
            "nextAiringEpisode": null, "isAdult": false
        }))
        .unwrap();
        assert_eq!(card.title, "Frieren");
        assert_eq!(card.cover.as_deref(), Some("https://c/l.jpg"));
        assert!(card.next_airing.is_none());
    }
}
