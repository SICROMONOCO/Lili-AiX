//! The log file and the diagnostics bundle.
//!
//! Everything the app logs also goes to a daily file (seven kept) in the app's log folder.
//! Failures are logged under their catalogue code ([`anilili_core::catalogue`]), so a code quoted
//! in a bug report can be found in the file.
//!
//! "Save diagnostics" writes one text file to the Downloads folder: versions, settings, which
//! accounts are signed in (not who), providers, recent failures and the log's tail; on Linux also
//! the GPU, display server, desktop and WebKit's acceleration policy. It is never
//! uploaded; the viewer decides whether to share it. Tokens, cookies, OAuth codes and the user's
//! home folder are scrubbed first.

use std::path::{Path, PathBuf};

use tauri::{AppHandle, Manager, Runtime, State};

use crate::app::{AppState, CmdResult, fail};
use anilili_core::catalogue as c;

const IDENTIFIER: &str = "app.anilili.desktop";
const LOG_PREFIX: &str = "anilili";
const TAIL_LINES: usize = 3000;

/// Where the log files live: the same folder Tauri's `app_log_dir` names, worked out before the
/// app exists so logging covers startup.
pub fn log_dir() -> PathBuf {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join(IDENTIFIER);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Library/Logs").join(IDENTIFIER);
    #[cfg(not(any(windows, target_os = "macos")))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir)
        .join(IDENTIFIER);
    if cfg!(target_os = "macos") { base } else { base.join("logs") }
}

/// Console plus the daily file. The returned guard flushes the file on drop: keep it for the
/// life of the process.
pub fn init_logging() -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::Layer;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let filter = || {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,anilili_desktop=debug,anilili_core=debug".into())
    };
    let console = tracing_subscriber::fmt::layer().with_filter(filter());
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(LOG_PREFIX)
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir());
    match file {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            let file = tracing_subscriber::fmt::layer().with_ansi(false).with_writer(writer).with_filter(filter());
            tracing_subscriber::registry().with(console).with(file).init();
            Some(guard)
        }
        Err(e) => {
            tracing_subscriber::registry().with(console).init();
            tracing::warn!(error = %e, "no log file; logging to the console only");
            None
        }
    }
}

/// Writes the diagnostics file to Downloads and returns its path.
#[tauri::command]
pub async fn diagnostics_save<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<String> {
    let dir = app.path().download_dir().map_err(|e| fail(c::APP_DIAGNOSTICS, e.to_string()))?;
    let path = dir.join(format!("Anilili-diagnostics-{}.txt", timestamp(anilili_core::store::now())));
    #[cfg(target_os = "linux")]
    let display = linux_display(&app).await;
    #[cfg(not(target_os = "linux"))]
    let display = String::new();
    let report = scrub(&build_report(&app, &state, &display));
    std::fs::write(&path, report).map_err(|e| fail(c::APP_DIAGNOSTICS, format!("{}: {e}", path.display())))?;
    tracing::info!(path = %path.display(), "diagnostics saved");
    Ok(path.to_string_lossy().into_owned())
}

/// Opens the folder holding the log files.
#[tauri::command]
pub fn diagnostics_open_logs() -> CmdResult<()> {
    reveal(&log_dir())
}

/// Shows a file (or folder) in the system file manager.
#[tauri::command]
pub fn reveal_path(path: String) -> CmdResult<()> {
    reveal(Path::new(&path))
}

fn reveal(path: &Path) -> CmdResult<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let arg = if path.is_dir() { format!("\"{}\"", path.display()) } else { format!("/select,\"{}\"", path.display()) };
        std::process::Command::new("explorer").raw_arg(arg).spawn().map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg("-R").arg(path).spawn().map_err(|e| e.to_string())?;
    #[cfg(not(any(windows, target_os = "macos")))]
    std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn().map_err(|e| e.to_string())?;
    Ok(())
}

/// Linux: what the window is drawn with. Flicker over the video (the webview's frames dropping
/// out above it) shows on some setups and not others, and which ones depends on the GPU driver,
/// Wayland or X11, the desktop and WebKit's renderer, none of which the rest of the report shows.
#[cfg(target_os = "linux")]
async fn linux_display<R: Runtime>(app: &AppHandle<R>) -> String {
    use std::fmt::Write;
    use gtk::glib::prelude::ObjectExt;
    use webkit2gtk::{SettingsExt, WebViewExt};

    // GDK's backend and WebKit's acceleration policy are read on the main thread.
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let asked = app.get_webview_window("main").is_some_and(|window| {
        window
            .with_webview(move |platform| {
                let backend = gtk::gdk::Display::default().map(|display| display.type_().name());
                let policy = platform
                    .downcast_ref::<tauri_runtime_wry::Webview>()
                    .and_then(|native| native.inner().settings())
                    .map(|settings| format!("{:?}", settings.hardware_acceleration_policy()));
                let _ = sender.send((backend, policy));
            })
            .is_ok()
    });
    let (backend, policy) = match asked {
        true => tokio::time::timeout(std::time::Duration::from_secs(5), receiver).await.ok().and_then(Result::ok).unwrap_or_default(),
        false => (None, None),
    };

    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let session = env("XDG_SESSION_TYPE");
    let backend = match (backend, session.as_deref()) {
        (Some("GdkWaylandDisplay"), _) => "Wayland".to_owned(),
        (Some("GdkX11Display"), Some("wayland")) => "X11 through XWayland".to_owned(),
        (Some("GdkX11Display"), _) => "X11".to_owned(),
        (Some(other), _) => other.to_owned(),
        (None, _) => "unknown".to_owned(),
    };
    let desktop = env("XDG_CURRENT_DESKTOP").or_else(|| env("DESKTOP_SESSION")).unwrap_or_else(|| "unknown".into());
    // Overrides that change how GTK, WebKit or the GL driver draw (WEBKIT_DISABLE_DMABUF_RENDERER …).
    let overrides: Vec<String> = std::env::vars()
        .filter(|(name, _)| {
            name.starts_with("WEBKIT_")
                || name.starts_with("__GLX_")
                || name.starts_with("__EGL_")
                || name.starts_with("__NV_")
                || ["GDK_BACKEND", "GDK_GL", "GDK_DEBUG", "GSK_RENDERER", "LIBGL_ALWAYS_SOFTWARE", "MESA_LOADER_DRIVER_OVERRIDE", "DRI_PRIME"]
                    .contains(&name.as_str())
        })
        .map(|(name, value)| format!("{name}={value}"))
        .collect();

    let mut out = String::new();
    let _ = writeln!(out, "Display:   {backend} (session {})", session.as_deref().unwrap_or("unknown"));
    let _ = writeln!(out, "Desktop:   {desktop}");
    let _ = writeln!(out, "GPU:       {}", crate::player_linux::gl().unwrap_or("unknown (the video layer hasn't drawn)"));
    let _ = writeln!(out, "WebKit:    hardware acceleration {}", policy.as_deref().unwrap_or("unknown"));
    let _ = writeln!(out, "Overrides: {}", if overrides.is_empty() { "none".into() } else { overrides.join(" ") });
    out
}

fn build_report<R: Runtime>(app: &AppHandle<R>, state: &AppState, display: &str) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let info = app.package_info();
    let _ = writeln!(out, "Anilili Desktop diagnostics\n===========================\n");
    let _ = writeln!(out, "Written:   {} (UTC)", timestamp(anilili_core::store::now()));
    let _ = writeln!(out, "App:       {} {}", info.name, info.version);
    let _ = writeln!(out, "System:    {} {}", std::env::consts::OS, std::env::consts::ARCH);
    let _ = writeln!(out, "WebView:   {}", app.webview_version().unwrap_or_else(|e| format!("unknown ({e})")));
    let mpv = app.try_state::<crate::player::Player>().and_then(|p| p.mpv_version()).unwrap_or_else(|| {
        match app.try_state::<crate::player::Unavailable>() {
            Some(why) => format!("not running: {}", why.0),
            None => "not running".into(),
        }
    });
    let _ = writeln!(out, "mpv:       {mpv}");
    out.push_str(display);
    let _ = writeln!(out, "AniList:   {}", if state.anilist.has_token() { "signed in" } else { "signed out" });
    let _ = writeln!(out, "MAL:       {}", if state.mal_user.read().unwrap().is_some() { "signed in" } else { "signed out" });
    let providers: Vec<String> = state.plugins.read().unwrap().list().into_iter().map(|p| format!("{} {}", p.id, p.version)).collect();
    let _ = writeln!(out, "Providers: {}", if providers.is_empty() { "none".into() } else { providers.join(", ") });
    let _ = writeln!(out, "\nSettings\n--------\n{}", serde_json::to_string_pretty(&state.settings()).unwrap_or_default());

    let lines = log_tail(TAIL_LINES);
    let failures: Vec<&String> = lines.iter().filter(|l| l.contains(" code=D")).collect();
    let _ = writeln!(out, "\nRecent failures ({})\n---------------", failures.len());
    for line in failures.iter().rev().take(60).rev() {
        let _ = writeln!(out, "{line}");
    }
    let _ = writeln!(out, "\nLog (last {} lines)\n---", lines.len());
    for line in &lines {
        let _ = writeln!(out, "{line}");
    }
    out
}

/// The last `max` lines across the log files, oldest first.
fn log_tail(max: usize) -> Vec<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(log_dir())
        .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|x| x == "log")).collect())
        .unwrap_or_default();
    files.sort();
    let mut lines = std::collections::VecDeque::with_capacity(max);
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        for line in text.lines() {
            if lines.len() == max {
                lines.pop_front();
            }
            lines.push_back(line.to_owned());
        }
    }
    lines.into()
}

/// Removes what must never leave the machine: tokens, cookies, OAuth codes and state, and the
/// user's home folder (which may carry their name).
pub fn scrub(text: &str) -> String {
    let mut out = text.to_owned();
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = home.to_string_lossy().into_owned();
        if home.len() > 3 {
            out = out.replace(&home, "~").replace(&home.replace('\\', "/"), "~");
        }
    }
    for marker in ["access_token=", "refresh_token=", "Bearer ", "cf_clearance=", "__cf_bm=", "?code=", "&code=", "&state=", "?state="] {
        out = redact_after(&out, marker);
    }
    // Whole header values.
    out.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            match ["cookie: ", "authorization: "].iter().find_map(|h| lower.find(h).map(|i| i + h.len())) {
                Some(start) => format!("{}[redacted]", &line[..start]),
                None => line.to_owned(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_after(text: &str, marker: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(marker) {
        out.push_str(&rest[..i + marker.len()]);
        rest = &rest[i + marker.len()..];
        let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || "-._~%+/=".contains(c))).unwrap_or(rest.len());
        if end > 0 {
            out.push_str("[redacted]");
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// `20261004-183012` for unix seconds (UTC).
fn timestamp(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrubs_secrets() {
        let text = "GET https://x/cb?code=abc123&state=zz9 ok\nhttp://localhost#access_token=eyJ.abc.def&x=1\nCookie: cf_clearance=Q; a=b\nauthorization: Bearer t0k3n";
        let out = scrub(text);
        for secret in ["abc123", "zz9", "eyJ.abc.def", "cf_clearance=Q", "t0k3n"] {
            assert!(!out.contains(secret), "{secret} survived: {out}");
        }
        assert!(out.contains("code=DNET-01") || !text.contains("code=DNET"), "catalogue codes stay");
        assert_eq!(scrub("warn code=DNET-03/h503 detail"), "warn code=DNET-03/h503 detail");
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(timestamp(0), "19700101-000000");
        assert_eq!(timestamp(1_791_138_592), "20261004-182952");
    }
}
