//! Checking for and installing releases from the signed channel (see `anilili_core::update`).

use anilili_core::plugin::{PluginInfo, Plugins};
use anilili_core::update::{self, Update};
use serde::Serialize;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime, State};

use anilili_core::catalogue as c;

use crate::app::{AppState, CmdResult, CommandError, as_entry, fail};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current: String,
    pub update: Option<Update>,
}

/// `entry` for an update failure, unless the release itself was rejected (bad signature or
/// checksum), which has its own code: that one is worth a report, a network blip is not.
fn update_failure(entry: &'static c::Entry) -> impl Fn(anilili_core::Error) -> CommandError {
    move |e| {
        let detail = e.to_string();
        if detail.contains("rejected") || detail.contains("checksum mismatch") {
            fail(c::UPD_SIGNATURE, detail)
        } else {
            as_entry(entry)(e)
        }
    }
}

#[tauri::command]
pub async fn update_check<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<UpdateStatus> {
    let current = app.package_info().version.to_string();
    let update = update::check(&state.http_client(), &current).await.map_err(update_failure(c::UPD_CHECK))?;
    tracing::info!(current, newer = update.as_ref().map_or("none", |u| u.version.as_str()), "update check");
    Ok(UpdateStatus { current, update })
}

/// The route a click on the update notification opens: not a page, the frontend opens the update
/// dialog for it (src/main.tsx).
const UPDATE_ROUTE: &str = "/update";

/// "A new version is out" as a system notification; a click downloads it (the update dialog).
#[tauri::command]
pub fn update_notify<R: Runtime>(app: AppHandle<R>, update: Update) {
    let title = format!("Anilili {} is available", update.version);
    let body = if update.changelog.is_empty() { "Click to download and install it.".to_owned() } else { update.changelog.clone() };
    crate::notify::show(
        &app,
        &crate::notify::Toast {
            tag: "update",
            group: crate::notify::APP,
            title: &title,
            body: &body,
            image: None,
            route: Some(UPDATE_ROUTE),
            action: Some("Update"),
        },
    );
}

/// The installer `update_download` fetched and checked, waiting for `update_apply`. Kept here
/// rather than handed to the page, so only a file this process verified is ever run.
static DOWNLOADED: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// Downloads the installer and checks it against the signed checksum, reporting progress as
/// `update-progress` (bytes done, total). Installing is a separate step, once the viewer says so.
#[tauri::command]
pub async fn update_download<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, update: Update) -> CmdResult<()> {
    let name = update.url.rsplit('/').next().filter(|n| n.ends_with(".exe") || n.ends_with(".dmg") || n.ends_with(".deb") || n.ends_with(".AppImage")).unwrap_or("Anilili-setup.exe");
    let path = app.path().temp_dir().map_err(|e| fail(c::UPD_INSTALL, e.to_string()))?.join(name);
    let events = app.clone();
    // Once per whole percent: the body arrives in thousands of small chunks.
    let shown = std::sync::atomic::AtomicU64::new(u64::MAX);
    update::download_installer(&state.http_client(), &update, &path, |done, total| {
        let percent = done * 100 / total.max(1);
        if shown.swap(percent, std::sync::atomic::Ordering::Relaxed) != percent {
            let _ = events.emit("update-progress", (done, total));
        }
    })
    .await
    .map_err(update_failure(c::UPD_INSTALL))?;
    tracing::info!(path = %path.display(), version = %update.version, "update downloaded");
    *DOWNLOADED.lock().unwrap() = Some(path);
    Ok(())
}

/// Installs the downloaded update and quits. On Windows the installer runs passively (a progress
/// bar, no wizard: `/P`), as an update (keeps shortcuts, no uninstall: `/UPDATE`), and reopens the
/// app when it's done (`/R`); it closes this copy itself if it's still running.
#[tauri::command]
pub async fn update_apply<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    let path = DOWNLOADED.lock().unwrap().clone().ok_or_else(|| fail(c::UPD_INSTALL, "no update has been downloaded".to_owned()))?;
    tracing::info!(path = %path.display(), "starting installer");
    #[cfg(target_os = "macos")]
    {
        let dmg = path.clone();
        let installed = tauri::async_runtime::spawn_blocking(move || install_dmg(&dmg)).await.map_err(|e| e.to_string()).and_then(|r| r);
        if let Err(e) = installed {
            // No write access to the app's folder, say: show the image so it can be dragged over.
            tracing::warn!(error = %e, "could not replace the app; opening the disk image instead");
            std::process::Command::new("open").arg(&path).spawn().map_err(|e| fail(c::UPD_INSTALL, format!("could not open the disk image: {e}")))?;
        }
    }
    if cfg!(target_os = "linux") {
        return Err(fail(c::UPD_INSTALL, "Linux builds update through their package; reinstall the new package".to_owned()));
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    std::process::Command::new(&path)
        .args(["/P", "/R", "/UPDATE"])
        .spawn()
        .map_err(|e| fail(c::UPD_INSTALL, format!("could not start the installer: {e}")))?;
    app.exit(0);
    Ok(())
}

/// macOS: a `.dmg` can't be run like the Windows installer. Mount it, swap its app in for the
/// running one (keeping the installed bundle's name and folder), and reopen it once this process
/// has quit, since the single-instance plugin would hand a launch made sooner back to us. We
/// downloaded the image ourselves, not a browser, so it carries no quarantine flag and Gatekeeper
/// doesn't ask again.
#[cfg(target_os = "macos")]
fn install_dmg(dmg: &std::path::Path) -> Result<(), String> {
    use std::process::Command;
    let run = |cmd: &mut Command| -> Result<(), String> {
        let out = cmd.output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()) }
    };
    // …/Anilili.app/Contents/MacOS/<binary>
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = exe.ancestors().nth(3).filter(|p| p.extension().is_some_and(|e| e == "app")).ok_or("not running from an .app bundle")?.to_path_buf();
    let folder = bundle.parent().ok_or("the app has no folder")?;
    let mount = dmg.with_extension("mount");
    let _ = run(Command::new("hdiutil").args(["detach", "-quiet", "-force"]).arg(&mount));
    run(Command::new("hdiutil").args(["attach", "-nobrowse", "-readonly", "-noautoopen", "-quiet", "-mountpoint"]).arg(&mount).arg(dmg))?;
    let replaced = (|| {
        let app = std::fs::read_dir(&mount)
            .map_err(|e| e.to_string())?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or("no app in the disk image")?;
        // Copy beside the old app first, so a failed copy leaves it untouched.
        let staged = folder.join(".Anilili-update.app");
        let old = folder.join(".Anilili-old.app");
        let _ = std::fs::remove_dir_all(&staged);
        let _ = std::fs::remove_dir_all(&old);
        run(Command::new("ditto").arg(&app).arg(&staged))?;
        std::fs::rename(&bundle, &old).map_err(|e| e.to_string())?;
        if let Err(e) = std::fs::rename(&staged, &bundle) {
            let _ = std::fs::rename(&old, &bundle);
            return Err(e.to_string());
        }
        // The running process keeps its open files; nothing else needs the old bundle.
        let _ = std::fs::remove_dir_all(&old);
        Ok(())
    })();
    let _ = run(Command::new("hdiutil").args(["detach", "-quiet"]).arg(&mount));
    replaced?;
    Command::new("/bin/sh")
        .args(["-c", r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; open "$2""#, "sh"])
        .arg(std::process::id().to_string())
        .arg(&bundle)
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn providers_list(state: State<'_, AppState>) -> Vec<PluginInfo> {
    state.plugins.read().unwrap().list()
}

/// Fetches newer provider modules from the signed channel, then reloads providers.
/// Returns the ids that were updated.
#[tauri::command]
pub async fn providers_update<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<Vec<String>> {
    let http = state.http_client();
    let installed = state.plugins.read().unwrap().list();
    let releases = update::check_providers(&http).await.map_err(update_failure(c::UPD_PROVIDERS))?;
    let dir = app.path().app_data_dir().map_err(|e| fail(c::UPD_PROVIDERS, e.to_string()))?.join("providers");
    let mut updated = Vec::new();
    for release in releases {
        let current = installed.iter().find(|p| p.id == release.id).map(|p| p.version.as_str()).unwrap_or("0");
        if update::is_newer(&release.version, current) {
            update::download_provider(&http, &release, &dir).await.map_err(update_failure(c::UPD_PROVIDERS))?;
            updated.push(release.id);
        }
    }
    if !updated.is_empty() {
        let reloaded = Plugins::load(
            http,
            &crate::app::plugin_dirs(&app),
            Some(state.browser.clone()),
            Some(state.store.clone()),
            crate::app::wasm_cache_dir(&app).as_deref(),
        )
            .await
            .map_err(update_failure(c::UPD_PROVIDERS))?;
        *state.plugins.write().unwrap() = Arc::new(reloaded);
        state.sources.lock().unwrap().clear();
    }
    Ok(updated)
}
