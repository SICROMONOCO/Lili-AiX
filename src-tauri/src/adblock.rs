//! Ad blocking for embedded players. `anilili_core::adblock` decides; this feeds it the main
//! window's requests (WebView2's `WebResourceRequested`, iframes included) and keeps its filter
//! lists: the copy bundled with the installer, replaced by a fresher one fetched weekly.
//!
//! A refused request gets an empty 200, as Android answers: a page whose beacon fails sometimes
//! retries it in a loop, or stops starting the video.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, RwLock};
use std::time::{Duration, SystemTime};

use anilili_core::adblock::{Blocker, Kind, Refusal};
use tauri::{AppHandle, Manager, Runtime};

/// File name and source of each list. Keep in step with scripts/fetch-filters.mjs.
const LISTS: &[(&str, &str)] = &[
    ("easylist.txt", "https://easylist.to/easylist/easylist.txt"),
    ("easyprivacy.txt", "https://easylist.to/easylist/easyprivacy.txt"),
    ("ublock-filters.txt", "https://ublockorigin.github.io/uAssets/filters/filters.txt"),
    ("ublock-badware.txt", "https://ublockorigin.github.io/uAssets/filters/badware.txt"),
];
const REFRESH_EVERY: Duration = Duration::from_secs(7 * 24 * 3600);

/// Android's list from the first request; the filter lists join once read.
static BLOCKER: LazyLock<RwLock<Arc<Blocker>>> = LazyLock::new(|| RwLock::new(Arc::new(Blocker::new())));
/// Each refused host is logged once a session, as Android names each foreign origin once.
static LOGGED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Mutex::default);

/// Loads the lists off the main thread, then refreshes any older than a week.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    std::thread::spawn(move || {
        load(&app);
        tauri::async_runtime::spawn(async move {
            // After startup's own requests.
            tokio::time::sleep(Duration::from_secs(30)).await;
            if refresh(&app).await {
                load(&app);
            }
        });
    });
}

fn load<R: Runtime>(app: &AppHandle<R>) {
    let started = std::time::Instant::now();
    let texts: Vec<String> =
        LISTS.iter().filter_map(|(name, _)| list_path(app, name)).filter_map(|path| std::fs::read_to_string(path).ok()).collect();
    if texts.is_empty() {
        tracing::warn!("no filter lists found: embeds get Android's list only");
        return;
    }
    let blocker = Blocker::with_lists(texts.iter().map(String::as_str));
    tracing::info!(lists = blocker.lists(), took = ?started.elapsed(), "embed filter lists loaded");
    *BLOCKER.write().unwrap() = Arc::new(blocker);
}

/// The fetched copy when there is one, else the bundled one (`vendor/filters` in development).
fn list_path<R: Runtime>(app: &AppHandle<R>, name: &str) -> Option<PathBuf> {
    let fetched = app.path().app_data_dir().ok().map(|d| d.join("filters").join(name));
    let bundled = app.path().resource_dir().ok().map(|d| d.join("filters").join(name));
    #[cfg(debug_assertions)]
    let dev = Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/filters").join(name));
    #[cfg(not(debug_assertions))]
    let dev: Option<PathBuf> = None;
    [fetched, bundled, dev].into_iter().flatten().find(|p| p.is_file())
}

/// Fetches each list whose copy is missing or older than a week. True when any was replaced.
async fn refresh<R: Runtime>(app: &AppHandle<R>) -> bool {
    let Ok(dir) = app.path().app_data_dir().map(|d| d.join("filters")) else { return false };
    let state = app.state::<crate::app::AppState>();
    let mut replaced = false;
    for (name, url) in LISTS {
        let path = dir.join(name);
        let age = std::fs::metadata(&path).and_then(|m| m.modified()).ok().and_then(|t| SystemTime::now().duration_since(t).ok());
        if age.is_some_and(|a| a < REFRESH_EVERY) {
            continue;
        }
        match state.http.get_bytes("filter lists", url, &[], Duration::from_secs(60)).await {
            // Every list opens with its `! Title:` header; anything else is an error page.
            Ok((body, _)) if String::from_utf8_lossy(&body[..body.len().min(2000)]).contains("! Title:") => {
                let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &body));
                match written {
                    Ok(()) => replaced = true,
                    Err(e) => tracing::warn!(list = name, error = %e, "couldn't save a filter list"),
                }
            }
            Ok(_) => tracing::warn!(list = name, "a filter list download wasn't a filter list"),
            // The copy in hand keeps working; the next launch tries again.
            Err(e) => tracing::warn!(list = name, error = %e, "couldn't refresh a filter list"),
        }
    }
    replaced
}

/// Puts the blocker in front of every request the window's webview makes.
#[cfg(windows)]
pub fn install<R: Runtime>(window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    window.with_webview(|webview| {
        let Some(webview) = webview.downcast_ref::<tauri_runtime_wry::Webview>() else {
            tracing::warn!("not the wry runtime: embeds aren't filtered");
            return;
        };
        // SAFETY: WebView2 COM calls on the thread that owns the webview, which `with_webview`
        // runs us on.
        if let Err(e) = unsafe { hook(webview.controller(), webview.environment()) } {
            tracing::warn!(error = %e, "couldn't filter embed requests");
        }
    })
}

#[cfg(not(windows))]
pub fn install<R: Runtime>(_window: &tauri::WebviewWindow<R>) -> tauri::Result<()> {
    Ok(())
}

#[cfg(windows)]
unsafe fn hook(
    controller: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller,
    environment: webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Environment,
) -> windows::core::Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use webview2_com::{WebResourceRequestedEventHandler, take_pwstr};
    use windows::core::{Interface, PWSTR, w};

    unsafe {
        let core = controller.CoreWebView2()?;
        // Requests from iframes are only raised with the source-kinds filter (WebView2Feedback#1114);
        // wry registers its custom protocols the same way.
        match core.cast::<ICoreWebView2_22>() {
            Ok(core22) => core22.AddWebResourceRequestedFilterWithRequestSourceKinds(
                w!("*"),
                COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL,
            )?,
            Err(_) => core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?,
        }
        let mut token = Default::default();
        core.add_WebResourceRequested(
            &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                let Some(args) = args else { return Ok(()) };
                let request = args.Request()?;
                let mut uri = PWSTR::null();
                request.Uri(&mut uri)?;
                let uri = take_pwstr(uri);
                if !uri.starts_with("http") {
                    return Ok(());
                }
                let mut referer = PWSTR::null();
                let source = match request.Headers().and_then(|h| h.GetHeader(w!("Referer"), &mut referer)) {
                    Ok(()) => take_pwstr(referer),
                    Err(_) => String::new(),
                };
                let mut context = COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL;
                args.ResourceContext(&mut context)?;
                let blocker = BLOCKER.read().unwrap().clone();
                if let Some(refusal) = blocker.check(&uri, &source, kind_of(context)) {
                    log_refusal(&uri, &refusal);
                    let response = environment.CreateWebResourceResponse(None, 200, w!("OK"), w!("Access-Control-Allow-Origin: *"))?;
                    args.SetResponse(&response)?;
                } else if context == COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT
                    && let Some(referer) = anilili_core::adblock::frame_referer(&uri)
                {
                    // An iframe can't choose its Referer, and Kwik refuses the app's (see `frame_referer`).
                    request.Headers()?.SetHeader(w!("Referer"), &windows::core::HSTRING::from(referer))?;
                }
                Ok(())
            })),
            &mut token,
        )?;
    }
    Ok(())
}

#[cfg(windows)]
fn kind_of(context: webview2_com::Microsoft::Web::WebView2::Win32::COREWEBVIEW2_WEB_RESOURCE_CONTEXT) -> Kind {
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    match context {
        // The app's own page can't be navigated away (main.rs), so a document is a frame's.
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_DOCUMENT => Kind::Subdocument,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_SCRIPT => Kind::Script,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_STYLESHEET => Kind::Stylesheet,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_IMAGE => Kind::Image,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FONT => Kind::Font,
        // Subtitles are as much the video as the video is.
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_MEDIA | COREWEBVIEW2_WEB_RESOURCE_CONTEXT_TEXT_TRACK => Kind::Media,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_XML_HTTP_REQUEST => Kind::Xhr,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_FETCH => Kind::Fetch,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_WEBSOCKET => Kind::Websocket,
        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_PING => Kind::Ping,
        _ => Kind::Other,
    }
}

fn log_refusal(uri: &str, refusal: &Refusal) {
    let Some(host) = url::Url::parse(uri).ok().and_then(|u| u.host_str().map(str::to_owned)) else { return };
    let mut logged = LOGGED.lock().unwrap();
    if logged.len() < 500 && logged.insert(host.clone()) {
        tracing::info!(%host, ?refusal, "embed request refused");
    }
}
