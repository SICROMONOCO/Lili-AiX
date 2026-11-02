// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod adblock;
mod app;
mod backup;
mod catalog;
mod clearance;
mod diagnostics;
mod discord;
mod downloads;
mod glass;
mod library;
mod media;
mod notifications;
mod notify;
mod opensubtitles;
mod player;
mod progress_queue;
#[cfg(target_os = "macos")]
mod player_mac;
#[cfg(target_os = "linux")]
mod player_linux;
mod recs;
mod simkl;
mod updates;
mod usage;
mod watch;

use tauri::Manager;

/// The route to open on launch: an `anilili://` link the app was started with (Windows passes it
/// as an argument), else, in debug builds, `ANILILI_ROUTE=/anime/154587`, so pages can be checked
/// without clicking through the UI. Anything but an app route is ignored there: Git Bash rewrites
/// `/anime/1` into `C:/…/Git/anime/1` before the app sees it, which opened on "Nothing here" (set
/// `MSYS_NO_PATHCONV=1` there).
#[tauri::command]
fn initial_route() -> app::CmdResult<Option<String>> {
    if let Some(link) = anilili_core::links::from_args(std::env::args().skip(1)) {
        return Ok(Some(anilili_core::links::Link::parse(&link)?.to_route()));
    }
    if !cfg!(debug_assertions) {
        return Ok(None);
    }
    Ok(std::env::var("ANILILI_ROUTE").ok().filter(|r| r.starts_with('/') && !r.contains(':')))
}

/// An `anilili://` link opened while the app runs: shown here, or the reason it can't be.
fn open_link<R: tauri::Runtime>(app: &tauri::AppHandle<R>, link: &str) {
    match anilili_core::links::Link::parse(link) {
        Ok(link) => {
            tracing::info!(link = %link.to_url(), "opening a link");
            let _ = tauri::Emitter::emit(app, "open-route", link.to_route());
        }
        Err(failure) => app::report(app, failure.into()),
    }
}

/// Development aid: the page's own measurements (frame gaps, time to settle), into the app log.
#[tauri::command]
fn dev_log(message: String) {
    tracing::info!(target: "ui", "{message}");
}

/// Development aid: writing a route to `target/dev-route.txt` navigates the running app.
#[cfg(debug_assertions)]
fn watch_dev_route<R: tauri::Runtime>(app: tauri::AppHandle<R>) {
    use tauri::Emitter;
    let file = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/dev-route.txt");
    let _ = std::fs::remove_file(&file);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            if let Ok(route) = std::fs::read_to_string(&file) {
                let _ = std::fs::remove_file(&file);
                let route = route.trim().to_owned();
                if !route.is_empty() {
                    tracing::info!(%route, "dev navigate");
                    let _ = app.emit("dev-navigate", route);
                }
            }
            // `{"cmd": "...", "args": {...}}`, run by the frontend through `invoke`.
            let invoke = file.with_file_name("dev-invoke.json");
            if let Ok(text) = std::fs::read_to_string(&invoke) {
                let _ = std::fs::remove_file(&invoke);
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    tracing::info!(%value, "dev invoke");
                    let _ = app.emit("dev-invoke", value);
                }
            }
        }
    });
}

/// Every webview's browser arguments, one shared string. `--enable-smooth-scrolling` turns on the
/// Chromium animation WebView2 ships without, so a mouse wheel glides the page to a stop like a
/// trackpad's continuous input instead of jumping a notch at a time. The other switches are wry's
/// own defaults, which naming *any* arguments replaces. Every builder goes through here, not just
/// the main window's: our webviews share one data directory, and WebView2 refuses an environment
/// whose arguments differ from the running browser process's. On macOS (WKWebView) and the Linux
/// webkit the call does nothing, and native scrolling already animates.
pub(crate) fn with_browser_args<'a, R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: tauri::WebviewWindowBuilder<'a, R, M>,
) -> tauri::WebviewWindowBuilder<'a, R, M> {
    builder.additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --enable-smooth-scrolling")
}

/// The main window, built in code so it can carry the two guards embedded players need:
/// every popup an iframe opens is denied, and nothing may navigate the app itself away
/// (embeds' redirect hijacks). Navigations *inside* an iframe are unaffected.
fn create_main_window<R: tauri::Runtime>(app: &mut tauri::App<R>) -> tauri::Result<()> {
    let window = with_browser_args(tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::default()))
        .title("Anilili")
        .inner_size(1280.0, 760.0)
        .min_inner_size(800.0, 480.0)
        .transparent(true)
        // A dark title bar on Windows 10, which has no caption colour (see `paint_title_bar`).
        .theme(Some(tauri::Theme::Dark))
        // An embedded player's clock, for its Discord card (the script says how).
        .initialization_script_for_all_frames(include_str!("embed-progress.js"))
        // Development aid: `ANILILI_BACKGROUND=1` opens the window without taking focus, for checks
        // run while someone is working.
        .focused(!(cfg!(debug_assertions) && std::env::var_os("ANILILI_BACKGROUND").is_some()))
        .on_new_window(|url, _features| {
            tracing::info!(%url, "blocked a popup");
            tauri::webview::NewWindowResponse::Deny
        })
        .on_navigation(|url| {
            let own = matches!(url.scheme(), "tauri" | "asset")
                || matches!(url.host_str(), Some("tauri.localhost") | Some("localhost") | Some("127.0.0.1"));
            if !own {
                tracing::info!(%url, "blocked a top-level navigation");
            }
            own
        })
        .build()?;
    // Closing the main window is quitting. Tauri waits for the last window, and a helper (the
    // browser bridge, a login window) kept the process and its player alive with nothing to close.
    let handle = app.handle().clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Destroyed => handle.exit(0),
        // Coming back to the app is when another device's changes are wanted, as on Android.
        tauri::WindowEvent::Focused(true) => library::sync_with_account(&handle, false),
        _ => {}
    });
    adblock::install(&window)?;
    #[cfg(windows)]
    paint_title_bar(&window);
    Ok(())
}

/// The title bar in the app's own colours rather than the system's light grey: the sidebar's
/// (`--sidebar`, oklch(0.205 0 0) = #171717), so the bar and the sidebar read as one frame, and its
/// text (#fafafa). Windows 11 only; earlier versions keep the dark theme's caption.
#[cfg(windows)]
fn paint_title_bar<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_CAPTION_COLOR, DWMWA_TEXT_COLOR, DwmSetWindowAttribute};

    /// COLORREF is 0x00BBGGRR.
    const fn colorref(rgb: u32) -> u32 {
        ((rgb & 0xff) << 16) | (rgb & 0xff00) | ((rgb >> 16) & 0xff)
    }
    let Ok(handle) = window.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    for (attribute, rgb) in [(DWMWA_CAPTION_COLOR, 0x171717), (DWMWA_TEXT_COLOR, 0xfafafa)] {
        let color = colorref(rgb);
        // SAFETY: a live top-level HWND and a 4-byte COLORREF, as these attributes take.
        let result =
            unsafe { DwmSetWindowAttribute(h.hwnd.get() as _, attribute as u32, (&raw const color).cast(), size_of::<u32>() as u32) };
        if result != 0 {
            tracing::debug!(attribute, result, "title bar colour not applied (before Windows 11?)");
        }
    }
}

/// A video file passed on the command line (Open with… / file association) at first launch.
#[tauri::command]
fn launch_file() -> Option<String> {
    std::env::args().skip(1).find(|a| std::path::Path::new(a).is_file())
}

fn main() {
    // WebView2 must start transparent so mpv's video shows through it. This has to be set before
    // the webview exists: the API setter still flashes white.
    #[cfg(windows)]
    // SAFETY: single-threaded at this point; nothing else reads the environment.
    unsafe {
        std::env::set_var("WEBVIEW2_DEFAULT_BACKGROUND_COLOR", "0x00000000");
    }

    // Kept for the life of the process: dropping it stops the log file's writer.
    let _log_guard = diagnostics::init_logging();

    tauri::Builder::default()
        .runtime(tauri_runtime_wry::Wry::default())
        // A second launch focuses this window instead of starting another mpv; a video file passed
        // to it (Open with…) plays here, and so does an `anilili://` link.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            if let Some(link) = anilili_core::links::from_args(args.iter().skip(1).cloned()) {
                open_link(app, &link);
            } else if let Some(file) = args.iter().skip(1).find(|a| std::path::Path::new(a).is_file()) {
                let _ = tauri::Emitter::emit(app, "open-file", file.clone());
            }
        }))
        // Only for its configuration: the installer registers `anilili://` from it.
        .plugin(tauri_plugin_deep_link::init())
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // Fullscreen belongs to the player, not to the next launch.
                .with_state_flags(tauri_plugin_window_state::StateFlags::all() - tauri_plugin_window_state::StateFlags::FULLSCREEN)
                .build(),
        )
        .setup(|app| {
            create_main_window(app)?;
            let state = app::AppState::new(app)?;
            let settings = state.settings();
            app.manage(state);
            adblock::start(app.handle());
            library::start_sync(app.handle());
            glass::apply(app.handle(), &settings.appearance);
            // On Windows the player is the app; on macOS (no video path yet, and libmpv is the
            // developer's own build) the rest of the app opens without it.
            #[cfg(windows)]
            player::setup(app, &settings)?;
            #[cfg(not(windows))]
            if let Err(e) = player::setup(app, &settings) {
                tracing::error!(error = %e, "player unavailable");
                app.manage(player::Unavailable(e.to_string()));
            }
            downloads::start(app.handle());
            notifications::start(app.handle());
            progress_queue::start(app.handle());
            usage::start(app.handle());
            #[cfg(debug_assertions)]
            watch_dev_route(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            dev_log,
            library::account_profile,
            library::backdrop_save,
            library::backdrop_load,
            library::account_refresh,
            library::open_account_page,
            library::open_discord,
            watch::embed_presence,
            opensubtitles::opensubtitles_account,
            opensubtitles::opensubtitles_login,
            opensubtitles::opensubtitles_logout,
            opensubtitles::opensubtitles_signup,
            opensubtitles::opensubtitles_search,
            opensubtitles::opensubtitles_download,
            catalog::dns_engaged,
            catalog::watch_order,
            recs::recs_home,
            recs::dice_pool,
            recs::dice_taste,
            recs::dice_feedback,
            backup::backup_export,
            backup::backup_inspect,
            backup::backup_restore,
            backup::backup_suggest_passphrase,
            diagnostics::diagnostics_save,
            diagnostics::diagnostics_open_logs,
            diagnostics::reveal_path,
            player::player_load,
            player::player_control,
            player::player_snapshot,
            player::subtitle_reopen,
            catalog::home,
            catalog::home_more,
            catalog::search,
            catalog::anime_detail,
            catalog::anime_logo,
            catalog::dub_titles,
            catalog::anime_tags,
            glass::native_glass,
            catalog::schedule,
            catalog::episodes,
            catalog::sources,
            watch::watch,
            watch::now_playing,
            watch::skip_times,
            watch::storyboard,
            watch::storyboard_image,
            library::library_list,
            library::library_sync,
            library::library_status,
            library::library_set,
            library::progress_for,
            library::continue_watching,
            library::continue_hide,
            library::clear_history,
            library::clear_cache,
            library::settings_get,
            library::settings_set,
            library::auth_status,
            library::auth_login,
            library::auth_logout,
            library::anilist_list,
            library::account_list,
            library::mal_status,
            library::mal_login,
            library::mal_logout,
            simkl::simkl_status,
            simkl::simkl_login,
            simkl::simkl_login_cancel,
            simkl::simkl_logout,
            downloads::download_enqueue,
            downloads::downloads_list,
            downloads::download_cancel,
            downloads::download_retry,
            downloads::download_remove,
            downloads::download_reveal,
            downloads::download_play,
            downloads::downloads_state,
            downloads::downloads_pause,
            downloads::downloads_cancel_all,
            downloads::downloads_retry_failed,
            downloads::downloads_clear_unfinished,
            downloads::downloads_open_folder,
            downloads::download_check,
            notifications::notifications_list,
            notifications::notifications_unread,
            notifications::notifications_clear,
            notifications::reminders_list,
            notifications::reminder_toggle,
            notifications::notification_test,
            updates::update_check,
            updates::update_notify,
            updates::update_download,
            updates::update_apply,
            updates::providers_list,
            updates::providers_update,
            clearance::clearance_experiment,
            clearance::fingerprint_compare,
            clearance::browser_fetch_test,
            initial_route,
            launch_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Anilili");
}
