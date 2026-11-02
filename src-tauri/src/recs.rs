//! Recommendations and the dice, as Android makes them (`data/recs/Recommendations.kt`,
//! `ui/random/`). `anilili_core::recs` does the arithmetic; this keeps the published graph on disk
//! (refreshed daily), gathers what the viewer has watched, and turns ids into cards.
//!
//! Taste comes from: the AniList or MyAnimeList list when signed in; this computer's library
//! (for titles not on that list, so a viewer who never signs in still gets recommendations); what
//! was played here; and the dice's swipes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anilili_core::anilist::{Pick, RANDOM_POOL_PAGES};
use anilili_core::catalogue as c;
use anilili_core::model::AnimeCard;
use anilili_core::recs::{self, ListRating, NeighbourGraph, PlayedTitle};
use rand::seq::SliceRandom;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::app::{AppState, CmdResult, as_entry};

/// The build writes the graph daily; a day-old one recommends the same things.
const REFRESH_AFTER: Duration = Duration::from_secs(24 * 3600);
/// How often a failed or unchanged check may be retried within one run.
const CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
const FEEDBACK_KEY: &str = "taste.dice";
/// Swipes remembered; the oldest go first.
const MAX_FEEDBACK: usize = 300;
const DICE_POOL: usize = 60;

#[derive(Default)]
struct Slot {
    graph: Option<Arc<NeighbourGraph>>,
    checked: Option<Instant>,
}

static GRAPH: tokio::sync::Mutex<Slot> = tokio::sync::Mutex::const_new(Slot { graph: None, checked: None });

/// The graph from memory, then disk, then the media index. `None` only when none has one (a
/// fresh install offline).
async fn graph<R: Runtime>(app: &AppHandle<R>) -> Option<Arc<NeighbourGraph>> {
    let mut slot = GRAPH.lock().await;
    let file = app.path().app_data_dir().ok()?.join(recs::PATH);
    if slot.graph.is_none()
        && let Ok(bytes) = std::fs::read(&file)
    {
        match tokio::task::spawn_blocking(move || NeighbourGraph::from_gzip(&bytes)).await {
            Ok(Ok(graph)) => slot.graph = Some(Arc::new(graph)),
            Ok(Err(e)) => tracing::warn!(error = %e, "neighbour graph on disk unreadable"),
            Err(e) => tracing::warn!(error = %e, "neighbour graph on disk unreadable"),
        }
    }
    let age = std::fs::metadata(&file).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    let stale = slot.graph.is_none() || age.is_none_or(|a| a > REFRESH_AFTER);
    if stale && slot.checked.is_none_or(|t| t.elapsed() > CHECK_INTERVAL) {
        slot.checked = Some(Instant::now());
        match download(app, file).await {
            Ok(Some(graph)) => {
                tracing::info!(titles = graph.len(), edges = graph.edge_count(), generated = ?graph.generated, "neighbour graph updated");
                slot.graph = Some(Arc::new(graph));
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "neighbour graph download failed"),
        }
    }
    slot.graph.clone()
}

/// Parsed before it replaces anything: a body that won't parse must not overwrite one that does.
async fn download<R: Runtime>(app: &AppHandle<R>, file: PathBuf) -> Result<Option<NeighbourGraph>, String> {
    let state = app.state::<AppState>();
    let url = format!("{}/{}", anilili_core::media_index::BASE, recs::PATH);
    let bytes = match state.http.get_bytes("media index", &url, &[("Cache-Control", "no-cache")], Duration::from_secs(60)).await {
        Ok((bytes, _)) => bytes,
        Err(anilili_core::Error::Http { status: 404, .. }) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let parsed = {
        let bytes = bytes.clone();
        tokio::task::spawn_blocking(move || NeighbourGraph::from_gzip(&bytes)).await.map_err(|e| e.to_string())??
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let partial = file.with_extension("part");
    std::fs::write(&partial, &bytes).and_then(|()| std::fs::rename(&partial, &file)).map_err(|e| e.to_string())?;
    Ok(Some(parsed))
}

/// What the viewer has watched, as the engine reads it, and every title worth never recommending.
struct Taste {
    list: Vec<ListRating>,
    played: Vec<PlayedTitle>,
    feedback: Vec<(i64, f64)>,
    known: HashSet<i64>,
}

async fn taste(state: &State<'_, AppState>) -> Taste {
    let mut list: Vec<ListRating> = Vec::new();
    let signed_in = state.anilist.has_token() || state.mal_access_token().await.is_some();
    if signed_in {
        match crate::library::account_list(state.clone()).await {
            Ok(entries) => list.extend(entries.into_iter().map(|e| ListRating {
                anilist_id: e.anime.id,
                status: e.status,
                score: e.score,
                updated_at: e.updated_at,
            })),
            // The rest still describes the viewer; the row is better than none.
            Err(e) => tracing::debug!(code = %e.code, "account list unavailable for recommendations"),
        }
    }
    let on_list: HashSet<i64> = list.iter().map(|e| e.anilist_id).collect();
    for entry in state.store.library().unwrap_or_default() {
        if on_list.contains(&entry.anime.id) {
            continue;
        }
        if let Some(status) = crate::library::to_anilist_status(&entry.status) {
            list.push(ListRating { anilist_id: entry.anime.id, status: status.to_owned(), score: 0.0, updated_at: entry.updated_at });
        }
    }
    let mut played: HashMap<i64, PlayedTitle> = HashMap::new();
    for row in state.store.all_progress().unwrap_or_default() {
        let title = played.entry(row.anime_id).or_insert(PlayedTitle { anilist_id: row.anime_id, episodes_watched: 0, updated_at: 0 });
        title.episodes_watched += u32::from(row.finished());
        title.updated_at = title.updated_at.max(row.updated_at);
    }
    let feedback: Vec<(i64, f64)> = state.store.setting(FEEDBACK_KEY).unwrap_or_default();
    let mut known: HashSet<i64> = list.iter().map(|e| e.anilist_id).collect();
    known.extend(played.keys());
    Taste { list, played: played.into_values().collect(), feedback, known }
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BecauseRow {
    pub seed: AnimeCard,
    pub items: Vec<AnimeCard>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HomeRecs {
    /// "For you": the whole taste, blended.
    pub for_you: Vec<AnimeCard>,
    /// "Because you watched X", for the latest title the viewer clearly liked.
    pub because: Option<BecauseRow>,
}

/// Home's two rows. Empty until there's something to go on (or no graph offline).
#[tauri::command]
pub async fn recs_home<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<HomeRecs> {
    let show_adult = state.settings().show_adult;
    let key = format!("recs-home:{show_adult}");
    if let Some(hit) = state.store.cache_get::<HomeRecs>(&key, 10 * 60) {
        return Ok(hit);
    }
    let Some(graph) = graph(&app).await else { return Ok(HomeRecs { for_you: vec![], because: None }) };
    let taste = taste(&state).await;
    let now = anilili_core::store::now();
    let signals = recs::signals(&taste.list, &taste.played, &taste.feedback, now);
    let for_you: Vec<i64> = recs::recommend(&graph, &signals, &taste.known, !show_adult, 30).iter().map(|r| r.anilist_id).collect();
    let seed = recs::featured_seed(&graph, &taste.list, &taste.played, now);
    let because: Vec<i64> = seed
        .map(|seed| recs::because(&graph, seed, &taste.known, !show_adult, 24).iter().map(|r| r.anilist_id).collect())
        .unwrap_or_default();

    let mut ids: Vec<i64> = for_you.clone();
    ids.extend(seed);
    ids.extend(&because);
    let cards: HashMap<i64, AnimeCard> = cards(&state, &ids, show_adult).await?.into_iter().map(|p| (p.card.id, p.card)).collect();
    let pick = |ids: &[i64]| ids.iter().filter_map(|id| cards.get(id).cloned()).collect::<Vec<_>>();
    let recs = HomeRecs {
        for_you: pick(&for_you),
        because: seed
            .and_then(|seed| cards.get(&seed).cloned())
            .map(|seed| BecauseRow { seed, items: pick(&because) })
            .filter(|row| !row.items.is_empty()),
    };
    let _ = state.store.cache_put(&key, &recs);
    Ok(recs)
}

/// One page of the dice's pool, shuffled, leaving out titles already started unless that would
/// leave nothing (a viewer who has seen every popular title still gets a pick).
#[tauri::command]
pub async fn dice_pool(state: State<'_, AppState>, page: u32) -> CmdResult<Vec<Pick>> {
    let page = page.clamp(1, RANDOM_POOL_PAGES);
    let show_adult = state.settings().show_adult;
    let key = format!("dice-pool:{page}:{show_adult}");
    let mut picks = match state.store.cache_get::<Vec<Pick>>(&key, 24 * 3600) {
        Some(hit) => hit,
        None => {
            let picks = state.anilist.random_pool(page, !show_adult).await.map_err(as_entry(c::NET_DICE))?;
            let _ = state.store.cache_put(&key, &picks);
            picks
        }
    };
    let started: HashSet<i64> = state.store.all_progress().unwrap_or_default().iter().map(|p| p.anime_id).collect();
    if picks.iter().any(|p| !started.contains(&p.card.id)) {
        picks.retain(|p| !started.contains(&p.card.id));
    }
    picks.shuffle(&mut rand::rng());
    Ok(picks)
}

/// A random handful of the viewer's recommendations, for the dice to mix in: taken from well down
/// the list rather than the top, so a roll still surprises.
#[tauri::command]
pub async fn dice_taste<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<Vec<Pick>> {
    let Some(graph) = graph(&app).await else { return Ok(vec![]) };
    let show_adult = state.settings().show_adult;
    let taste = taste(&state).await;
    let signals = recs::signals(&taste.list, &taste.played, &taste.feedback, anilili_core::store::now());
    let mut ids: Vec<i64> = recs::recommend(&graph, &signals, &taste.known, !show_adult, DICE_POOL).iter().map(|r| r.anilist_id).collect();
    ids.shuffle(&mut rand::rng());
    ids.truncate(12);
    cards(&state, &ids, show_adult).await
}

/// A dice card opened (swiped right) or skipped: a light signal for the taste model.
#[tauri::command]
pub fn dice_feedback(state: State<'_, AppState>, id: i64, opened: bool) -> CmdResult<()> {
    let mut feedback: Vec<(i64, f64)> = state.store.setting(FEEDBACK_KEY).unwrap_or_default();
    feedback.retain(|(i, _)| *i != id);
    feedback.push((id, if opened { recs::DICE_OPENED } else { recs::DICE_SKIPPED }));
    let excess = feedback.len().saturating_sub(MAX_FEEDBACK);
    feedback.drain(..excess);
    state.store.set_setting(FEEDBACK_KEY, &feedback)?;
    Ok(())
}

/// Cards for ids, in their order: from the store's card cache where it has the synopsis, else
/// AniList (one request per 50).
async fn cards(state: &State<'_, AppState>, ids: &[i64], show_adult: bool) -> CmdResult<Vec<Pick>> {
    if ids.is_empty() {
        return Ok(vec![]);
    }
    let mut picks = state.anilist.cards_by_ids(ids).await.map_err(as_entry(c::NET_RECS))?;
    picks.retain(|p| show_adult || !p.card.is_adult);
    for pick in &picks {
        let _ = state.store.remember(&pick.card);
    }
    Ok(picks)
}
