//! Providers as WebAssembly components (`wit/provider.wit`), run in wasmtime.
//!
//! A provider is a sandbox: its only way out is the `http` import, served by the app's client
//! and limited to the hosts the provider declares in `info.hosts`. Each call gets a fresh store
//! with a memory cap and a CPU deadline (epoch interruption), so a broken or hostile module can
//! neither hang the app nor reach anything it did not declare.
//!
//! Modules load from the bundled `providers/` resource directory and from the user's data
//! directory; when both hold the same provider id, the higher version wins. That is how a provider
//! fix ships without an app release: a newer, checksum-verified module dropped into the data
//! directory (see [`crate::update`] for the signed channel that delivers them).

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Cache, CacheConfig, Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::model::{Audio, Server, SkipTimes, Stream, Subtitle, Window};
use crate::net::{Http, USER_AGENT};
use crate::{Error, Result};

/// Generated from `wit/provider.wit`, in its own module so its names (`Server`, `Title`,
/// `Result`) do not collide with the crate's.
mod wit {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "provider",
        imports: { default: async },
        exports: { default: async },
    });
}

use wit::Provider;
use wit::anilili::provider::host::{Request, Response};
use wit::anilili::provider::types::{Info, Title};

const MEMORY_LIMIT: usize = 64 << 20;
/// The longest one `browser-fetch` may take: a challenge to clear, then the request in the page.
const BROWSER_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
/// Epoch ticks (100 ms each) a call may run: network waits count, so this is generous.
const CALL_DEADLINE_TICKS: u64 = 450;

/// A browser engine the app lends providers for challenge-gated sites (the core has no webview;
/// the desktop app implements this with a tiny WebView2 window).
pub trait BrowserFetch: Send + Sync {
    #[allow(clippy::type_complexity)]
    fn fetch(
        &self,
        origin: String,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(u16, Vec<u8>), String>> + Send>>;
}

struct HostState {
    browser: Option<Arc<dyn BrowserFetch>>,
    store: Option<Arc<crate::store::Store>>,
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
    http: Http,
    hosts: Vec<String>,
    provider: String,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
    }
}

fn host_allowed(url: &str, hosts: &[String]) -> bool {
    let Ok(parsed) = url::Url::parse(url) else { return false };
    if parsed.scheme() != "https" {
        return false;
    }
    let Some(host) = parsed.host_str() else { return false };
    hosts.iter().any(|allowed| host == allowed || host.ends_with(&format!(".{allowed}")))
}

impl wit::anilili::provider::host::Host for HostState {
    async fn http(&mut self, req: Request) -> std::result::Result<Response, String> {
        if !host_allowed(&req.url, &self.hosts) {
            return Err(format!("{} may not reach {}", self.provider, req.url));
        }
        let method = wreq::Method::from_bytes(req.method.as_bytes()).map_err(|e| e.to_string())?;
        // The provider's own User-Agent wins when it sets one. An app API can choose between serving
        // its API and serving its website by the first User-Agent it sees, and appending ours in
        // front of the provider's put the browser one first: measurably, AnimeRift answered 302 to
        // its site (and then a Next.js 404 page) instead of 201 to /auth/register/device until its
        // Dart/3.10 client UA led (2026-10-06). Providers that set none still get ours.
        let mut builder = self.http.client().request(method, &req.url).timeout(Duration::from_secs(20));
        if !req.headers.iter().any(|(name, _)| name.eq_ignore_ascii_case("user-agent")) {
            builder = builder.header("User-Agent", USER_AGENT);
        }
        for (name, value) in &req.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(body) = req.body {
            builder = builder.body(body);
        }
        let response = builder.send().await.map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let headers =
            response.headers().iter().filter_map(|(k, v)| v.to_str().ok().map(|v| (k.as_str().to_owned(), v.to_owned()))).collect();
        let body = response.bytes().await.map_err(|e| e.to_string())?.to_vec();
        Ok(Response { status, headers, body })
    }

    async fn browser_fetch(&mut self, origin: String, req: Request) -> std::result::Result<Response, String> {
        if !host_allowed(&req.url, &self.hosts) || !host_allowed(&origin, &self.hosts) {
            return Err(format!("{} may not reach {}", self.provider, req.url));
        }
        let browser = self.browser.clone().ok_or("no browser engine available")?;
        // The epoch deadline only counts while guest code runs, so a host call that never returns
        // would hold the provider forever; bound it here.
        let fetch = browser.fetch(origin, req.method, req.url.clone(), req.headers, req.body);
        let (status, body) =
            tokio::time::timeout(BROWSER_FETCH_TIMEOUT, fetch).await.map_err(|_| format!("browser fetch of {} timed out", req.url))??;
        Ok(Response { status, headers: vec![], body })
    }

    async fn log(&mut self, message: String) {
        tracing::info!(provider = %self.provider, "{message}");
    }

    async fn cache_get(&mut self, key: String) -> Option<String> {
        let store = self.store.as_ref()?;
        let entry: (String, i64) = store.cache_get_stale(&format!("plugin:{}:{key}", self.provider))?;
        (crate::store::now() < entry.1).then_some(entry.0)
    }

    async fn cache_set(&mut self, key: String, value: String, ttl_seconds: u32) {
        if let Some(store) = &self.store {
            let expires = crate::store::now() + i64::from(ttl_seconds);
            let _ = store.cache_put(&format!("plugin:{}:{key}", self.provider), &(value, expires));
        }
    }
}

impl wit::anilili::provider::types::Host for HostState {}

/// One loaded provider module.
pub struct Plugin {
    pub info: PluginInfo,
    pub path: PathBuf,
    component: Component,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub hosts: Vec<String>,
}

/// Provider order after Miruro's servers, best first, measured 2026-10-04: MegaPlay answers in
/// 0.2 s with sub, dub and many subtitles; UniqueStream carries Crunchyroll's encodes but goes
/// through the proxy; Zoko's uncached segments are slow (Android demoted it for that);
/// AnimeOnsen is DASH and sub only; AnimeHeaven is hard-subbed MP4. Unknown ids (a provider
/// added through the update channel) come after these, in load order.
/// Rift is last on purpose, and is meant to be picked by hand: it is Arabic only — its subtitles
/// are burned into the picture — and Android keeps it out of every automatic choice unless the
/// viewer asked for Arabic (`ProviderCatalog.declaredLanguages`). The desktop has no per-provider
/// language anywhere, so position is the only lever there is, and last means an English viewer
/// reaches it only after every other server has failed. Give this list a language column before
/// that is fixed properly.
pub const PRIORITY: &[&str] = &["megaplay", "anipm", "uniquestream", "anizone", "pahe-direct", "zoko", "animeonsen", "animeheaven", "rift"];

/// Providers asked one query at a time. AnimeRift's gateway rate-limits each endpoint tightly: a
/// sub and a dub search arriving together had one refused (two runs in three, measured
/// 2026-10-06), and on a first run both calls registered a device of their own at once, leaving
/// one behind on the service. In turn, the second call finds the first one's session and search.
pub const ONE_AT_A_TIME: &[&str] = &["rift"];

fn priority(provider: &str) -> usize {
    PRIORITY.iter().position(|p| *p == provider).unwrap_or(PRIORITY.len())
}

/// What the app asks a provider for.
#[derive(Debug, Clone)]
pub struct TitleQuery {
    pub anilist_id: i64,
    pub mal_id: Option<i64>,
    pub titles: Vec<String>,
    pub episode: f64,
    pub audio: Audio,
}

pub struct Plugins {
    browser: Option<Arc<dyn BrowserFetch>>,
    store: Option<Arc<crate::store::Store>>,
    engine: Engine,
    linker: Linker<HostState>,
    http: Http,
    plugins: Vec<Arc<Plugin>>,
}

impl Plugins {
    /// Loads every `.wasm` in `dirs` (later directories override earlier ones per id when newer).
    /// With `cache_dir`, compiled code is kept there: compiling all the providers took about 6 s
    /// at every launch (debug build), during which the app could do nothing; from the cache they
    /// load in milliseconds. Entries are keyed by the module and wasmtime's own version and
    /// settings, so a changed provider or wasmtime upgrade simply compiles again.
    pub async fn load(
        http: Http,
        dirs: &[PathBuf],
        browser: Option<Arc<dyn BrowserFetch>>,
        store: Option<Arc<crate::store::Store>>,
        cache_dir: Option<&Path>,
    ) -> Result<Self> {
        let mut config = Config::new();
        config.epoch_interruption(true).wasm_component_model(true);
        if let Some(dir) = cache_dir {
            let mut cache = CacheConfig::new();
            cache.with_directory(dir);
            // A cache that can't be set up only costs time; compile without it.
            match Cache::new(cache) {
                Ok(cache) => _ = config.cache(Some(cache)),
                Err(e) => tracing::warn!(error = %e, "no wasm cache"),
            }
        }
        let engine = Engine::new(&config).map_err(|e| Error::Other(format!("wasm engine: {e}")))?;
        // The CPU clock for deadlines: one tick every 100 ms, for the life of the process.
        let ticker = engine.weak();
        std::thread::Builder::new()
            .name("wasm-epoch".into())
            .spawn(move || {
                while let Some(engine) = ticker.upgrade() {
                    engine.increment_epoch();
                    drop(engine);
                    std::thread::sleep(Duration::from_millis(100));
                }
            })
            .map_err(|e| Error::Other(e.to_string()))?;

        let mut linker = Linker::<HostState>::new(&engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| Error::Other(format!("wasi: {e}")))?;
        Provider::add_to_linker::<_, HasSelf<_>>(&mut linker, |s| s).map_err(|e| Error::Other(format!("linker: {e}")))?;

        let mut this = Self { browser, store, engine, linker, http, plugins: Vec::new() };
        let mut by_id: HashMap<String, Arc<Plugin>> = HashMap::new();
        for dir in dirs {
            let Ok(entries) = std::fs::read_dir(dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("wasm") {
                    continue;
                }
                match this.load_one(&path).await {
                    Ok(plugin) => {
                        let newer = by_id
                            .get(&plugin.info.id)
                            .is_none_or(|existing| crate::update::is_newer(&plugin.info.version, &existing.info.version));
                        if newer {
                            tracing::info!(id = %plugin.info.id, version = %plugin.info.version, path = %path.display(), "provider loaded");
                            by_id.insert(plugin.info.id.clone(), Arc::new(plugin));
                        }
                    }
                    Err(e) => tracing::warn!(path = %path.display(), error = %e, "provider failed to load"),
                }
            }
        }
        let mut plugins: Vec<_> = by_id.into_values().collect();
        plugins.sort_by(|a, b| a.info.id.cmp(&b.info.id));
        this.plugins = plugins;
        Ok(this)
    }

    fn store(&self, provider: &str, hosts: Vec<String>) -> Store<HostState> {
        let state = HostState {
            browser: self.browser.clone(),
            store: self.store.clone(),
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
            limits: StoreLimitsBuilder::new().memory_size(MEMORY_LIMIT).build(),
            http: self.http.clone(),
            hosts,
            provider: provider.to_owned(),
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_epoch_deadline(CALL_DEADLINE_TICKS);
        store.epoch_deadline_trap();
        store
    }

    async fn load_one(&self, path: &Path) -> Result<Plugin> {
        let component = Component::from_file(&self.engine, path).map_err(|e| Error::Other(format!("compile: {e}")))?;
        let mut store = self.store("loading", vec![]);
        let bindings = Provider::instantiate_async(&mut store, &component, &self.linker)
            .await
            .map_err(|e| Error::Other(format!("instantiate: {e}")))?;
        let Info { id, name, version, hosts } = bindings.call_info(&mut store).await.map_err(|e| Error::Other(format!("info: {e}")))?;
        Ok(Plugin { info: PluginInfo { id, name, version, hosts }, path: path.to_owned(), component })
    }

    pub fn list(&self) -> Vec<PluginInfo> {
        self.plugins.iter().map(|p| p.info.clone()).collect()
    }

    /// Asks every provider for servers, concurrently; a failing provider only logs. The result is
    /// in [`PRIORITY`] order, which is the tiebreak when the app picks a server by itself.
    pub async fn servers(&self, query: &TitleQuery) -> Vec<Server> {
        self.servers_for(std::slice::from_ref(query)).await
    }

    /// [`Plugins::servers`] for several queries at once (an episode's sub and dub): every
    /// provider and every query concurrently, except that a provider in [`ONE_AT_A_TIME`] answers
    /// its queries in turn. The result is query by query, each in [`PRIORITY`] order: what calls
    /// to [`Plugins::servers`] one after another would return.
    pub async fn servers_for(&self, queries: &[TitleQuery]) -> Vec<Server> {
        let calls = self.plugins.iter().map(|plugin| {
            let plugin = Arc::clone(plugin);
            async move {
                if ONE_AT_A_TIME.contains(&plugin.info.id.as_str()) {
                    let mut servers = Vec::new();
                    for (at, query) in queries.iter().enumerate() {
                        servers.extend(self.ask(&plugin, at, query).await);
                    }
                    servers
                } else {
                    let asks = queries.iter().enumerate().map(|(at, query)| self.ask(&plugin, at, query));
                    futures_util::future::join_all(asks).await.into_iter().flatten().collect()
                }
            }
        });
        let mut servers: Vec<(usize, Server)> = futures_util::future::join_all(calls).await.into_iter().flatten().collect();
        // Query by query, as separate calls returned them; stable within, so a provider's own
        // servers keep their order (AnimeHeaven's mirrors are failover).
        servers.sort_by_key(|(at, s)| (*at, priority(&s.provider)));
        servers.into_iter().map(|(_, s)| s).collect()
    }

    /// One provider's servers for query number `at`, tagged with it; a failure only logs.
    async fn ask(&self, plugin: &Plugin, at: usize, query: &TitleQuery) -> Vec<(usize, Server)> {
        match self.call(plugin, query).await {
            Ok(servers) => servers.into_iter().map(|s| (at, s)).collect(),
            Err(e) => {
                tracing::warn!(provider = %plugin.info.id, error = %e, "provider failed");
                vec![]
            }
        }
    }

    async fn call(&self, plugin: &Plugin, query: &TitleQuery) -> Result<Vec<Server>> {
        let mut store = self.store(&plugin.info.id, plugin.info.hosts.clone());
        let bindings = Provider::instantiate_async(&mut store, &plugin.component, &self.linker)
            .await
            .map_err(|e| Error::Other(format!("instantiate: {e}")))?;
        let title = Title {
            anilist_id: query.anilist_id as u32,
            mal_id: query.mal_id.map(|m| m as u32),
            titles: query.titles.clone(),
            episode: query.episode,
            dub: query.audio == Audio::Dub,
        };
        let result = bindings
            .call_servers(&mut store, &title)
            .await
            .map_err(|e| Error::Other(format!("{} trapped: {e}", plugin.info.id)))?
            .map_err(|e| Error::Other(format!("{}: {e}", plugin.info.id)))?;
        let track = if query.audio == Audio::Dub { "dub" } else { "ssub" };
        Ok(result
            .into_iter()
            .map(|s| Server {
                id: format!("{}/{}/{}", plugin.info.id, track, s.name),
                provider: plugin.info.id.clone(),
                label: plugin.info.name.clone(),
                name: s.name,
                track: if s.dub {
                    "dub".into()
                } else if s.subtitles.is_empty() {
                    "sub".into()
                } else {
                    "ssub".into()
                },
                audio: if s.dub { Audio::Dub } else { Audio::Sub },
                headers: s.headers.into_iter().collect(),
                streams: s
                    .streams
                    .into_iter()
                    .map(|st| Stream { url: st.url, format: st.format, quality: st.quality, height: st.height })
                    .collect(),
                embed: None,
                subtitles: s.subtitles.into_iter().map(|sub| Subtitle { url: sub.url, label: sub.label, language: sub.language }).collect(),
                thumbnails: s.thumbnails,
            })
            .collect())
    }

    /// Skip times a provider reported, for when the catalogue has none.
    pub fn skip_from(server_skip: Option<(f64, f64)>, outro: Option<(f64, f64)>) -> Option<SkipTimes> {
        let w = |p: Option<(f64, f64)>| p.map(|(start, end)| Window { start, end });
        let skip = SkipTimes { intro: w(server_skip), outro: w(outro), recap: None };
        (skip.intro.is_some() || skip.outro.is_some()).then_some(skip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_allowlist() {
        let hosts = vec!["ani.pm".to_owned(), "settlar.io".to_owned()];
        assert!(host_allowed("https://ani.pm/api/x", &hosts));
        assert!(host_allowed("https://embed.settlar.io/api", &hosts));
        assert!(!host_allowed("https://evil-ani.pm/x", &hosts));
        assert!(!host_allowed("http://ani.pm/x", &hosts));
        assert!(!host_allowed("https://example.com/", &hosts));
    }
}
