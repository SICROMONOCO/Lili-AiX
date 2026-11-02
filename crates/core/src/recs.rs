//! Recommendations, as the Android app makes them (`data/recs/`): a published "people who liked this
//! also liked" graph, added up on the device against what the viewer has watched. No account, no
//! server and no upload of anyone's list.
//!
//! - [`NeighbourGraph`] is the media index's `recs/neighbours-v1.tsv.gz`, built daily from
//!   AniList's community recommendation votes blended with tag similarity:
//!   `<id>\t<franchise root>\t<flags>\t<id>:<weight>,…`, weights 1..100, best first.
//! - [`signals`] turns a list and this device's history into weights; [`recommend`] adds the
//!   neighbours up per franchise and spreads the result so one favourite can't fill a row.
//!
//! A port of `NeighbourGraph.kt` and `TasteEngine.kt`: change them together.

use std::collections::{HashMap, HashSet};
use std::io::Read;

pub const PATH: &str = "recs/neighbours-v1.tsv.gz";
const HEADER: &str = "#anilili-neighbours";
const FORMAT_VERSION: u32 = 1;
const FLAG_ADULT: u8 = 1;
const FLAG_UNRELEASED: u8 = 2;

/// Every title's nearest neighbours, as sorted parallel arrays (CSR): ~16,000 titles and
/// ~600,000 edges stay a few megabytes.
pub struct NeighbourGraph {
    ids: Vec<i64>,
    roots: Vec<i64>,
    flags: Vec<u8>,
    offsets: Vec<usize>,
    neighbours: Vec<i64>,
    weights: Vec<u8>,
    /// When the build wrote the file, as it wrote it.
    pub generated: Option<String>,
}

impl NeighbourGraph {
    /// The file as published (gzip).
    pub fn from_gzip(bytes: &[u8]) -> Result<Self, String> {
        let mut text = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut text).map_err(|e| format!("not gzip: {e}"))?;
        Self::parse(&text)
    }

    /// Parses the decompressed file. A header it doesn't know is an error, so a caller keeps the
    /// graph it had; a malformed line costs only that title.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let text = std::str::from_utf8(bytes).map_err(|_| "not text".to_owned())?;
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap_or_default().split('\t').collect();
        if header.first() != Some(&HEADER) {
            return Err("not a neighbour file".into());
        }
        let version = header.get(1).and_then(|v| v.parse::<u32>().ok());
        if version != Some(FORMAT_VERSION) {
            return Err(format!("unsupported neighbour file version {version:?}"));
        }
        let generated = header.get(2).map(|g| g.to_string());

        /// id, franchise root, flags, (neighbour, weight)…
        type Row = (i64, i64, u8, Vec<(i64, u8)>);
        let mut rows: Vec<Row> = Vec::new();
        for line in lines {
            let mut fields = line.split('\t');
            let Some(id) = fields.next().and_then(|f| f.parse::<i64>().ok()) else { continue };
            let root = fields.next().and_then(|f| f.parse::<i64>().ok()).unwrap_or(id);
            let flags = fields.next().and_then(|f| f.parse::<u8>().ok()).unwrap_or(0);
            let edges = fields
                .next()
                .unwrap_or_default()
                .split(',')
                .filter_map(|pair| {
                    let (n, w) = pair.split_once(':')?;
                    Some((n.parse::<i64>().ok()?, w.parse::<u32>().ok()?.clamp(1, 100) as u8))
                })
                .collect();
            rows.push((id, root, flags, edges));
        }
        // The build writes ids in order; a hand-edited file may not. One row per id, the first.
        rows.sort_by_key(|r| r.0);
        rows.dedup_by_key(|r| r.0);

        let mut graph = NeighbourGraph {
            ids: Vec::with_capacity(rows.len()),
            roots: Vec::with_capacity(rows.len()),
            flags: Vec::with_capacity(rows.len()),
            offsets: Vec::with_capacity(rows.len() + 1),
            neighbours: Vec::new(),
            weights: Vec::new(),
            generated,
        };
        for (id, root, flags, edges) in rows {
            graph.ids.push(id);
            graph.roots.push(root);
            graph.flags.push(flags);
            graph.offsets.push(graph.neighbours.len());
            for (n, w) in edges {
                graph.neighbours.push(n);
                graph.weights.push(w);
            }
        }
        graph.offsets.push(graph.neighbours.len());
        Ok(graph)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    pub fn edge_count(&self) -> usize {
        self.neighbours.len()
    }

    fn index_of(&self, id: i64) -> Option<usize> {
        self.ids.binary_search(&id).ok()
    }

    /// The first entry of `id`'s franchise, or `id` when the graph doesn't know it.
    pub fn root_of(&self, id: i64) -> i64 {
        self.index_of(id).map_or(id, |i| self.roots[i])
    }

    fn has_flag(&self, id: i64, flag: u8) -> bool {
        self.index_of(id).is_some_and(|i| self.flags[i] & flag != 0)
    }

    pub fn is_adult(&self, id: i64) -> bool {
        self.has_flag(id, FLAG_ADULT)
    }

    pub fn is_unreleased(&self, id: i64) -> bool {
        self.has_flag(id, FLAG_UNRELEASED)
    }

    /// `id`'s neighbours with their weights (1..100), best first.
    pub fn neighbours(&self, id: i64) -> impl Iterator<Item = (i64, u8)> + '_ {
        let range = self.index_of(id).map_or(0..0, |i| self.offsets[i]..self.offsets[i + 1]);
        range.map(|e| (self.neighbours[e], self.weights[e]))
    }

    fn has_neighbours(&self, id: i64) -> bool {
        self.neighbours(id).next().is_some()
    }
}

/// How much one title says about the viewer's taste: positive liked, negative gave up on. Roughly
/// -1..2; a plain "completed, no score" is 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TasteSignal {
    pub anilist_id: i64,
    pub weight: f64,
}

/// One title on the viewer's list service.
#[derive(Debug, Clone, PartialEq)]
pub struct ListRating {
    pub anilist_id: i64,
    /// AniList's status: CURRENT, PLANNING, COMPLETED, PAUSED, DROPPED, REPEATING.
    pub status: String,
    /// 0..10, 0 meaning unscored.
    pub score: f64,
    /// Unix seconds of the entry's last change; 0 when unknown.
    pub updated_at: i64,
}

/// One title played on this device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlayedTitle {
    pub anilist_id: i64,
    pub episodes_watched: u32,
    pub updated_at: i64,
}

/// One recommended title: always a franchise's first entry, and `because` is the watched title
/// that contributed most ("Because you watched …").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recommendation {
    pub anilist_id: i64,
    pub score: f64,
    pub because: i64,
}

/// Scores under this are a dislike on any viewer's scale.
const LOW_SCORE: f64 = 5.0;
const YEAR_SECS: f64 = 365.0 * 24.0 * 3600.0;
const MIN_RECENCY: f64 = 0.35;
const REPEAT_SEED_DECAY: f64 = 0.8;
const CANDIDATE_POOL_FACTOR: usize = 4;
const FALLBACK_MIN_EPISODES: u32 = 3;
/// Weight of a title opened from the dice, and of one swiped away.
pub const DICE_OPENED: f64 = 0.3;
pub const DICE_SKIPPED: f64 = -0.1;

/// The weight each title carries. Scores are read against the viewer's own habits (someone who
/// gives everything an 8 means "fine" by it), and the list service wins over this device's
/// history for a title on both. Dice feedback is light and only counts where nothing stronger is
/// known.
pub fn signals(list: &[ListRating], played: &[PlayedTitle], feedback: &[(i64, f64)], now: i64) -> Vec<TasteSignal> {
    let scored: Vec<f64> = list.iter().map(|e| e.score).filter(|s| *s > 0.0).collect();
    let mean = if scored.is_empty() { 0.0 } else { scored.iter().sum::<f64>() / scored.len() as f64 };
    let spread =
        if scored.len() < 2 { 1.0 } else { (scored.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / scored.len() as f64).sqrt().max(1.0) };

    let mut order: Vec<i64> = Vec::new();
    let mut weights: HashMap<i64, f64> = HashMap::new();
    for entry in list {
        let Some(base) = status_weight(entry, mean, spread) else { continue };
        if !weights.contains_key(&entry.anilist_id) {
            order.push(entry.anilist_id);
        }
        weights.insert(entry.anilist_id, base * recency(entry.updated_at, now));
    }
    for title in played {
        if weights.contains_key(&title.anilist_id) {
            continue;
        }
        let base = match title.episodes_watched {
            3.. => 0.9,
            1.. => 0.45,
            // Opened and played into, but never past the watched threshold.
            _ => 0.15,
        };
        order.push(title.anilist_id);
        weights.insert(title.anilist_id, base * recency(title.updated_at, now));
    }
    for &(id, weight) in feedback {
        if let std::collections::hash_map::Entry::Vacant(slot) = weights.entry(id) {
            order.push(id);
            slot.insert(weight);
        }
    }
    order.into_iter().map(|id| TasteSignal { anilist_id: id, weight: weights[&id] }).filter(|s| s.weight != 0.0).collect()
}

fn status_weight(entry: &ListRating, mean: f64, spread: f64) -> Option<f64> {
    let score = entry.score;
    let relative = if score > 0.0 { ((score - mean) / spread).clamp(-2.0, 2.0) } else { 0.0 };
    Some(match entry.status.to_ascii_uppercase().as_str() {
        "COMPLETED" => {
            if score <= 0.0 {
                1.0
            } else if score < LOW_SCORE {
                -0.6
            } else {
                (1.0 + 0.45 * relative).clamp(0.2, 2.0)
            }
        }
        "REPEATING" => 1.6,
        "CURRENT" => {
            if score > 0.0 && score < LOW_SCORE {
                -0.3
            } else {
                0.8 + 0.2 * relative.max(0.0)
            }
        }
        "PAUSED" => 0.25,
        "DROPPED" => {
            if score >= LOW_SCORE + 2.0 {
                0.0
            } else {
                -0.8
            }
        }
        // Wanting to watch something says a little about taste; it hasn't been tried yet.
        "PLANNING" => 0.25,
        _ => return None,
    })
}

/// Older watches count for less, never for nothing; unknown dates count in full.
fn recency(updated_at: i64, now: i64) -> f64 {
    if updated_at <= 0 {
        return 1.0;
    }
    let years = (now - updated_at).max(0) as f64 / YEAR_SECS;
    0.5f64.powf(years).max(MIN_RECENCY)
}

/// The best titles to recommend, best first. Titles in `exclude`, every other entry of their
/// franchises and the signals' own titles never are. Candidates are scored per franchise (five
/// seasons can't outvote a film on headcount) and each watched franchise counts once.
pub fn recommend(
    graph: &NeighbourGraph,
    signals: &[TasteSignal],
    exclude: &HashSet<i64>,
    hide_adult: bool,
    limit: usize,
) -> Vec<Recommendation> {
    if signals.is_empty() {
        return Vec::new();
    }
    let mut excluded: HashSet<i64> = exclude.iter().map(|id| graph.root_of(*id)).collect();
    excluded.extend(signals.iter().map(|s| graph.root_of(s.anilist_id)));

    // Each watched franchise speaks once, with its strongest member's voice.
    let mut seeds: Vec<(i64, Vec<&TasteSignal>)> = Vec::new();
    for signal in signals {
        let root = graph.root_of(signal.anilist_id);
        match seeds.iter_mut().find(|(r, _)| *r == root) {
            Some((_, members)) => members.push(signal),
            None => seeds.push((root, vec![signal])),
        }
    }

    let mut totals: HashMap<i64, f64> = HashMap::new();
    let mut best: HashMap<i64, f64> = HashMap::new();
    let mut because: HashMap<i64, i64> = HashMap::new();
    for (_, members) in &seeds {
        let strongest = members.iter().max_by(|a, b| a.weight.abs().total_cmp(&b.weight.abs())).expect("a seed has a member");
        let mut local: HashMap<i64, f64> = HashMap::new();
        for member in members {
            for (neighbour, weight) in graph.neighbours(member.anilist_id) {
                let root = graph.root_of(neighbour);
                if excluded.contains(&root) || graph.is_unreleased(neighbour) {
                    continue;
                }
                if hide_adult && (graph.is_adult(neighbour) || graph.is_adult(root)) {
                    continue;
                }
                let strength = f64::from(weight) / 100.0;
                let slot = local.entry(root).or_insert(0.0);
                if strength > *slot {
                    *slot = strength;
                }
            }
        }
        for (root, strength) in local {
            let contribution = strongest.weight * strength;
            *totals.entry(root).or_insert(0.0) += contribution;
            if contribution > *best.get(&root).unwrap_or(&0.0) {
                best.insert(root, contribution);
                because.insert(root, strongest.anilist_id);
            }
        }
    }

    let mut ranked: Vec<Recommendation> = totals
        .into_iter()
        .filter(|(root, total)| *total > 0.0 && because.contains_key(root))
        .map(|(root, total)| Recommendation { anilist_id: root, score: total, because: because[&root] })
        .collect();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.anilist_id.cmp(&b.anilist_id)));
    ranked.truncate(limit * CANDIDATE_POOL_FACTOR);
    diversify(ranked, limit)
}

/// The neighbours of one title, for "Because you watched X": [`recommend`] with a single seed.
pub fn because(graph: &NeighbourGraph, seed: i64, exclude: &HashSet<i64>, hide_adult: bool, limit: usize) -> Vec<Recommendation> {
    recommend(graph, &[TasteSignal { anilist_id: seed, weight: 1.0 }], exclude, hide_adult, limit)
}

/// The seed worth a "Because you watched" row: the most recently finished title the viewer didn't
/// dislike that the graph has neighbours for, so the row changes when they finish something. With
/// nothing finished on a list, the latest title played well into on this device.
pub fn featured_seed(graph: &NeighbourGraph, list: &[ListRating], played: &[PlayedTitle], now: i64) -> Option<i64> {
    let weights: HashMap<i64, f64> = signals(list, played, &[], now).into_iter().map(|s| (s.anilist_id, s.weight)).collect();
    let mut finished: Vec<&ListRating> = list
        .iter()
        .filter(|e| matches!(e.status.to_ascii_uppercase().as_str(), "COMPLETED" | "REPEATING"))
        .filter(|e| weights.get(&e.anilist_id).copied().unwrap_or(0.0) > 0.0)
        .collect();
    finished.sort_by_key(|e| std::cmp::Reverse(e.updated_at));
    if let Some(e) = finished.iter().find(|e| graph.has_neighbours(e.anilist_id)) {
        return Some(e.anilist_id);
    }
    let on_list: HashSet<i64> = list.iter().map(|e| e.anilist_id).collect();
    let mut recent: Vec<&PlayedTitle> =
        played.iter().filter(|p| !on_list.contains(&p.anilist_id) && p.episodes_watched >= FALLBACK_MIN_EPISODES).collect();
    recent.sort_by_key(|p| std::cmp::Reverse(p.updated_at));
    recent.iter().find(|p| graph.has_neighbours(p.anilist_id)).map(|p| p.anilist_id)
}

/// Each further pick credited to the same watched title is worth a little less.
fn diversify(mut ranked: Vec<Recommendation>, limit: usize) -> Vec<Recommendation> {
    let mut credited: HashMap<i64, i32> = HashMap::new();
    let mut picked = Vec::with_capacity(limit);
    while picked.len() < limit && !ranked.is_empty() {
        let (index, _) = ranked
            .iter()
            .enumerate()
            .map(|(i, c)| (i, c.score * REPEAT_SEED_DECAY.powi(*credited.get(&c.because).unwrap_or(&0))))
            .fold((0, f64::NEG_INFINITY), |best, next| if next.1 > best.1 { next } else { best });
        let choice = ranked.remove(index);
        *credited.entry(choice.because).or_insert(0) += 1;
        picked.push(choice);
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 → (2 season 2 of 1) franchise; 10, 11, 12 are other shows; 99 is adult; 98 unreleased.
    fn graph() -> NeighbourGraph {
        NeighbourGraph::parse(
            b"#anilili-neighbours\t1\t2026-10-04\n\
              1\t1\t0\t10:90,11:60,2:80,99:95,98:85\n\
              2\t1\t0\t10:50,12:40\n\
              5\t5\t0\t11:90,12:70\n\
              10\t10\t0\t1:90\n\
              11\t11\t0\t5:80\n\
              12\t12\t0\t\n\
              99\t99\t1\t1:50\n\
              98\t98\t2\t1:50\n",
        )
        .unwrap()
    }

    #[test]
    fn parses_the_published_format() {
        let g = graph();
        assert_eq!(g.len(), 8);
        assert_eq!(g.generated.as_deref(), Some("2026-10-04"));
        assert_eq!(g.root_of(2), 1);
        assert_eq!(g.root_of(777), 777, "unknown ids are their own root");
        assert!(g.is_adult(99) && g.is_unreleased(98) && !g.is_adult(1));
        assert_eq!(g.neighbours(1).next(), Some((10, 90)));
        assert!(NeighbourGraph::parse(b"#anilili-neighbours\t2\nx").is_err(), "a future format is refused");
        assert!(NeighbourGraph::parse(b"<html>").is_err());
    }

    #[test]
    fn recommends_neighbours_of_what_was_liked() {
        let g = graph();
        let liked = [TasteSignal { anilist_id: 1, weight: 1.0 }];
        let recs = recommend(&g, &liked, &HashSet::new(), true, 10);
        let ids: Vec<i64> = recs.iter().map(|r| r.anilist_id).collect();
        // Never the seed's own franchise (2 is season 2 of 1), the adult or the unreleased title.
        assert_eq!(ids, [10, 11]);
        assert!(recs.iter().all(|r| r.because == 1));
        // A dropped title pushes its neighbours down.
        let mixed = [TasteSignal { anilist_id: 1, weight: 1.0 }, TasteSignal { anilist_id: 5, weight: -0.8 }];
        let ids: Vec<i64> = recommend(&g, &mixed, &HashSet::new(), true, 10).iter().map(|r| r.anilist_id).collect();
        assert_eq!(ids, [10], "11 neighbours the dropped title more than the liked one");
        // Excluded titles stay out.
        let ids: Vec<i64> = recommend(&g, &liked, &HashSet::from([10]), true, 10).iter().map(|r| r.anilist_id).collect();
        assert_eq!(ids, [11]);
    }

    #[test]
    fn weighs_the_list_against_the_viewers_own_scale() {
        let now = 2_000_000_000;
        let list = [
            ListRating { anilist_id: 1, status: "COMPLETED".into(), score: 9.0, updated_at: now },
            ListRating { anilist_id: 2, status: "COMPLETED".into(), score: 7.0, updated_at: now },
            ListRating { anilist_id: 3, status: "DROPPED".into(), score: 0.0, updated_at: now },
            ListRating { anilist_id: 4, status: "COMPLETED".into(), score: 3.0, updated_at: now },
        ];
        let played = [
            PlayedTitle { anilist_id: 1, episodes_watched: 12, updated_at: now },
            PlayedTitle { anilist_id: 8, episodes_watched: 5, updated_at: now },
        ];
        let s = signals(&list, &played, &[(9, DICE_OPENED), (1, DICE_SKIPPED)], now);
        let w = |id| s.iter().find(|x| x.anilist_id == id).map(|x| x.weight);
        assert!(w(1).unwrap() > w(2).unwrap(), "a 9 beats a 7 on this viewer's scale");
        assert_eq!(w(3), Some(-0.8));
        assert_eq!(w(4), Some(-0.6), "below 5 is a dislike whatever the average");
        assert_eq!(w(8), Some(0.9), "played here, not on the list");
        assert_eq!(w(9), Some(DICE_OPENED));
        // A year-old completion counts half.
        let old = [ListRating { anilist_id: 1, status: "COMPLETED".into(), score: 0.0, updated_at: now - YEAR_SECS as i64 }];
        assert!((signals(&old, &[], &[], now)[0].weight - 0.5).abs() < 1e-9);
    }

    #[test]
    fn features_the_latest_finished_title() {
        let g = graph();
        let list = [
            ListRating { anilist_id: 10, status: "COMPLETED".into(), score: 0.0, updated_at: 100 },
            ListRating { anilist_id: 5, status: "COMPLETED".into(), score: 0.0, updated_at: 200 },
            ListRating { anilist_id: 1, status: "CURRENT".into(), score: 0.0, updated_at: 300 },
        ];
        assert_eq!(featured_seed(&g, &list, &[], 400), Some(5), "finished, not in progress");
        let played = [PlayedTitle { anilist_id: 11, episodes_watched: 4, updated_at: 50 }];
        assert_eq!(featured_seed(&g, &[], &played, 400), Some(11), "the device's history without a list");
    }

    #[test]
    fn one_favourite_does_not_fill_the_row() {
        let ranked = vec![
            Recommendation { anilist_id: 1, score: 1.0, because: 7 },
            Recommendation { anilist_id: 2, score: 0.95, because: 7 },
            Recommendation { anilist_id: 3, score: 0.9, because: 8 },
        ];
        let ids: Vec<i64> = diversify(ranked, 3).iter().map(|r| r.anilist_id).collect();
        assert_eq!(ids, [1, 3, 2]);
    }
}
