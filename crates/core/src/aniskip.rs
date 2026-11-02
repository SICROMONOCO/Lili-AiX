//! AniSkip (api.aniskip.com), the community's intro/outro/recap timings, keyed by MyAnimeList id.
//! A port of Android's `AniSkipClient.kt`: the live lookup behind the timings Miruro, the
//! providers and the media index's daily mirror don't have.
//!
//! - Every candidate comes back; the one whose `episodeLength` is nearest the stream playing wins,
//!   because timings are per encode.
//! - A failure on the full type list is retried with openings and endings alone (AniSkip 500s on
//!   the long list and answers the short one); a second failure is an error, so an outage is never
//!   cached as "no timings".
//! - When an episode has none, AniSkip's relation rules may say it is filed under another MAL
//!   entry (one per cour, where providers number straight through); the first matching rule wins.

use std::time::Duration;

use serde_json::Value;

use crate::model::{SkipTimes, Window};
use crate::net::Http;
use crate::{Error, Result};

const SERVICE: &str = "AniSkip";
const API: &str = "https://api.aniskip.com/v2";
const ALL_TYPES: &[&str] = &["op", "ed", "mixed-op", "mixed-ed", "recap"];
const CORE_TYPES: &[&str] = &["op", "ed"];

#[derive(Clone)]
pub struct AniSkip {
    http: Http,
}

#[derive(Debug, Clone, PartialEq)]
struct Segment {
    kind: String,
    window: Window,
    episode_length: Option<f64>,
}

/// "Episodes `from_start..=from_end` of the title asked about are episodes from `to_start` of
/// `to_mal`."
#[derive(Debug, Clone, PartialEq)]
struct Rule {
    from_start: i64,
    from_end: Option<i64>,
    to_mal: i64,
    to_start: i64,
    to_end: Option<i64>,
}

impl AniSkip {
    pub fn new(http: Http) -> Self {
        Self { http }
    }

    /// The timings for an episode, `None` when AniSkip has none. `length` (seconds, 0 when
    /// unknown) picks among submissions for different encodes.
    pub async fn skip_times(&self, mal: i64, episode: i64, length: f64) -> Result<Option<SkipTimes>> {
        if let Some(found) = self.lookup(mal, episode, length).await? {
            return Ok(Some(found));
        }
        // Only an empty answer pays for the redirect table.
        let rules = self.rules(mal).await.unwrap_or_default();
        match redirect(&rules, mal, episode) {
            Some((to_mal, to_episode)) => self.lookup(to_mal, to_episode, length).await,
            None => Ok(None),
        }
    }

    async fn lookup(&self, mal: i64, episode: i64, length: f64) -> Result<Option<SkipTimes>> {
        let body = match self.fetch(mal, episode, length, ALL_TYPES).await {
            Ok(body) => body,
            Err(_) => self.fetch(mal, episode, length, CORE_TYPES).await?,
        };
        Ok(body.and_then(|b| parse(&b, length)))
    }

    async fn fetch(&self, mal: i64, episode: i64, length: f64, types: &[&str]) -> Result<Option<Value>> {
        let types: String = types.iter().map(|t| format!("types%5B%5D={t}&")).collect();
        let url = format!("{API}/skip-times/{mal}/{episode}?{types}episodeLength={length}");
        self.get(&url).await
    }

    async fn rules(&self, mal: i64) -> Result<Vec<Rule>> {
        let Some(body) = self.get(&format!("{API}/relation-rules/{mal}")).await? else { return Ok(Vec::new()) };
        Ok(parse_rules(&body))
    }

    async fn get(&self, url: &str) -> Result<Option<Value>> {
        match self.http.get_bytes(SERVICE, url, &[("Accept", "application/json")], Duration::from_secs(12)).await {
            Ok((body, _)) => serde_json::from_slice(&body).map(Some).map_err(|e| Error::decode(SERVICE, e)),
            Err(Error::Http { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }
}

fn parse(body: &Value, length: f64) -> Option<SkipTimes> {
    if body.get("found").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let segments: Vec<Segment> = body
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let start = r.pointer("/interval/startTime")?.as_f64()?;
            let end = r.pointer("/interval/endTime")?.as_f64()?;
            (start.is_finite() && end.is_finite() && end > start).then(|| Segment {
                kind: r.get("skipType").and_then(Value::as_str).unwrap_or_default().to_owned(),
                window: Window { start, end },
                episode_length: r.get("episodeLength").and_then(Value::as_f64),
            })
        })
        .collect();
    let skip = SkipTimes {
        intro: pick(&segments, "op", "mixed-op", length),
        outro: pick(&segments, "ed", "mixed-ed", length),
        recap: pick(&segments, "recap", "recap", length),
    };
    (skip.intro.is_some() || skip.outro.is_some() || skip.recap.is_some()).then_some(skip)
}

/// The plain kind before its "mixed" fallback, nearest in length to the stream playing.
fn pick(segments: &[Segment], primary: &str, fallback: &str, length: f64) -> Option<Window> {
    let of = |kind: &str| segments.iter().filter(|s| s.kind == kind).collect::<Vec<_>>();
    let candidates = Some(of(primary)).filter(|c| !c.is_empty()).unwrap_or_else(|| of(fallback));
    candidates
        .into_iter()
        .min_by(|a, b| {
            let off = |s: &Segment| if length <= 0.0 { 0.0 } else { s.episode_length.map_or(f64::MAX, |l| (l - length).abs()) };
            off(a).total_cmp(&off(b))
        })
        .map(|s| s.window)
}

fn parse_rules(body: &Value) -> Vec<Rule> {
    if body.get("found").and_then(Value::as_bool) != Some(true) {
        return Vec::new();
    }
    body.get("rules")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|r| {
            Some(Rule {
                from_start: r.pointer("/from/start")?.as_i64()?,
                from_end: r.pointer("/from/end").and_then(Value::as_i64),
                to_mal: r.pointer("/to/malId")?.as_i64()?,
                to_start: r.pointer("/to/start")?.as_i64()?,
                to_end: r.pointer("/to/end").and_then(Value::as_i64),
            })
        })
        .collect()
}

/// Where an episode's timings are filed, by the first rule that covers it; `None` when no rule
/// does, or the only one points back at the lookup already made.
fn redirect(rules: &[Rule], mal: i64, episode: i64) -> Option<(i64, i64)> {
    rules.iter().find_map(|rule| {
        if episode < rule.from_start || rule.from_end.is_some_and(|end| episode > end) {
            return None;
        }
        let mapped = rule.to_start + (episode - rule.from_start);
        if mapped < 1 || rule.to_end.is_some_and(|end| mapped > end) || (rule.to_mal == mal && mapped == episode) {
            return None;
        }
        Some((rule.to_mal, mapped))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn picks_the_encode_nearest_the_stream() {
        let body = json!({"found": true, "results": [
            {"interval": {"startTime": 10.0, "endTime": 100.0}, "skipType": "op", "episodeLength": 1420.0},
            {"interval": {"startTime": 20.0, "endTime": 110.0}, "skipType": "op", "episodeLength": 1440.0},
            {"interval": {"startTime": 0.0, "endTime": 60.0}, "skipType": "recap", "episodeLength": 1440.0},
            {"interval": {"startTime": 1300.0, "endTime": 1390.0}, "skipType": "mixed-ed", "episodeLength": 1440.0},
        ]});
        let skip = parse(&body, 1441.0).unwrap();
        assert_eq!(skip.intro, Some(Window { start: 20.0, end: 110.0 }));
        assert_eq!(skip.recap, Some(Window { start: 0.0, end: 60.0 }));
        // No plain ending: the mixed one stands in.
        assert_eq!(skip.outro, Some(Window { start: 1300.0, end: 1390.0 }));
        assert_eq!(parse(&json!({"found": false, "results": []}), 0.0), None);
    }

    #[test]
    fn relation_rules_redirect_as_androids() {
        let rules = parse_rules(&json!({"found": true, "rules": [
            {"from": {"start": 13, "end": 24}, "to": {"malId": 2, "start": 1, "end": 12}},
            {"from": {"start": 1, "end": 12}, "to": {"malId": 1, "start": 1}},
        ]}));
        assert_eq!(redirect(&rules, 1, 14), Some((2, 2)));
        // A rule pointing back at the same lookup is not a redirect.
        assert_eq!(redirect(&rules, 1, 5), None);
        assert_eq!(redirect(&rules, 1, 30), None);
    }
}
