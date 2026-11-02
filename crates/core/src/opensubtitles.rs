//! OpenSubtitles' REST API (Android's `OpenSubtitlesClient.kt`; keep the two in step): sign in,
//! search a title's episode in a few languages, and turn a file into a download link the player
//! can load.
//!
//! The consumer key identifies the app and is built in (`build.rs` reads it from the environment
//! or the private Gradle properties Android's build uses); a build without one has OpenSubtitles
//! switched off. Searching needs only the key; downloading needs a signed-in account, whose token
//! lasts a day and comes with no refresh token, so the app signs in again with the saved
//! credentials when it lapses. The AI translation and transcription endpoints are left alone.

use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

use crate::net::Http;
use crate::{Error, Result};

const SERVICE: &str = "OpenSubtitles";
const API: &str = "https://api.opensubtitles.com/api/v1";
const TIMEOUT: Duration = Duration::from_secs(20);
/// The API caps a search at three languages.
const MAX_LANGUAGES: usize = 3;

/// One subtitle file a search found.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleFile {
    pub file_id: i64,
    /// ISO 639-1, sometimes with a region (`pt-br`).
    pub language: String,
    pub release: String,
    pub file_name: String,
    pub downloads: i64,
    pub hearing_impaired: bool,
    /// Uploaded by someone OpenSubtitles trusts.
    pub trusted: bool,
    /// Translated by a machine rather than a person.
    pub machine_translated: bool,
}

/// A download link for one file, valid for a few hours.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubtitleLink {
    pub url: String,
    pub file_name: String,
    /// Downloads left today on the account, when the API says.
    pub remaining: Option<i64>,
}

#[derive(Clone)]
pub struct OpenSubtitles {
    http: Http,
    key: &'static str,
    user_agent: String,
}

impl OpenSubtitles {
    /// `key` empty: not configured, and every call fails with that said. `user_agent` must name
    /// the app and its version (`Anilili Desktop v0.1.1`): the API refuses anonymous clients.
    pub fn new(http: Http, key: &'static str, user_agent: String) -> Self {
        Self { http, key, user_agent }
    }

    pub fn configured(&self) -> bool {
        !self.key.is_empty()
    }

    /// Signs in; the token lasts about a day.
    pub async fn login(&self, username: &str, password: &str) -> Result<String> {
        let body = json!({ "username": username.trim(), "password": password });
        let root = self.send(self.request("POST", "/login", None).json(&body)).await?;
        root.get("token")
            .and_then(Value::as_str)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| Error::Decode { service: SERVICE, detail: "no token in the login answer".into() })
    }

    /// Files for a title's episode in `languages` (ISO 639-1, at most three are asked for), the
    /// most downloaded first.
    pub async fn search(&self, query: &str, languages: &[String], season: Option<u32>, episode: Option<u32>) -> Result<Vec<SubtitleFile>> {
        let languages = normalize_languages(languages);
        let mut params: Vec<(&str, String)> = vec![
            ("query", query.trim().to_lowercase()),
            ("order_by", "download_count".into()),
            ("order_direction", "desc".into()),
        ];
        if !languages.is_empty() {
            params.push(("languages", languages));
        }
        if let Some(season) = season.filter(|s| *s > 0) {
            params.push(("season_number", season.to_string()));
        }
        if let Some(episode) = episode.filter(|e| *e > 0) {
            params.push(("episode_number", episode.to_string()));
        }
        let query = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(&params).finish();
        let root = self.send(self.request("GET", &format!("/subtitles?{query}"), None)).await?;
        Ok(parse_search(&root))
    }

    /// A link for one file. Needs the account's token; it counts against its daily allowance.
    pub async fn download(&self, token: &str, file_id: i64) -> Result<SubtitleLink> {
        let root = self.send(self.request("POST", "/download", Some(token)).json(&json!({ "file_id": file_id }))).await?;
        let url = root
            .get("link")
            .and_then(Value::as_str)
            .filter(|u| !u.is_empty())
            .ok_or_else(|| Error::Decode { service: SERVICE, detail: "no link in the download answer".into() })?;
        Ok(SubtitleLink {
            url: url.to_owned(),
            file_name: root.get("file_name").and_then(Value::as_str).unwrap_or_default().to_owned(),
            remaining: root.get("remaining").and_then(Value::as_i64),
        })
    }

    fn request(&self, method: &str, path: &str, token: Option<&str>) -> wreq::RequestBuilder {
        let url = format!("{API}{path}");
        let client = self.http.client();
        let mut request = match method {
            "POST" => client.post(url),
            _ => client.get(url),
        }
        .timeout(TIMEOUT)
        .header("Accept", "application/json")
        .header("Api-Key", self.key)
        .header("User-Agent", &self.user_agent);
        if let Some(token) = token {
            request = request.header("Authorization", format!("Bearer {token}"));
        }
        request
    }

    async fn send(&self, request: wreq::RequestBuilder) -> Result<Value> {
        if !self.configured() {
            return Err(Error::Other("OpenSubtitles isn't configured in this build".into()));
        }
        let response = request.send().await?;
        let status = response.status().as_u16();
        let body = response.bytes().await?;
        let root: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if !(200..300).contains(&status) {
            // The API says what went wrong in `message` (a spent allowance, a wrong password).
            let detail = root
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| crate::net::problem_detail(&body));
            return Err(Error::Http { service: SERVICE, status, detail });
        }
        if root.is_null() {
            return Err(Error::Decode { service: SERVICE, detail: crate::net::problem_detail(&body) });
        }
        Ok(root)
    }
}

/// Lowercase, de-duplicated, at most three, comma-joined: what the API accepts.
fn normalize_languages(languages: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for language in languages {
        let language = language.trim().to_lowercase();
        if !language.is_empty() && !out.contains(&language) {
            out.push(language);
        }
    }
    out.truncate(MAX_LANGUAGES);
    out.join(",")
}

/// A search answer's files: one entry per file, each carrying its subtitle's details.
fn parse_search(root: &Value) -> Vec<SubtitleFile> {
    let Some(items) = root.get("data").and_then(Value::as_array) else { return vec![] };
    let mut files = Vec::new();
    for item in items {
        let Some(a) = item.get("attributes") else { continue };
        let text = |key: &str| a.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
        let flag = |key: &str| a.get(key).and_then(Value::as_bool).unwrap_or(false);
        let release = Some(text("release"))
            .filter(|r| !r.is_empty())
            .or_else(|| a.pointer("/feature_details/title").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| SERVICE.into());
        for file in a.get("files").and_then(Value::as_array).into_iter().flatten() {
            let Some(file_id) = file.get("file_id").and_then(Value::as_i64) else { continue };
            files.push(SubtitleFile {
                file_id,
                language: text("language"),
                release: release.clone(),
                file_name: file.get("file_name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                downloads: a.get("download_count").and_then(Value::as_i64).unwrap_or(0),
                hearing_impaired: flag("hearing_impaired"),
                trusted: flag("from_trusted"),
                machine_translated: flag("machine_translated") || flag("ai_translated"),
            });
        }
    }
    files
}

/// What an `Error` from this module means for the viewer: a spent daily allowance, a refused
/// sign-in, or anything else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    SignedOut,
    Allowance,
    Other,
}

pub fn refusal(error: &Error) -> Refusal {
    match error {
        Error::Http { service: SERVICE, status: 401, .. } => Refusal::SignedOut,
        // 406 is the API's "you have downloaded your allowed subtitles for today".
        Error::Http { service: SERVICE, status: 406, .. } => Refusal::Allowance,
        Error::Http { service: SERVICE, detail, .. } if detail.to_lowercase().contains("allowed") => Refusal::Allowance,
        _ => Refusal::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_search_answer_one_entry_per_file() {
        let root = json!({ "data": [
            { "attributes": {
                "language": "en", "release": "[Sub] Show - 03 [1080p]", "download_count": 1200,
                "hearing_impaired": false, "from_trusted": true, "machine_translated": false,
                "files": [ { "file_id": 11, "file_name": "show-03.srt" }, { "file_id": 12, "file_name": "show-03.ass" } ]
            } },
            { "attributes": {
                "language": "es", "release": "", "feature_details": { "title": "Show" },
                "ai_translated": true, "files": [ { "file_id": 21 } ]
            } },
            { "attributes": { "language": "fr", "files": [ { "file_name": "no id" } ] } }
        ] });
        let files = parse_search(&root);
        assert_eq!(files.len(), 3);
        assert_eq!((files[0].file_id, files[0].trusted, files[0].downloads), (11, true, 1200));
        assert_eq!(files[1].file_name, "show-03.ass");
        assert_eq!(files[2].release, "Show");
        assert!(files[2].machine_translated);
    }

    #[test]
    fn languages_are_lowercase_unique_and_three_at_most() {
        let langs = ["EN", "en", "es", "pt-BR", "fr"].map(String::from);
        assert_eq!(normalize_languages(&langs), "en,es,pt-br");
    }

    #[test]
    fn refusals() {
        let http = |status, detail: &str| Error::Http { service: SERVICE, status, detail: detail.into() };
        assert_eq!(refusal(&http(401, "")), Refusal::SignedOut);
        assert_eq!(refusal(&http(406, "")), Refusal::Allowance);
        assert_eq!(refusal(&http(403, "You have downloaded your allowed 20 subtitles")), Refusal::Allowance);
        assert_eq!(refusal(&http(500, "")), Refusal::Other);
    }
}
