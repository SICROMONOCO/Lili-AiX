//! AnimeRift (`com.riftapps.animerift`), the Arabic catalogue behind the Anime Rift Android app.
//!
//! Port of Android's `RiftProvider.kt` as it stands after `30c057a4` — keep the two in step.
//!
//! Not a website: a private mobile API reached through Firebase, and the only Arabic server here.
//! Arabic subtitles (burned into the picture) on nearly everything, an Arabic dub on some titles,
//! and the service publishes its own intro/outro windows.
//!
//! 1. Firebase installation → remote config, which names the gateway (it has moved once:
//!    `gateway.anime-rift.com` → `gateway.embrassme.top/api/v4`) → `/auth/register/device`.
//! 2. `/library/search` → candidates. No AniList id field, but `medium_picture` is usually an
//!    `anilistcdn` cover whose filename is the media id (`bx21-…` is AniList 21): the exact join.
//! 3. `/library/episodes/{id}` → episode ids, with intro/outro windows.
//! 4. `/library/episode/sources` → one row per server and quality, plus the service's own server
//!    ranking (`configs[].priority`); then `can_play` and `direct_link` per row.
//!
//! What Android learned on a device (2026-10-06), built in here from the start:
//!
//! * **Stop at the first server that yields streams.** cr2 publishes one row per quality; going on
//!   to the next server reached streamtape, whose 5 s ticket wait ran the source budget out and took
//!   three good cr2 streams down with it. Android's first version ran 51 s and reported "unavailable
//!   for this episode" while holding three playable streams.
//! * **streamtape goes last** whatever the service ranks it (it ranks streamtape 3, above
//!   rift-streamer at 5), because its resolve is a ticket and a wait rather than a URL — and from
//!   the Android app its API is behind a Cloudflare challenge. Its wait is not honoured here at all:
//!   a WASM component has no way to sleep that the host budgets for, so the ticket is spent at once
//!   and a host that is not ready yet is simply skipped. That is strictly better than Android's 5 s,
//!   and costs nothing when the row is never reached.
//! * **"No Arabic dub for this episode" is an answer, not a failure.** The Android provider reports
//!   it as such so the remembered title match survives; here it is an empty list, which is this
//!   interface's way of saying the same thing (see `servers`).
//! * **A 429 is the gateway saying "slow down", not "I moved".** Android re-read remote config for
//!   it, which was one more request into the limit.
//!
//! Two things the Android side carries that this interface has no place for:
//!
//! * `episodeAvailability`, the "does it carry a dub at all" probe: here the app asks for the audio
//!   it wants and a provider answers for that episode, so a dub that stops early is just an empty
//!   list for the episodes past it. The probe, and the cache entry that remembered it, are gone.
//! * The declared Arabic language (`ProviderCatalog.declaredLanguages` on Android), which keeps the
//!   server out of an automatic choice unless the viewer asked for Arabic. The desktop has no
//!   per-provider language anywhere, so this provider has to sit last in `plugin.rs`'s `PRIORITY`
//!   and be picked by hand until that exists. See the note on `PRIORITY`.
//!
//! No `integrity` JWT and no `/ads_manager/claim`: the gateway checks neither (measured; the Android
//! provider documents both).

use std::collections::HashMap;

use provider_kit::{self as kit, Candidate, DAY, types::*};
use serde_json::{Value, json};

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
    with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
});

const INSTALLATIONS: &str =
    "https://firebaseinstallations.googleapis.com/v1/projects/anime-rift-4142e/installations";
const REMOTE_CONFIG: &str = "https://firebaseremoteconfig.googleapis.com/v1/projects/536921039715/namespaces/firebase:fetch";
const GATEWAY_ENTRY: &str = "anime_rift_android_gateway_base_url_v4";
/// Where the gateway was last known to be, for a bootstrap that cannot read remote config.
const GATEWAY_FALLBACK: &str = "https://gateway.embrassme.top/api/v4";
const ANDROID_PACKAGE: &str = "com.riftapps.animerift";
const ANDROID_CERT: &str = "AF40CE82A52AA4107F311D8B9727D01C8D02250B";
const INSTALL_SOURCE: &str = "IS_INSTALLED_FROM_PLAY_PACKAGE_INSTALLER";
const FIREBASE_APP_ID: &str = "1:536921039715:android:78825c96b74de921b8e956";
const FIREBASE_API_KEY: &str = "AIzaSyBiLkiGEm7ruugny3tDFHEZvqli8yv1k7I";
const FIREBASE_CLIENT: &str = "H4sIAAAAAAAA_6tWykhNLCpJSk0sKVayio7VUSpLLSrOzM9TslIyUqoFAFyivEQfAAAA";
const FIREBASE_USER_AGENT: &str = "Dalvik/2.1.0 (Linux; U; Android 14; Pixel 8 Pro Build/AP2A.240805.015)";
const DART_USER_AGENT: &str = "Dart/3.10 (dart:io)";
const APP_BUILD: &str = "68";
/// The version every request claims. Measured: 3.10.0 is answered `RELEASE_VERSION_NO_LONGER_SUPPORTED`,
/// 3.13.5 and 99.0.0 are both accepted, and no ceiling was found.
const APP_VERSION: &str = "99.0.0";
const PLAY_EVENT: &str = "play_episode_unlocked";
const STREAMTAPE: &str = "https://api.streamtape.com/file/dl";
const DEVICE_SUSPENDED: &str = "DEVICE_SUSPENDED";
const RATE_LIMITED: &str = "HTTP_429";
const RATE_LIMIT_WAIT: std::time::Duration = std::time::Duration::from_millis(1500);

const SESSION_KEY: &str = "session";
/// Every registration leaves a device behind on their side, so the session is kept, not minted.
const SESSION_TTL: u32 = 30 * DAY;
/// An airing title gains a row a week; a missing episode also triggers one fresh read.
const EPISODES_TTL: u32 = 30 * 60;
const TITLE_TTL: u32 = 7 * DAY;
const TITLE_MISS_TTL: u32 = DAY;

/// The floor for a match made on the title alone.
///
/// Android also multiplies by the year, and this interface's `title` record carries no year, so the
/// name is all there is. Android measured its three title-only matches at 1.000, 1.000 and 0.550 on
/// the name; with the year gone the floor is set above the tightest of those rather than at
/// Android's 0.55, because a wrong match here plays a different show.
const MIN_TITLE_SCORE: f64 = 0.75;
/// Hosts that answer `direct_link` with a ticket and a wait rather than a URL; see `hosts`.
const TICKET_HOSTS: &[&str] = &["streamtape"];
/// Bounds on one resolve: each stream costs a `can_play` and a `direct_link`.
const MAX_STREAMS: usize = 6;
const MAX_RESOLVES: usize = 8;
const UNRANKED: i64 = 50;
const DEFAULT_QUALITY: &str = "720P";

// ---- Session -------------------------------------------------------------------------------

#[derive(Clone)]
struct Session {
    fid: String,
    token: String,
    gateway: String,
    device_id: String,
    build_id: String,
}

impl Session {
    /// The held session, or a fresh bootstrap when there is none or it is incomplete.
    fn get() -> Result<Session, String> {
        if let Some(raw) = kit::host::cache_get(SESSION_KEY)
            && let Ok(v) = serde_json::from_str::<Value>(&raw)
        {
            let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
            let session = Session {
                fid: s("fid"),
                token: s("token"),
                gateway: s("gateway"),
                device_id: s("deviceId"),
                build_id: s("buildId"),
            };
            if !session.fid.is_empty() && !session.gateway.is_empty() && !session.device_id.is_empty() {
                return Ok(session);
            }
        }
        bootstrap()
    }

    fn save(&self) {
        let value = json!({
            "fid": self.fid,
            "token": self.token,
            "gateway": self.gateway,
            "deviceId": self.device_id,
            "buildId": self.build_id,
        });
        kit::host::cache_set(SESSION_KEY, &value.to_string(), SESSION_TTL);
    }
}

/// A fresh Firebase installation, the gateway it names, and a device registered against it.
fn bootstrap() -> Result<Session, String> {
    let (fid, token) = install()?;
    let configured = remote_config(&fid, &token);
    // Worth a line in the log every time: this URL has moved once already, and a client pointed at
    // the old one fails in ways that look like the service being down.
    kit::host::log(&format!(
        "rift fid={fid} gateway={}",
        configured.clone().unwrap_or_else(|| format!("{GATEWAY_FALLBACK} (remote config gave none)"))
    ));
    let gateway = configured.unwrap_or_else(|| GATEWAY_FALLBACK.to_owned());
    let (build_id, device_id) = register(&fid, &gateway)?;
    let session = Session { fid, token, gateway, device_id, build_id };
    session.save();
    Ok(session)
}

fn install() -> Result<(String, String), String> {
    // Sent uncompressed, though the Android client gzips it: the endpoint accepts a plain body.
    let body = json!({
        "fid": "",
        "appId": FIREBASE_APP_ID,
        "authVersion": "FIS_v2",
        "sdkVersion": "a:19.1.0",
    });
    let headers = vec![
        ("X-Android-Package".to_owned(), ANDROID_PACKAGE.to_owned()),
        ("X-Android-Cert".to_owned(), ANDROID_CERT.to_owned()),
        ("x-goog-api-key".to_owned(), FIREBASE_API_KEY.to_owned()),
        ("x-firebase-client".to_owned(), FIREBASE_CLIENT.to_owned()),
        ("Accept".to_owned(), "application/json".to_owned()),
        ("User-Agent".to_owned(), FIREBASE_USER_AGENT.to_owned()),
    ];
    let root = send_with("POST", INSTALLATIONS, &headers, Some(body.to_string().into_bytes()))?;
    let fid = root.get("fid").and_then(Value::as_str).unwrap_or_default().to_owned();
    let token = root
        .get("authToken")
        .and_then(|t| t.get("token"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if fid.is_empty() || token.is_empty() {
        return Err(format!("Firebase returned no installation ({root})"));
    }
    Ok((fid, token))
}

/// The gateway URL remote config names, which is the only authority for it.
fn remote_config(fid: &str, token: &str) -> Option<String> {
    let body = json!({
        "appVersion": APP_VERSION,
        "appInstanceIdToken": token,
        "appBuild": APP_BUILD,
        "appInstanceId": fid,
        "analyticsUserProperties": {},
        "appId": FIREBASE_APP_ID,
        "platformVersion": "34",
        "sdkVersion": "23.0.1",
        "packageName": ANDROID_PACKAGE,
    });
    let headers = vec![
        ("X-Goog-Api-Key".to_owned(), FIREBASE_API_KEY.to_owned()),
        ("X-Android-Package".to_owned(), ANDROID_PACKAGE.to_owned()),
        ("X-Android-Cert".to_owned(), ANDROID_CERT.to_owned()),
        ("X-Goog-Firebase-Installations-Auth".to_owned(), token.to_owned()),
        ("Accept".to_owned(), "application/json".to_owned()),
        ("User-Agent".to_owned(), FIREBASE_USER_AGENT.to_owned()),
    ];
    let root = send_with("POST", REMOTE_CONFIG, &headers, Some(body.to_string().into_bytes())).ok()?;
    root.get("entries")
        .and_then(|e| e.get(GATEWAY_ENTRY))
        .and_then(Value::as_str)
        .map(|g| g.replace('"', "").trim().to_owned())
        .filter(|g| !g.is_empty())
}

/// A synthetic device, and the id the service will know it by.
fn register(fid: &str, gateway: &str) -> Result<(String, String), String> {
    let seed = fnv(fid);
    let build_id = format!("AP2A.240805.{}.015", 100_000 + seed % 900_000);
    let device_id = format!("{fid}:{}", fcm_token(seed));
    let info = json!({
        "model": "Pixel 8 Pro",
        "brand": "google",
        "manufacturer": "Google",
        "device": "google",
        "hardware": "qcom",
        "product": "google",
        "androidVersion": "14",
        "sdkInt": 34,
        "securityPatch": "2025-01-01",
        "isPhysicalDevice": true,
        "supportedAbis": ["arm64-v8a"],
        "tags": "release-keys",
        "type": "user",
        "buildId": build_id,
        "host": format!("build-server-{seed:08x}"),
        "fingerprint": format!("google/Pixel 8 Pro/google:14/{build_id}/V.1e9a_{seed:08x}:user/release-keys"),
    });
    let body = json!({
        "deviceId": device_id,
        "current_app_version": APP_VERSION,
        "device_os": "android",
        "device_environment": "production",
        "device_info": info.to_string(),
        "install_source": INSTALL_SOURCE,
        "deviceOsId": build_id,
        "firebaseInstallationId": fid,
        "apn_token": Value::Null,
        "install_mode": 2,
    });
    let headers = gateway_headers(fid, &device_id, &build_id);
    let url = format!("{gateway}/auth/register/device");
    kit::host::log(&format!("registering device at {url}"));
    let root = send_with("POST", &url, &headers, Some(body.to_string().into_bytes()))?;
    // The service answers a refusal with 200 and an errorCode as readily as with a status code, so
    // the body is the authority here, not the status.
    if let Some(code) = root.get("errorCode").and_then(Value::as_str) {
        return Err(format!("device registration at {url} refused ({code})"));
    }
    if root.get("sessionKey").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err("device registration returned no session".into());
    }
    Ok((build_id, device_id))
}

fn gateway_headers(fid: &str, device_id: &str, build_id: &str) -> Vec<(String, String)> {
    vec![
        ("x-device-os-id".to_owned(), build_id.to_owned()),
        ("user-agent".to_owned(), DART_USER_AGENT.to_owned()),
        ("x-device-release-version".to_owned(), APP_VERSION.to_owned()),
        ("x-firebase-app-check".to_owned(), "null".to_owned()),
        ("authorization".to_owned(), "Bearer null".to_owned()),
        ("content-type".to_owned(), "application/json; charset=UTF-8".to_owned()),
        ("x-installation-source".to_owned(), INSTALL_SOURCE.to_owned()),
        ("accept".to_owned(), "application/json".to_owned()),
        ("x-firebase-id".to_owned(), fid.to_owned()),
        ("x-device-id".to_owned(), device_id.to_owned()),
        // Measured 2026-10-06 by dropping one header at a time from a registration that answers 201:
        // the gateway refuses with `403 ACCESS_DENIED` without this, without `x-device-language`,
        // without `x-installation-source` or without `x-os`, and with
        // `403 UNSUPPORTED_PLATFORM` without a `user-agent`. `integrity`, `authorization`,
        // `x-platform`, `x-firebase-app-check` and `x-device-os-id` are all optional.
        ("x-device-timezone".to_owned(), timestamp()),
        ("x-device-language".to_owned(), "ar".to_owned()),
        ("x-platform".to_owned(), "Mobile".to_owned()),
        ("x-os".to_owned(), "android".to_owned()),
    ]
}

/// The local-looking timestamp the client sends, with no zone attached.
///
/// The host lends a provider no clock, so this is UTC: the guest's `std` is WASI, whose wall clock
/// is the one thing here that is not the app's. Android sends its own local time in this field; the
/// service takes it as a claim about the device rather than as an authority, and UTC is within a
/// day of every zone the app ships to.
fn timestamp() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_secs(), d.subsec_micros()))
        .unwrap_or((0, 0));
    timestamp_at(seconds.0, seconds.1)
}

/// [`timestamp`] for a given instant, so the format can be tested without a clock.
fn timestamp_at(seconds: u64, micros: u32) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (rest / 3600, (rest % 3600) / 60, rest % 60);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{micros:06}")
}

/// Days since 1970-01-01 to a calendar date (`civil_from_days`, Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 { month_prime + 3 } else { month_prime - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

// ---- Transport -----------------------------------------------------------------------------

fn send_with(method: &str, url: &str, headers: &[(String, String)], body: Option<Vec<u8>>) -> Result<Value, String> {
    let pairs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let response = kit::send(method, url, &pairs, body)?;
    let parsed = serde_json::from_slice::<Value>(&response.body).ok().filter(Value::is_object);
    // A refusal with a JSON body that names no errorCode (a 429's does not) is still a refusal:
    // read as a success it was an empty search, which reads as "no match". When the body is not one
    // of the service's own error objects it is logged with its status, because that is the only way
    // to tell a moved gateway from a Cloudflare page from an outage.
    Ok(match parsed {
        Some(v) if response.status < 400 || v.get("errorCode").is_some() => v,
        _ => {
            let snippet: String = String::from_utf8_lossy(&response.body).chars().take(200).collect();
            kit::host::log(&format!("{url} answered HTTP {} — {}", response.status, snippet.replace('\n', " ")));
            json!({ "errorCode": format!("HTTP_{}", response.status) })
        }
    })
}

/// One gateway call, repairing a stale session once.
///
/// Two failures are "the session is stale": a device the service has retired, which only being
/// refused can reveal, and a gateway URL that has moved, which is otherwise indistinguishable from
/// an outage. A rate limit is neither — it is waited out once — and nor is any other refusal in the
/// service's own words: re-reading remote config for those is one more request into the limit,
/// which is what Android did.
fn api(session: &Session, path: &str, method: &str, body: Option<&Value>) -> Result<Value, String> {
    let url = format!("{}{path}", session.gateway);
    let send = || send_with(method, &url, &gateway_headers(&session.fid, &session.device_id, &session.build_id), body.map(|b| b.to_string().into_bytes()));
    let mut root = send()?;
    // The limit is per endpoint and tight: the app asks for sub and dub at once, and the second
    // search arriving beside the first was refused (measured 2026-10-06, two in three runs). A
    // moment's wait clears it; the wait is wall time, which the host's call deadline counts.
    if root.get("errorCode").and_then(Value::as_str) == Some(RATE_LIMITED) {
        std::thread::sleep(RATE_LIMIT_WAIT);
        root = send()?;
    }
    let Some(code) = root.get("errorCode").and_then(Value::as_str) else {
        return Ok(root);
    };
    if code == RATE_LIMITED {
        return Err(format!("Rift rate limited on {path}"));
    }
    let repaired = if code == DEVICE_SUSPENDED {
        bootstrap().ok()
    } else if !code.starts_with("HTTP_") {
        // The service's own refusal (`ACCESS_DENIED` and the like) proves the gateway is where the
        // session says; only an answer that isn't the service's (`send_with`'s `HTTP_…`: an HTML
        // page, a 404) can mean it moved. Re-reading remote config for every refusal sent a blocked
        // client to Firebase once per request, for nothing.
        None
    } else {
        // The gateway is the one part of the session that moves; re-read it and retry once.
        Session::get().ok().and_then(|held| {
            let moved = remote_config(&held.fid, &held.token)?;
            if moved == held.gateway {
                return None;
            }
            let fresh = Session { gateway: moved, ..held };
            fresh.save();
            Some(fresh)
        })
    };
    let Some(fresh) = repaired else {
        return Err(format!("Rift failed {path} ({code})"));
    };
    let url = format!("{}{path}", fresh.gateway);
    let root = send_with(method, &url, &gateway_headers(&fresh.fid, &fresh.device_id, &fresh.build_id), body.map(|b| b.to_string().into_bytes()))?;
    match root.get("errorCode").and_then(Value::as_str) {
        Some(code) => Err(format!("Rift failed {path} ({code})")),
        None => Ok(root),
    }
}

// ---- Matching ------------------------------------------------------------------------------

/// A search hit, with the two extra signals this API gives away.
struct Hit {
    id: String,
    title: String,
    alternative: Option<String>,
    /// The AniList id its cover URL encodes; see [`anilist_id`].
    anilist: Option<u32>,
}

/// The AniList id a cover URL gives away.
///
/// There is no AniList or MAL id field anywhere in this API, but `.medium_picture` is usually still
/// an `anilistcdn` URL, and AniList names those files after the media id:
/// `.../cover/large/bx21-ELSYx3yMPcKM.jpg` is AniList 21. Android measured this exact for 7 of 7
/// known titles and for 87 of 150 items on one home page. `main_picture` is deliberately not read:
/// it is a proxied copy whose path is a hash and carries no id at all.
fn anilist_id(item: &Value) -> Option<u32> {
    for field in ["medium_picture", "large_picture", "thumbnail_picture"] {
        let Some(url) = item.get(field).and_then(Value::as_str) else { continue };
        let Some(rest) = url.split("anilistcdn/media/anime/").nth(1) else { continue };
        let Some(name) = rest.split('/').nth(2) else { continue };
        let digits: String = name
            .trim_start_matches("bx")
            .trim_start_matches('b')
            .trim_start_matches("nx")
            .trim_start_matches('n')
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(id) = digits.parse() {
            return Some(id);
        }
    }
    None
}

/// The candidate to use, or none when nothing is close enough to be worth playing.
///
/// An exact AniList id wins outright. Otherwise the best title score decides, with a candidate that
/// positively identifies itself as a *different* AniList entry discounted heavily rather than
/// dropped — a wrong id in their dataset should not be able to hide a right answer, and it should not
/// be able to beat one either. `kit::candidate_score` carries the season guard with it, so a
/// sequel's name cannot resolve to its first season.
fn best<'a>(titles: &[String], anilist_id: u32, hits: &'a [Hit]) -> Option<&'a Hit> {
    if let Some(hit) = hits.iter().find(|h| h.anilist == Some(anilist_id)) {
        return Some(hit);
    }
    let score = |hit: &Hit| {
        let mut score = kit::candidate_score(titles, &Candidate { id: hit.id.clone(), title: hit.title.clone() });
        if let Some(alt) = &hit.alternative {
            let alt = kit::candidate_score(titles, &Candidate { id: hit.id.clone(), title: alt.clone() });
            score = score.max(alt);
        }
        if hit.anilist.is_some() && hit.anilist != Some(anilist_id) {
            score *= 0.4;
        }
        score
    };
    hits.iter()
        .map(|h| (score(h), h))
        .filter(|(s, _)| *s >= MIN_TITLE_SCORE)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, h)| h)
}

fn search(session: &Session, query: &str) -> Result<Vec<Hit>, String> {
    // `text_direction=jp` is what makes the service match a romaji title against its Japanese-script
    // index; without it the same query came back empty for titles that resolve with it.
    let root = api(
        session,
        "/library/search?page=0&sort_by=release_year&sort_direction=1&text_direction=jp",
        "POST",
        Some(&json!({ "query": query })),
    )?;
    Ok(root
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("_id").and_then(Value::as_str).filter(|i| !i.is_empty())?.to_owned();
                    Some(Hit {
                        id,
                        title: item.get("title").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        alternative: item.get("alternative_title").and_then(Value::as_str).map(str::to_owned),
                        anilist: anilist_id(item),
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// The Rift id for an AniList title, remembered for a week (a miss for a day).
///
/// `kit::resolve_id` does this shape for the other providers, but it scores `Candidate`s that carry
/// no AniList id, and that id is the only exact join this API offers — so the loop is written out
/// here to keep it.
fn resolve_title_id(session: &Session, title: &Title) -> Result<Option<String>, String> {
    kit::cached(&format!("id:{}", title.anilist_id), TITLE_TTL, TITLE_MISS_TTL, || {
        let queries = kit::search_queries(&title.titles, 3);
        let mut found: Vec<Hit> = Vec::new();
        let mut last_error = None;
        for query in &queries {
            match search(session, query) {
                Ok(hits) => found.extend(hits),
                Err(e) => last_error = Some(e),
            }
            if best(&title.titles, title.anilist_id, &found).is_some() {
                break;
            }
        }
        if let Some(hit) = best(&title.titles, title.anilist_id, &found) {
            return Ok(Some(hit.id.clone()));
        }
        // "Not on this service" is remembered for a day, so it is only said when every search was
        // answered. A refused one leaves the question open: with the title's own name refused
        // (rate limited) and the synonyms after it finding other shows, One Piece was cached as
        // missing and the server stayed away for a day.
        if let Some(e) = last_error {
            return Err(e);
        }
        Ok(None)
    })
}

// ---- Episodes ------------------------------------------------------------------------------

#[derive(Clone)]
struct Episode {
    id: String,
    number: i64,
    intro: Option<(f64, f64)>,
    outro: Option<(f64, f64)>,
}

fn window(value: Option<&Value>) -> Option<(f64, f64)> {
    let node = value?;
    Some((node.get("start")?.as_f64()?, node.get("end")?.as_f64()?))
}

fn fetch_episodes(session: &Session, title_id: &str) -> Result<Vec<Episode>, String> {
    let root = api(
        session,
        &format!("/library/episodes/{title_id}?sort_by_latest=1&with_arcs=true&with_favorites=true"),
        "GET",
        None,
    )?;
    let mut episodes: Vec<Episode> = root
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("_id").and_then(Value::as_str).filter(|i| !i.is_empty())?.to_owned();
                    let number = item.get("episode_number").and_then(Value::as_i64).filter(|n| *n > 0)?;
                    Some(Episode {
                        id,
                        number,
                        intro: window(item.get("intro")),
                        outro: window(item.get("outro")),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    episodes.sort_by_key(|e| e.number);
    Ok(episodes)
}

fn encode_episodes(episodes: &[Episode]) -> String {
    let rows: Vec<Value> = episodes
        .iter()
        .map(|e| {
            json!({
                "id": e.id,
                "number": e.number,
                "intro": e.intro.map(|(s, e)| json!([s, e])),
                "outro": e.outro.map(|(s, e)| json!([s, e])),
            })
        })
        .collect();
    Value::Array(rows).to_string()
}

fn decode_episodes(raw: &str) -> Vec<Episode> {
    let pair = |v: Option<&Value>| {
        let a = v?.as_array()?;
        Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
    };
    serde_json::from_str::<Value>(raw)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(Episode {
                        id: row.get("id").and_then(Value::as_str)?.to_owned(),
                        number: row.get("number").and_then(Value::as_i64)?,
                        intro: pair(row.get("intro")),
                        outro: pair(row.get("outro")),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The episode rows, kept for [`EPISODES_TTL`].
///
/// The row for the episode asked about is read again **once** when the held list has no such
/// episode: an airing title gains one a week, and the one that just aired is the one a viewer is
/// most likely to open. Android holds this list for the life of the process, which on a TV box is
/// days; here there is no process memory, so the cache is the only place it can live.
fn episodes(session: &Session, title_id: &str) -> Result<Vec<Episode>, String> {
    let key = format!("episodes:{title_id}");
    let raw = kit::cached(&key, EPISODES_TTL, 60, || {
        Ok(Some(encode_episodes(&fetch_episodes(session, title_id)?)))
    })?;
    Ok(decode_episodes(raw.as_deref().unwrap_or_default()))
}

fn episode_row(session: &Session, title_id: &str, number: i64) -> Result<Option<Episode>, String> {
    if let Some(row) = episodes(session, title_id)?.into_iter().find(|e| e.number == number) {
        return Ok(Some(row));
    }
    let fresh = fetch_episodes(session, title_id)?;
    kit::host::cache_set(&format!("episodes:{title_id}"), &encode_episodes(&fresh), EPISODES_TTL);
    Ok(fresh.into_iter().find(|e| e.number == number))
}

// ---- Hosts ---------------------------------------------------------------------------------

struct Host {
    id: String,
    provider: String,
    server: String,
    /// The raw `sub_title` tag, blank where the service did not state one.
    audio: String,
    qualities: Vec<String>,
    rank: i64,
}

fn priorities(root: &Value) -> HashMap<String, i64> {
    root.get("configs")
        .and_then(Value::as_array)
        .map(|configs| {
            configs
                .iter()
                .filter_map(|c| {
                    let key = c.get("server_key").and_then(Value::as_str).filter(|k| !k.is_empty())?;
                    Some((key.to_owned(), c.get("priority").and_then(Value::as_i64)?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The hosts worth trying for one audio track, best first.
///
/// Three things decide the order. A host that answers with a ticket goes after every other, whatever
/// its published priority: the service ranks streamtape above rift-streamer, but a streamtape
/// resolve is a wait this component cannot honour (see the module comment). Then the service's own
/// ranking, `configs[].priority` — the maintained one, which is why a server they add or retire
/// needs no edit here, and why this does not carry Android's hardcoded list. Then the best quality a
/// host offers, which puts the 1080p rows ahead of the 240p ones for the same server.
///
/// `wanted` is one of Rift's own tags, `ar_sub` or `ar_dub`. Arabic only: this provider exists to be
/// the Arabic server, and the `en_*` and `it_*` rows beside it are the same catalogue under audio the
/// other providers already carry. An untagged row is taken for the subtitle track, after every
/// tagged one, because Android reads a blank tag as Arabic and dropping it would lose those hosts.
fn hosts(root: &Value, wanted: &str) -> Vec<Host> {
    let ranks = priorities(root);
    let mut hosts: Vec<Host> = root
        .get("items")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let id = item.get("_id").and_then(Value::as_str).filter(|i| !i.is_empty())?.to_owned();
                    let audio = item.get("sub_title").and_then(Value::as_str).unwrap_or_default();
                    let matches = audio.starts_with(wanted) || (wanted == "ar_sub" && audio.trim().is_empty());
                    if !matches {
                        return None;
                    }
                    let provider = item.get("provider").and_then(Value::as_str).unwrap_or_default().to_owned();
                    let rank = ranks.get(&provider).copied().unwrap_or(UNRANKED);
                    Some(Host {
                        id,
                        provider,
                        server: item.get("server_name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        audio: audio.to_owned(),
                        qualities: item
                            .get("qualities")
                            .and_then(Value::as_array)
                            .map(|q| q.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                            .unwrap_or_default(),
                        rank,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    hosts.sort_by(|a, b| {
        let ticket = |h: &Host| usize::from(TICKET_HOSTS.contains(&h.provider.as_str()));
        ticket(a)
            .cmp(&ticket(b))
            .then(a.rank.cmp(&b.rank))
            // A tagged row before an untagged one at the same rank: `ar_sub` is a statement, a blank
            // tag is an inference.
            .then(usize::from(a.audio.trim().is_empty()).cmp(&usize::from(b.audio.trim().is_empty())))
            .then(best_height(b).cmp(&best_height(a)))
    });
    hosts
}

fn best_height(host: &Host) -> u32 {
    host.qualities.iter().filter_map(|q| quality_height(q)).max().unwrap_or(0)
}

/// The qualities to ask a host for, highest first.
///
/// A host that publishes none is asked for 720P, which is what Android does with them. A host that
/// does not carry the one asked for fails that attempt alone: each quality is its own request, so
/// the host's remaining qualities are still tried.
fn quality_order(qualities: &[String]) -> Vec<String> {
    if qualities.is_empty() {
        return vec![DEFAULT_QUALITY.to_owned()];
    }
    let mut ordered = qualities.to_vec();
    ordered.sort_by_key(|q| std::cmp::Reverse(quality_height(q).unwrap_or(0)));
    ordered.dedup();
    ordered
}

/// `1080P` for the player, and the raw label when it names no number.
fn quality_label(quality: &str) -> Option<String> {
    match quality_height(quality) {
        Some(height) => Some(format!("{height}p")),
        None if !quality.is_empty() => Some(quality.to_owned()),
        None => None,
    }
}

fn quality_height(quality: &str) -> Option<u32> {
    let digits: String = quality.chars().filter(char::is_ascii_digit).collect();
    match digits.len() {
        3 | 4 => digits.parse().ok(),
        _ => None,
    }
}

// ---- Resolve -------------------------------------------------------------------------------

struct Link {
    url: Option<String>,
    headers: Vec<(String, String)>,
    tracks: Vec<Subtitle>,
}

/// The resolve's answer, in either of the two shapes it arrives in.
///
/// `url_response` carries a playable URL and `http_headers` naming what the CDN wants on it. For
/// `rift-streamer` that is `x-player` and `x-device-id`, and its playlist is refused without them
/// (measured: 404 and an HTML page bare, `mpegurl` with them); the host puts these on every request
/// mpv makes for the stream, so they ride the playlist and the segments alike. The synthetic `Host`
/// header Android's extension adds is not reproduced — Android measured the MP4 hosts playing with
/// no headers at all, so it was never doing anything, and a `Host` header cannot be set portably.
///
/// `ticket_response` carries no URL: that host's exchange is done by the caller, which is the one
/// place a request must be made from inside the resolve.
fn link(root: &Value) -> Link {
    let tracks: Vec<Subtitle> = root
        .get("tracks")
        .and_then(Value::as_array)
        .map(|tracks| {
            tracks
                .iter()
                .filter_map(|track| {
                    let url = ["file", "url"]
                        .iter()
                        .find_map(|k| track.get(*k).and_then(Value::as_str))
                        .filter(|u| !u.is_empty())?
                        .to_owned();
                    let label = ["label", "language"]
                        .iter()
                        .find_map(|k| track.get(*k).and_then(Value::as_str))
                        .unwrap_or("Arabic")
                        .to_owned();
                    let language = kit::language(&label);
                    Some(Subtitle { url, label, language })
                })
                .collect()
        })
        .unwrap_or_default();
    let mut tracks = tracks;
    tracks.dedup_by(|a, b| a.url == b.url);

    let headers: Vec<(String, String)> = root
        .get("http_headers")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_str().filter(|s| !s.is_empty())?.to_owned())))
                .collect()
        })
        .unwrap_or_default();

    let url = if root.get("url_response").and_then(Value::as_bool) == Some(true) {
        root.get("videoUrl").and_then(Value::as_str).filter(|u| !u.is_empty()).map(str::to_owned)
    } else {
        None
    };
    Link { url, headers, tracks }
}

/// The streamtape ticket, spent at once. See the module comment for why there is no wait here.
fn ticket_url(root: &Value) -> Option<String> {
    let file = root.get("fileId").and_then(Value::as_str).filter(|f| !f.is_empty())?;
    let ticket = root.get("ticket").and_then(Value::as_str).filter(|t| !t.is_empty())?;
    let answer = kit::get_json(&format!("{STREAMTAPE}?file={file}&ticket={ticket}"), &[]).ok()?;
    answer.get("result")?.get("url")?.as_str().filter(|u| !u.is_empty()).map(str::to_owned)
}

// ---- Provider ------------------------------------------------------------------------------

struct Rift;

impl Guest for Rift {
    fn info() -> Info {
        Info {
            id: "rift".into(),
            // Named as Android names it (ProviderCatalog's label), because "AnimeRift" alone says
            // nothing about the one thing that makes it worth a picker row.
            name: "AnimeRift (Arabic)".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            hosts: vec![
                "firebaseinstallations.googleapis.com".into(),
                "firebaseremoteconfig.googleapis.com".into(),
                // The gateway's domain rather than the host, because it has already moved once
                // (anime-rift.com → embrassme.top) and a subdomain move should not need a release.
                "embrassme.top".into(),
                "anime-rift.com".into(),
                "api.streamtape.com".into(),
            ],
        }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        if title.episode.fract() != 0.0 || title.episode < 1.0 {
            return Ok(vec![]);
        }
        let episode = title.episode as i64;
        let session = Session::get()?;
        let Some(title_id) = resolve_title_id(&session, &title)? else {
            return Ok(vec![]);
        };
        // No such episode, or the title is not the one matched: both are answers, not failures, and
        // the empty list is how this interface gives them. Android calls the second one
        // NothingForThisRequest so that its remembered match survives; the cache here is keyed by
        // AniList id and is not dropped by a failure, so the same end is reached by returning early.
        let Some(row) = episode_row(&session, &title_id, episode)? else {
            return Ok(vec![]);
        };

        let wanted = if title.dub { "ar_dub" } else { "ar_sub" };
        let root = api(
            &session,
            "/library/episode/sources",
            "POST",
            Some(&json!({ "animeId": title_id.as_str(), "episodeId": row.id.as_str(), "episode_number": episode })),
        )?;
        let hosts = hosts(&root, wanted);
        if hosts.is_empty() {
            // "No Arabic dub for this episode" is the common case, not an error: the dub is a
            // property of the title, and a long-running one stops carrying it.
            return Ok(vec![]);
        }

        let mut streams: Vec<MediaStream> = Vec::new();
        let mut subtitles: Vec<Subtitle> = Vec::new();
        let mut headers: Vec<(String, String)> = Vec::new();
        let mut server_name = String::new();
        let mut served_by: Option<&str> = None;
        let mut resolves = 0usize;

        for host in &hosts {
            if streams.len() >= MAX_STREAMS || resolves >= MAX_RESOLVES {
                break;
            }
            // One server's streams are enough. A server publishes one row per quality (cr2 had three
            // for One Piece 1100), so this waits for the server to change rather than the row; going
            // on is what lost a resolve that already held three good cr2 streams.
            if let Some(served) = served_by
                && host.provider != served
            {
                break;
            }
            let Ok(can_play) = api(
                &session,
                "/library/episode/source/can_play",
                "POST",
                Some(&json!({
                    "episodeId": row.id.as_str(),
                    "hostId": host.id.as_str(),
                    "is_download": false,
                    "event_name": PLAY_EVENT,
                })),
            ) else {
                continue;
            };
            let session_id = can_play.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_owned();
            if session_id.is_empty() {
                continue;
            }
            for quality in quality_order(&host.qualities) {
                if streams.len() >= MAX_STREAMS || resolves >= MAX_RESOLVES {
                    break;
                }
                resolves += 1;
                let Ok(direct) = api(
                    &session,
                    "/library/episode/source/direct_link",
                    "POST",
                    Some(&json!({
                        "id": host.id.as_str(),
                        "quality": quality.as_str(),
                        "with_internal_player": "1",
                        "sessionId": session_id.as_str(),
                    })),
                ) else {
                    continue;
                };
                let mut resolved = link(&direct);
                if resolved.url.is_none() && direct.get("ticket_response").and_then(Value::as_bool) == Some(true) {
                    resolved.url = ticket_url(&direct);
                }
                let Some(url) = resolved.url else { continue };
                streams.push(MediaStream {
                    format: if url.contains(".m3u8") { "hls".into() } else { "mp4".into() },
                    quality: quality_label(&quality),
                    height: quality_height(&quality),
                    url,
                });
                if headers.is_empty() {
                    headers = resolved.headers;
                    server_name = host.server.clone();
                }
                for track in resolved.tracks {
                    if !subtitles.iter().any(|s| s.url == track.url) {
                        subtitles.push(track);
                    }
                }
                served_by = Some(&host.provider);
            }
        }

        if streams.is_empty() {
            return Ok(vec![]);
        }
        Ok(vec![Server {
            name: format!("{server_name} · Arabic {}", if title.dub { "dub" } else { "sub" }),
            dub: title.dub,
            headers,
            streams,
            subtitles,
            skip_intro: row.intro,
            skip_outro: row.outro,
            thumbnails: None,
        }])
    }
}

export!(Rift);

/// A stable number from a string, for the synthetic device's build id and host name.
fn fnv(value: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The FCM token half of a device id. Nothing registers it; the service only stores it.
fn fcm_token(seed: u64) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut state = seed | 1;
    let token: String = (0..80)
        .map(|_| {
            // xorshift64*, so the same installation keeps the same token between runs.
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            ALPHABET[(state.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % ALPHABET.len()] as char
        })
        .collect();
    format!("APA91b{token}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(text: &str) -> Value {
        serde_json::from_str(text).expect("fixture is valid JSON")
    }

    #[test]
    fn reads_the_anilist_id_out_of_every_cover_shape() {
        // Measured against the live service: 7 of 7 known titles, 87 of 150 home items.
        for (url, want) in [
            ("https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx21-ELSYx3yMPcKM.jpg", 21),
            ("https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/b6702-KI4qgSMyI8Pm.png", 6702),
            ("https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/nx10033-V7xnlgAVtaVR.jpg", 10033),
            ("https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/15323.jpg", 15323),
        ] {
            assert_eq!(anilist_id(&json!({ "medium_picture": url })), Some(want), "{url}");
        }
    }

    #[test]
    fn a_proxied_cover_yields_no_id() {
        // `main_picture` is a copy under covers.embrassme.top whose path is a hash. Reading it
        // instead of medium_picture is the mistake this pins: 0 of 150 items resolved.
        assert_eq!(anilist_id(&obj(r#"{"main_picture":"https://covers.embrassme.top/9cb4/v1/cover-0.jpg"}"#)), None);
        assert_eq!(anilist_id(&obj(r#"{"medium_picture":"https://cdn.myanimelist.net/images/anime/1877/160134.jpg"}"#)), None);
    }

    #[test]
    fn an_exact_anilist_id_wins_over_a_better_title() {
        let hits = vec![
            Hit { id: "c1".into(), title: "sousou no frieren".into(), alternative: None, anilist: None },
            Hit { id: "c2".into(), title: "frieren: beyond journey's end".into(), alternative: None, anilist: Some(154587) },
        ];
        let titles = vec!["sousou no frieren".to_owned()];
        assert_eq!(best(&titles, 154587, &hits).map(|h| h.id.as_str()), Some("c2"));
    }

    #[test]
    fn a_candidate_claiming_another_anilist_entry_loses_a_tie() {
        let hits = vec![
            Hit { id: "right".into(), title: "one piece".into(), alternative: None, anilist: None },
            Hit { id: "wrong".into(), title: "one piece".into(), alternative: None, anilist: Some(999_999) },
        ];
        let titles = vec!["one piece".to_owned()];
        assert_eq!(best(&titles, 21, &hits).map(|h| h.id.as_str()), Some("right"));
    }

    #[test]
    fn a_wrong_season_cannot_take_the_right_season_s_place() {
        let titles = vec!["black clover 2nd season".to_owned()];
        let season_one = vec![Hit { id: "s1".into(), title: "black clover".into(), alternative: None, anilist: None }];
        assert!(best(&titles, 999_999, &season_one).is_none(), "season 1 must not answer a season 2 request");
    }

    #[test]
    fn nothing_close_enough_is_refused_rather_than_guessed() {
        let titles = vec!["naruto".to_owned()];
        let hits = vec![Hit { id: "x".into(), title: "bleach".into(), alternative: None, anilist: None }];
        assert!(best(&titles, 20, &hits).is_none());
    }

    #[test]
    fn hosts_are_ranked_by_the_service_then_by_quality_and_streamtape_is_last() {
        // The service ranks streamtape (3) above rift-streamer (5); it answers with a ticket and a
        // wait, so it goes after both whatever `configs` says.
        let root = obj(
            r#"{"items":[
                 {"_id":"st","provider":"streamtape","server_name":"STP","sub_title":"ar_sub","qualities":["1080P"]},
                 {"_id":"vrv","provider":"rift-streamer","server_name":"VRV","sub_title":"ar_sub","qualities":["240P","1080P"]},
                 {"_id":"cr72","provider":"cr2","server_name":"CR2","sub_title":"ar_sub","qualities":["720P"]},
                 {"_id":"cr08","provider":"cr2","server_name":"CR2","sub_title":"ar_sub","qualities":["1080P"]},
                 {"_id":"dub","provider":"cr2","server_name":"CR2","sub_title":"ar_dub","qualities":["1080P"]},
                 {"_id":"italian","provider":"cr2","server_name":"CR2","sub_title":"it_dub","qualities":["1080P"]}
               ],
               "configs":[{"server_key":"cr2","priority":2},{"server_key":"streamtape","priority":3},{"server_key":"rift-streamer","priority":5}]}"#,
        );
        let rows = hosts(&root, "ar_sub");
        let order: Vec<&str> = rows.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(order, vec!["cr08", "cr72", "vrv", "st"], "1080p before 720p, then rift-streamer, then the ticket host");

        // Arabic only: the Italian dub row and the Arabic dub row are not subtitle answers.
        assert!(rows.iter().all(|h| h.audio != "it_dub" && h.audio != "ar_dub"));
        let dubs = hosts(&root, "ar_dub");
        assert_eq!(dubs.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), vec!["dub"]);
    }

    #[test]
    fn an_untagged_row_is_taken_for_the_subtitle_track_and_ranked_behind_a_tagged_one() {
        let root = obj(
            r#"{"items":[
                 {"_id":"blank","provider":"cr2","server_name":"CR2","sub_title":"","qualities":["1080P"]},
                 {"_id":"tagged","provider":"cr2","server_name":"CR2","sub_title":"ar_sub","qualities":["240P"]}
               ]}"#,
        );
        let rows = hosts(&root, "ar_sub");
        assert_eq!(rows.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), vec!["tagged", "blank"]);
        assert!(hosts(&root, "ar_dub").is_empty(), "a blank tag says nothing about audio");
    }

    #[test]
    fn the_device_timestamp_is_a_zoneless_local_looking_stamp() {
        assert_eq!(timestamp_at(0, 0), "1970-01-01T00:00:00.000000");
        // 2026-01-01T00:00:00Z, and a leap day, so the calendar conversion is exercised on both.
        assert_eq!(timestamp_at(1_767_225_600, 123), "2026-01-01T00:00:00.000123");
        assert_eq!(timestamp_at(1_709_164_800, 0), "2024-02-29T00:00:00.000000");
        assert_eq!(timestamp_at(1_767_225_600 + 3661, 999_999), "2026-01-01T01:01:01.999999");
    }

    #[test]
    fn episodes_round_trip_through_the_cache_encoding_with_their_windows() {
        let rows = vec![Episode { id: "a".into(), number: 1, intro: Some((31.0, 111.0)), outro: Some((1376.0, 1447.0)) }];
        let decoded = decode_episodes(&encode_episodes(&rows));
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].number, 1);
        assert_eq!(decoded[0].intro, Some((31.0, 111.0)));
        assert_eq!(decoded[0].outro, Some((1376.0, 1447.0)));
        // A row with no window must come back without one rather than with a zero-length one.
        assert_eq!(decode_episodes(r#"[{"id":"b","number":2,"intro":null,"outro":null}]"#)[0].intro, None);
    }

    #[test]
    fn qualities_are_asked_for_highest_first_and_labelled_for_the_player() {
        assert_eq!(quality_order(&["240P".into(), "1080P".into(), "720P".into()]), vec!["1080P", "720P", "240P"]);
        assert_eq!(quality_order(&[]), vec!["720P"]);
        assert_eq!(quality_label("1080P").as_deref(), Some("1080p"));
        assert_eq!(quality_height("720p"), Some(720));
        assert_eq!(quality_height("auto"), None);
        assert_eq!(quality_label("auto").as_deref(), Some("auto"));
    }

    #[test]
    fn a_url_response_carries_its_headers_and_a_ticket_response_carries_none() {
        let hls = link(&obj(
            r#"{"url_response":true,"videoUrl":"https://media-1.rift-content.com/video/hls/x/one_piece_ep_1.m3u8?t=1",
                "http_headers":{"x-player":"in-app","x-device-id":"abc"},"tracks":[]}"#,
        ));
        assert!(hls.url.as_deref().unwrap_or_default().ends_with("one_piece_ep_1.m3u8?t=1"));
        assert!(hls.headers.iter().any(|(k, v)| k == "x-player" && v == "in-app"));

        let mp4 = link(&obj(r#"{"url_response":true,"videoUrl":"https://media.spacetoonfanboy.xyz/a/1080p/v1/master.mp4","tracks":[]}"#));
        assert!(mp4.url.as_deref().unwrap_or_default().ends_with("master.mp4"));
        assert!(mp4.headers.is_empty(), "an MP4 host needs no headers");

        let ticket = link(&obj(r#"{"ticket_response":true,"fileId":"f","ticket":"t","wait_time":5,"tracks":[]}"#));
        assert_eq!(ticket.url, None, "a ticket is spent by the caller, not here");
    }
}
