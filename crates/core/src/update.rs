//! Update channel: the Android app's signed Nostr manifest scheme, on a desktop d-tag.
//!
//! A release is a Nostr addressable event (kind 30078) signed by the project key, with
//! d-tag [`D_TAG`]. The same signed event is mirrored as a file so a check is one request. What is
//! fetched is never trusted as plain JSON: the event's id is recomputed (NIP-01) and its BIP-340
//! Schnorr signature verified against the baked-in [`PUBKEY_HEX`], so whoever controls the mirror
//! can withhold an update but never forge one. The installer itself is then checked against the
//! manifest's SHA-256 before it runs.
//!
//! Content: `{"version":"0.2.0","changelog":"…","windows":{"url","sizeBytes","sha256"},"macos":{…}}`.

use std::time::Duration;

use k256::schnorr::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::net::Http;
use crate::{Error, Result};

const SERVICE: &str = "Updates";
/// The project key shared with the Android update channel.
pub const PUBKEY_HEX: &str = "f1abd1ac685aed1f5145271283cd9418bf13aa92ed26399df191ae0c08b22207";
pub const KIND: u64 = 30078;
/// Distinct from Android's `anilili-update`, so a desktop never sees an APK and vice versa.
pub const D_TAG: &str = "anilili-desktop-update";
pub const MIRROR_URL: &str = "https://huggingface.co/pommieye/kitsune-7b/resolve/main/latest/desktop.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub version: String,
    pub changelog: String,
    pub url: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub published_at: i64,
}

#[derive(Debug, Deserialize)]
struct Event {
    id: String,
    pubkey: String,
    created_at: i64,
    kind: u64,
    tags: Vec<Vec<String>>,
    content: String,
    sig: String,
}

/// The newest signed release for this platform, if it is newer than `current`.
pub async fn check(http: &Http, current: &str) -> Result<Option<Update>> {
    let Some(body) = manifest(http, MIRROR_URL).await? else { return Ok(None) };
    let event: Event = serde_json::from_slice(&body).map_err(|e| Error::decode(SERVICE, e))?;
    verify(&event)?;
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "windows"
    };
    Ok(parse(&event, platform)?.filter(|update| is_newer(&update.version, current)))
}

/// A channel's mirrored manifest, or `None` while nothing has been published on it: the mirror
/// answers 404 until the first release, and "no release" is not a failure.
async fn manifest(http: &Http, url: &str) -> Result<Option<bytes::Bytes>> {
    match http.get_bytes(SERVICE, url, &[("Accept", "application/json")], Duration::from_secs(15)).await {
        Ok((body, _)) => Ok(Some(body)),
        Err(Error::Http { status: 404, .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

fn hex32(text: &str) -> Option<[u8; 32]> {
    let bytes = hex(text)?;
    bytes.try_into().ok()
}

fn hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).ok()).collect()
}

/// NIP-01 id: sha256 of the compact JSON `[0, pubkey, created_at, kind, tags, content]`.
fn event_id(event: &Event) -> [u8; 32] {
    let preimage = json!([0, event.pubkey, event.created_at, event.kind, event.tags, event.content]);
    Sha256::digest(serde_json::to_string(&preimage).unwrap_or_default().as_bytes()).into()
}

fn verify(event: &Event) -> Result<()> {
    verify_event(event, D_TAG)
}

fn verify_event(event: &Event, d_tag: &str) -> Result<()> {
    let reject = |why: &str| Err(Error::decode(SERVICE, format!("update manifest rejected: {why}")));
    if !event.pubkey.eq_ignore_ascii_case(PUBKEY_HEX) {
        return reject("wrong signer");
    }
    if event.kind != KIND {
        return reject("wrong kind");
    }
    if !event.tags.iter().any(|t| t.len() >= 2 && t[0] == "d" && t[1] == d_tag) {
        return reject("wrong channel");
    }
    let id = event_id(event);
    if hex32(&event.id) != Some(id) {
        return reject("id does not match content");
    }
    let key = hex32(PUBKEY_HEX).and_then(|k| VerifyingKey::from_bytes(&k).ok());
    let signature = hex(&event.sig).and_then(|s| Signature::try_from(s.as_slice()).ok());
    match (key, signature) {
        (Some(key), Some(signature)) if key.verify_raw(&id, &signature).is_ok() => Ok(()),
        _ => reject("bad signature"),
    }
}

/// The release for `platform` (`windows`, `macos` or `linux`), or `None` when this release has no build
/// for it: the other platform's update is no update here, not a failure.
fn parse(event: &Event, platform: &str) -> Result<Option<Update>> {
    let manifest: Value = serde_json::from_str(&event.content).map_err(|e| Error::decode(SERVICE, e))?;
    let Some(asset) = manifest.get(platform) else { return Ok(None) };
    let field = |key: &str| asset.get(key).and_then(Value::as_str).map(str::to_owned);
    Ok(Some(Update {
        version: manifest.get("version").and_then(Value::as_str).unwrap_or_default().to_owned(),
        changelog: manifest.get("changelog").and_then(Value::as_str).unwrap_or_default().to_owned(),
        url: field("url").ok_or_else(|| Error::decode(SERVICE, "release without a URL"))?,
        size_bytes: asset.get("sizeBytes").and_then(Value::as_u64).unwrap_or_default(),
        sha256: field("sha256").ok_or_else(|| Error::decode(SERVICE, "release without a checksum"))?.to_ascii_lowercase(),
        published_at: event.created_at,
    }))
}

/// `0.10.0` > `0.9.3`; pre-release suffixes are ignored.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let parts = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v').split(['-', '+']).next().unwrap_or_default().split('.').map(|p| p.parse().unwrap_or(0)).collect()
    };
    let (a, b) = (parts(candidate), parts(current));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    false
}

/// Downloads the installer to `path`, verifying its SHA-256 against the manifest.
pub async fn download_installer(http: &Http, update: &Update, path: &std::path::Path, on_progress: impl Fn(u64, u64)) -> Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let response = http.client().get(&update.url).timeout(Duration::from_secs(30 * 60)).send().await?;
    if !response.status().is_success() {
        return Err(Error::Http { service: SERVICE, status: response.status().as_u16(), detail: "installer download refused".into() });
    }
    let total = response.content_length().unwrap_or(update.size_bytes);
    let mut file = tokio::fs::File::create(path).await.map_err(|e| Error::Storage(e.to_string()))?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(|e| Error::Storage(e.to_string()))?;
        done += chunk.len() as u64;
        on_progress(done, total);
    }
    file.flush().await.map_err(|e| Error::Storage(e.to_string()))?;
    let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if digest != update.sha256 {
        let _ = tokio::fs::remove_file(path).await;
        return Err(Error::Other(format!("installer checksum mismatch ({digest})")));
    }
    Ok(())
}

/// Provider modules ride the same key on their own channel.
pub const PROVIDERS_D_TAG: &str = "anilili-desktop-providers";
pub const PROVIDERS_MIRROR_URL: &str = "https://huggingface.co/pommieye/kitsune-7b/resolve/main/latest/providers.json";

/// One provider module offered by the channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRelease {
    pub id: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
}

/// The provider modules the signed channel currently offers (`{"providers": [...]}`).
pub async fn check_providers(http: &Http) -> Result<Vec<ProviderRelease>> {
    let Some(body) = manifest(http, PROVIDERS_MIRROR_URL).await? else { return Ok(vec![]) };
    let event: Event = serde_json::from_slice(&body).map_err(|e| Error::decode(SERVICE, e))?;
    verify_event(&event, PROVIDERS_D_TAG)?;
    let manifest: Value = serde_json::from_str(&event.content).map_err(|e| Error::decode(SERVICE, e))?;
    let releases = manifest.get("providers").cloned().unwrap_or(Value::Array(vec![]));
    serde_json::from_value(releases).map_err(|e| Error::decode(SERVICE, e))
}

/// Downloads a module to `dir/<id>-<version>.wasm`, refusing it unless its SHA-256 matches.
pub async fn download_provider(http: &Http, release: &ProviderRelease, dir: &std::path::Path) -> Result<std::path::PathBuf> {
    let (bytes, _) = http.get_bytes(SERVICE, &release.url, &[], Duration::from_secs(60)).await?;
    let digest: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    if digest != release.sha256.to_ascii_lowercase() {
        return Err(Error::Other(format!("{} module checksum mismatch", release.id)));
    }
    let safe = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).collect::<String>();
    std::fs::create_dir_all(dir).map_err(|e| Error::Storage(e.to_string()))?;
    let path = dir.join(format!("{}-{}.wasm", safe(&release.id), safe(&release.version)));
    std::fs::write(&path, &bytes).map_err(|e| Error::Storage(e.to_string()))?;
    Ok(path)
}

/// Verifies any event from the project key on a given channel (checks a live event in tooling,
/// e.g. the Android channel's, with the exact code the app uses).
pub fn verify_channel(event_json: &[u8], d_tag: &str) -> Result<()> {
    let event: Event = serde_json::from_slice(event_json).map_err(|e| Error::decode(SERVICE, e))?;
    verify_event(&event, d_tag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_order() {
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("0.10.0", "0.9.3"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-beta", "0.1.0"));
        assert!(is_newer("v1.0", "0.99.99"));
    }

    #[test]
    fn linux_release_is_separate() {
        // A Windows/macOS release offers Linux nothing (not an error), and a Linux
        // asset is picked only when asked for as Linux.
        let event = Event {
            id: String::new(),
            pubkey: String::new(),
            created_at: 1_760_000_000,
            kind: KIND,
            tags: vec![vec!["d".into(), D_TAG.into()]],
            content: r#"{"version":"0.2.0","windows":{"url":"https://x/a.exe","sizeBytes":5,"sha256":"AB"},"linux":{"url":"https://x/a.deb","sizeBytes":6,"sha256":"CD"}}"#.into(),
            sig: String::new(),
        };
        let linux = parse(&event, "linux").unwrap().unwrap();
        assert_eq!(linux.url, "https://x/a.deb");
        assert_eq!(linux.sha256, "cd");
        assert_eq!(parse(&event, "windows").unwrap().unwrap().url, "https://x/a.exe");
        let win_only = Event {
            content: r#"{"version":"0.2.0","windows":{"url":"https://x/a.exe","sizeBytes":5,"sha256":"AB"}}"#.into(),
            ..Event { id: String::new(), pubkey: String::new(), created_at: 0, kind: KIND, tags: vec![], content: String::new(), sig: String::new() }
        };
        assert!(parse(&win_only, "linux").unwrap().is_none());
    }

    #[test]
    fn signed_event_round_trip() {
        use k256::schnorr::SigningKey;
        use k256::schnorr::signature::hazmat::PrehashSigner;
        // A throwaway key: proves id + signature verification, with the pubkey check bypassed.
        let signing = SigningKey::from_bytes(&[7u8; 32]).unwrap();
        let pubkey: String = signing.verifying_key().to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let mut event = Event {
            id: String::new(),
            pubkey: pubkey.clone(),
            created_at: 1_760_000_000,
            kind: KIND,
            tags: vec![vec!["d".into(), D_TAG.into()]],
            content: r#"{"version":"0.2.0","windows":{"url":"https://x/a.exe","sizeBytes":5,"sha256":"AB"}}"#.into(),
            sig: String::new(),
        };
        let id = event_id(&event);
        event.id = id.iter().map(|b| format!("{b:02x}")).collect();
        let sig: Signature = signing.sign_prehash(&id).unwrap();
        event.sig = sig.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        let key = VerifyingKey::from_bytes(&hex32(&pubkey).unwrap()).unwrap();
        assert!(key.verify_raw(&id, &sig).is_ok());
        // The real check refuses it: it is not the project key.
        assert!(verify(&event).is_err());
        let update = parse(&event, "windows").unwrap().unwrap();
        assert_eq!(update.sha256, "ab");
        assert_eq!(update.version, "0.2.0");
        // A Windows-only release offers a Mac nothing, and says so without an error.
        assert!(parse(&event, "macos").unwrap().is_none());
    }
}
