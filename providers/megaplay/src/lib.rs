//! MegaPlay (megaplay.buzz), AniKoto's player, reached directly: the same backend as Miruro's
//! `bee`, kept here so it plays when Miruro is down (the reason `pahe-direct` exists too). The id
//! is `megaplay` because `anikoto` is a Miruro provider key.
//!
//! 1. `/stream/ani/{anilistId}/{ep}/{sub|dub}` (Referer `hianimes.re`) → a page with `data-id`, the
//!    file id. MegaPlay doesn't map every show to an AniList id, so `/stream/mal/{malId}/…` next.
//! 2. `/{route}?id=…&id=…` → `tracks` (VTT), intro/outro, and the source: in the clear under
//!    `sources` on routes that still do that, otherwise `enc`, AES-256-CBC with a key and IV pinned
//!    from MegaPlay's own `newclient.min.js` (as on Android: recovering them per resolve would be a
//!    brute force over the script's literals). The route names are read out of that script, longest
//!    first (a successor is the old name plus a suffix), cached a day; `stream/getSources` is the
//!    fallback.
//! 3. The HLS master on a pool CDN host, gated on `Referer: https://megaplay.buzz/`.
//!
//! Port of Android's MegaPlay path in `AnivexaClient` (its "anikoto" server).

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use provider_kit::{self as kit, DAY, types::*};
use serde_json::Value;

wit_bindgen::generate!({
    path: "../../wit",
    world: "provider",
    with: { "anilili:provider/host@1.0.0": provider_kit::host, "anilili:provider/types@1.0.0": provider_kit::types },
});

const ORIGIN: &str = "https://megaplay.buzz";
const EMBED_REFERER: &str = "https://hianimes.re/";
const FALLBACK_ROUTE: &str = "stream/getSources";
/// The cipher over `enc`, as `newclient.min.js` spells it (Android, 2026-09-17; still valid
/// 2026-10-04). The key is zero-padded to 32 bytes.
const ENC_KEY: &[u8] = b"i?LMTAx0Q6,:}50U";
const ENC_IV: &[u8; 16] = b"W0;27ToaUpl_P%'c";

struct MegaPlay;

/// `data-id` on an embed page, following one nested iframe if the page wraps another.
fn file_id(url: &str) -> Result<Option<String>, String> {
    let response = kit::send("GET", url, &[("Referer", EMBED_REFERER)], None)?;
    if response.status == 404 {
        return Ok(None);
    }
    let page = String::from_utf8_lossy(&response.body).into_owned();
    if let Some(id) = data_id(&page) {
        return Ok(Some(id));
    }
    let Some(iframe) = kit::tags(&page, "iframe").map(|t| kit::attr(t, "src")).find(|s| !s.is_empty()) else { return Ok(None) };
    let nested = kit::absolute_url(url, &iframe);
    if kit::host_of(&nested) != kit::host_of(ORIGIN) {
        return Ok(None);
    }
    Ok(data_id(&kit::get_text(&nested, &[("Referer", EMBED_REFERER)])?))
}

fn data_id(page: &str) -> Option<String> {
    kit::tags(page, "div")
        .chain(kit::tags(page, "span"))
        .map(|t| kit::attr(t, "data-id"))
        .find(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The source routes named in MegaPlay's client script, longest first, then the fallback.
fn routes(embed_page_url: &str) -> Vec<String> {
    let discovered = kit::cached("routes", DAY, 3600, || {
        let script = kit::get_text(&format!("{ORIGIN}/lib/newclient.min.js"), &[("Referer", embed_page_url)])?;
        let mut found: Vec<String> = Vec::new();
        for (i, _) in script.match_indices("stream/getSources") {
            let route: String = script[i..].chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '-' | '_')).collect();
            if !found.contains(&route) {
                found.push(route);
            }
        }
        found.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
        Ok((!found.is_empty()).then(|| found.join(",")))
    });
    let mut routes: Vec<String> = discovered.ok().flatten().map(|r| r.split(',').map(str::to_owned).collect()).unwrap_or_default();
    if !routes.iter().any(|r| r == FALLBACK_ROUTE) {
        routes.push(FALLBACK_ROUTE.to_owned());
    }
    routes
}

fn decrypt(enc: &str) -> Option<String> {
    let mut data = kit::base64_decode(enc).filter(|d| !d.is_empty())?;
    let mut key = [0u8; 32];
    key[..ENC_KEY.len()].copy_from_slice(ENC_KEY);
    let plain = cbc::Decryptor::<aes::Aes256>::new(&key.into(), ENC_IV.into()).decrypt_padded_mut::<Pkcs7>(&mut data).ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

/// The manifest URL in a sources answer, in the clear or encrypted.
fn source_file(answer: &Value) -> Option<String> {
    let clear = match answer.get("sources") {
        Some(Value::Object(o)) => o.get("file").and_then(Value::as_str),
        Some(Value::Array(a)) => a.first().and_then(|s| s.get("file")).and_then(Value::as_str),
        _ => None,
    };
    if let Some(file) = clear.filter(|f| !f.is_empty()) {
        return Some(file.to_owned());
    }
    let enc = answer.get("enc").and_then(Value::as_str).filter(|e| !e.is_empty())?;
    let Some(plain) = decrypt(enc) else {
        // The next key rotation: worth a line in the log rather than a silent miss.
        kit::host::log(&format!("megaplay enc did not decrypt (length {})", enc.len()));
        return None;
    };
    let file = serde_json::from_str::<Value>(&plain).ok()?.get("file")?.as_str()?.to_owned();
    (!file.is_empty()).then_some(file)
}

fn window(answer: &Value, key: &str) -> Option<(f64, f64)> {
    let w = answer.get(key)?;
    let (start, end) = (w.get("start")?.as_f64()?, w.get("end")?.as_f64()?);
    (end > start).then_some((start, end))
}

impl Guest for MegaPlay {
    fn info() -> Info {
        Info { id: "megaplay".into(), name: "mega".into(), version: env!("CARGO_PKG_VERSION").into(), hosts: vec!["megaplay.buzz".into()] }
    }

    fn servers(title: Title) -> Result<Vec<Server>, String> {
        let audio = if title.dub { "dub" } else { "sub" };
        let episode = kit::episode_label(title.episode);
        let mut embed = format!("{ORIGIN}/stream/ani/{}/{episode}/{audio}", title.anilist_id);
        let mut id = file_id(&embed)?;
        if id.is_none()
            && let Some(mal) = title.mal_id
        {
            embed = format!("{ORIGIN}/stream/mal/{mal}/{episode}/{audio}");
            id = file_id(&embed)?;
        }
        let Some(id) = id else { return Ok(vec![]) };

        let mut answer = Value::Null;
        let mut file = None;
        for route in routes(&embed) {
            let url = format!("{ORIGIN}/{route}?id={id}&id={id}");
            let Ok(reply) = kit::get_json(&url, &[("Referer", "https://megaplay.buzz/"), ("X-Requested-With", "XMLHttpRequest")]) else {
                continue;
            };
            // A retired route still answers, with tracks and skip times but no file: keep the
            // first answer for those, and move on until one names a file.
            if answer.is_null() {
                answer = reply.clone();
            }
            if let Some(found) = source_file(&reply) {
                answer = reply;
                file = Some(found);
                break;
            }
        }
        let Some(file) = file else { return Err("MegaPlay named no stream on any route".into()) };

        let mut subtitles: Vec<Subtitle> = Vec::new();
        let mut thumbnails = None;
        for track in answer.get("tracks").and_then(Value::as_array).into_iter().flatten() {
            let kind = track.get("kind").and_then(Value::as_str).unwrap_or("captions");
            // The JW Player convention for seek previews. None sent on 2026-10-06, but this
            // backend's relatives do send them.
            if kind == "thumbnails" {
                thumbnails = track.get("file").and_then(Value::as_str).filter(|u| u.starts_with("http")).map(str::to_owned);
                continue;
            }
            let Some(url) = track.get("file").and_then(Value::as_str).filter(|_| matches!(kind, "captions" | "subtitles")) else {
                continue;
            };
            let label = track.get("label").and_then(Value::as_str).unwrap_or("Subtitles");
            let subtitle = Subtitle { url: url.into(), label: label.into(), language: kit::language(label) };
            if track.get("default").and_then(Value::as_bool) == Some(true) {
                subtitles.insert(0, subtitle);
            } else {
                subtitles.push(subtitle);
            }
        }
        kit::host::log(&format!("megaplay resolved anilist={} ep={episode} {audio} file-id={id}", title.anilist_id));
        Ok(vec![Server {
            name: "MegaPlay".into(),
            dub: title.dub,
            headers: vec![("Referer".into(), "https://megaplay.buzz/".into()), ("Origin".into(), ORIGIN.into())],
            streams: vec![MediaStream { url: file, format: "hls".into(), quality: None, height: None }],
            subtitles,
            skip_intro: window(&answer, "intro"),
            skip_outro: window(&answer, "outro"),
            thumbnails,
        }])
    }
}

export!(MegaPlay);
