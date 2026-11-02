//! Simkl: a tracker beside the list service, not a third one. It hears about episodes watched,
//! titles added to or taken off the library, and statuses chosen here, whether AniList,
//! MyAnimeList or neither is signed in. Nothing is read back from it.
//!
//! Kept in step with Android's `SimklAuthManager.kt`, `SimklClient.kt` and `SimklPayloads.kt`:
//! - one "Mobile, desktop & browser" client id (public, no secret) for both apps. The desktop signs
//!   in with the device flow: a code approved at simkl.com/pin, opened in the browser with the code
//!   filled in. It needs no redirect, so it works the same in dev builds, which are not registered
//!   for `anilili://`;
//! - access tokens last 7 days; refresh tokens 180, sliding, and they do **not** rotate, but a
//!   refresh kills the previous access token at once;
//! - every write names the title by AniList id (plus MAL id when known), with flat episode
//!   numbers and no title, so an unmatched id is reported in `not_found` rather than fuzzy-matched
//!   to a sequel of the same name;
//! - one POST a second per user token at most: Simkl suspends a `client_id` that keeps going over.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::net::Http;
use crate::store::now;
use crate::{Error, Result};

const SERVICE: &str = "Simkl";
pub const CLIENT_ID: &str = "6cb9e1ceb75ce68500b9c2a673a92a24c0fbbde1fe524c1b55b7540352cb090f";
/// The `app-name` every API call carries, so Simkl can tell the two apps' traffic apart.
pub const APP_NAME: &str = "anilili-desktop";
const API: &str = "https://api.simkl.com";
/// Without a scope Simkl grants read-only; an unknown scope is silently downgraded, so the granted
/// one is checked on the way back.
const SCOPE: &str = "media:read media:write";
/// Refresh this long before the reported expiry so calls never race it.
const EXPIRY_MARGIN: i64 = 10 * 60;
/// A declined code is never reported (Simkl keeps answering `authorization_pending` until the code
/// dies at 15 minutes); its docs ask for a shorter timeout of our own.
const DEVICE_TIMEOUT: i64 = 10 * 60;
const MIN_POST_GAP: Duration = Duration::from_millis(1_100);
/// A ceiling on one history write; the longest series here is about 1,150 episodes.
pub const MAX_HISTORY_EPISODES: i32 = 3_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SimklTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
    #[serde(default)]
    pub username: Option<String>,
}

impl SimklTokens {
    pub fn needs_refresh(&self) -> bool {
        now() >= self.expires_at - EXPIRY_MARGIN
    }
}

/// A device-flow code. `device_code` is what polls; it is never shown or logged.
#[derive(Debug, Clone)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: String,
    pub device_code: String,
    /// Unix seconds.
    pub expires_at: i64,
    pub interval: u64,
}

pub enum DevicePoll {
    Pending,
    SlowDown,
    Approved(SimklTokens),
    Expired,
}

/// One title's state on the account, from `/sync/watched`.
#[derive(Debug, Clone, PartialEq)]
pub struct WatchState {
    pub found: bool,
    /// `watching`, `plantowatch`, `hold`, `dropped`, `completed`, or none.
    pub list: Option<String>,
    pub watched_before: bool,
}

pub struct Simkl {
    http: Http,
    version: String,
    last_post: tokio::sync::Mutex<Option<std::time::Instant>>,
}

impl Simkl {
    pub fn new(http: Http, version: impl Into<String>) -> Self {
        Self { http, version: version.into(), last_post: tokio::sync::Mutex::new(None) }
    }

    fn user_agent(&self) -> String {
        format!("{APP_NAME}/{}", self.version)
    }

    async fn form(&self, path: &str, form: &[(&str, &str)]) -> Result<(u16, Value)> {
        let body: String = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let response = self
            .http
            .client()
            .post(format!("{API}{path}"))
            .timeout(Duration::from_secs(20))
            .header("User-Agent", self.user_agent())
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await?;
        Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
    }

    /// Asks for a code to show.
    pub async fn device_code(&self) -> Result<DeviceCode> {
        let (status, v) = self.form("/oauth2/device", &[("client_id", CLIENT_ID), ("scope", SCOPE)]).await?;
        if status >= 400 {
            return Err(Error::Http { service: SERVICE, status, detail: oauth_error(&v).unwrap_or("device code refused").to_owned() });
        }
        let text = |key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
        let device_code = text("device_code").ok_or_else(|| Error::decode(SERVICE, "no device_code"))?;
        let user_code = text("user_code").ok_or_else(|| Error::decode(SERVICE, "no user_code"))?;
        let verification_uri = text("verification_uri").unwrap_or_else(|| "https://simkl.com/pin".into());
        let expires_in = v.get("expires_in").and_then(Value::as_i64).unwrap_or(900);
        Ok(DeviceCode {
            verification_uri_complete: text("verification_uri_complete").unwrap_or_else(|| verification_uri.clone()),
            verification_uri,
            user_code,
            device_code,
            expires_at: now() + expires_in.min(DEVICE_TIMEOUT),
            interval: v.get("interval").and_then(Value::as_u64).unwrap_or(5).max(5),
        })
    }

    /// One poll of a device code.
    pub async fn poll_device(&self, code: &DeviceCode) -> Result<DevicePoll> {
        if now() >= code.expires_at {
            return Ok(DevicePoll::Expired);
        }
        let (status, v) = self
            .form(
                "/oauth2/token",
                &[
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("client_id", CLIENT_ID),
                    ("device_code", &code.device_code),
                ],
            )
            .await?;
        if status < 300 {
            return tokens_from(&v).map(DevicePoll::Approved);
        }
        match oauth_error(&v) {
            Some("authorization_pending") => Ok(DevicePoll::Pending),
            Some("slow_down") => Ok(DevicePoll::SlowDown),
            Some("expired_token") => Ok(DevicePoll::Expired),
            other => Err(Error::Http { service: SERVICE, status, detail: other.unwrap_or("device code refused").to_owned() }),
        }
    }

    /// A new access token for the same grant. An `Error::Http` with a 4xx other than 429 means the
    /// grant is gone (revoked, or past 180 days); anything else is worth trying again later.
    pub async fn refresh(&self, tokens: &SimklTokens) -> Result<SimklTokens> {
        let (status, v) = self
            .form("/oauth2/token", &[("grant_type", "refresh_token"), ("client_id", CLIENT_ID), ("refresh_token", &tokens.refresh_token)])
            .await?;
        if status >= 400 {
            return Err(Error::Http { service: SERVICE, status, detail: oauth_error(&v).unwrap_or("refresh refused").to_owned() });
        }
        let mut fresh = tokens_from(&v)?;
        fresh.username = tokens.username.clone();
        Ok(fresh)
    }

    /// Revokes the whole grant. Simkl answers 200 whether or not anything was revoked.
    pub async fn revoke(&self, tokens: &SimklTokens) {
        let _ = self.form("/oauth2/revoke", &[("client_id", CLIENT_ID), ("token", &tokens.refresh_token)]).await;
    }

    async fn call(&self, token: &str, path: &str, body: Option<Value>) -> Result<Value> {
        let mut url = url::Url::parse(&format!("{API}{path}")).expect("static url");
        url.query_pairs_mut().append_pair("client_id", CLIENT_ID).append_pair("app-name", APP_NAME).append_pair("app-version", &self.version);
        let request = match &body {
            Some(body) => self.http.client().post(url.as_str()).header("Content-Type", "application/json").body(body.to_string()),
            None => self.http.client().get(url.as_str()),
        }
        .timeout(Duration::from_secs(30))
        .header("User-Agent", self.user_agent())
        .header("Authorization", format!("Bearer {token}"));
        let response = if body.is_some() {
            // Every POST, the `/sync/watched` lookup included, is spaced process-wide.
            let mut last = self.last_post.lock().await;
            if let Some(at) = *last {
                let since = at.elapsed();
                if since < MIN_POST_GAP {
                    tokio::time::sleep(MIN_POST_GAP - since).await;
                }
            }
            let sent = request.send().await;
            *last = Some(std::time::Instant::now());
            sent?
        } else {
            request.send().await?
        };
        let status = response.status().as_u16();
        let bytes = response.bytes().await?;
        if status >= 400 {
            return Err(Error::Http { service: SERVICE, status, detail: crate::net::problem_detail(&bytes) });
        }
        Ok(serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// The account's display name.
    pub async fn account_name(&self, token: &str) -> Result<Option<String>> {
        let v = self.call(token, "/users/settings", None).await?;
        Ok(v.pointer("/user/name").and_then(Value::as_str).filter(|n| !n.is_empty()).map(str::to_owned))
    }

    /// The account's numeric id: the usage ping's `people` hashes it (`docs/USAGE-PING.md`).
    pub async fn account_id(&self, token: &str) -> Result<Option<i64>> {
        let v = self.call(token, "/users/settings", None).await?;
        Ok(v.pointer("/account/id").and_then(Value::as_i64).filter(|id| *id > 0))
    }

    // The writes take the token by value so a retry wrapper can hand each attempt its own.

    /// POST `/sync/history`; false when Simkl matched none of the ids.
    pub async fn add_to_history(&self, token: String, body: Value) -> Result<bool> {
        Ok(!not_found(&self.call(&token, "/sync/history", Some(body)).await?))
    }

    /// POST `/sync/add-to-list`; false when Simkl matched none of the ids.
    pub async fn add_to_list(&self, token: String, body: Value) -> Result<bool> {
        Ok(!not_found(&self.call(&token, "/sync/add-to-list", Some(body)).await?))
    }

    /// POST `/sync/history/remove`; false when Simkl matched none of the ids.
    pub async fn remove_from_history(&self, token: String, body: Value) -> Result<bool> {
        Ok(!not_found(&self.call(&token, "/sync/history/remove", Some(body)).await?))
    }

    /// One title's watchlist status, without pulling the whole library.
    pub async fn watch_state(&self, token: String, query: Value) -> Result<WatchState> {
        let v = self.call(&token, "/sync/watched", Some(query)).await?;
        Ok(watch_state_from(&v))
    }
}

fn tokens_from(v: &Value) -> Result<SimklTokens> {
    let text = |key: &str| v.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    if let Some(scope) = v.get("scope").and_then(Value::as_str)
        && !scope.split(' ').any(|s| s == "media:write")
    {
        return Err(Error::Other("Simkl granted read-only access, so nothing could be tracked".into()));
    }
    let tokens = SimklTokens {
        access_token: text("access_token"),
        refresh_token: text("refresh_token"),
        expires_at: now() + v.get("expires_in").and_then(Value::as_i64).unwrap_or(604_800),
        username: None,
    };
    if tokens.access_token.is_empty() || tokens.refresh_token.is_empty() {
        return Err(Error::decode(SERVICE, "token response without tokens"));
    }
    Ok(tokens)
}

fn oauth_error(v: &Value) -> Option<&str> {
    v.get("error").and_then(Value::as_str)
}

fn watch_state_from(v: &Value) -> WatchState {
    let Some(first) = v.as_array().and_then(|a| a.first()) else {
        return WatchState { found: false, list: None, watched_before: false };
    };
    if first.get("result").and_then(Value::as_str) == Some("not_found") {
        return WatchState { found: false, list: None, watched_before: false };
    }
    WatchState {
        found: true,
        list: first.get("list").and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned),
        watched_before: first.get("last_watched_at").and_then(Value::as_str).is_some(),
    }
}

// ---- request bodies (Android's SimklPayloads) ------------------------------------------------

pub fn ids(anilist_id: i64, mal_id: Option<i64>) -> Value {
    match mal_id.filter(|id| *id > 0) {
        Some(mal) => json!({ "anilist": anilist_id, "mal": mal }),
        None => json!({ "anilist": anilist_id }),
    }
}

/// Episodes 1 through `progress`: what an AniList count means, and idempotent on Simkl (an episode
/// it already has is skipped), so it also fills in a series Simkl joined halfway.
pub fn history(ids: Value, progress: i32) -> Value {
    let episodes: Vec<Value> = (1..=progress.clamp(0, MAX_HISTORY_EPISODES)).map(|n| json!({ "number": n })).collect();
    json!({ "anime": [{ "ids": ids, "episodes": episodes }] })
}

/// The whole series watched.
pub fn completed(ids: Value) -> Value {
    json!({ "anime": [{ "ids": ids, "status": "completed" }] })
}

pub fn add_to_list(ids: Value, to: &str) -> Value {
    json!({ "anime": [{ "ids": ids, "to": to }] })
}

pub fn remove(ids: Value) -> Value {
    json!({ "anime": [{ "ids": ids }] })
}

pub fn watched_query(ids: Value) -> Value {
    json!([{ "ids": ids }])
}

pub fn not_found(response: &Value) -> bool {
    response.get("not_found").and_then(Value::as_object).is_some_and(|m| m.values().any(|v| v.as_array().is_some_and(|a| !a.is_empty())))
}

/// The desktop library's status → Simkl's watchlist. `completed` is not a list move (it goes
/// through [`completed`], which marks the episodes too), and `planning` is a save
/// ([`saved_action`]), so neither maps here.
pub fn list_status(local: &str) -> Option<&'static str> {
    Some(match local {
        "watching" => "watching",
        "paused" => "hold",
        "dropped" => "dropped",
        _ => return None,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum SavedAction {
    AddPlanned,
    Remove,
    Nothing,
}

/// The rule every list sync here follows: a save only adds a title the account doesn't track
/// (never demoting one being watched), and a removal only takes off a title that is still just a
/// plan (never deleting watch history).
pub fn saved_action(saved: bool, list: Option<&str>, watched_before: bool) -> SavedAction {
    match (saved, list, watched_before) {
        (true, None, false) => SavedAction::AddPlanned,
        (false, Some("plantowatch"), false) => SavedAction::Remove,
        _ => SavedAction::Nothing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_carry_a_known_mal_id_only() {
        assert_eq!(ids(21, Some(21)), json!({ "anilist": 21, "mal": 21 }));
        assert_eq!(ids(185736, None), json!({ "anilist": 185736 }));
        assert_eq!(ids(185736, Some(0)), json!({ "anilist": 185736 }));
    }

    #[test]
    fn history_marks_every_episode_up_to_progress() {
        assert_eq!(
            history(ids(20958, Some(25777)), 3),
            json!({ "anime": [{ "ids": { "anilist": 20958, "mal": 25777 }, "episodes": [{ "number": 1 }, { "number": 2 }, { "number": 3 }] }] })
        );
        let capped = history(ids(1, None), 1_000_000);
        assert_eq!(capped["anime"][0]["episodes"].as_array().unwrap().len(), MAX_HISTORY_EPISODES as usize);
    }

    #[test]
    fn other_bodies() {
        assert_eq!(completed(ids(1, None)), json!({ "anime": [{ "ids": { "anilist": 1 }, "status": "completed" }] }));
        assert_eq!(add_to_list(ids(1, None), "hold"), json!({ "anime": [{ "ids": { "anilist": 1 }, "to": "hold" }] }));
        assert_eq!(watched_query(ids(1, None)), json!([{ "ids": { "anilist": 1 } }]));
    }

    #[test]
    fn not_found_reads_any_array() {
        assert!(!not_found(&json!({ "added": { "shows": 1 }, "not_found": { "movies": [], "shows": [] } })));
        assert!(not_found(&json!({ "not_found": { "shows": [{ "ids": { "anilist": 1 } }] } })));
        assert!(!not_found(&json!({ "added": { "shows": 1 } })));
    }

    #[test]
    fn statuses() {
        assert_eq!(list_status("watching"), Some("watching"));
        assert_eq!(list_status("paused"), Some("hold"));
        assert_eq!(list_status("dropped"), Some("dropped"));
        assert_eq!(list_status("completed"), None);
        assert_eq!(list_status("planning"), None);
    }

    #[test]
    fn saves_and_removals_never_lose_history() {
        assert_eq!(saved_action(true, None, false), SavedAction::AddPlanned);
        assert_eq!(saved_action(true, Some("watching"), true), SavedAction::Nothing);
        assert_eq!(saved_action(true, None, true), SavedAction::Nothing);
        assert_eq!(saved_action(false, Some("plantowatch"), false), SavedAction::Remove);
        assert_eq!(saved_action(false, Some("plantowatch"), true), SavedAction::Nothing);
        assert_eq!(saved_action(false, Some("completed"), true), SavedAction::Nothing);
    }

    #[test]
    fn watch_state_reads_the_lookup() {
        assert_eq!(watch_state_from(&json!([{ "result": "not_found" }])), WatchState { found: false, list: None, watched_before: false });
        assert_eq!(
            watch_state_from(&json!([{ "result": true, "simkl": 1, "list": "watching", "last_watched_at": "2026-01-01T00:00:00Z" }])),
            WatchState { found: true, list: Some("watching".into()), watched_before: true }
        );
        assert_eq!(
            watch_state_from(&json!([{ "result": false, "simkl": 1, "list": null, "last_watched_at": null }])),
            WatchState { found: true, list: None, watched_before: false }
        );
        assert_eq!(watch_state_from(&Value::Null), WatchState { found: false, list: None, watched_before: false });
    }

    #[test]
    fn a_read_only_grant_is_refused() {
        assert!(tokens_from(&json!({ "access_token": "a", "refresh_token": "r", "expires_in": 604800, "scope": "media:read" })).is_err());
        assert!(tokens_from(&json!({ "access_token": "a", "refresh_token": "r", "expires_in": 604800, "scope": "media:read media:write" })).is_ok());
    }
}
