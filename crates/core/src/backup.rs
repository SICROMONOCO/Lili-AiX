//! The backup file both apps read and write: `anilili-backup`, version 1, defined by the Android
//! app (`data/backup/AppBackup.kt`, `BackupCrypto.kt`). A file written here restores there, and the
//! other way round.
//!
//! The document is readable JSON: settings (by Android's preference key names), the local saved
//! list (`watchlist`), Continue Watching (`history`), search history and custom lists. Every field
//! has a default, so an older or newer writer's file still reads; unknown fields are ignored on
//! both sides, which is how the desktop carries what Android has no place for (a library status
//! per saved title, its own settings under `desktop.` keys) without breaking Android's reader.
//!
//! Logins travel only when the viewer sets a passphrase, sealed as Android seals them:
//! PBKDF2-HMAC-SHA256 (the iteration count is in the file; 210,000 when written here) to a 32-byte
//! key, then AES-256-GCM with a 12-byte IV, the document's `format/version` as associated data so
//! a sealed blob can't be moved into another file. Only the service names sit outside the seal.
//! MyAnimeList travels as a one-time authorization code minted for whoever restores the file, not
//! as tokens: MAL rotates refresh tokens, so two devices sharing one pair log each other out.

use std::collections::BTreeMap;

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Sha256;

use crate::catalogue as c;
use crate::error::Failure;

pub const FORMAT: &str = "anilili-backup";
/// Bumped only when an older reader could *misread* a newer file; adding a field doesn't count.
pub const VERSION: u32 = 1;
pub const KDF: &str = "pbkdf2-hmac-sha256";
pub const ITERATIONS: u32 = 210_000;
pub const MIN_PASSPHRASE_LENGTH: usize = 8;
/// A real backup is tens of kilobytes; this keeps the wrong file from reaching the parser.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;
const SALT_BYTES: usize = 16;
const IV_BYTES: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Document {
    pub format: String,
    pub version: u32,
    /// Unix milliseconds.
    pub created_at: i64,
    pub app_version: String,
    /// Preference key to value (Android's names; the desktop's own under `desktop.`).
    pub settings: serde_json::Map<String, Value>,
    pub watchlist: Vec<WatchlistEntry>,
    pub history: Vec<HistoryEntry>,
    pub search_history: Vec<String>,
    pub custom_lists: Vec<CustomList>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<SealedCredentials>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            created_at: 0,
            app_version: String::new(),
            settings: serde_json::Map::new(),
            watchlist: vec![],
            history: vec![],
            search_history: vec![],
            custom_lists: vec![],
            credentials: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WatchlistEntry {
    pub anilist_id: i64,
    pub title: String,
    pub cover: Option<String>,
    pub format: Option<String>,
    pub average_score: Option<i32>,
    /// Unix milliseconds.
    pub added_at: i64,
    /// The desktop library's status (`watching`, `planning`…). Android has none and ignores it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EpisodeResume {
    pub position_ms: i64,
    pub duration_ms: i64,
}

/// One Continue Watching row per title: the episode in progress, the episodes finished, and where
/// each part-watched episode was left (keyed `"7"`, or `"7.5"` for a special).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HistoryEntry {
    pub anilist_id: i64,
    pub title: String,
    pub cover: Option<String>,
    pub episode_number: f64,
    pub episode_title: Option<String>,
    /// The server it was played from; Android reads it as a preference, so empty means "any".
    pub provider: String,
    /// `sub` or `dub`.
    pub category: String,
    pub position_ms: i64,
    pub duration_ms: i64,
    /// Unix milliseconds.
    pub updated_at: i64,
    pub watched_episodes: Vec<f64>,
    /// Seeded from an account rather than played here; never written into a backup.
    pub from_remote: bool,
    pub resume_points: BTreeMap<String, EpisodeResume>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CustomList {
    pub name: String,
    pub entries: Vec<WatchlistEntry>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SealedCredentials {
    pub kdf: String,
    pub iterations: u32,
    pub salt: String,
    pub iv: String,
    pub ciphertext: String,
    /// Service names only (`anilist`, `mal`, `opensubtitles`), so a restore can say what it holds.
    pub services: Vec<String>,
}

impl Default for SealedCredentials {
    fn default() -> Self {
        Self {
            kdf: KDF.into(),
            iterations: ITERATIONS,
            salt: String::new(),
            iv: String::new(),
            ciphertext: String::new(),
            services: vec![],
        }
    }
}

/// What is inside the seal.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Credentials {
    #[serde(rename = "aniListToken")]
    pub anilist_token: Option<String>,
    pub open_subtitles_username: Option<String>,
    pub open_subtitles_password: Option<String>,
    /// A MyAnimeList authorization code minted for whoever restores the file: single use, short-lived.
    pub mal_code: Option<String>,
    /// The PKCE `plain` verifier the code is bound to.
    pub mal_verifier: Option<String>,
    /// Unix milliseconds the code was obtained.
    pub mal_issued_at: i64,
}

impl Credentials {
    pub fn has_anilist(&self) -> bool {
        self.anilist_token.as_deref().is_some_and(|t| !t.is_empty())
    }

    pub fn has_mal_handoff(&self) -> bool {
        self.mal_code.as_deref().is_some_and(|c| !c.is_empty()) && self.mal_verifier.as_deref().is_some_and(|v| !v.is_empty())
    }

    pub fn has_open_subtitles(&self) -> bool {
        self.open_subtitles_username.as_deref().is_some_and(|u| !u.is_empty())
            && self.open_subtitles_password.as_deref().is_some_and(|p| !p.is_empty())
    }

    pub fn services(&self) -> Vec<String> {
        [(self.has_anilist(), "anilist"), (self.has_mal_handoff(), "mal"), (self.has_open_subtitles(), "opensubtitles")]
            .into_iter()
            .filter(|(present, _)| *present)
            .map(|(_, name)| name.to_owned())
            .collect()
    }
}

impl Document {
    /// "31 settings · 12 saved · 8 continue watching", as Android describes a file.
    pub fn describe(&self) -> String {
        describe_counts(&[
            (self.settings.len(), "setting", "settings"),
            (self.watchlist.len(), "saved", "saved"),
            (self.custom_lists.len(), "list", "lists"),
            (self.history.len(), "continue watching", "continue watching"),
            (self.search_history.len(), "search", "searches"),
            (self.credentials.as_ref().map_or(0, |c| c.services.len()), "login", "logins"),
        ])
    }

    fn associated_data(&self) -> Vec<u8> {
        format!("{}/{}", self.format, self.version).into_bytes()
    }

    /// Seals `credentials` into the document under `passphrase` (nothing is sealed when empty).
    pub fn seal(&mut self, passphrase: &str, credentials: &Credentials) -> Result<(), Failure> {
        let services = credentials.services();
        if services.is_empty() {
            self.credentials = None;
            return Ok(());
        }
        if passphrase.chars().count() < MIN_PASSPHRASE_LENGTH {
            return Err(Failure::new(c::BAK_WRITE, format!("the passphrase must be at least {MIN_PASSPHRASE_LENGTH} characters")));
        }
        let mut salt = [0u8; SALT_BYTES];
        let mut iv = [0u8; IV_BYTES];
        rand::fill(&mut salt);
        rand::fill(&mut iv);
        let key = derive_key(passphrase, &salt, ITERATIONS);
        let plaintext = serde_json::to_vec(credentials).map_err(|e| Failure::new(c::BAK_WRITE, e.to_string()))?;
        let aad = self.associated_data();
        let ciphertext = Aes256Gcm::new(&key.into())
            .encrypt(Nonce::from_slice(&iv), Payload { msg: &plaintext, aad: &aad })
            .map_err(|_| Failure::new(c::BAK_WRITE, "sealing the logins failed"))?;
        self.credentials = Some(SealedCredentials {
            kdf: KDF.into(),
            iterations: ITERATIONS,
            salt: BASE64.encode(salt),
            iv: BASE64.encode(iv),
            ciphertext: BASE64.encode(ciphertext),
            services,
        });
        Ok(())
    }

    /// The logins in the file, or `None` when it carries none.
    pub fn open(&self, passphrase: &str) -> Result<Option<Credentials>, Failure> {
        let Some(sealed) = &self.credentials else { return Ok(None) };
        let damaged = || Failure::new(c::BAK_UNREADABLE, "the logins in that backup are damaged");
        if sealed.kdf != KDF {
            return Err(Failure::new(c::BAK_NEWER, format!("logins sealed with {}", sealed.kdf)));
        }
        let salt = BASE64.decode(&sealed.salt).map_err(|_| damaged())?;
        let iv = BASE64.decode(&sealed.iv).map_err(|_| damaged())?;
        let ciphertext = BASE64.decode(&sealed.ciphertext).map_err(|_| damaged())?;
        if iv.len() != IV_BYTES || sealed.iterations == 0 {
            return Err(damaged());
        }
        if passphrase.is_empty() {
            return Err(Failure::new(c::BAK_PASSPHRASE, "no passphrase given"));
        }
        let key = derive_key(passphrase, &salt, sealed.iterations);
        let aad = self.associated_data();
        // A wrong passphrase and a damaged file look the same to GCM; the passphrase is likelier.
        let plaintext = Aes256Gcm::new(&key.into())
            .decrypt(Nonce::from_slice(&iv), Payload { msg: &ciphertext, aad: &aad })
            .map_err(|_| Failure::new(c::BAK_PASSPHRASE, "the authentication tag did not match"))?;
        serde_json::from_slice(&plaintext).map(Some).map_err(|_| damaged())
    }
}

/// Parses a backup, refusing anything that isn't one (the usual wrong pick is a MAL export).
pub fn decode(bytes: &[u8]) -> Result<Document, Failure> {
    if bytes.len() > MAX_BYTES {
        return Err(Failure::new(c::BAK_UNREADABLE, format!("{} bytes is far larger than any backup", bytes.len())));
    }
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return Err(Failure::new(c::BAK_UNREADABLE, "the chosen file is empty"));
    }
    let not_json = |e: serde_json::Error| Failure::new(c::BAK_UNREADABLE, format!("not the JSON a backup is written as: {e}"));
    let value: Value = serde_json::from_slice(bytes).map_err(not_json)?;
    // Read from the file itself: the field defaults to FORMAT, so any JSON object would pass.
    // Both apps always write it.
    match value.get("format").and_then(Value::as_str) {
        Some(FORMAT) => {}
        Some(other) => return Err(Failure::new(c::BAK_UNREADABLE, format!("the file says it is \"{other}\", not {FORMAT}"))),
        None => return Err(Failure::new(c::BAK_UNREADABLE, "the file doesn't say it is a backup")),
    }
    let document: Document = serde_json::from_value(value).map_err(not_json)?;
    if document.version > VERSION {
        return Err(Failure::new(c::BAK_NEWER, format!("format v{}", document.version)));
    }
    Ok(document)
}

/// Indented, as Android writes it: people keep these files and sometimes open them.
pub fn encode(document: &Document) -> String {
    serde_json::to_string_pretty(document).unwrap_or_default()
}

/// `K7M4-9QXR`: Crockford-style (no I, L, O, U), typeable on a TV remote, as Android offers.
pub fn suggest_passphrase() -> String {
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTVWXYZ";
    let group = || (0..4).map(|_| ALPHABET[rand::random_range(0..ALPHABET.len())] as char).collect::<String>();
    format!("{}-{}", group(), group())
}

/// PBKDF2 (RFC 8018) with HMAC-SHA256, one 32-byte block.
pub fn derive_key(passphrase: &str, salt: &[u8], iterations: u32) -> [u8; 32] {
    let mac = || <Hmac<Sha256> as Mac>::new_from_slice(passphrase.as_bytes()).expect("HMAC takes any key length");
    let mut u = {
        let mut m = mac();
        m.update(salt);
        m.update(&1u32.to_be_bytes());
        m.finalize().into_bytes()
    };
    let mut out: [u8; 32] = u.into();
    for _ in 1..iterations {
        let mut m = mac();
        m.update(&u);
        u = m.finalize().into_bytes();
        for (o, b) in out.iter_mut().zip(u.iter()) {
            *o ^= b;
        }
    }
    out
}

/// `"7"` for a whole episode, `"7.5"` for a special: Android's resume-point keys.
pub fn episode_key(number: f64) -> String {
    if number.fract() == 0.0 { format!("{}", number as i64) } else { number.to_string() }
}

pub fn describe_counts(parts: &[(usize, &str, &str)]) -> String {
    let text: Vec<String> =
        parts.iter().filter(|(n, _, _)| *n > 0).map(|(n, one, many)| format!("{n} {}", if *n == 1 { one } else { many })).collect();
    if text.is_empty() { "nothing".into() } else { text.join(" · ") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn derives_keys_as_rfc_7914_lists() {
        // PBKDF2-HMAC-SHA256 test vectors (RFC 7914 §11 and the widely used RFC 6070 analogues).
        assert_eq!(hex(&derive_key("password", b"salt", 1)), "120fb6cffcf8b32c43e7225256c4f837a86548c92ccc35480805987cb70be17b");
        assert_eq!(hex(&derive_key("password", b"salt", 2)), "ae4d0c95af6b46d32d0adff928f06dd02a303f8ef3c251dfd6e2d85a95474c43");
        assert_eq!(hex(&derive_key("password", b"salt", 4096)), "c5e478d59288c841aa530db6845c4c8d962893a001ce4e11a4963873aa98134a");
    }

    #[test]
    fn seals_and_opens_logins() {
        let mut document = Document { created_at: 1, ..Document::default() };
        let credentials = Credentials { anilist_token: Some("tok".into()), ..Credentials::default() };
        document.seal("K7M4-9QXR", &credentials).unwrap();
        let sealed = document.credentials.as_ref().unwrap();
        assert_eq!(sealed.services, ["anilist"]);
        assert!(!encode(&document).contains("tok\""), "the token never appears in the clear");
        // Round trip through the file text, as a restore would.
        let read = decode(encode(&document).as_bytes()).unwrap();
        assert_eq!(read.open("K7M4-9QXR").unwrap(), Some(credentials));
        assert_eq!(read.open("WRONG-PASS").unwrap_err().entry, c::BAK_PASSPHRASE);
        // Bound to the header: the same blob under another version doesn't open.
        let mut moved = read.clone();
        moved.version = 2;
        assert!(moved.open("K7M4-9QXR").is_err());
        assert!(document.clone().seal("short", &Credentials { anilist_token: Some("t".into()), ..Default::default() }).is_err());
    }

    #[test]
    fn opens_logins_sealed_elsewhere() {
        // Sealed independently, so this checks the scheme, not just a round trip; the case is
        // shared with Android (fixtures/parity/backup-sealed.json).
        let case: Value = serde_json::from_str(include_str!("../../../fixtures/parity/backup-sealed.json")).unwrap();
        let text = case["file"].to_string();
        let opened = decode(text.as_bytes()).unwrap().open(case["passphrase"].as_str().unwrap()).unwrap().unwrap();
        assert_eq!(opened.anilist_token.as_deref(), case["expect"]["anilistToken"].as_str());
        assert_eq!(opened.has_mal_handoff(), case["expect"]["malHandoff"].as_bool().unwrap());
        assert_eq!(opened.mal_issued_at, case["expect"]["malIssuedAt"].as_i64().unwrap());
    }

    #[test]
    fn reads_an_android_file() {
        // Shaped like AppBackup.encode's output, with fields this reader doesn't know.
        let text = r#"{
            "format": "anilili-backup", "version": 1, "createdAt": 1790000000000, "appVersion": "2.9.0",
            "settings": {"autoplay": true, "preferred_subtitle_language": "eng", "theme_accent": "blue"},
            "watchlist": [{"anilistId": 154587, "title": "Frieren", "cover": null, "format": "TV", "averageScore": 90, "addedAt": 5}],
            "history": [{"anilistId": 21, "title": "ONE PIECE", "cover": "c", "episodeNumber": 1100.0, "provider": "bee",
                         "category": "sub", "positionMs": 60000, "durationMs": 1440000, "updatedAt": 9,
                         "watchedEpisodes": [1098.0, 1099.0], "fromRemote": false,
                         "resumePoints": {"1097": {"positionMs": 5000, "durationMs": 1440000}}, "somethingNew": 1}],
            "searchHistory": ["frieren"], "customLists": [{"name": "Faves", "entries": [], "createdAt": 3}]
        }"#;
        let document = decode(text.as_bytes()).unwrap();
        assert_eq!(document.watchlist[0].anilist_id, 154587);
        assert_eq!(document.history[0].watched_episodes, [1098.0, 1099.0]);
        assert_eq!(document.history[0].resume_points["1097"].position_ms, 5000);
        assert_eq!(document.describe(), "3 settings · 1 saved · 1 list · 1 continue watching · 1 search");
        assert!(document.open("anything").unwrap().is_none());
    }

    #[test]
    fn refuses_what_is_not_a_backup() {
        assert_eq!(decode(b"").unwrap_err().entry, c::BAK_UNREADABLE);
        assert_eq!(decode(br#"{"format":"mal-export"}"#).unwrap_err().entry, c::BAK_UNREADABLE);
        assert_eq!(decode(br#"{"hello":"world"}"#).unwrap_err().entry, c::BAK_UNREADABLE);
        assert_eq!(decode(b"[]").unwrap_err().entry, c::BAK_UNREADABLE);
        assert_eq!(decode(br#"{"format":"anilili-backup","version":9}"#).unwrap_err().entry, c::BAK_NEWER);
        assert_eq!(decode(b"<xml/>").unwrap_err().entry, c::BAK_UNREADABLE);
        assert_eq!(episode_key(7.0), "7");
        assert_eq!(episode_key(7.5), "7.5");
        let pass = suggest_passphrase();
        assert_eq!(pass.len(), 9);
        assert!(!pass.contains(['I', 'L', 'O', 'U', '0', '1']));
    }
}
