//! A title's whole franchise as a watch guide: seasons, then films, OVAs, specials and the rest, in
//! one running order. A port of Android's `buildWatchOrder` (ui/detail/WatchOrder.kt) and its series
//! walk (MiruroRepository `animeSeries`); change them together.
//!
//! The season chain comes from the media index (`series`, the generator's own walk), so every member
//! is fetched in one request; without it, the PREQUEL/SEQUEL relations are walked a hop at a time.
//! The rest of the franchise is read off the members' relation edges, which costs nothing more.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::Result;
use crate::anilist::{AniList, Member};
use crate::model::AnimeCard;

/// Bounds the walk when the index has no chain.
const MAX_SERIES_ENTRIES: usize = 16;
const MAX_SERIES_DEPTH: usize = 8;

/// Relations that are the same story told on, told beside, or told again. ADAPTATION and SOURCE
/// point at the manga, CHARACTER at a title that merely shares a face, OTHER at anything at all.
const FRANCHISE_RELATIONS: &[&str] =
    &["PREQUEL", "SEQUEL", "PARENT", "SIDE_STORY", "SUMMARY", "ALTERNATIVE", "SPIN_OFF", "COMPILATION", "CONTAINS"];
const SEASON_RELATIONS: &[&str] = &["PREQUEL", "SEQUEL"];
/// Formats a watch guide lists: not reading formats, and not MUSIC, or a running order drowns in
/// opening videos.
const FRANCHISE_FORMATS: &[&str] = &["TV", "TV_SHORT", "MOVIE", "SPECIAL", "OVA", "ONA"];
/// Episodic entries that count as numbered seasons.
const SEASON_FORMATS: &[&str] = &["TV", "TV_SHORT", "ONA"];

/// Which shelf an entry sits on, in the order the guide reads: the numbered run, then the films,
/// then the extras.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Shelf {
    Seasons,
    Movies,
    Ovas,
    Specials,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchOrderEntry {
    pub anime: AnimeCard,
    pub shelf: Shelf,
    /// Franchise-wide, in airing order: the same number whichever shelf is shown.
    pub position: usize,
    /// Franchise-wide, in the story's own order (PREQUEL/SEQUEL chains, ties by airing date) —
    /// a prequel made later still comes first. Equals `position` whenever the two agree.
    pub chronological: usize,
    /// 1-based place in the season chain; `None` off it, and for a one-entry chain (calling the
    /// anime itself "Season 1" would invent a series).
    pub season_number: Option<usize>,
    /// How the entry joins the series ("Side story", "Summary"), for the extras.
    pub relation: Option<String>,
    /// The title the page is about.
    pub is_current: bool,
}

/// The watch guide for `id`.
pub async fn watch_order(anilist: &AniList, index_series: Option<Vec<i64>>, id: i64, hide_adult: bool) -> Result<Vec<WatchOrderEntry>> {
    let series = match index_series.filter(|ids| ids.contains(&id)) {
        Some(ids) => anilist.members(&ids).await?,
        None => walk(anilist, id, hide_adult).await?,
    };
    let Some(info) = series.iter().find(|m| m.card.id == id).cloned() else { return Ok(Vec::new()) };
    Ok(build(&info, &series, hide_adult))
}

/// The PREQUEL/SEQUEL chain, a hop at a time (each member's relations decide the next hop).
async fn walk(anilist: &AniList, id: i64, hide_adult: bool) -> Result<Vec<Member>> {
    let mut found: HashMap<i64, Member> = HashMap::new();
    let mut expanded: HashSet<i64> = HashSet::new();
    let mut frontier = vec![id];
    for _ in 0..MAX_SERIES_DEPTH {
        let batch: Vec<i64> = frontier.iter().copied().filter(|id| expanded.insert(*id)).take(MAX_SERIES_ENTRIES).collect();
        if batch.is_empty() || found.len() >= MAX_SERIES_ENTRIES {
            break;
        }
        let mut next = Vec::new();
        for member in anilist.members(&batch).await? {
            for (kind, neighbour) in &member.relations {
                if !SEASON_RELATIONS.contains(&kind.as_str()) || (hide_adult && neighbour.card.is_adult) {
                    continue;
                }
                if !expanded.contains(&neighbour.card.id) && found.len() + next.len() < MAX_SERIES_ENTRIES {
                    next.push(neighbour.card.id);
                }
            }
            found.insert(member.card.id, member);
        }
        next.sort_unstable();
        next.dedup();
        frontier = next;
    }
    Ok(found.into_values().collect())
}

/// `buildWatchOrder`: the chain plus everything its members' franchise relations reach.
pub fn build(info: &Member, series: &[Member], hide_adult: bool) -> Vec<WatchOrderEntry> {
    let mut members: Vec<Member> = vec![info.clone()];
    for m in series {
        if m.card.id != info.card.id && !(hide_adult && m.card.is_adult) {
            members.push(m.clone());
        }
    }
    let chain: Vec<Member> = members.clone();
    let mut labels: HashMap<i64, String> = HashMap::new();
    for member in &chain {
        for (kind, node) in &member.relations {
            if !FRANCHISE_RELATIONS.contains(&kind.as_str()) || (hide_adult && node.card.is_adult) {
                continue;
            }
            let known = members.iter().any(|m| m.card.id == node.card.id);
            if !known && !node.card.format.as_deref().is_some_and(|f| FRANCHISE_FORMATS.contains(&f)) {
                continue;
            }
            if !known {
                members.push(node.clone());
            }
            if !SEASON_RELATIONS.contains(&kind.as_str()) {
                labels.entry(node.card.id).or_insert_with(|| pretty(kind));
            }
        }
    }
    members.sort_by_key(|m| m.airing_key());
    let story_at: HashMap<i64, usize> = story_order(&members)
        .unwrap_or_else(|| members.iter().map(|m| m.card.id).collect())
        .into_iter()
        .enumerate()
        .map(|(index, id)| (id, index + 1))
        .collect();
    // The seasons: the chain's episodic entries (the title itself always), in airing order.
    let chain_ids: HashSet<i64> = chain.iter().map(|m| m.card.id).collect();
    let seasons: Vec<i64> = members
        .iter()
        .filter(|m| {
            m.card.id == info.card.id
                || (chain_ids.contains(&m.card.id) && m.card.format.as_deref().is_some_and(|f| SEASON_FORMATS.contains(&f)))
        })
        .map(|m| m.card.id)
        .collect();
    let mut entries: Vec<WatchOrderEntry> = members
        .into_iter()
        .enumerate()
        .map(|(index, m)| {
            let season_index = seasons.iter().position(|id| *id == m.card.id);
            let shelf = match (season_index, m.card.format.as_deref()) {
                (Some(_), _) => Shelf::Seasons,
                (None, Some("MOVIE")) => Shelf::Movies,
                (None, Some("OVA")) => Shelf::Ovas,
                (None, Some("SPECIAL")) => Shelf::Specials,
                _ => Shelf::Other,
            };
            WatchOrderEntry {
                relation: labels.get(&m.card.id).cloned().filter(|_| shelf != Shelf::Seasons),
                season_number: season_index.filter(|_| seasons.len() > 1).map(|i| i + 1),
                is_current: m.card.id == info.card.id,
                position: index + 1,
                chronological: story_at[&m.card.id],
                shelf,
                anime: m.card,
            }
        })
        .collect();
    // Grouped for display, seasons in season order.
    entries.sort_by_key(|e| (e.shelf, e.season_number.unwrap_or(e.position)));
    entries
}

/// The franchise in the story's own order, or `None` when the relation data cannot hold one. AniList's
/// PREQUEL/SEQUEL edges say which of two titles happens first *in the story*, wherever they aired
/// (Fate/Zero is a 2011 prequel of a 2006 series); everything the edges say nothing about slots in by
/// airing date. A cycle in the edges (dirty data) answers `None`, and the caller falls back to airing
/// order for everyone rather than let the two guides disagree by half.
fn story_order(members: &[Member]) -> Option<Vec<i64>> {
    use std::collections::BinaryHeap;

    let ids: HashSet<i64> = members.iter().map(|m| m.card.id).collect();
    let mut after: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut before: HashMap<i64, Vec<i64>> = HashMap::new();
    for member in members {
        for (kind, node) in &member.relations {
            if !SEASON_RELATIONS.contains(&kind.as_str()) || !ids.contains(&node.card.id) {
                continue;
            }
            // "PREQUEL, X" on this title means X happened earlier; "SEQUEL, X" means later.
            let (earlier, later) = if kind == "PREQUEL" { (&node.card.id, &member.card.id) } else { (&member.card.id, &node.card.id) };
            after.entry(*later).or_default().push(*earlier);
            before.entry(*earlier).or_default().push(*later);
        }
    }
    // `members` is in airing order, so its index is the tiebreak: of the entries the story frees
    // up, the earliest-aired is placed first. The same relation seen from both sides ("SEQUEL, B"
    // on A, "PREQUEL, A" on B) counts twice on both sides, so it still cancels out.
    let place: HashMap<i64, usize> = members.iter().enumerate().map(|(index, m)| (m.card.id, index)).collect();
    let mut waiting: HashMap<i64, usize> = after.iter().map(|(id, earlier)| (*id, earlier.len())).collect();
    let mut ready: BinaryHeap<std::cmp::Reverse<usize>> = members
        .iter()
        .filter(|m| !waiting.contains_key(&m.card.id))
        .map(|m| std::cmp::Reverse(place[&m.card.id]))
        .collect();
    let mut out = Vec::with_capacity(members.len());
    while let Some(std::cmp::Reverse(index)) = ready.pop() {
        let id = members[index].card.id;
        out.push(id);
        for later in before.get(&id).into_iter().flatten() {
            if let Some(0) = waiting.get_mut(later).map(|n| {
                *n -= 1;
                *n
            }) {
                ready.push(std::cmp::Reverse(place[later]));
            }
        }
    }
    (out.len() == members.len()).then_some(out)
}

/// `SIDE_STORY` → "Side story".
fn pretty(kind: &str) -> String {
    let lower = kind.replace('_', " ").to_lowercase();
    let mut chars = lower.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(id: i64, format: &str, year: i32, relations: Vec<(&str, Member)>) -> Member {
        Member {
            card: AnimeCard {
                id,
                title: format!("#{id}"),
                title_romaji: None,
                cover: None,
                color: None,
                banner: None,
                format: Some(format.into()),
                status: None,
                episodes: None,
                score: None,
                year: Some(year),
                season: None,
                genres: vec![],
                next_airing: None,
                is_adult: false,
            },
            start: (year, 1, 1),
            relations: relations.into_iter().map(|(k, m)| (k.to_owned(), m)).collect(),
        }
    }

    #[test]
    fn shelves_the_franchise_in_running_order() {
        let movie = member(9, "MOVIE", 2022, vec![]);
        let recap = member(8, "SPECIAL", 2021, vec![]);
        let manga = member(7, "MANGA", 2015, vec![]);
        let s1 = member(1, "TV", 2020, vec![("SEQUEL", member(2, "TV", 2021, vec![])), ("ADAPTATION", manga)]);
        let s2 = member(2, "TV", 2021, vec![("PREQUEL", member(1, "TV", 2020, vec![])), ("SIDE_STORY", movie), ("SUMMARY", recap)]);
        let order = build(&s2, &[s1, s2.clone()], true);
        let shelves: Vec<(i64, Shelf, Option<usize>)> = order.iter().map(|e| (e.anime.id, e.shelf, e.season_number)).collect();
        assert_eq!(
            shelves,
            [(1, Shelf::Seasons, Some(1)), (2, Shelf::Seasons, Some(2)), (9, Shelf::Movies, None), (8, Shelf::Specials, None)]
        );
        assert!(order[1].is_current);
        assert_eq!(order[2].relation.as_deref(), Some("Side story"));
        assert!(order.iter().all(|e| e.anime.id != 7), "the manga is not part of a watch order");
        // The chain and the calendar agree here, so the story order is the airing order.
        assert!(order.iter().all(|e| e.chronological == e.position));
    }

    #[test]
    fn a_prequel_that_aired_later_leads_the_story() {
        // Fate/Zero and its like: the 2011 series is, in the story, a prequel of the 2006 one
        // (AniList says so from the later title's side, as a SEQUEL edge pointing back).
        let movie = member(9, "MOVIE", 2009, vec![]);
        let sn = member(1, "TV", 2006, vec![("SIDE_STORY", movie)]);
        let zero = member(2, "TV", 2011, vec![("SEQUEL", sn.clone())]);
        let order = build(&zero, &[sn, zero.clone()], true);
        let at = |id: i64| order.iter().find(|e| e.anime.id == id).unwrap();
        // Airing: the 2006 series, the 2009 film, the 2011 prequel.
        assert_eq!((at(1).position, at(9).position, at(2).position), (1, 2, 3));
        // Story: nothing constrains the film, so it airs-into place ahead of the prequel…
        assert_eq!((at(9).chronological, at(2).chronological, at(1).chronological), (1, 2, 3));
        assert!(at(2).is_current);
    }

    #[test]
    fn cyclic_relations_leave_the_story_at_the_airing_order() {
        // Each title claims the other as its sequel: dirty data, and half a story order would
        // lie worse than none.
        let a = member(1, "TV", 2020, vec![("SEQUEL", member(2, "TV", 2021, vec![]))]);
        let b = member(2, "TV", 2021, vec![("SEQUEL", member(1, "TV", 2020, vec![]))]);
        let order = build(&a.clone(), &[a, b], true);
        assert!(order.iter().all(|e| e.chronological == e.position));
    }
}
