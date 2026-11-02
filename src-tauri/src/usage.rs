//! The once-a-day usage ping every Anilili app sends (`docs/USAGE-PING.md`, version 2): a device
//! ID, platform, version, which list services are signed in and a hash of each account's ID (so one
//! person's devices can be linked), the computer (system, maker, model, screen, language), the
//! main settings, and how long the app was open and watched since the last accepted ping. The
//! server adds the country and city. Always on: Settings → App → Privacy says what is sent.
//!
//! The ID is a salted hash of the operating system's machine ID, which outlives the app, so a
//! reinstall is recognised as the same computer. The day of the last accepted ping and the usage
//! counters are kept in the store beside the settings but outside them, so a backup never carries
//! them: restored onto another computer, they would skip a day, miss `first` and report another
//! computer's minutes. Release builds only.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use anilili_core::model::Audio;
use anilili_core::store::{Store, now};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime};

use crate::app::AppState;
use crate::catalog::civil_date;

const ENDPOINT: &str = "https://pulse.anilili.workers.dev/v1/ping";
const DAY_KEY: &str = "usage_ping_day";
/// The minutes and sessions counted since the last accepted ping.
const COUNTERS_KEY: &str = "usage_counters";
/// A random stand-in for a machine ID that can't be read; a reinstall that clears app data loses it.
const FALLBACK_KEY: &str = "usage_device_fallback";
const SALT: &str = "anilili-usage-v1:";
const PERSON_SALT: &str = "anilili-person-v1:";
/// Away this long, coming back is a new session.
const SESSION_GAP: Duration = Duration::from_secs(5 * 60);

/// The operating system this build is for.
const PLATFORM: &str = if cfg!(target_os = "macos") {
    "macos"
} else if cfg!(target_os = "linux") {
    "linux"
} else {
    "windows"
};

/// Tries shortly after launch, then hourly so a day change is noticed in a long session; counts
/// the minutes once a minute.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    if cfg!(debug_assertions) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            check(&handle).await;
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut away_since: Option<Instant> = None;
        let mut first = true;
        loop {
            tick(&handle, &mut away_since, &mut first);
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

// ---- the counters ----

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
struct Counters {
    minutes: u32,
    watched: u32,
    sessions: u32,
}

impl Counters {
    /// What a minute adds: in front (the window focused and not minimized) or playing counts as in
    /// the app; playing is also watched; coming back after `SESSION_GAP` away, or the first
    /// minute in front, is a new session.
    fn count(&mut self, in_front: bool, playing: bool, new_session: bool) {
        if in_front || playing {
            self.minutes = (self.minutes + 1).min(2880);
        }
        if playing {
            self.watched = (self.watched + 1).min(2880);
        }
        if new_session {
            self.sessions = (self.sessions + 1).min(500);
        }
    }

    /// What is left after `sent` was accepted: whatever was counted while it was on its way.
    fn minus(self, sent: Counters) -> Counters {
        Counters {
            minutes: self.minutes.saturating_sub(sent.minutes),
            watched: self.watched.saturating_sub(sent.watched),
            sessions: self.sessions.saturating_sub(sent.sessions),
        }
    }
}

/// The counters live in the store; this lock makes each read-modify-write whole.
static COUNTING: Mutex<()> = Mutex::new(());

fn update_counters(store: &Store, change: impl FnOnce(Counters) -> Counters) {
    let _guard = COUNTING.lock().unwrap_or_else(|e| e.into_inner());
    let before: Counters = store.setting(COUNTERS_KEY).unwrap_or_default();
    let after = change(before);
    if after != before {
        let _ = store.set_setting(COUNTERS_KEY, &after);
    }
}

fn tick<R: Runtime>(app: &AppHandle<R>, away_since: &mut Option<Instant>, first: &mut bool) {
    let in_front = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_focused().unwrap_or(false) && !w.is_minimized().unwrap_or(false));
    let playing = app.try_state::<crate::player::Player>().is_some_and(|p| p.is_playing());
    let new_session = in_front && (*first || away_since.is_some_and(|t| t.elapsed() >= SESSION_GAP));
    if in_front {
        *first = false;
        *away_since = None;
    } else if away_since.is_none() {
        *away_since = Some(Instant::now());
    }
    if in_front || playing || new_session {
        update_counters(&app.state::<AppState>().store, |mut c| {
            c.count(in_front, playing, new_session);
            c
        });
    }
}

// ---- the ping ----

async fn check<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let today = utc_day(now());
    let last: Option<String> = state.store.setting(DAY_KEY);
    if last.as_deref() == Some(today.as_str()) {
        return;
    }
    let accounts = accounts(
        state.anilist.has_token(),
        state.mal_tokens.lock().await.is_some(),
        state.simkl_tokens.lock().await.is_some(),
    );
    let version = app.package_info().version.to_string();
    let device = device_id(&raw_device_id(&state.store));
    let counters: Counters = state.store.setting(COUNTERS_KEY).unwrap_or_default();
    let more = Details {
        people: people(app).await,
        env: environment(app),
        settings: settings(&state.settings()),
        usage: counters,
    };
    let Some(body) = body(&device, PLATFORM, &version, &accounts, last.as_deref(), &today, &more) else { return };
    let sent = state
        .http
        .client()
        .post(ENDPOINT)
        .timeout(Duration::from_secs(20))
        .header("Content-Type", "application/json")
        .body(body.to_string())
        .send()
        .await;
    match sent {
        Ok(response) if response.status().is_success() => {
            let _ = state.store.set_setting(DAY_KEY, &today);
            update_counters(&state.store, |now| now.minus(counters));
        }
        Ok(response) => tracing::warn!(status = response.status().as_u16(), "usage ping refused"),
        Err(e) => tracing::info!(error = %e, "usage ping failed; next hour"),
    }
}

/// Version 2's fields beyond version 1's.
struct Details {
    people: Vec<String>,
    env: Value,
    settings: Value,
    usage: Counters,
}

/// One hash per signed-in list account, AniList, MAL, Simkl, of the account's numeric ID. An ID
/// that can't be had right now (offline, a refused token) is left out until the next ping.
async fn people<R: Runtime>(app: &AppHandle<R>) -> Vec<String> {
    let state = app.state::<AppState>();
    let mut ids: Vec<(&str, i64)> = Vec::new();
    if state.anilist.has_token() {
        let viewer = state.viewer.read().unwrap().as_ref().map(|v| v.id);
        if let Some(id) = viewer.filter(|id| *id > 0) {
            ids.push(("anilist", id));
        }
    }
    if let Some(token) = state.mal_access_token().await {
        match state.mal.user(&token).await {
            Ok(user) if user.id > 0 => ids.push(("mal", user.id)),
            Ok(_) => {}
            Err(e) => tracing::info!(error = %e, "no MAL id for the usage ping"),
        }
    }
    if let Some(token) = crate::simkl::access_token(app, None).await {
        match state.simkl.account_id(&token).await {
            Ok(Some(id)) => ids.push(("simkl", id)),
            Ok(None) => {}
            Err(e) => tracing::info!(error = %e, "no Simkl id for the usage ping"),
        }
    }
    ids.into_iter().map(|(service, id)| person_hash(service, id)).collect()
}

/// The hash every app makes of a list account: the same account, the same hash, on every device.
fn person_hash(service: &str, id: i64) -> String {
    hex(&Sha256::digest(format!("{PERSON_SALT}{service}:{id}")))
}

/// The app's main settings, under the contract's names. The desktop plays at the best a server
/// offers (no default quality), always in its own player.
fn settings(s: &crate::app::Settings) -> Value {
    json!({
        "quality": "auto",
        "theme": clean(&s.appearance.background, 20),
        "audio": if matches!(s.audio, Audio::Dub) { "dub" } else { "sub" },
        "player": "internal",
    })
}

/// The computer, once per process (only the screen is asked each time: it can change).
fn environment<R: Runtime>(app: &AppHandle<R>) -> Value {
    static FIXED: OnceLock<Value> = OnceLock::new();
    let mut env = FIXED
        .get_or_init(|| {
            let (os, maker, model, lang) = platform::describe();
            json!({
                "os": os.map(|v| clean(&v, 24)),
                "maker": maker.map(|v| clean(&v, 40)),
                "model": model.map(|v| clean(&v, 60)),
                "lang": lang.as_deref().and_then(bcp47),
                // The desktop is English only.
                "app_lang": "en",
            })
        })
        .clone();
    let screen = app
        .get_webview_window("main")
        .and_then(|w| w.primary_monitor().ok().flatten())
        .map(|m| format!("{}x{}", m.size().width, m.size().height));
    env["screen"] = json!(screen);
    // Absent rather than null: the server drops what it doesn't get.
    if let Some(map) = env.as_object_mut() {
        map.retain(|_, v| !v.is_null() && v.as_str() != Some(""));
    }
    env
}

/// A value as the server takes it: letters, digits, spaces and a few marks, trimmed, at most `max`
/// characters.
fn clean(value: &str, max: usize) -> String {
    let kept: String = value
        .chars()
        .map(|c| if c.is_alphanumeric() || " ._+-()/,'&".contains(c) { c } else { ' ' })
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect::<String>().trim().to_owned()
}

/// A locale as the system spells it (`sq_AL.UTF-8`, `en_US@rg=gbzzzz`, `en-US`) as a BCP 47 tag
/// (`sq-AL`); none for the C locale.
fn bcp47(raw: &str) -> Option<String> {
    let tag = raw.split(['.', '@']).next()?.trim().replace('_', "-");
    let valid = !tag.is_empty()
        && tag != "C"
        && tag != "POSIX"
        && tag.len() <= 20
        && tag.split('-').all(|part| !part.is_empty() && part.len() <= 8 && part.chars().all(|c| c.is_ascii_alphanumeric()));
    valid.then_some(tag)
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
    };

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// A string value (the 64-bit view, from a 32-bit build too).
    pub fn string(root: HKEY, key: &str, name: &str) -> Option<String> {
        let (key, name) = (wide(key), wide(name));
        let mut buffer = [0u16; 256];
        let mut size = (buffer.len() * 2) as u32;
        // SAFETY: both names are NUL-terminated, and `size` is the buffer's length in bytes.
        let status = unsafe {
            RegGetValueW(root, key.as_ptr(), name.as_ptr(), RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY, std::ptr::null_mut(), buffer.as_mut_ptr().cast(), &mut size)
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let value = String::from_utf16_lossy(&buffer[..(size as usize / 2).saturating_sub(1)]).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }

    fn dword(root: HKEY, key: &str, name: &str) -> Option<u32> {
        let (key, name) = (wide(key), wide(name));
        let mut value = 0u32;
        let mut size = size_of::<u32>() as u32;
        // SAFETY: both names are NUL-terminated, and `value` is the 4 bytes a DWORD takes.
        let status = unsafe {
            RegGetValueW(root, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD | RRF_SUBKEY_WOW6464KEY, std::ptr::null_mut(), (&raw mut value).cast(), &mut size)
        };
        (status == ERROR_SUCCESS).then_some(value)
    }

    /// Windows' `MachineGuid`, set when Windows is installed.
    pub fn machine_id() -> Option<String> {
        string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Cryptography", "MachineGuid")
    }

    /// System (`10.0.22631`), maker and model (the firmware's), and the user's language.
    pub fn describe() -> (Option<String>, Option<String>, Option<String>, Option<String>) {
        const VERSION: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";
        const BIOS: &str = r"HARDWARE\DESCRIPTION\System\BIOS";
        let os = match (dword(HKEY_LOCAL_MACHINE, VERSION, "CurrentMajorVersionNumber"), string(HKEY_LOCAL_MACHINE, VERSION, "CurrentBuildNumber")) {
            (Some(major), Some(build)) => {
                Some(format!("{major}.{}.{build}", dword(HKEY_LOCAL_MACHINE, VERSION, "CurrentMinorVersionNumber").unwrap_or(0)))
            }
            _ => None,
        };
        (
            os,
            string(HKEY_LOCAL_MACHINE, BIOS, "SystemManufacturer"),
            string(HKEY_LOCAL_MACHINE, BIOS, "SystemProductName"),
            string(HKEY_CURRENT_USER, r"Control Panel\International", "LocaleName"),
        )
    }
}

#[cfg(target_os = "macos")]
mod platform {
    fn run(program: &str, args: &[&str]) -> Option<String> {
        let output = std::process::Command::new(program).args(args).output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (output.status.success() && !text.is_empty()).then_some(text)
    }

    /// The Mac's hardware UUID, as `ioreg` reports it: `"IOPlatformUUID" = "…"`.
    pub fn machine_id() -> Option<String> {
        let text = run("/usr/sbin/ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"])?;
        let line = text.lines().find(|l| l.contains("\"IOPlatformUUID\""))?;
        let id = line.rsplit('"').nth(1)?.trim().to_owned();
        (!id.is_empty()).then_some(id)
    }

    /// System (`15.0`), Apple and the model (`Mac14,2`), and the language.
    pub fn describe() -> (Option<String>, Option<String>, Option<String>, Option<String>) {
        (
            run("/usr/bin/sw_vers", &["-productVersion"]),
            Some("Apple".to_owned()),
            run("/usr/sbin/sysctl", &["-n", "hw.model"]),
            run("/usr/bin/defaults", &["read", "-g", "AppleLocale"]),
        )
    }
}

#[cfg(target_os = "linux")]
mod platform {
    fn read(path: &str) -> Option<String> {
        std::fs::read_to_string(path).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
    }

    /// systemd's machine ID, or D-Bus's copy of it on systems without systemd.
    pub fn machine_id() -> Option<String> {
        read("/etc/machine-id").or_else(|| read("/var/lib/dbus/machine-id"))
    }

    /// The distribution (`Ubuntu 24.04`), the firmware's maker and model, and the language.
    pub fn describe() -> (Option<String>, Option<String>, Option<String>, Option<String>) {
        let release = read("/etc/os-release").unwrap_or_default();
        let field = |key: &str| {
            release.lines().find_map(|l| l.strip_prefix(&format!("{key}="))).map(|v| v.trim_matches('"').to_owned()).filter(|v| !v.is_empty())
        };
        let os = match (field("NAME"), field("VERSION_ID")) {
            (Some(name), Some(version)) => Some(format!("{name} {version}")),
            (name, _) => name.or_else(|| field("PRETTY_NAME")),
        };
        let lang = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()));
        (os, read("/sys/class/dmi/id/sys_vendor"), read("/sys/class/dmi/id/product_name"), lang)
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod platform {
    pub fn machine_id() -> Option<String> {
        None
    }
    pub fn describe() -> (Option<String>, Option<String>, Option<String>, Option<String>) {
        (None, None, None, None)
    }
}

/// The ID sent for a raw one: salted, so it matches nothing outside Anilili.
fn device_id(raw: &str) -> String {
    hex(&Sha256::digest(format!("{SALT}{raw}")))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn raw_device_id(store: &Store) -> String {
    static MACHINE: OnceLock<Option<String>> = OnceLock::new();
    if let Some(id) = MACHINE.get_or_init(platform::machine_id) {
        return id.clone();
    }
    if let Some(id) = store.setting::<String>(FALLBACK_KEY) {
        return id;
    }
    let id = format!("{:032x}", rand::random::<u128>());
    let _ = store.set_setting(FALLBACK_KEY, &id);
    id
}

fn utc_day(unix: i64) -> String {
    let (year, month, day) = civil_date(unix);
    format!("{year:04}-{month:02}-{day:02}")
}

fn accounts(anilist: bool, mal: bool, simkl: bool) -> Vec<&'static str> {
    [("anilist", anilist), ("mal", mal), ("simkl", simkl)].into_iter().filter(|(_, on)| *on).map(|(name, _)| name).collect()
}

/// What to send today, or `None` when today's ping was already accepted. The desktop has no build
/// number, so `build` is 0.
fn body(
    device: &str,
    platform: &str,
    version: &str,
    accounts: &[&str],
    last_day: Option<&str>,
    today: &str,
    more: &Details,
) -> Option<Value> {
    if last_day == Some(today) {
        return None;
    }
    Some(json!({
        "device": device,
        "platform": platform,
        "version": version,
        "build": 0,
        "accounts": accounts,
        "first": last_day.is_none(),
        "people": more.people,
        "env": more.env,
        "settings": more.settings,
        "usage": more.usage,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEVICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn none() -> Details {
        Details { people: Vec::new(), env: json!({}), settings: json!({}), usage: Counters::default() }
    }

    #[test]
    fn the_device_id_is_the_salted_hash_every_app_makes() {
        // The same vector is in the Android and iOS tests.
        assert_eq!(device_id("9774d56d682e549c"), "0eb0cde3e2356397f8217597eace93a803af7fcf676c9fa42d98c23a5f7f5b97");
    }

    #[test]
    fn the_person_hash_is_the_one_every_app_makes() {
        // The same vector is in the Android and iOS tests.
        assert_eq!(person_hash("anilist", 123456), "79f2ba9972b3e2f1fb4ee6ce0e47fe7ca11faa2c8d7d435eed44eac40f938860");
        assert_ne!(person_hash("anilist", 123456), person_hash("mal", 123456));
    }

    #[test]
    fn this_machine_has_an_id() {
        let id = platform::machine_id();
        assert!(id.as_deref().is_some_and(|id| id.len() >= 16), "{id:?}");
    }

    #[test]
    fn this_machine_describes_itself() {
        let (os, maker, model, lang) = platform::describe();
        assert!(os.is_some(), "no OS version");
        assert!(maker.is_some() && model.is_some(), "no maker or model: {maker:?} {model:?}");
        assert!(lang.as_deref().and_then(bcp47).is_some(), "no language: {lang:?}");
    }

    #[test]
    fn days_are_utc() {
        // 23:30 on Oct 11 in New York is already Oct 12 in UTC.
        assert_eq!(utc_day(1_791_775_800), "2026-10-12");
        assert_eq!(utc_day(0), "1970-01-01");
    }

    #[test]
    fn once_a_day() {
        assert!(body(DEVICE, "windows", "0.1.4", &[], Some("2026-10-12"), "2026-10-12", &none()).is_none());
    }

    #[test]
    fn the_body_uses_the_contracts_names() {
        let more = Details {
            people: vec!["ab".repeat(32)],
            env: json!({"os": "10.0.22631", "maker": "Dell Inc.", "model": "XPS 15 9530", "screen": "1920x1080", "lang": "sq-XK", "app_lang": "en"}),
            settings: json!({"quality": "auto", "theme": "amoled", "audio": "sub", "player": "internal"}),
            usage: Counters { minutes: 95, watched: 60, sessions: 3 },
        };
        let sent = body(DEVICE, "macos", "0.1.4", &accounts(true, false, true), Some("2026-10-11"), "2026-10-12", &more).unwrap();
        assert_eq!(
            sent,
            json!({"device": DEVICE, "platform": "macos", "version": "0.1.4", "build": 0, "accounts": ["anilist", "simkl"], "first": false,
                   "people": ["ab".repeat(32)],
                   "env": {"os": "10.0.22631", "maker": "Dell Inc.", "model": "XPS 15 9530", "screen": "1920x1080", "lang": "sq-XK", "app_lang": "en"},
                   "settings": {"quality": "auto", "theme": "amoled", "audio": "sub", "player": "internal"},
                   "usage": {"minutes": 95, "watched": 60, "sessions": 3}})
        );
    }

    #[test]
    fn first_only_without_an_earlier_ping() {
        assert_eq!(body(DEVICE, "linux", "0.1.4", &[], None, "2026-10-12", &none()).unwrap()["first"], true);
        assert_eq!(body(DEVICE, "linux", "0.1.4", &[], Some("2025-01-01"), "2026-10-12", &none()).unwrap()["first"], false);
    }

    #[test]
    fn minutes_count_in_front_or_playing_and_sessions_on_return() {
        let mut c = Counters::default();
        c.count(true, false, true);
        c.count(true, true, false);
        c.count(false, true, false);
        c.count(false, false, false);
        assert_eq!(c, Counters { minutes: 3, watched: 2, sessions: 1 });
    }

    #[test]
    fn what_was_counted_while_sending_is_kept() {
        let sent = Counters { minutes: 10, watched: 4, sessions: 2 };
        let now = Counters { minutes: 12, watched: 4, sessions: 3 };
        assert_eq!(now.minus(sent), Counters { minutes: 2, watched: 0, sessions: 1 });
    }

    #[test]
    fn locales_become_bcp47_tags() {
        assert_eq!(bcp47("sq_AL.UTF-8").as_deref(), Some("sq-AL"));
        assert_eq!(bcp47("en_US@rg=gbzzzz").as_deref(), Some("en-US"));
        assert_eq!(bcp47("de-DE").as_deref(), Some("de-DE"));
        assert_eq!(bcp47("C"), None);
        assert_eq!(bcp47("POSIX.UTF-8"), None);
    }

    #[test]
    fn values_are_cleaned_for_the_server() {
        assert_eq!(clean("  Dell   Inc.™ ", 40), "Dell Inc.");
        assert_eq!(clean("Micro-Star International Co., Ltd.", 24), "Micro-Star International");
        assert_eq!(clean("To Be Filled By O.E.M.", 60), "To Be Filled By O.E.M.");
    }
}
