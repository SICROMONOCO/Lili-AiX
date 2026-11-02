//! Cloudflare clearance, earned in a webview and replayed from code.
//!
//! A cold HTTP client gets Cloudflare's "Just a moment…" JavaScript challenge however good its TLS
//! fingerprint is. A real browser engine solves it and receives `cf_clearance`, which Cloudflare
//! binds to the client that earned it. So the replay copies that client:
//!
//! - **Don't override the webview's user agent.** WebView2 then claims one browser in
//!   `User-Agent` while its client hints (`sec-ch-ua`) still name the real engine (Edge WebView2
//!   154), an inconsistency Cloudflare refuses. The webview solves as itself.
//! - The replay is `wreq` with Edge's TLS/HTTP2 signature plus the ML-DSA signature schemes
//!   (our BoringSSL patch), so its JA4 equals WebView2's; with the webview's own `User-Agent`,
//!   `sec-ch-ua*` and `Accept-Language`, captured from a local echo endpoint.
//!
//! [`browser_fetch`] replays first and falls back to making the request inside the bridge page.

use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

use crate::app::AppState;
use crate::with_browser_args;

/// What a request needs to carry to be the client that earned clearance.
#[derive(Debug, Clone)]
pub struct Clearance {
    pub headers: Vec<(String, String)>,
}

#[cfg(not(target_os = "linux"))]
const IDENTITY_HEADERS: &[&str] = &["user-agent", "sec-ch-ua", "sec-ch-ua-mobile", "sec-ch-ua-platform", "accept-language"];

async fn webview_identity<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<(String, String)>, String> {
    #[cfg(target_os = "linux")]
    return linux_identity(app).await;
    #[cfg(not(target_os = "linux"))]
    window_identity(app).await
}

/// The identity read from a request the webview makes to `/echo`, in a hidden window.
#[cfg(not(target_os = "linux"))]
async fn window_identity<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<(String, String)>, String> {
    let state = app.state::<AppState>();
    let url: url::Url = format!("http://127.0.0.1:{}/echo", state.proxy.port()).parse().map_err(|e| format!("{e}"))?;
    let window = with_browser_args(WebviewWindowBuilder::new(app, "identity", WebviewUrl::External(url)))
        .inner_size(200.0, 120.0)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let identity = loop {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let echo = state.proxy.last_echo();
        if echo.iter().any(|(k, _)| k == "sec-ch-ua") || started.elapsed() > Duration::from_secs(5) {
            break echo;
        }
    };
    let _ = window.close();
    Ok(identity.into_iter().filter(|(k, _)| IDENTITY_HEADERS.contains(&k.as_str())).collect())
}

/// Linux: WebKitGTK sends no client hints, so its identity is its user agent and language, read
/// from the main webview's own settings. A window on `/echo` was not hidden there: GTK showed it,
/// and tiling window managers gave it half the screen, the request headers printed on it.
#[cfg(target_os = "linux")]
async fn linux_identity<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<(String, String)>, String> {
    use webkit2gtk::{SettingsExt, WebViewExt};

    let window = app.get_webview_window("main").ok_or("main window missing")?;
    let (sender, agent) = tokio::sync::oneshot::channel();
    window
        .with_webview(move |platform| {
            let agent = platform
                .downcast_ref::<tauri_runtime_wry::Webview>()
                .and_then(|native| native.inner().settings())
                .and_then(|settings| settings.user_agent())
                .map(|agent| agent.to_string());
            let _ = sender.send(agent);
        })
        .map_err(|e| e.to_string())?;
    let agent = tokio::time::timeout(Duration::from_secs(5), agent)
        .await
        .map_err(|_| "reading the webview's user agent timed out".to_owned())?
        .map_err(|e| e.to_string())?
        .ok_or("the webview has no user agent")?;
    // WebKit's Accept-Language: the first of the locale's language names, `en_US` as `en-US`.
    let language = gtk::glib::language_names()
        .into_iter()
        .find(|name| !matches!(name.as_str(), "C" | "POSIX") && !name.contains(['.', '@']))
        .map_or_else(|| "en-US".to_owned(), |name| name.replace('_', "-"));
    Ok(vec![("user-agent".into(), agent), ("accept-language".into(), language)])
}

/// Earns clearance for `origin` (e.g. `https://animepahe.pw/`).
pub async fn solve<R: Runtime>(app: &AppHandle<R>, origin: &str, timeout: Duration) -> Result<Clearance, String> {
    let url: url::Url = origin.parse().map_err(|e| format!("{e}"))?;
    let mut headers = webview_identity(app).await?;
    let window = with_browser_args(WebviewWindowBuilder::new(app, "clearance", WebviewUrl::External(url.clone())))
        .title("Checking your connection…")
        .inner_size(420.0, 520.0)
        .skip_taskbar(true)
        .focused(false)
        .build()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let result = loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let cookies = window.cookies_for_url(url.clone()).map_err(|e| e.to_string())?;
        if cookies.iter().any(|c| c.name() == "cf_clearance") {
            let cookie = cookies.iter().map(|c| format!("{}={}", c.name(), c.value())).collect::<Vec<_>>().join("; ");
            tracing::info!(%origin, secs = started.elapsed().as_secs_f32(), "clearance earned");
            headers.push(("cookie".into(), cookie));
            break Ok(Clearance { headers });
        }
        if started.elapsed() > timeout {
            break Err(format!("no clearance from {origin} within {}s", timeout.as_secs()));
        }
    };
    let _ = window.close();
    result
}

/// Development aid: earn clearance on `origin`, then replay `test_url` from `wreq` with it.
/// `classical: true` replays with the Chrome profile instead, whose ClientHello lacks only the
/// ML-DSA signature schemes: the control for whether they are what Cloudflare checks.
#[tauri::command]
pub async fn clearance_experiment<R: Runtime>(
    app: AppHandle<R>,
    origin: String,
    test_url: String,
    classical: Option<bool>,
) -> Result<String, String> {
    let clearance = solve(&app, &origin, Duration::from_secs(40)).await?;
    let classical = classical.unwrap_or(false);
    let http = if classical { anilili_core::net::Http::new() } else { anilili_core::net::Http::edge() }.map_err(|e| e.to_string())?;
    let mut request = http.client().get(&test_url).header("Accept", "application/json, text/plain, */*").header("Referer", origin.as_str());
    for (name, value) in &clearance.headers {
        request = request.header(name.as_str(), value.as_str());
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let body: String = response.text().await.unwrap_or_default().chars().take(300).collect();
    let names: Vec<&str> = clearance.headers.iter().map(|(k, _)| k.as_str()).collect();
    let tls = if classical { "classical sigalgs" } else { "Edge TLS + ML-DSA" };
    let verdict = format!("replay ({tls} + {names:?}): HTTP {status}; body starts: {body}");
    tracing::info!("{verdict}");
    Ok(verdict)
}

/// Development aid: the webview's own TLS/HTTP2 fingerprint, read from tls.peet.ws in the webview
/// and handed back through the local echo endpoint, next to what `wreq` presents.
#[tauri::command]
pub async fn fingerprint_compare<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    let state = app.state::<AppState>();
    let echo = format!("http://127.0.0.1:{}/echo?d=", state.proxy.port());
    let script = format!(
        "window.addEventListener('load', () => setTimeout(() => {{ location.href = '{echo}' + encodeURIComponent(document.body.innerText); }}, 500));"
    );
    let window = with_browser_args(WebviewWindowBuilder::new(
        &app,
        "fingerprint",
        WebviewUrl::External("https://tls.peet.ws/api/all".parse().unwrap()),
    ))
    .initialization_script(&script)
    .inner_size(300.0, 200.0)
    .skip_taskbar(true)
    .build()
    .map_err(|e| e.to_string())?;
    let started = Instant::now();
    let webview = loop {
        tokio::time::sleep(Duration::from_millis(300)).await;
        if let Some((_, q)) = state.proxy.last_echo().into_iter().find(|(k, _)| k == "x-echo-query") {
            break url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "d").map(|(_, v)| v.into_owned()).unwrap_or_default();
        }
        if started.elapsed() > Duration::from_secs(20) {
            break String::new();
        }
    };
    let _ = window.close();
    let pick = |json: &str| -> String {
        let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
        format!(
            "ja4={} | ja4_r={} | akamai={} | h2_settings={}",
            v.pointer("/tls/ja4").and_then(|x| x.as_str()).unwrap_or("?"),
            v.pointer("/tls/ja4_r").and_then(|x| x.as_str()).unwrap_or("?"),
            v.pointer("/http2/akamai_fingerprint").and_then(|x| x.as_str()).unwrap_or("?"),
            v.pointer("/http2/sent_frames/0/settings").map(|x| x.to_string()).unwrap_or_default()
        )
    };
    let mut lines = vec![format!("webview: {}", pick(&webview))];
    for (name, http) in [("wreq edge", anilili_core::net::Http::edge()), ("wreq chrome", anilili_core::net::Http::new())] {
        let http = http.map_err(|e| e.to_string())?;
        let body =
            http.client().get("https://tls.peet.ws/api/all").send().await.map_err(|e| e.to_string())?.text().await.unwrap_or_default();
        lines.push(format!("{name}: {}", pick(&body)));
    }
    let report = lines.join(
        "
",
    );
    tracing::info!(
        "fingerprints
{report}"
    );
    Ok(report)
}

/// A request to a challenge-gated site (a provider's `browser-fetch`).
///
/// First as a replay: `wreq` with WebView2's TLS/HTTP2 signature, the webview's identity headers
/// and the cookies the webview holds for the site (WebView2 windows share one cookie store, so a
/// clearance the bridge page earned is readable from any of them). Fast, and concurrent. When
/// that is challenged (no clearance yet, or it expired), the request is made inside the bridge
/// page instead, which also earns a fresh clearance for the replays after it.
pub async fn browser_fetch<R: Runtime>(
    app: &AppHandle<R>,
    origin: &str,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>), String> {
    match replay_fetch(app, method, url, headers, body).await {
        Ok(Some(response)) => return Ok(response),
        Ok(None) => tracing::info!(%url, "replay challenged; fetching in the bridge page"),
        Err(error) => tracing::warn!(%url, %error, "replay failed; fetching in the bridge page"),
    }
    page_fetch(app, origin, method, url, headers, body).await
}

const BRIDGE_LABEL: &str = "browser-bridge";

/// The webview's identity headers, captured once per run (they change only with WebView2).
static IDENTITY: tokio::sync::OnceCell<Vec<(String, String)>> = tokio::sync::OnceCell::const_new();
static EDGE: std::sync::OnceLock<anilili_core::net::Http> = std::sync::OnceLock::new();

/// `Ok(None)` when Cloudflare challenged the replay.
async fn replay_fetch<R: Runtime>(
    app: &AppHandle<R>,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<Option<(u16, Vec<u8>)>, String> {
    let parsed: url::Url = url.parse().map_err(|e| format!("{e}"))?;
    // The bridge page's cookies (the main window's view of the store did not include them).
    // Reading them blocks on the UI thread, so not on an async worker, and not forever.
    let Some(webview) = app.get_webview_window(BRIDGE_LABEL) else { return Ok(None) };
    let read = tauri::async_runtime::spawn_blocking(move || webview.cookies_for_url(parsed));
    let cookies = tokio::time::timeout(Duration::from_secs(3), read)
        .await
        .map_err(|_| "reading the bridge page's cookies timed out".to_owned())?
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    if !cookies.iter().any(|c| c.name() == "cf_clearance") {
        return Ok(None);
    }
    let identity = IDENTITY.get_or_try_init(|| webview_identity(app)).await?;
    tracing::debug!(%url, "replaying clearance");
    let http = match EDGE.get() {
        Some(http) => http,
        None => {
            let http = anilili_core::net::Http::edge().map_err(|e| e.to_string())?;
            EDGE.get_or_init(|| http)
        }
    };
    let method = wreq::Method::from_bytes(method.as_bytes()).map_err(|e| e.to_string())?;
    let mut request = http.client().request(method, url);
    for (name, value) in identity.iter().chain(headers) {
        request = request.header(name.as_str(), value.as_str());
    }
    let cookie = cookies.iter().map(|c| format!("{}={}", c.name(), c.value())).collect::<Vec<_>>().join("; ");
    request = request.header("cookie", cookie);
    if let Some(body) = body {
        request = request.body(body.to_vec());
    }
    let response = request.send().await.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let challenged = response.headers().get("cf-mitigated").is_some_and(|v| v == "challenge");
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if challenged || (matches!(status, 403 | 503) && bytes.windows(15).any(|w| w == b"Just a moment..")) {
        return Ok(None);
    }
    Ok(Some((status, bytes.to_vec())))
}

/// The request made *inside* the bridge page: it runs `fetch()` with the page's own cookies and
/// engine, then hands the response back by navigating to the local `/bridge` endpoint. Requests
/// are serialized per process.
async fn page_fetch<R: Runtime>(
    app: &AppHandle<R>,
    origin: &str,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<(u16, Vec<u8>), String> {
    static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    // When a challenge just failed to clear, requests queued behind it fail at once rather than
    // each waiting out its own timeout.
    static LAST_FAILURE: std::sync::Mutex<Option<(String, Instant)>> = std::sync::Mutex::new(None);
    let _guard = LOCK.lock().await;
    if let Some((failed, at)) = LAST_FAILURE.lock().unwrap().clone()
        && failed == origin
        && at.elapsed() < Duration::from_secs(60)
    {
        return Err(format!("{origin} did not get past its challenge a moment ago"));
    }
    let state = app.state::<AppState>();
    let origin_url: url::Url = origin.parse().map_err(|e| format!("{e}"))?;

    // Showing while a request runs: hidden webviews throttle the timers and frames a challenge
    // script needs, and never clear. Hidden again once it is done (see below); the window itself
    // stays, as the replay reads the site's cookies through it.
    let window = match app.get_webview_window(BRIDGE_LABEL) {
        Some(w) => w,
        None => {
            let window = with_browser_args(WebviewWindowBuilder::new(app, BRIDGE_LABEL, WebviewUrl::External(origin_url.clone())))
                .title("Anilili browser")
                .inner_size(160.0, 120.0)
                .position(0.0, 0.0)
                .skip_taskbar(true)
                .focused(false)
                .focusable(false)
                .always_on_bottom(true)
                .decorations(false)
                // The site's ads open popups too.
                .on_new_window(|url, _features| {
                    tracing::info!(%url, "blocked a popup from the bridge page");
                    tauri::webview::NewWindowResponse::Deny
                })
                .build()
                .map_err(|e| e.to_string())?;
            #[cfg(windows)]
            make_unseen(&window);
            window
        }
    };
    let _ = window.show();
    let raw = bridge_request(&window, &state, origin, &origin_url, method, url, headers, body).await;
    // Out of sight until the next request: left up, a user took it for an ad they could not close.
    let _ = window.hide();
    let raw = raw.inspect_err(|_| *LAST_FAILURE.lock().unwrap() = Some((origin.to_owned(), Instant::now())))?;
    let value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if let Some(error) = value.get("e").and_then(|e| e.as_str()) {
        return Err(format!("in-page fetch failed: {error}"));
    }
    let status = value.get("s").and_then(|s| s.as_u64()).unwrap_or(0) as u16;
    let zipped = base64_decode(value.get("z").and_then(|b| b.as_str()).unwrap_or("")).ok_or("bad body encoding")?;
    let mut body = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(zipped.as_slice()), &mut body)
        .map_err(|e| format!("bad gzip from page: {e}"))?;
    Ok((status, body))
}

/// Keeps the showing bridge window out of sight and out of the way. "Always on bottom" alone does not:
/// every `show()` is a plain `SW_SHOW`, which raises the window, and it sat over the player's title
/// until the request finished (minutes, when requests queued). So it is drawn at 1/255 opacity and
/// lets clicks through, which leaves it showing as far as the page's timers are concerned.
#[cfg(windows)]
fn make_unseen<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes, SetWindowLongPtrW, WS_EX_LAYERED, WS_EX_TRANSPARENT,
    };

    // Through tao, so the styles it rewrites on every show and hide keep WS_EX_LAYERED and
    // WS_EX_TRANSPARENT (the layered attributes below survive as long as the style does).
    let _ = window.set_ignore_cursor_events(true);
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    let hwnd = h.hwnd.get();
    let _ = window.run_on_main_thread(move || {
        let hwnd = hwnd as _;
        // SAFETY: the bridge's live top-level HWND, on the thread that owns it.
        unsafe {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, GetWindowLongPtrW(hwnd, GWL_EXSTYLE) | (WS_EX_LAYERED | WS_EX_TRANSPARENT) as isize);
            if SetLayeredWindowAttributes(hwnd, 0, 1, LWA_ALPHA) == 0 {
                tracing::warn!("the browser bridge window could not be made see-through");
            }
        }
    });
}

/// [`page_fetch`]'s wait in the (showing) bridge page: the page's raw answer, or an error when the
/// challenge did not clear in time.
#[allow(clippy::too_many_arguments)]
async fn bridge_request<R: Runtime>(
    window: &tauri::WebviewWindow<R>,
    state: &AppState,
    origin: &str,
    origin_url: &url::Url,
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<String, String> {
    // Be on the origin (a previous request left the page on /bridge, or another site is loaded),
    // and give a challenge time to clear.
    let on_origin = |w: &tauri::WebviewWindow<R>| {
        w.url().ok().and_then(|u| u.host_str().map(str::to_owned)) == origin_url.host_str().map(str::to_owned)
    };
    // A single `navigate` off the bridge page is sometimes dropped, so it is repeated (and also
    // asked of the page itself) until the page is on the origin.
    let go_home = |w: &tauri::WebviewWindow<R>| {
        let _ = w.navigate(origin_url.clone());
        let _ = w.eval(format!("location.replace({})", serde_json::to_string(origin_url.as_str()).unwrap_or_default()));
    };
    let mut last_nudge = None::<Instant>;
    // The request runs once the page itself says it is past the challenge (its title stops being
    // Cloudflare's "Just a moment..."; a cookie alone is not proof, an old one may be stale). The
    // script is re-sent until it answers: it does nothing on a page still challenging or loading,
    // and runs only once per page.
    let id: String = (0..12).map(|_| format!("{:x}", rand::random::<u8>() & 0xf)).collect();
    let options = serde_json::json!({
        "method": method,
        "headers": headers.iter().cloned().collect::<std::collections::HashMap<_, _>>(),
        "body": body.map(|b| String::from_utf8_lossy(b).into_owned()),
        "credentials": "include",
    });
    let script = format!(
        r#"(async () => {{
  if (document.readyState !== "complete" || /just a moment|attention required/i.test(document.title) || window.__aniliBridge) return;
  window.__aniliBridge = true;
  let out;
  try {{
    const r = await fetch({url}, {options});
    // gzip in the page: the result travels back inside a navigation URL, so size matters.
    const zipped = await new Response(r.body.pipeThrough(new CompressionStream("gzip"))).arrayBuffer();
    const bytes = new Uint8Array(zipped);
    let bin = ""; for (let i = 0; i < bytes.length; i += 0x8000) bin += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
    out = {{ s: r.status, z: btoa(bin) }};
  }} catch (e) {{ out = {{ e: String(e) }}; }}
  location.href = {bridge} + "?id=" + {id} + "&d=" + encodeURIComponent(JSON.stringify(out));
}})();"#,
        url = serde_json::to_string(url).unwrap_or_default(),
        options = options,
        bridge = serde_json::to_string(&state.proxy.bridge_url()).unwrap_or_default(),
        id = serde_json::to_string(&id).unwrap_or_default(),
    );
    let started = Instant::now();
    let raw = loop {
        if started.elapsed() > Duration::from_secs(45) {
            let at = window.url().map(|u| u.to_string()).unwrap_or_default();
            return Err(format!("{origin} did not get past its challenge (page at {at})"));
        }
        if on_origin(window) {
            let _ = window.eval(&script);
        } else if last_nudge.is_none_or(|t| t.elapsed() > Duration::from_secs(4)) {
            go_home(window);
            last_nudge = Some(Instant::now());
        }
        // Once the script has started, the answer comes in a fetch's time; a page that is still
        // challenging stays quiet and the script is sent again.
        if let Some(raw) = state.proxy.wait_bridge(&id, Duration::from_millis(800)).await {
            break raw;
        }
    };
    // Back to the origin straight away, so the next request finds the page ready.
    go_home(window);
    Ok(raw)
}

fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut buffer, mut bits) = (0u32, 0);
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            _ => return None,
        } as u32;
        buffer = (buffer << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// The browser engine lent to WebAssembly providers (`browser-fetch`).
pub struct AppBrowser<R: Runtime> {
    pub app: AppHandle<R>,
}

impl<R: Runtime> anilili_core::plugin::BrowserFetch for AppBrowser<R> {
    fn fetch(
        &self,
        origin: String,
        method: String,
        url: String,
        headers: Vec<(String, String)>,
        body: Option<Vec<u8>>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(u16, Vec<u8>), String>> + Send>> {
        let app = self.app.clone();
        Box::pin(async move { browser_fetch(&app, &origin, &method, &url, &headers, body.as_deref()).await })
    }
}

/// Development aid: one request through the browser bridge.
#[tauri::command]
pub async fn browser_fetch_test<R: Runtime>(app: AppHandle<R>, origin: String, url: String) -> Result<String, String> {
    let (status, body) = browser_fetch(&app, &origin, "GET", &url, &[("Accept".into(), "application/json".into())], None).await?;
    let _ = std::fs::write(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/bridge-last.txt"), &body);
    let verdict = format!(
        "browser bridge: HTTP {status}; {} bytes; starts: {}",
        body.len(),
        String::from_utf8_lossy(&body).chars().take(200).collect::<String>()
    );
    tracing::info!("{verdict}");
    Ok(verdict)
}
