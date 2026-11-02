//! System notifications: Windows toasts, the desktop's version of Android's notification shade.
//!
//! As on Android, a notification is posted under a tag and a group, so a later one with the same
//! tag replaces it instead of piling up beside it (an episode's "airs soon" becomes its "is out"),
//! and a group can be cleared at once (opening the Notifications page clears the feed's toasts,
//! as Android clears its channel). Clicking one opens its route in the app.
//!
//! The toast's artwork must be a local file for an unpackaged app, so posters are fetched into the
//! cache folder first. Release builds post as the installer's shortcut (its AppUserModelID is the
//! bundle identifier); debug builds have no shortcut and borrow PowerShell's id, which Windows
//! always accepts, so their toasts say "Windows PowerShell".

use std::path::PathBuf;

use tauri::{AppHandle, Manager, Runtime};

/// The groups notifications are posted under (Android's channels).
pub const EPISODES: &str = "episodes";
pub const ANILIST: &str = "anilist";
pub const DOWNLOADS: &str = "downloads";
pub const APP: &str = "app";

pub struct Toast<'a> {
    /// With `group`, what a later toast replaces.
    pub tag: &'a str,
    pub group: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    /// A local image, shown as the toast's picture.
    pub image: Option<PathBuf>,
    /// The app route a click opens.
    pub route: Option<&'a str>,
    /// A button doing the same as a click, as Android's "View anime".
    pub action: Option<&'a str>,
}

/// Posts a toast; a failure is logged, never raised (a notification is never worth an error).
pub fn show<R: Runtime>(app: &AppHandle<R>, toast: &Toast<'_>) {
    #[cfg(windows)]
    if let Err(e) = windows_toast::show(app, toast) {
        tracing::warn!(error = %e, tag = toast.tag, "toast failed");
    }
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        // Notification Center through AppleScript: shown, but a click doesn't open the route yet
        // (that wants a UNUserNotificationCenter delegate; see docs/MACOS.md).
        let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{}\" with title \"Anilili\" subtitle \"{}\"", quote(toast.body), quote(toast.title));
        if let Err(e) = std::process::Command::new("osascript").args(["-e", &script]).spawn() {
            tracing::warn!(error = %e, tag = toast.tag, "notification failed");
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = app;
        // Freedesktop notifications via notify-send where present; no replacement-ID or
        // action support, so a click does not open the route (documented limitation).
        let shown = std::process::Command::new("notify-send")
            .args(["--app-name", "Anilili", toast.title, toast.body])
            .spawn();
        match shown {
            Ok(_) => tracing::debug!(tag = toast.tag, "notification sent (Linux)"),
            Err(e) => tracing::info!(title = toast.title, body = toast.body, error = %e, "notification (notify-send unavailable)"),
        }
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = app;
        tracing::info!(title = toast.title, body = toast.body, "notification (not shown on this platform)");
    }
}

/// Takes a group's toasts out of the notification centre.
pub fn clear_group<R: Runtime>(app: &AppHandle<R>, group: &str) {
    #[cfg(windows)]
    if let Err(e) = windows_toast::clear_group(app, group) {
        tracing::debug!(error = %e, group, "clearing toasts failed");
    }
    #[cfg(not(windows))]
    let _ = (app, group);
}

/// A poster for a toast, fetched once into the cache folder; `None` when it can't be had.
pub async fn artwork<R: Runtime>(app: &AppHandle<R>, url: Option<&str>) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let url = url.filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    url.hash(&mut hasher);
    let dir = app.path().app_cache_dir().ok()?.join("toast-art");
    // Windows goes by the extension.
    let extension = url
        .rsplit('/')
        .next()
        .and_then(|name| name.split('?').next())
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif"))
        .unwrap_or_else(|| "jpg".into());
    let path = dir.join(format!("{:016x}.{extension}", hasher.finish()));
    if path.exists() {
        return Some(path);
    }
    let http = app.state::<crate::app::AppState>().http.clone();
    let bytes = http.client().get(url).timeout(std::time::Duration::from_secs(15)).send().await.ok()?.bytes().await.ok()?;
    if bytes.is_empty() {
        return None;
    }
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::write(&path, &bytes).ok()?;
    Some(path)
}

/// Brings the window forward and opens `route`, as a clicked notification does.
fn open<R: Runtime>(app: &AppHandle<R>, route: &str) {
    use tauri::Emitter;
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    if route.starts_with('/') {
        let _ = app.emit("open-route", route);
    }
}

#[cfg(windows)]
mod windows_toast {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use tauri::{AppHandle, Runtime};
    use windows::Data::Xml::Dom::XmlDocument;
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager};
    use windows::core::{HSTRING, IInspectable, Interface};

    use super::Toast;

    const POWERSHELL: &str = "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe";

    /// The last toasts posted, kept alive so their click handlers stay registered.
    static LIVE: Mutex<VecDeque<ToastNotification>> = Mutex::new(VecDeque::new());

    fn app_id<R: Runtime>(app: &AppHandle<R>) -> String {
        if cfg!(debug_assertions) { POWERSHELL.to_owned() } else { app.config().identifier.clone() }
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;")
    }

    pub fn show<R: Runtime>(app: &AppHandle<R>, toast: &Toast<'_>) -> windows::core::Result<()> {
        let route = toast.route.unwrap_or("");
        let image = toast
            .image
            .as_ref()
            .map(|p| {
                let src = format!("file:///{}", p.to_string_lossy().replace('\\', "/"));
                format!(r#"<image placement="appLogoOverride" src="{}" alt=""/>"#, escape(&src))
            })
            .unwrap_or_default();
        let actions = match toast.action {
            Some(label) if !route.is_empty() => {
                format!(r#"<actions><action content="{}" arguments="{}" activationType="foreground"/></actions>"#, escape(label), escape(route))
            }
            _ => String::new(),
        };
        let xml = format!(
            r#"<toast launch="{}" activationType="foreground"><visual><binding template="ToastGeneric">{image}<text>{}</text><text>{}</text></binding></visual>{actions}</toast>"#,
            escape(route),
            escape(toast.title),
            escape(toast.body),
        );
        let document = XmlDocument::new()?;
        document.LoadXml(&HSTRING::from(xml))?;
        let notification = ToastNotification::CreateToastNotification(&document)?;
        notification.SetTag(&HSTRING::from(truncate(toast.tag)))?;
        notification.SetGroup(&HSTRING::from(toast.group))?;
        let handle = app.clone();
        notification.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, args| {
            let arguments = args
                .as_ref()
                .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
                .and_then(|a| a.Arguments().ok())
                .map(|a| a.to_string())
                .unwrap_or_default();
            super::open(&handle, &arguments);
            Ok(())
        }))?;
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id(app)))?.Show(&notification)?;
        let mut live = LIVE.lock().unwrap();
        live.push_back(notification);
        while live.len() > 64 {
            live.pop_front();
        }
        Ok(())
    }

    pub fn clear_group<R: Runtime>(app: &AppHandle<R>, group: &str) -> windows::core::Result<()> {
        ToastNotificationManager::History()?.RemoveGroupWithId(&HSTRING::from(group), &HSTRING::from(app_id(app)))
    }

    /// Windows refuses tags over 64 characters.
    fn truncate(tag: &str) -> &str {
        tag.char_indices().nth(64).map(|(i, _)| &tag[..i]).unwrap_or(tag)
    }
}
