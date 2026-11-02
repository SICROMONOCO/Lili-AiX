//! MyAnimeList: OAuth (Authorization Code + PKCE `plain`, the only method MAL supports), the
//! user's anime list, and progress/status updates.
//!
//! The Android app's lessons, kept here:
//! - public client (no secret), redirect registered as `https://localhost`;
//! - access tokens expire (trust `expires_in`), refresh tokens **rotate on every refresh**: store
//!   the new pair each time, and only drop the session when the old access token is already dead;
//! - MAL is weak for metadata, so AniList stays the catalogue and MAL only supplies lists. MAL ids
//!   are mapped to AniList titles in batches.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::net::Http;
use crate::store::now;
use crate::{Error, Result};

const SERVICE: &str = "MyAnimeList";
pub const CLIENT_ID: &str = "4ae5f8056db821737a4f40f8816177cd";
const AUTHORIZE: &str = "https://myanimelist.net/v1/oauth2/authorize";
const TOKEN: &str = "https://myanimelist.net/v1/oauth2/token";
const API: &str = "https://api.myanimelist.net/v2";
/// Refresh this long before the reported expiry so calls never race it.
const EXPIRY_MARGIN: i64 = 5 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MalTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds.
    pub expires_at: i64,
}

impl MalTokens {
    pub fn needs_refresh(&self) -> bool {
        now() >= self.expires_at - EXPIRY_MARGIN
    }
    pub fn expired(&self) -> bool {
        now() >= self.expires_at
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MalUser {
    pub id: i64,
    pub name: String,
    pub picture: Option<String>,
}

/// One row of the user's MAL list, before mapping to AniList.
#[derive(Debug, Clone)]
pub struct MalEntry {
    pub mal_id: i64,
    /// MAL vocabulary: watching, completed, on_hold, dropped, plan_to_watch.
    pub status: String,
    pub rewatching: bool,
    pub progress: i32,
    pub score: f64,
    pub updated_at: i64,
}

/// The login URL. PKCE `plain`: the verifier *is* the challenge.
pub fn authorize_url(state: &str, verifier: &str) -> String {
    let mut url = url::Url::parse(AUTHORIZE).expect("static url");
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("state", state)
        .append_pair("code_challenge", verifier)
        .append_pair("code_challenge_method", "plain");
    url.to_string()
}

/// A URL-safe random string of `bytes` random bytes (43 chars for 32: inside MAL's 43–128).
pub fn random_token(bytes: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let chars = bytes * 4 / 3 + 1;
    (0..chars).map(|_| ALPHABET[(rand::random::<u8>() & 63) as usize] as char).collect()
}

pub struct Mal {
    http: Http,
}

impl Mal {
    pub fn new(http: Http) -> Self {
        Self { http }
    }

    async fn token_request(&self, form: &[(&str, &str)]) -> Result<MalTokens> {
        let body: String = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let response = self
            .http
            .client()
            .post(TOKEN)
            .timeout(Duration::from_secs(20))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let status = response.status().as_u16();
        let value: Value = serde_json::from_slice(&response.bytes().await?).map_err(|e| Error::decode(SERVICE, e))?;
        if status >= 400 {
            let detail = value.get("message").or(value.get("error")).and_then(Value::as_str).unwrap_or("token request failed");
            return Err(Error::Http { service: SERVICE, status, detail: detail.to_owned() });
        }
        Ok(MalTokens {
            access_token: value.get("access_token").and_then(Value::as_str).unwrap_or_default().to_owned(),
            refresh_token: value.get("refresh_token").and_then(Value::as_str).unwrap_or_default().to_owned(),
            expires_at: now() + value.get("expires_in").and_then(Value::as_i64).unwrap_or(3600),
        })
    }

    pub async fn exchange_code(&self, code: &str, verifier: &str) -> Result<MalTokens> {
        self.token_request(&[("client_id", CLIENT_ID), ("grant_type", "authorization_code"), ("code", code), ("code_verifier", verifier)])
            .await
    }

    pub async fn refresh(&self, tokens: &MalTokens) -> Result<MalTokens> {
        self.token_request(&[("client_id", CLIENT_ID), ("grant_type", "refresh_token"), ("refresh_token", &tokens.refresh_token)]).await
    }

    async fn get(&self, token: &str, url: &str) -> Result<Value> {
        let response =
            self.http.client().get(url).timeout(Duration::from_secs(20)).header("Authorization", format!("Bearer {token}")).send().await?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await?;
        if status >= 400 {
            return Err(Error::Http { service: SERVICE, status, detail: crate::net::problem_detail(&bytes) });
        }
        serde_json::from_slice(&bytes).map_err(|e| Error::decode(SERVICE, e))
    }

    pub async fn user(&self, token: &str) -> Result<MalUser> {
        let v = self.get(token, &format!("{API}/users/@me?fields=picture")).await?;
        Ok(MalUser {
            id: v.get("id").and_then(Value::as_i64).unwrap_or_default(),
            name: v.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
            picture: v.get("picture").and_then(Value::as_str).map(str::to_owned),
        })
    }

    /// The account's profile with its anime totals. MAL keeps no banner or daily history.
    pub async fn account(&self, token: &str) -> Result<crate::anilist::Account> {
        let v = self.get(token, &format!("{API}/users/@me?fields=picture,joined_at,anime_statistics")).await?;
        let stats = v.get("anime_statistics");
        let number = |key: &str| stats.and_then(|s| s.get(key)).and_then(Value::as_f64).unwrap_or_default();
        let name = v.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
        Ok(crate::anilist::Account {
            id: v.get("id").and_then(Value::as_i64).unwrap_or_default(),
            url: format!("https://myanimelist.net/profile/{}", url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>()),
            name,
            avatar: v.get("picture").and_then(Value::as_str).map(str::to_owned),
            banner: None,
            joined: v.get("joined_at").and_then(Value::as_str).and_then(parse_rfc3339),
            titles: number("num_items") as i64,
            episodes: number("num_episodes") as i64,
            minutes: (number("num_days") * 24.0 * 60.0).round() as i64,
            mean_score: Some(number("mean_score")).filter(|m| *m > 0.0),
            activity: Vec::new(),
        })
    }

    /// The whole anime list, following MAL's paging.
    pub async fn list(&self, token: &str) -> Result<Vec<MalEntry>> {
        let mut url = Some(format!("{API}/users/@me/animelist?fields=list_status&limit=1000&nsfw=true"));
        let mut entries = Vec::new();
        while let Some(page) = url.take() {
            let v = self.get(token, &page).await?;
            for item in v.get("data").and_then(Value::as_array).into_iter().flatten() {
                let (Some(id), Some(status)) = (item.pointer("/node/id").and_then(Value::as_i64), item.get("list_status")) else {
                    continue;
                };
                entries.push(MalEntry {
                    mal_id: id,
                    status: status.get("status").and_then(Value::as_str).unwrap_or("watching").to_owned(),
                    rewatching: status.get("is_rewatching").and_then(Value::as_bool).unwrap_or(false),
                    progress: status.get("num_episodes_watched").and_then(Value::as_i64).unwrap_or_default() as i32,
                    score: status.get("score").and_then(Value::as_f64).unwrap_or_default(),
                    updated_at: status.get("updated_at").and_then(Value::as_str).and_then(parse_rfc3339).unwrap_or_default(),
                });
            }
            url = v.pointer("/paging/next").and_then(Value::as_str).map(str::to_owned);
        }
        Ok(entries)
    }

    /// Takes a title off the list. MAL answers 404 when it wasn't on it, which is the same outcome.
    pub async fn delete(&self, token: &str, mal_id: i64) -> Result<()> {
        let response = self
            .http
            .client()
            .delete(format!("{API}/anime/{mal_id}/my_list_status"))
            .timeout(Duration::from_secs(20))
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await?;
        let status = response.status().as_u16();
        if status >= 400 && status != 404 {
            let bytes = response.bytes().await?;
            return Err(Error::Http { service: SERVICE, status, detail: crate::net::problem_detail(&bytes) });
        }
        Ok(())
    }

    /// Sets status and/or watched-episode count for one title.
    pub async fn update(&self, token: &str, mal_id: i64, status: Option<&str>, watched: Option<i32>) -> Result<()> {
        // Built to a String first: the serializer is not Send, so it must not live across an await.
        let body = {
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            if let Some(status) = status {
                form.append_pair("status", status);
            }
            if let Some(watched) = watched {
                form.append_pair("num_watched_episodes", &watched.to_string());
            }
            form.finish()
        };
        let response = self
            .http
            .client()
            .patch(format!("{API}/anime/{mal_id}/my_list_status"))
            .timeout(Duration::from_secs(20))
            .header("Authorization", format!("Bearer {token}"))
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await?;
        let status_code = response.status().as_u16();
        if status_code >= 400 {
            let bytes = response.bytes().await?;
            return Err(Error::Http { service: SERVICE, status: status_code, detail: crate::net::problem_detail(&bytes) });
        }
        Ok(())
    }
}

/// MAL list status → AniList `MediaListStatus` (the app's list vocabulary).
pub fn to_anilist_status(status: &str, rewatching: bool) -> &'static str {
    if rewatching {
        return "REPEATING";
    }
    match status {
        "completed" => "COMPLETED",
        "on_hold" => "PAUSED",
        "dropped" => "DROPPED",
        "plan_to_watch" => "PLANNING",
        _ => "CURRENT",
    }
}

/// Local library status → MAL list status.
pub fn from_local_status(status: &str) -> Option<&'static str> {
    Some(match status {
        "watching" => "watching",
        "planning" => "plan_to_watch",
        "completed" => "completed",
        "paused" => "on_hold",
        "dropped" => "dropped",
        _ => return None,
    })
}

/// `2026-10-04T12:34:56+00:00` → unix seconds (UTC offsets honoured), without a date crate.
fn parse_rfc3339(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>().ok());
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let time: String = rest.chars().take(8).collect();
    let mut t = time.split(':').map(|p| p.parse::<i64>().ok());
    let (h, min, s) = (t.next()??, t.next()??, t.next()?.unwrap_or(0));
    let offset = rest.get(8..).map(|tz| tz.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit())).unwrap_or("");
    let offset_secs = match offset.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let hm: Vec<i64> = offset[1..].split(':').filter_map(|p| p.parse().ok()).collect();
            let secs = hm.first().copied().unwrap_or(0) * 3600 + hm.get(1).copied().unwrap_or(0) * 60;
            if sign == '+' { secs } else { -secs }
        }
        _ => 0,
    };
    // Days from civil (Howard Hinnant).
    let (y, m) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + min * 60 + s - offset_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_both_ways() {
        assert_eq!(to_anilist_status("on_hold", false), "PAUSED");
        assert_eq!(to_anilist_status("completed", true), "REPEATING");
        assert_eq!(from_local_status("planning"), Some("plan_to_watch"));
    }

    #[test]
    fn rfc3339() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00+00:00"), Some(0));
        assert_eq!(parse_rfc3339("2025-10-04T00:00:00+00:00"), Some(1_759_536_000));
        assert_eq!(parse_rfc3339("2025-10-04T02:00:00+02:00"), Some(1_759_536_000));
    }

    #[test]
    fn pkce_verifier_length_in_range() {
        let v = random_token(32);
        assert!((43..=128).contains(&v.len()), "{}", v.len());
        assert!(authorize_url("s", &v).contains("code_challenge_method=plain"));
    }
}
