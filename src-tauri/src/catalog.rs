//! Browse/search/detail/episodes commands, each cached in SQLite.
//!
//! The pattern: a fresh cache hit answers immediately; otherwise fetch, store, answer; and if the
//! fetch fails, answer from a stale cache entry rather than an error. Browsing therefore keeps
//! working offline and spends little of AniList's 30–90 requests/minute.

use std::time::Instant;

use anilili_core::catalogue;
use anilili_core::media_index;
use anilili_core::model::{
    AiringEntry, AnimeDetail, Episode, EpisodeList, Home, HomeRowPage, NextAiring, SearchCorrection, SearchFilters, SearchPage, Sources,
};
use anilili_core::spelling::{SpellingTitle, TitleSpeller};
use anilili_core::store::now;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tauri::State;

use crate::app::{AppState, CmdResult, SOURCES_TTL};

/// `entry` names what failed (`NET_HOME`) when there is neither a fresh nor a stale copy.
async fn cached<T, F>(state: &AppState, entry: &'static catalogue::Entry, key: &str, max_age: i64, fetch: F) -> CmdResult<T>
where
    T: Serialize + DeserializeOwned,
    F: std::future::Future<Output = anilili_core::Result<T>>,
{
    if let Some(hit) = state.store.cache_get::<T>(key, max_age) {
        return Ok(hit);
    }
    match fetch.await {
        Ok(value) => {
            if let Err(e) = state.store.cache_put(key, &value) {
                tracing::warn!(%key, error = %e, "cache write failed");
            }
            Ok(value)
        }
        Err(e) => match state.store.cache_get_stale::<T>(key) {
            Some(stale) => {
                tracing::warn!(%key, error = %e, "serving stale cache");
                Ok(stale)
            }
            None => Err(crate::app::as_entry(entry)(e)),
        },
    }
}

/// The AniList season for a month (1-12).
fn season_of(month: u32) -> &'static str {
    match month {
        12 | 1 | 2 => "WINTER",
        3..=5 => "SPRING",
        6..=8 => "SUMMER",
        _ => "FALL",
    }
}

/// (year, month) from a unix time, without a date crate.
fn year_month(unix: i64) -> (i32, u32) {
    let (year, month, _) = civil_date(unix);
    (year, month)
}

/// The UTC calendar date of a Unix time.
pub(crate) fn civil_date(unix: i64) -> (i32, u32, u32) {
    let days = unix.div_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    (year, month, day)
}

#[tauri::command]
pub async fn home(state: State<'_, AppState>) -> CmdResult<Home> {
    let (year, month) = year_month(now());
    // December belongs to next year's winter season on AniList.
    let season_year = if month == 12 { year + 1 } else { year };
    let season = season_of(month);
    let show_adult = state.settings().show_adult;
    // Keep each visibility choice separate, including stale answers from before a toggle.
    let key = format!("home-v3:{season}:{season_year}:{show_adult}");
    let mut home: Home = cached(&state, catalogue::NET_HOME, &key, 30 * 60, state.anilist.home(season, season_year, show_adult)).await?;
    // The hero rotates titles with wide art; backdrops add the ones AniList has no banner for.
    if let Some(backdrops) = backdrops(&state).await {
        for card in
            [&mut home.trending, &mut home.seasonal, &mut home.popular, &mut home.top_rated, &mut home.upcoming, &mut home.newest, &mut home.movies]
                .into_iter()
                .flatten()
        {
            if card.banner.is_none() {
                card.banner = backdrops.get(&card.id).cloned();
            }
        }
    }
    Ok(home)
}

/// The next page of one of Home's rows (`home` opens each with its first), asked for when a row is
/// scrolled to its end. Cached like Home itself; `newest`'s pages are windows on "now", so their
/// edges can repeat or skip a title between calls — the UI drops ids its row already shows.
#[tauri::command]
pub async fn home_more(state: State<'_, AppState>, row: String, page: u32) -> CmdResult<HomeRowPage> {
    let (year, month) = year_month(now());
    // December belongs to next year's winter season on AniList.
    let season_year = if month == 12 { year + 1 } else { year };
    let season = season_of(month);
    let show_adult = state.settings().show_adult;
    let key = format!("home-more-v2:{row}:{season}:{season_year}:{}:{show_adult}", page.max(1));
    cached(&state, catalogue::NET_HOME, &key, 30 * 60, state.anilist.home_row(&row, page.max(1), season, season_year, show_adult)).await
}

/// Below this many results a title search is checked for a misspelling (Android's
/// `SPELLING_CHECK_BELOW`): a term that found a dozen titles meant what it said.
const SPELLING_CHECK_BELOW: usize = 10;

/// A page of Browse, corrected for spelling the way a web search does it (Android's
/// `discoverCorrectingSpelling`).
///
/// AniList returns nothing at all for a misspelled word, so "naurto" used to end on "Nothing
/// found". When the term finds nothing and the media index's title list says what it most likely
/// meant, the page holds that instead and says so; when it found a little, the page keeps it and the
/// likelier spelling is only offered. Only the first page is checked: the UI asks for the rest with
/// the corrected term and `exact`, which is also what "Search instead for" sends.
#[tauri::command]
pub async fn search(state: State<'_, AppState>, filters: SearchFilters, exact: Option<bool>) -> CmdResult<SearchPage> {
    let typed = filters.query.as_deref().map(str::trim).unwrap_or_default().to_owned();
    let first_title_page = !typed.is_empty() && filters.page.unwrap_or(1) <= 1;
    let page = match search_page(&state, &filters).await {
        Ok(page) if first_title_page && page.results.len() < SPELLING_CHECK_BELOW => with_index_titles(&state, &filters, page).await,
        Ok(page) => page,
        // AniList refused or couldn't be reached, with nothing cached: the title list still knows
        // the titles, so a search for one finds it.
        Err(e) if first_title_page => {
            let page = with_index_titles(&state, &filters, SearchPage { results: Vec::new(), has_next: false, correction: None }).await;
            if page.results.is_empty() {
                return Err(e);
            }
            tracing::warn!(query = %typed, error = ?e, "title search answered from the media index");
            page
        }
        Err(e) => return Err(e),
    };
    if typed.is_empty() || filters.page.unwrap_or(1) > 1 || exact == Some(true) || page.results.len() >= SPELLING_CHECK_BELOW {
        return Ok(page);
    }
    let Some(suggested) = spelling_suggestion(&state, &typed).await else { return Ok(page) };
    if !page.results.is_empty() {
        return Ok(SearchPage { correction: Some(SearchCorrection { typed, suggested, applied: false }), ..page });
    }
    let corrected = SearchFilters { query: Some(suggested.clone()), ..filters };
    match search_page(&state, &corrected).await {
        Ok(found) if !found.results.is_empty() => {
            Ok(SearchPage { correction: Some(SearchCorrection { typed, suggested, applied: true }), ..found })
        }
        // A correction that finds nothing either is no help; the plain empty page says enough.
        _ => Ok(page),
    }
}

/// What a title search that found little or nothing was most likely meant to be; see
/// `anilili_core::spelling`. `None` when it reads like a title, or the title list can't be had.
async fn spelling_suggestion(state: &AppState, query: &str) -> Option<String> {
    let speller = speller(state).await?;
    let hide_adult = !state.settings().show_adult;
    let query = query.to_owned();
    tauri::async_runtime::spawn_blocking(move || speller.suggest(&query, hide_adult, 8).map(|s| s.text)).await.ok().flatten()
}

/// A thin first page of a title search filled out from the media index's title list (Android's
/// `withLocalTitleMatches`): AniList's own results first, as they are current, then the titles it
/// didn't give. AniList matches whole words only, so this is what finds "one pie" while it is
/// still being typed, and what keeps a search answering when AniList sends back nothing.
async fn with_index_titles(state: &AppState, filters: &SearchFilters, page: SearchPage) -> SearchPage {
    let Some(titles) = index_titles(state).await else { return page };
    let show_adult = state.settings().show_adult;
    let dubbed = if filters.dubbed_only { Some(dub_ids(state).await) } else { None };
    let watched = watched_ids(state, filters.hide_watched.as_deref());
    let filters = filters.clone();
    let local = tauri::async_runtime::spawn_blocking(move || {
        media_index::search_titles(&titles, &filters, show_adult, dubbed.as_deref(), &watched)
    })
    .await
    .unwrap_or_default();
    if local.is_empty() {
        return page;
    }
    let mut results = page.results;
    for card in local {
        if !results.iter().any(|r| r.id == card.id) {
            results.push(card);
        }
    }
    SearchPage { results, ..page }
}

/// The media index's title list, held in memory for a day: the speller is built from it, and it
/// answers title searches AniList answers thinly.
async fn index_titles(state: &AppState) -> Option<std::sync::Arc<Vec<media_index::IndexTitle>>> {
    const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
    let mut held = state.index_titles.lock().await;
    if let Some((loaded, titles)) = held.as_ref()
        && loaded.elapsed() < MAX_AGE
    {
        return Some(titles.clone());
    }
    // `-v2`: the rows now keep the poster, year, status, episodes and score a card needs.
    let titles = index_cached(state, "index-titles-v2", 24 * 3600, move |i| async move { i.titles().await }).await?;
    if titles.is_empty() {
        return None;
    }
    let titles = std::sync::Arc::new(titles);
    *held = Some((Instant::now(), titles.clone()));
    Some(titles)
}

/// The speller, built once a day from the media index's title list (the list Android bundles).
async fn speller(state: &AppState) -> Option<std::sync::Arc<TitleSpeller>> {
    const MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
    let mut held = state.speller.lock().await;
    if let Some((built, speller)) = held.as_ref()
        && built.elapsed() < MAX_AGE
    {
        return Some(speller.clone());
    }
    let titles = index_titles(state).await?;
    let started = Instant::now();
    let speller = tauri::async_runtime::spawn_blocking(move || {
        TitleSpeller::new(
            titles
                .iter()
                .map(|t| SpellingTitle { adult: t.is_adult(), id: t.id, title: t.title.clone(), popularity: t.popularity.unwrap_or(0) })
                .collect(),
        )
    })
    .await
    .ok()
    .map(std::sync::Arc::new)?;
    tracing::info!(ms = started.elapsed().as_millis() as u64, "search speller built");
    *held = Some((Instant::now(), speller.clone()));
    Some(speller)
}

async fn search_page(state: &AppState, filters: &SearchFilters) -> CmdResult<SearchPage> {
    let show_adult = state.settings().show_adult;
    let dubbed = if filters.dubbed_only { Some(dub_ids(state).await) } else { None };
    let watched = watched_ids(state, filters.hide_watched.as_deref());
    // The id lists change as the viewer watches; their sizes keep a cached page from outliving them.
    // `-v2`: v1 holds the empty pages AniList gave while `isAdult: null` was sent, which would
    // otherwise come back as the stale answer whenever AniList refuses.
    let key = format!(
        "search-v2:{}:{show_adult}:{}:{}",
        serde_json::to_string(filters).unwrap_or_default(),
        dubbed.as_ref().map_or(0, Vec::len),
        watched.len()
    );
    let page = cached(state, catalogue::NET_SEARCH, &key, 15 * 60, state.anilist.search(filters, show_adult, dubbed.as_deref(), &watched)).await?;
    // An empty title search isn't kept: when it was AniList having a bad moment rather than a
    // title that doesn't exist, the cache would repeat it for 15 minutes, and after that hand it
    // back as the stale answer whenever AniList failed.
    if page.results.is_empty() && filters.query.as_deref().is_some_and(|q| !q.trim().is_empty()) {
        let _ = state.store.cache_delete(&key);
    }
    Ok(page)
}

/// What Browse's "Hide watched" leaves out (Android's `WatchedFilter`). `completed`: finished on
/// the list. `started`: also anything in progress, paused or dropped, and anything with progress on
/// this computer. Plan to watch is never hidden: it's a list of things not watched.
fn watched_ids(state: &AppState, mode: Option<&str>) -> Vec<i64> {
    let statuses: &[&str] = match mode {
        Some("completed") => &["completed"],
        Some("started") => &["completed", "watching", "paused", "dropped"],
        _ => return Vec::new(),
    };
    let mut ids: Vec<i64> = state
        .store
        .library()
        .unwrap_or_default()
        .into_iter()
        .filter(|e| statuses.contains(&e.status.as_str()))
        .map(|e| e.anime.id)
        .collect();
    if mode == Some("started") {
        ids.extend(state.store.progress_titles().unwrap_or_default());
    }
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// AniList's tags for Browse, kept a week; adult ones only when adult titles are shown.
#[tauri::command]
pub async fn anime_tags(state: State<'_, AppState>) -> CmdResult<Vec<anilili_core::anilist::Tag>> {
    let mut tags = cached(&state, catalogue::NET_SEARCH, "anilist-tags", 7 * 24 * 3600, state.anilist.tags()).await?;
    if !state.settings().show_adult {
        tags.retain(|t| !t.is_adult);
    }
    Ok(tags)
}

#[tauri::command]
pub async fn anime_detail(state: State<'_, AppState>, id: i64) -> CmdResult<AnimeDetail> {
    let record = index_detail(&state, id).await;
    let mut detail = match cached(&state, catalogue::NET_TITLE, &format!("detail:{id}"), 6 * 60 * 60, state.anilist.detail(id)).await {
        Ok(detail) => detail,
        // AniList refusing with nothing cached: the index's copy of AniList's record, thinner (no
        // characters) but a title page rather than an error.
        Err(e) => match record.as_ref().and_then(|r| r.anilist.as_ref()).and_then(anilili_core::anilist::parse_detail) {
            Some(detail) => {
                tracing::info!(id, "title page from the media index (AniList unavailable)");
                detail
            }
            None => return Err(e),
        },
    };
    detail.logo = series_logo(&state, id, record.as_ref()).await;
    if let Some(record) = &record
        && detail.card.banner.is_none()
    {
        detail.card.banner = record.images.banner.clone().filter(|b| !b.is_empty());
    }
    if detail.card.banner.is_none() {
        detail.card.banner = backdrops(&state).await.and_then(|b| b.get(&id).cloned());
    }
    let _ = state.store.remember(&detail.card);
    Ok(detail)
}

async fn index_detail(state: &AppState, id: i64) -> Option<anilili_core::media_index::IndexDetail> {
    index_cached(state, &format!("index-detail:{id}"), 24 * 3600, move |i| async move { i.detail(id).await }).await.flatten()
}

fn logo_of(record: &anilili_core::media_index::IndexDetail) -> Option<String> {
    record.images.logo.clone().filter(|l| !l.trim().is_empty())
}

/// The series' typography for a title (Android's `seriesLogo`): the title's own logo from the
/// media index, else the first one another season of the chain has (`siblingSeasonLogo`), so a
/// sequel the index drew no logo for still shows the series' name as art. `None` sets the title
/// in type instead; never an error.
async fn series_logo(state: &AppState, id: i64, record: Option<&anilili_core::media_index::IndexDetail>) -> Option<String> {
    let record = match record {
        Some(record) => record.clone(),
        None => index_detail(state, id).await?,
    };
    if let Some(logo) = logo_of(&record) {
        return Some(logo);
    }
    // Nearest seasons first: the chain is in airing order, so start beside this one.
    let at = record.series.iter().position(|s| *s == id).unwrap_or(0);
    let mut siblings: Vec<i64> = record.series.iter().copied().filter(|s| *s != id).collect();
    siblings.sort_by_key(|s| record.series.iter().position(|x| x == s).unwrap_or(0).abs_diff(at));
    for sibling in siblings.into_iter().take(6) {
        if let Some(logo) = index_detail(state, sibling).await.as_ref().and_then(logo_of) {
            return Some(logo);
        }
    }
    None
}

/// A title's logo, for places that only have its card (the Home hero).
#[tauri::command]
pub async fn anime_logo(state: State<'_, AppState>, id: i64) -> CmdResult<Option<String>> {
    Ok(series_logo(&state, id, None).await)
}

/// The title's whole franchise as a watch guide (seasons, movies, OVAs, specials, other), for the
/// title page's tabs. The season chain comes from the media index when it has one; cached a day.
#[tauri::command]
pub async fn watch_order(state: State<'_, AppState>, id: i64) -> CmdResult<Vec<anilili_core::franchise::WatchOrderEntry>> {
    let hide_adult = !state.settings().show_adult;
    let key = format!("watch-order:{id}:{hide_adult}");
    if let Some(hit) = state.store.cache_get(&key, 24 * 3600) {
        return Ok(hit);
    }
    let record = index_cached(&state, &format!("index-detail:{id}"), 24 * 3600, move |i| async move { i.detail(id).await }).await.flatten();
    let order = anilili_core::franchise::watch_order(&state.anilist, record.map(|r| r.series), id, hide_adult)
        .await
        .map_err(crate::app::as_entry(catalogue::NET_TITLE))?;
    for entry in &order {
        let _ = state.store.remember(&entry.anime);
    }
    let _ = state.store.cache_put(&key, &order);
    Ok(order)
}

/// Wide TMDB backdrops per AniList id, for titles AniList has no banner for.
async fn backdrops(state: &AppState) -> Option<std::collections::HashMap<i64, String>> {
    index_cached(state, "index-backdrops", 12 * 3600, move |i| async move { i.backdrops().await }).await
}

#[tauri::command]
pub async fn schedule(state: State<'_, AppState>, from: i64, to: i64) -> CmdResult<Vec<AiringEntry>> {
    let show_adult = state.settings().show_adult;
    // The media index publishes AniList's schedule per month: one request, no rate limit. AniList
    // itself when the index has nothing for the range.
    let index_key = format!("index-schedule:{}:{}:{show_adult}", from / 3600, to / 3600);
    if let Some(entries) = index_cached(&state, &index_key, 30 * 60, move |i| async move { i.schedule(from, to, show_adult).await }).await
        && !entries.is_empty()
    {
        return Ok(entries);
    }
    let key = format!("schedule:{}:{}:{show_adult}", from / 3600, to / 3600);
    cached(&state, catalogue::NET_SCHEDULE, &key, 30 * 60, state.anilist.schedule(from, to, show_adult)).await
}

async fn miruro_title(state: &AppState, id: i64) -> CmdResult<anilili_core::miruro::Title> {
    if let Some(title) = state.titles.lock().unwrap().get(&id).cloned() {
        return Ok(title);
    }
    let title = state
        .miruro
        .title(id)
        .await?
        .ok_or_else(|| crate::app::fail(catalogue::SRC_NONE_FOR_TITLE, format!("Miruro has no title for AniList {id}")))?;
    state.titles.lock().unwrap().insert(id, title.clone());
    Ok(title)
}

#[tauri::command]
pub async fn episodes(state: State<'_, AppState>, id: i64) -> CmdResult<EpisodeList> {
    let mut list = live_episodes(&state, id).await?;
    enrich_episodes(&state, id, &mut list).await;
    let (status, next) = airing_of(&state, id).await;
    schedule_episodes(&mut list.episodes, status.as_deref(), next.as_ref(), now());
    Ok(list)
}

/// AniList's status and next episode for a title, from the record its page caches (fetched when
/// it isn't): `(None, None)` when AniList can't be asked.
pub(crate) async fn airing_of(state: &AppState, id: i64) -> (Option<String>, Option<NextAiring>) {
    match cached::<AnimeDetail, _>(state, catalogue::NET_TITLE, &format!("detail:{id}"), 6 * 60 * 60, state.anilist.detail(id)).await {
        Ok(detail) => (detail.card.status, detail.card.next_airing),
        Err(_) => (None, None),
    }
}

/// How long after it airs an episode stays locked: the servers need that long to carry it, and a
/// card that opens onto "no servers" reads as the app being broken (Android's
/// `EPISODE_WATCHABLE_AFTER_SEC`).
pub const WATCHABLE_AFTER_SECS: i64 = 2 * 60 * 60;

/// The next episode to come out, and while it is still locked, when it airs. AniList moves its
/// next episode on the moment one airs, but a record cached before then still names it: it stays
/// locked for `WATCHABLE_AFTER_SECS`, then counts as out, and the one after it is next, time unknown.
fn next_out(next: Option<&NextAiring>, now: i64) -> (Option<f64>, Option<i64>) {
    match next {
        Some(next) if now < next.airing_at + WATCHABLE_AFTER_SECS => (Some(next.episode as f64), Some(next.airing_at)),
        Some(next) => (Some(next.episode as f64 + 1.0), None),
        None => (None, None),
    }
}

/// Whether an episode can't be played yet, by AniList's schedule alone (`watch` asks before
/// resolving): the next episode, while locked, and anything after it.
pub(crate) fn not_out_yet(number: f64, status: Option<&str>, next: Option<&NextAiring>, now: i64) -> bool {
    let (next, _) = next_out(next, now);
    status != Some("FINISHED") && next.is_some_and(|next| number >= next)
}

/// The list as it stands now, Android's `episodeIsUnaired`. Miruro lists a season's coming
/// episodes ahead of time, dated (Overgeared's 4 a week early, 2026-10-11), and asking it to play
/// one answers 502. The next episode keeps its slot, locked with its airing time for a countdown,
/// and is added there when the list doesn't have it yet; those after it go. Without AniList's
/// word, a dated episode is out once its day has come in Japan, where these broadcasts are dated,
/// two hours in.
fn schedule_episodes(episodes: &mut Vec<Episode>, status: Option<&str>, next: Option<&NextAiring>, now: i64) {
    let finished = status == Some("FINISHED");
    let (next_number, locked_until) = next_out(next, now);
    let (year, month, day) = civil_date(now - WATCHABLE_AFTER_SECS + 9 * 60 * 60);
    let today = format!("{year:04}-{month:02}-{day:02}");
    episodes.retain_mut(|e| {
        if let (Some(number), Some(airs_at)) = (next_number, locked_until)
            && e.number == number
        {
            e.airs_at = Some(airs_at);
            return true;
        }
        if finished || next_number.is_some_and(|number| e.number < number) {
            return true;
        }
        match e.aired_on.as_deref().and_then(|date| date.get(..10)) {
            Some(date) => date <= today.as_str(),
            None => next_number.is_none(),
        }
    });
    if let (Some(number), Some(airs_at)) = (next_number, locked_until)
        && !episodes.iter().any(|e| e.number == number)
        && episodes.iter().all(|e| e.number < number)
        && (number == 1.0 || episodes.iter().any(|e| e.number == number - 1.0))
    {
        episodes.push(Episode {
            number,
            title: None,
            synopsis: None,
            thumbnail: None,
            aired_on: None,
            duration_seconds: None,
            filler: false,
            skip: None,
            airs_at: Some(airs_at),
        });
    }
}

/// Miruro's list (cached briefly), else a stale copy, else, for a title Miruro doesn't know but
/// providers might: the media index's list, or the episode count AniList gives.
async fn live_episodes(state: &AppState, id: i64) -> CmdResult<EpisodeList> {
    let key = format!("episodes:{id}");
    // New episodes appear on the catalogue within minutes of airing; keep this short.
    if let Some(hit) = state.store.cache_get::<EpisodeList>(&key, 20 * 60).filter(|list| !list.episodes.is_empty()) {
        return Ok(hit);
    }
    let fetched = async {
        let title = miruro_title(state, id).await?;
        Ok::<_, crate::app::CommandError>(state.miruro.episodes(&title).await?)
    }
    .await;
    let e = match fetched {
        Ok(list) if !list.episodes.is_empty() => {
            let _ = state.store.cache_put(&key, &list);
            return Ok(list);
        }
        Ok(_) => crate::app::fail(catalogue::SRC_NONE_FOR_TITLE, format!("Miruro has no episodes for AniList {id}")),
        Err(e) => e,
    };
    if let Some(stale) = state.store.cache_get_stale::<EpisodeList>(&key).filter(|list| !list.episodes.is_empty()) {
        return Ok(stale);
    }
    if state.plugins.read().unwrap().list().is_empty() {
        return Err(e);
    }
    let index = index_cached(state, &format!("index-episodes:{id}"), 6 * 3600, move |i| async move { i.episodes(id).await }).await.unwrap_or_default();
    if !index.is_empty() {
        return Ok(EpisodeList { episodes: media_index::episodes_from_index(&index), sub_count: None, dub_count: None });
    }
    let known = state.store.cache_get_stale::<AnimeDetail>(&format!("detail:{id}")).and_then(|d| {
        fallback_episode_count(d.card.status.as_deref(), d.card.episodes, d.card.next_airing.map(|a| a.episode))
    });
    match known {
        Some(count) if count > 0 => Ok(EpisodeList {
            episodes: (1..=count)
                .map(|n| anilili_core::model::Episode {
                    number: n as f64,
                    title: None,
                    synopsis: None,
                    thumbnail: None,
                    aired_on: None,
                    duration_seconds: None,
                    filler: false,
                    skip: None,
                    airs_at: None,
                })
                .collect(),
            sub_count: None,
            dub_count: None,
        }),
        _ => Err(e),
    }
}

// AniList's episode total includes future episodes of an airing season.
fn fallback_episode_count(status: Option<&str>, total: Option<i32>, next: Option<i32>) -> Option<i32> {
    match status {
        Some("NOT_YET_RELEASED") => None,
        Some("RELEASING") => next.map(|episode| (episode - 1).max(0)).map(|aired| total.map_or(aired, |count| aired.min(count))),
        _ => total,
    }
}

#[cfg(test)]
mod episode_fallback_tests {
    use super::fallback_episode_count;

    #[test]
    fn airing_season_uses_aired_count_not_planned_total() {
        assert_eq!(fallback_episode_count(Some("RELEASING"), Some(59), Some(5)), Some(4));
        assert_eq!(fallback_episode_count(Some("RELEASING"), None, Some(5)), Some(4));
        assert_eq!(fallback_episode_count(Some("RELEASING"), Some(59), None), None);
        assert_eq!(fallback_episode_count(Some("NOT_YET_RELEASED"), Some(12), Some(1)), None);
        assert_eq!(fallback_episode_count(Some("FINISHED"), Some(12), None), Some(12));
    }
}

#[cfg(test)]
mod schedule_tests {
    use anilili_core::model::{Episode, NextAiring};

    use super::{WATCHABLE_AFTER_SECS, civil_date, not_out_yet, schedule_episodes};

    fn episode(number: f64, aired_on: Option<&str>) -> Episode {
        Episode {
            number,
            title: None,
            synopsis: None,
            thumbnail: None,
            aired_on: aired_on.map(str::to_owned),
            duration_seconds: None,
            filler: false,
            skip: None,
            airs_at: None,
        }
    }

    /// Overgeared on 2026-10-11, 18:46 UTC: 3 aired four hours before, Miruro already lists 4 and 5.
    const NOW: i64 = 1_791_744_402;
    const EP4_AIRS: i64 = 1_792_333_800;

    fn overgeared() -> Vec<Episode> {
        [(1.0, "2026-09-27"), (2.0, "2026-10-04"), (3.0, "2026-10-11"), (4.0, "2026-10-18"), (5.0, "2026-10-25")]
            .into_iter()
            .map(|(n, d)| episode(n, Some(d)))
            .collect()
    }

    fn numbers(episodes: &[Episode]) -> Vec<(f64, Option<i64>)> {
        episodes.iter().map(|e| (e.number, e.airs_at)).collect()
    }

    #[test]
    fn the_next_episode_waits_locked_in_its_slot_and_the_rest_go() {
        let mut list = overgeared();
        let next = NextAiring { episode: 4, airing_at: EP4_AIRS };
        schedule_episodes(&mut list, Some("RELEASING"), Some(&next), NOW);
        assert_eq!(numbers(&list), vec![(1.0, None), (2.0, None), (3.0, None), (4.0, Some(EP4_AIRS))]);
        assert!(not_out_yet(4.0, Some("RELEASING"), Some(&next), NOW));
        assert!(!not_out_yet(3.0, Some("RELEASING"), Some(&next), NOW));
    }

    #[test]
    fn a_next_episode_the_list_lacks_is_added_to_count_down() {
        let mut list = overgeared();
        list.truncate(3);
        schedule_episodes(&mut list, Some("RELEASING"), Some(&NextAiring { episode: 4, airing_at: EP4_AIRS }), NOW);
        assert_eq!(list.last().map(|e| (e.number, e.airs_at)), Some((4.0, Some(EP4_AIRS))));
    }

    #[test]
    fn an_aired_episode_stays_locked_while_the_servers_post_it_then_opens() {
        let next = NextAiring { episode: 4, airing_at: EP4_AIRS };
        let mut list = overgeared();
        schedule_episodes(&mut list, Some("RELEASING"), Some(&next), EP4_AIRS + WATCHABLE_AFTER_SECS - 60);
        assert_eq!(list.last().map(|e| (e.number, e.airs_at)), Some((4.0, Some(EP4_AIRS))));
        // A record cached before 4 aired, read after: 4 is out, 5 is next and not yet dated in.
        let mut list = overgeared();
        schedule_episodes(&mut list, Some("RELEASING"), Some(&next), EP4_AIRS + WATCHABLE_AFTER_SECS + 60);
        assert_eq!(numbers(&list), vec![(1.0, None), (2.0, None), (3.0, None), (4.0, None)]);
    }

    #[test]
    fn without_anilist_a_date_counts_from_its_day_in_japan() {
        // 3 is dated the 11th: out from 02:00 that day in Japan, 17:00 UTC on the 10th.
        let jst_day_2am = 1_791_651_600; // 2026-10-10 17:00 UTC
        let mut list = overgeared();
        schedule_episodes(&mut list, None, None, jst_day_2am);
        assert_eq!(list.len(), 3);
        let mut list = overgeared();
        schedule_episodes(&mut list, None, None, jst_day_2am - 1);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn a_finished_season_keeps_everything() {
        let mut list = overgeared();
        schedule_episodes(&mut list, Some("FINISHED"), None, NOW);
        assert_eq!(list.len(), 5);
    }

    #[test]
    fn calendar_dates_from_unix() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(1_791_744_402), (2026, 10, 11));
        assert_eq!(civil_date(1_709_164_800), (2024, 2, 29));
    }
}

/// What the media index adds to an episode list: stills, synopses and real titles, filler
/// episodes, skip times Miruro lacks (AniSkip's mirror), and the dub count when Miruro has none.
async fn enrich_episodes(state: &AppState, id: i64, list: &mut EpisodeList) {
    let (episodes_key, skip_key) = (format!("index-episodes:{id}"), format!("index-skip:{id}"));
    let (episodes, fillers, skip, dubs) = tokio::join!(
        index_cached(state, &episodes_key, 6 * 3600, move |i| async move { i.episodes(id).await }),
        index_cached(state, "index-fillers", 12 * 3600, move |i| async move { i.fillers().await }),
        index_cached(state, &skip_key, 24 * 3600, move |i| async move { i.skip(id).await }),
        index_cached(state, "index-dub-episodes", 12 * 3600, move |i| async move { i.dub_episodes().await }),
    );
    if let Some(rows) = episodes {
        media_index::merge_episodes(&mut list.episodes, &rows);
    }
    if let Some(filler) = fillers.as_ref().and_then(|f| f.get(&id)) {
        for episode in list.episodes.iter_mut().filter(|e| filler.contains(&e.number)) {
            episode.filler = true;
        }
    }
    // The mirror is keyed by AniList id but built from MAL's; skip it when the two disagree.
    let mal = state.store.cache_get_stale::<AnimeDetail>(&format!("detail:{id}")).and_then(|d| d.mal_id);
    if let Some(file) = skip.flatten().filter(|f| mal.is_none() || f.mal.is_none() || f.mal == mal) {
        for episode in list.episodes.iter_mut().filter(|e| e.skip.is_none()) {
            episode.skip = media_index::skip_for(&file, episode.number, episode.duration_seconds);
        }
    }
    if list.dub_count.is_none() {
        list.dub_count = dubs.and_then(|d| d.get(&id).copied());
    }
}

/// A media-index file through the cache: fresh, else fetched, else stale, else `None`. The index
/// only ever adds to a page, so its failures are logged, never shown.
/// Titles with a known English dub, for the cards' DUB tag and Browse's "Dubbed only": the media
/// index's dub crawl plus every title it has a dubbed episode for, as Android's `DubIndex` reads
/// them. Refreshed daily; empty when the index can't be reached.
pub async fn dub_ids(state: &AppState) -> Vec<i64> {
    let mut ids = index_cached(state, "index-dub-ids", 24 * 3600, move |i| async move { i.dub_index().await }).await.unwrap_or_default();
    let episodes = index_cached(state, "index-dub-episodes", 12 * 3600, move |i| async move { i.dub_episodes().await }).await.unwrap_or_default();
    ids.extend(episodes.into_keys());
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[tauri::command]
pub async fn dub_titles(state: State<'_, AppState>) -> CmdResult<Vec<i64>> {
    Ok(dub_ids(&state).await)
}

/// How long a page waits for the media index before showing what it has. The index only adds to a
/// page, and jsDelivr sometimes takes its full timeout; an answer that comes later still lands in
/// the cache for the next visit.
const INDEX_WAIT: std::time::Duration = std::time::Duration::from_secs(3);

async fn index_cached<T, F, Fut>(state: &AppState, key: &str, max_age: i64, fetch: F) -> Option<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
    F: FnOnce(media_index::MediaIndex) -> Fut,
    Fut: std::future::Future<Output = anilili_core::Result<T>> + Send + 'static,
{
    if let Some(hit) = state.store.cache_get::<T>(key, max_age) {
        return Some(hit);
    }
    let (store, owned_key, fetch) = (state.store.clone(), key.to_owned(), fetch(state.media_index.clone()));
    let task = tauri::async_runtime::spawn(async move {
        match fetch.await {
            Ok(value) => {
                let _ = store.cache_put(&owned_key, &value);
                Some(value)
            }
            Err(e) => {
                tracing::debug!(key = %owned_key, error = %e, "media index unavailable");
                None
            }
        }
    });
    match tokio::time::timeout(INDEX_WAIT, task).await {
        Ok(Ok(Some(value))) => Some(value),
        Ok(_) => state.store.cache_get_stale::<T>(key),
        Err(_) => {
            tracing::debug!(%key, "media index slow; showing the page without it for now");
            state.store.cache_get_stale::<T>(key)
        }
    }
}

#[tauri::command]
pub async fn sources(state: State<'_, AppState>, id: i64, episode: f64, refresh: Option<bool>) -> CmdResult<Sources> {
    resolve_sources(&state, id, episode, refresh.unwrap_or(false)).await
}

/// Whether DNS over HTTPS (Automatic) has switched to a secure resolver this session, because the
/// network's DNS was seen blocking a source.
#[tauri::command]
pub fn dns_engaged() -> bool {
    anilili_core::dns::engaged()
}

pub async fn resolve_sources(state: &AppState, id: i64, episode: f64, refresh: bool) -> CmdResult<Sources> {
    // An episode that hasn't aired has no streams, and the catalogue answers asking for one with
    // a server error (a link to it, or a list that still showed it, used to end there).
    let (status, next) = airing_of(state, id).await;
    if not_out_yet(episode, status.as_deref(), next.as_ref(), now()) {
        return Err(crate::app::fail(catalogue::SRC_NOT_AIRED, format!("anime {id} episode {episode}")));
    }
    let key = (id, episode.to_bits());
    if !refresh
        && let Some((at, sources)) = state.sources.lock().unwrap().get(&key)
        && at.elapsed() < SOURCES_TTL
    {
        return Ok(sources.clone());
    }
    let skip = state
        .store
        .cache_get_stale::<EpisodeList>(&format!("episodes:{id}"))
        .and_then(|list| list.episodes.into_iter().find(|e| e.number == episode))
        .and_then(|e| e.skip);
    // Miruro and the WebAssembly providers are asked at the same time; either alone is enough.
    let miruro = async {
        let title = miruro_title(state, id).await?;
        Ok::<_, crate::app::CommandError>(state.miruro.sources(&title.id, episode, skip).await?)
    };
    let (miruro, extra) = race_with_grace(miruro, plugin_servers(state, id, episode)).await;
    let sources = match miruro {
        Ok(mut sources) => {
            // After the native Miruro servers, before the embed-only ones.
            let at = sources.servers.iter().position(|s| !s.is_native()).unwrap_or(sources.servers.len());
            sources.servers.splice(at..at, extra);
            sources
        }
        Err(_) if !extra.is_empty() => Sources { servers: extra, skip },
        Err(e) => return Err(e),
    };
    state.sources.lock().unwrap().insert(key, (Instant::now(), sources.clone()));
    Ok(sources)
}

/// Runs Miruro and the providers together. Once Miruro has answered, providers get a short grace
/// period and are then dropped for this request, so a slow provider never delays playback; when
/// Miruro fails, providers get longer, since they are all there is.
async fn race_with_grace<M, P>(miruro: M, plugins: P) -> (CmdResult<Sources>, Vec<anilili_core::model::Server>)
where
    M: std::future::Future<Output = CmdResult<Sources>>,
    P: std::future::Future<Output = Vec<anilili_core::model::Server>>,
{
    use std::time::Duration;
    tokio::pin!(miruro, plugins);
    let mut extra = None;
    let result = tokio::select! {
        result = &mut miruro => result,
        servers = &mut plugins => {
            extra = Some(servers);
            miruro.await
        }
    };
    let extra = match extra {
        Some(servers) => servers,
        None => {
            let grace = if result.is_ok() { Duration::from_secs(4) } else { Duration::from_secs(25) };
            tokio::time::timeout(grace, plugins).await.unwrap_or_default()
        }
    };
    (result, extra)
}

/// Servers from the WebAssembly providers, sub and dub, for one episode.
///
/// The work runs as its own task: if this request stops waiting (see [`race_with_grace`]), the
/// providers still finish, which fills their caches and merges their servers into the cached list
/// for the next request of this episode.
async fn plugin_servers(state: &AppState, id: i64, episode: f64) -> Vec<anilili_core::model::Server> {
    use anilili_core::model::Audio;
    use anilili_core::plugin::TitleQuery;
    let plugins = state.plugins.read().unwrap().clone();
    if plugins.list().is_empty() {
        return vec![];
    }
    let cache = std::sync::Arc::clone(&state.sources);
    let detail = state.store.cache_get_stale::<AnimeDetail>(&format!("detail:{id}"));
    let mut titles = Vec::new();
    if let Some(d) = &detail {
        titles.push(d.card.title.clone());
        titles.extend(d.card.title_romaji.clone());
        titles.extend(d.synonyms.iter().cloned());
    }
    let query =
        |audio| TitleQuery { anilist_id: id, mal_id: detail.as_ref().and_then(|d| d.mal_id), titles: titles.clone(), episode, audio };
    let (sub_query, dub_query) = (query(Audio::Sub), query(Audio::Dub));
    let task = tauri::async_runtime::spawn(async move {
        // Sub and dub together, so a provider that must take them in turn can (`ONE_AT_A_TIME`).
        let servers = plugins.servers_for(&[sub_query, dub_query]).await;
        // A late answer still helps: merge it into this episode's cached server list.
        if let Some((_, sources)) = cache.lock().unwrap().get_mut(&(id, episode.to_bits())) {
            for server in &servers {
                if !sources.servers.iter().any(|s| s.id == server.id) {
                    let at = sources.servers.iter().position(|s| !s.is_native()).unwrap_or(sources.servers.len());
                    sources.servers.insert(at, server.clone());
                }
            }
        }
        servers
    });
    task.await.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_month_from_unix() {
        assert_eq!(year_month(0), (1970, 1));
        assert_eq!(year_month(1_759_536_000), (2025, 10)); // 2025-10-04
        assert_eq!(season_of(10), "FALL");
        assert_eq!(season_of(1), "WINTER");
    }
}
