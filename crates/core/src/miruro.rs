//! Miruro's catalog API: AniList id → Miruro title → episodes → every server for an episode.
//!
//! Plain GETs under `{mirror}/api/`. The catalog paths answer `application/octet-stream`: the
//! bytes XOR'd with the ASCII of `miruro/catalog`, then gzip (the site's own decoder; the key is a
//! literal in its bundle). Errors are RFC 7807 problem documents. Traps, learned on Android:
//!
//! - an unrecognised query is `400 Unsupported catalog request`, so the query strings are literal
//!   requirements, not defaults: `limit=100` for the title lookup, and `limit=10000` alone for the
//!   episode list (the site's own `limit.complete`). `kind=regular` was required there until
//!   2026-10-05 and is now refused, so specials come in the list and are filtered out here;
//! - `embed` is an object `{url}`, not a string;
//! - 4xx is the API's answer and every mirror serves the same API, so only transport failures and
//!   5xx move on to the next mirror.

use std::collections::BTreeMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::model::{Audio, Episode, EpisodeList, Server, SkipTimes, Sources, Stream, Subtitle, Window};
use crate::net::{Http, USER_AGENT};
use crate::{Error, Result};

const SERVICE: &str = "Miruro";
const CATALOG_KEY: &[u8] = b"miruro/catalog";
const CONFIG_TTL: Duration = Duration::from_secs(60 * 60);

pub const MIRRORS: &[&str] =
    &["https://www.miruro.to", "https://www.miruro.tv", "https://www.miruro.bz", "https://www.miruro.ru", "https://www.miruro.cx"];

/// One entry of `config.streaming`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderConfig {
    pub key: String,
    pub label: String,
    pub visible: bool,
    /// For an embed, the provider whose servers carry its iframe pages.
    pub parent: Option<String>,
    pub is_embed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Title {
    pub id: String,
    pub sub_count: Option<i32>,
    pub dub_count: Option<i32>,
}

pub struct Miruro {
    http: Http,
    preferred: Mutex<Option<&'static str>>,
    config: Mutex<Option<(Instant, Vec<ProviderConfig>)>>,
}

impl Miruro {
    pub fn new(http: Http) -> Self {
        Self { http, preferred: Mutex::new(None), config: Mutex::new(None) }
    }

    /// GET `{mirror}/api/{path}`, decoded to JSON, walking mirrors on transport failure / 5xx.
    pub async fn get(&self, path: &str) -> Result<Value> {
        let preferred = *self.preferred.lock().unwrap();
        let order = preferred.into_iter().chain(MIRRORS.iter().copied().filter(|m| Some(*m) != preferred));
        let mut last = None;
        for origin in order {
            let url = format!("{origin}/api/{path}");
            let referer = format!("{origin}/");
            let headers = [
                ("User-Agent", USER_AGENT),
                ("Referer", referer.as_str()),
                ("Accept", "application/json, application/octet-stream;q=0.9, */*;q=0.8"),
                ("Sec-Fetch-Site", "same-origin"),
                ("Sec-Fetch-Mode", "cors"),
                ("Sec-Fetch-Dest", "empty"),
            ];
            match self.http.get_bytes(SERVICE, &url, &headers, Duration::from_secs(20)).await {
                Ok((body, content_type)) => {
                    *self.preferred.lock().unwrap() = Some(origin);
                    let text = if content_type.as_deref().is_some_and(|t| t.starts_with("application/octet-stream")) {
                        decode_catalog_body(&body)?
                    } else {
                        String::from_utf8_lossy(&body).into_owned()
                    };
                    return serde_json::from_str(&text).map_err(|e| Error::decode(SERVICE, e));
                }
                Err(e @ Error::Http { status, .. }) if status < 500 => return Err(e),
                Err(e) => {
                    tracing::warn!(%origin, %path, error = %e, "Miruro mirror failed");
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or_else(|| Error::Network("no Miruro mirror answered".into())))
    }

    pub async fn config(&self) -> Result<Vec<ProviderConfig>> {
        if let Some((at, config)) = self.config.lock().unwrap().as_ref()
            && at.elapsed() < CONFIG_TTL
        {
            return Ok(config.clone());
        }
        let parsed = parse_config(&self.get("config").await?);
        *self.config.lock().unwrap() = Some((Instant::now(), parsed.clone()));
        Ok(parsed)
    }

    pub async fn title(&self, anilist_id: i64) -> Result<Option<Title>> {
        let value = self.get(&format!("v1/anime?anilist_id_in={anilist_id}&limit=100")).await?;
        Ok(parse_title(&value))
    }

    pub async fn episodes(&self, title: &Title) -> Result<EpisodeList> {
        let value = self.get(&format!("v1/anime/{}/episodes?limit=10000", title.id)).await?;
        Ok(EpisodeList { episodes: parse_episodes(&value), sub_count: title.sub_count, dub_count: title.dub_count })
    }

    /// Every server for one episode, in the site's provider order, embeds last.
    pub async fn sources(&self, title_id: &str, number: f64, skip: Option<SkipTimes>) -> Result<Sources> {
        let config = self.config().await?;
        let play = self.get(&format!("v1/anime/{title_id}/episodes/{}/play", number_text(number))).await?;
        Ok(Sources { servers: parse_servers(&config, &play), skip })
    }
}

pub fn decode_catalog_body(bytes: &[u8]) -> Result<String> {
    let unmasked: Vec<u8> = bytes.iter().enumerate().map(|(i, b)| b ^ CATALOG_KEY[i % CATALOG_KEY.len()]).collect();
    let mut text = String::new();
    flate2::read::GzDecoder::new(unmasked.as_slice())
        .read_to_string(&mut text)
        .map_err(|e| Error::decode(SERVICE, format!("catalog body: {e}")))?;
    Ok(text)
}

/// An episode number as the API's paths write it: `1`, not `1.0`.
pub fn number_text(number: f64) -> String {
    if number.fract() == 0.0 { format!("{}", number as i64) } else { number.to_string() }
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

fn num_of(v: &Value, key: &str) -> Option<f64> {
    match v.get(key)? {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

pub fn parse_config(root: &Value) -> Vec<ProviderConfig> {
    let order: Vec<&str> =
        root.get("providerOrder").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    let mut entries: Vec<ProviderConfig> = root
        .get("streaming")
        .and_then(Value::as_object)
        .map(|streaming| {
            streaming
                .iter()
                .filter(|(_, v)| v.is_object())
                .map(|(key, v)| ProviderConfig {
                    key: key.clone(),
                    label: str_of(v, "label").unwrap_or_else(|| key.clone()),
                    visible: v.get("visible").and_then(Value::as_bool).unwrap_or(true),
                    parent: str_of(v, "parent"),
                    is_embed: str_of(v, "relationship").as_deref() == Some("embed"),
                })
                .collect()
        })
        .unwrap_or_default();
    let rank = |key: &str| order.iter().position(|k| *k == key).unwrap_or(usize::MAX);
    entries.sort_by_key(|e| rank(&e.key));
    entries
}

pub fn parse_title(root: &Value) -> Option<Title> {
    let first = root.get("data")?.as_array()?.first()?;
    let counts = first.get("episode_counts");
    Some(Title {
        id: str_of(first, "id")?,
        sub_count: counts.and_then(|c| num_of(c, "sub")).map(|n| n as i32),
        dub_count: counts.and_then(|c| num_of(c, "dub")).map(|n| n as i32),
    })
}

pub fn parse_episodes(root: &Value) -> Vec<Episode> {
    root.get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                // The numbered run only: specials ("kind": "special") have numbers of their own that
                // would collide with it. A row without a kind is taken as regular.
                .filter(|e| str_of(e, "kind").is_none_or(|k| k == "regular"))
                .filter_map(|e| {
                    Some(Episode {
                        number: num_of(e, "episode_number")?,
                        title: str_of(e, "title"),
                        synopsis: str_of(e, "synopsis"),
                        thumbnail: str_of(e, "thumbnail_url"),
                        aired_on: str_of(e, "aired_on"),
                        duration_seconds: num_of(e, "duration_seconds"),
                        filler: is_filler(str_of(e, "canon_type").as_deref()),
                        skip: parse_skip(e.get("skip_times")),
                        airs_at: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn is_filler(canon_type: Option<&str>) -> bool {
    matches!(canon_type, Some(t) if t.contains("filler"))
}

/// Kinds seen: `op`, `ed`, `mixed_op`, `mixed_ed`. The plain kind wins: a mixed window overlaps
/// the episode's own content.
fn parse_skip(windows: Option<&Value>) -> Option<SkipTimes> {
    let windows = windows?.as_array()?;
    let pick = |kind: &str| {
        let find = |k: &str| {
            windows.iter().find(|w| w.get("kind").and_then(Value::as_str) == Some(k)).and_then(|w| {
                let (start, end) = (num_of(w, "start_seconds")?, num_of(w, "end_seconds")?);
                (end > start).then_some(Window { start, end })
            })
        };
        find(kind).or_else(|| find(&format!("mixed_{kind}")))
    };
    let skip = SkipTimes { intro: pick("op"), outro: pick("ed"), recap: pick("recap") };
    (skip.intro.is_some() || skip.outro.is_some() || skip.recap.is_some()).then_some(skip)
}

fn embed_url(v: &Value) -> Option<String> {
    match v.get("embed")? {
        Value::Object(o) => o.get("url").and_then(Value::as_str),
        Value::String(s) => Some(s.as_str()),
        _ => None,
    }
    .filter(|s| !s.trim().is_empty())
    .map(str::to_owned)
}

/// Flattens a `play` response into servers: per audio track, per visible provider (site order),
/// per server. Embed-only servers come after every native one.
pub fn parse_servers(config: &[ProviderConfig], play: &Value) -> Vec<Server> {
    let label_of = |key: &str| config.iter().find(|c| c.key == key).map(|c| c.label.clone()).unwrap_or_else(|| key.to_owned());
    let visible = |key: &str| config.iter().find(|c| c.key == key).is_none_or(|c| c.visible);
    let rank = |key: &str| config.iter().position(|c| c.key == key).unwrap_or(usize::MAX);

    let mut servers = Vec::new();
    for track in play.get("tracks").and_then(Value::as_array).into_iter().flatten() {
        let Some(track_name) = str_of(track, "track") else { continue };
        let audio = if track_name == "dub" { Audio::Dub } else { Audio::Sub };
        for provider in track.get("providers").and_then(Value::as_array).into_iter().flatten() {
            let Some(key) = str_of(provider, "provider") else { continue };
            if !visible(&key) {
                continue;
            }
            let subtitles: Vec<Subtitle> = provider
                .get("subtitles")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|s| {
                    Some(Subtitle {
                        url: str_of(s, "file").or_else(|| str_of(s, "url"))?,
                        label: str_of(s, "label").or_else(|| str_of(s, "language")).unwrap_or_else(|| "Subtitle".into()),
                        language: str_of(s, "language").unwrap_or_else(|| "en".into()),
                    })
                })
                .collect();
            // `{"sprite": …, "vtt": …}`, for the provider's servers alike. Only the VTT is used:
            // `sprite` has been null wherever `vtt` was set (2026-10-06).
            let thumbnails = provider.get("thumbnails").and_then(|t| str_of(t, "vtt"));
            for server in provider.get("servers").and_then(Value::as_array).into_iter().flatten() {
                let name = str_of(server, "server").unwrap_or_default();
                let headers: BTreeMap<String, String> = server
                    .get("headers")
                    .and_then(Value::as_object)
                    .map(|h| h.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned()))).collect())
                    .unwrap_or_default();
                let streams: Vec<Stream> = server
                    .get("streams")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|s| {
                        let url = str_of(s, "url")?;
                        let format = str_of(s, "format").unwrap_or_else(|| {
                            if url.contains(".mpd") {
                                "dash"
                            } else if url.contains(".mp4") {
                                "mp4"
                            } else {
                                "hls"
                            }
                            .into()
                        });
                        let height = s.get("resolution").and_then(|r| num_of(r, "height")).map(|h| h as u32);
                        Some(Stream { url, format, quality: str_of(s, "quality"), height })
                    })
                    .collect();
                let embed =
                    embed_url(server).or_else(|| server.get("streams").and_then(Value::as_array).into_iter().flatten().find_map(embed_url));
                if streams.is_empty() && embed.is_none() {
                    continue;
                }
                servers.push(Server {
                    id: format!("{key}/{track_name}/{name}"),
                    label: label_of(&key),
                    provider: key.clone(),
                    name,
                    track: track_name.clone(),
                    audio,
                    headers,
                    streams,
                    embed,
                    subtitles: subtitles.clone(),
                    thumbnails: thumbnails.clone(),
                });
            }
        }
    }
    // Native first, then the site's provider order; stable within a provider.
    servers.sort_by_key(|s| (!s.is_native(), rank(&s.provider)));
    servers
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config() -> Vec<ProviderConfig> {
        parse_config(&json!({
            "providerOrder": ["anikoto", "animepahe", "allmanga"],
            "streaming": {
                "animepahe": {"label": "kiwi", "visible": true},
                "anikoto": {"label": "bee", "visible": true},
                "allmanga": {"label": "ally", "visible": false},
                "bun": {"label": "bun", "visible": true, "parent": "anikoto", "relationship": "embed"}
            }
        }))
    }

    #[test]
    fn config_follows_provider_order() {
        let keys: Vec<_> = config().into_iter().map(|c| c.key).collect();
        assert_eq!(keys, ["anikoto", "animepahe", "allmanga", "bun"]);
    }

    #[test]
    fn decodes_catalog_body() {
        use std::io::Write;
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(br#"{"ok":true}"#).unwrap();
        let masked: Vec<u8> = gz.finish().unwrap().iter().enumerate().map(|(i, b)| b ^ CATALOG_KEY[i % CATALOG_KEY.len()]).collect();
        assert_eq!(decode_catalog_body(&masked).unwrap(), r#"{"ok":true}"#);
    }

    #[test]
    fn skip_prefers_plain_windows() {
        let eps = parse_episodes(&json!({"data": [{
            "episode_number": 1,
            "canon_type": "manga_canon",
            "skip_times": [
                {"kind": "mixed_op", "start_seconds": 58.0, "end_seconds": 148.0},
                {"kind": "op", "start_seconds": 3.0, "end_seconds": 93.0},
                {"kind": "mixed_ed", "start_seconds": 1400.0, "end_seconds": 1490.0}
            ]
        }]}));
        let skip = eps[0].skip.unwrap();
        assert_eq!(skip.intro, Some(Window { start: 3.0, end: 93.0 }));
        assert_eq!(skip.outro, Some(Window { start: 1400.0, end: 1490.0 }));
        assert!(!eps[0].filler);
    }

    #[test]
    fn servers_native_first_hidden_dropped() {
        let play = json!({"tracks": [
            {"track": "sub", "providers": [
                {"provider": "allmanga", "servers": [{"server": "x", "streams": [{"url": "https://a/x.m3u8"}]}]},
                {"provider": "anikoto", "thumbnails": {"sprite": null, "vtt": "https://m/preview.vtt"}, "servers": [
                    {"server": "embed-only", "embed": {"url": "https://e/1"}},
                    {"server": "HD-1", "headers": {"Referer": "https://m/"}, "streams": [{"url": "https://m/master.m3u8", "format": "hls"}]}
                ]},
                {"provider": "animepahe", "servers": [{"server": "animepahe", "streams": [
                    {"url": "https://v/uwu.m3u8", "format": "hls", "quality": "1080p", "resolution": {"width": 1920, "height": 1080}}
                ]}]}
            ]},
            {"track": "dub", "providers": [{"provider": "animepahe", "servers": [{"server": "animepahe", "streams": [{"url": "https://v/d.m3u8"}]}]}]}
        ]});
        let servers = parse_servers(&config(), &play);
        let ids: Vec<_> = servers.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["anikoto/sub/HD-1", "animepahe/sub/animepahe", "animepahe/dub/animepahe", "anikoto/sub/embed-only"]);
        assert_eq!(servers[0].label, "bee");
        assert_eq!(servers[0].headers.get("Referer").map(String::as_str), Some("https://m/"));
        assert_eq!(servers[0].thumbnails.as_deref(), Some("https://m/preview.vtt"));
        assert_eq!(servers[1].streams[0].height, Some(1080));
        assert_eq!(servers[1].thumbnails, None);
        assert_eq!(servers[2].audio, Audio::Dub);
    }

    #[test]
    fn number_text_drops_trailing_zero() {
        assert_eq!(number_text(1.0), "1");
        assert_eq!(number_text(12.5), "12.5");
    }
}
