//! OS media controls: hardware media keys, and the Windows media overlay (title, artwork,
//! play/pause, next/previous) via SystemMediaTransportControls.
//!
//! The controls live on their own thread; updates arrive over a channel and OS events go back to
//! mpv (play/pause/seek) or to the UI (next/previous episode, which only the page knows).

use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig};
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::player::Player;

#[derive(Debug, Clone)]
pub enum MediaUpdate {
    Metadata { title: String, subtitle: String, cover: Option<String>, duration: f64 },
    Playing { position: f64 },
    Paused { position: f64 },
    Stopped,
}

pub struct Media {
    tx: Option<Sender<MediaUpdate>>,
}

impl Media {
    pub fn update(&self, update: MediaUpdate) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(update);
        }
    }
}

pub fn start<R: Runtime>(app: &AppHandle<R>, hwnd: isize) -> Media {
    let (tx, rx) = mpsc::channel::<MediaUpdate>();
    let handle = app.clone();
    let spawned = std::thread::Builder::new().name("media-controls".into()).spawn(move || {
        // Linux (MPRIS) has no window handle; souvlaki ignores `hwnd` there.
        let hwnd = if cfg!(target_os = "linux") { None } else { Some(hwnd as *mut std::ffi::c_void) };
        let config = PlatformConfig { display_name: "Anilili", dbus_name: "anilili", hwnd };
        let mut controls = match MediaControls::new(config) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = ?e, "media controls unavailable");
                return;
            }
        };
        let events = handle.clone();
        if let Err(e) = controls.attach(move |event| on_event(&events, event)) {
            tracing::warn!(error = ?e, "media controls attach failed");
            return;
        }
        while let Ok(update) = rx.recv() {
            let result = match update {
                MediaUpdate::Metadata { title, subtitle, cover, duration } => controls.set_metadata(MediaMetadata {
                    title: Some(&title),
                    artist: Some(&subtitle),
                    album: Some("Anilili"),
                    cover_url: cover.as_deref(),
                    duration: (duration > 0.0).then(|| Duration::from_secs_f64(duration)),
                }),
                MediaUpdate::Playing { position } => controls
                    .set_playback(MediaPlayback::Playing { progress: Some(MediaPosition(Duration::from_secs_f64(position.max(0.0)))) }),
                MediaUpdate::Paused { position } => controls
                    .set_playback(MediaPlayback::Paused { progress: Some(MediaPosition(Duration::from_secs_f64(position.max(0.0)))) }),
                MediaUpdate::Stopped => controls.set_playback(MediaPlayback::Stopped),
            };
            if let Err(e) = result {
                tracing::debug!(error = ?e, "media controls update failed");
            }
        }
    });
    Media { tx: spawned.ok().map(|_| tx) }
}

fn on_event<R: Runtime>(app: &AppHandle<R>, event: MediaControlEvent) {
    // The player may not exist: on macOS it can fail to start (see main.rs).
    let Some(player) = app.try_state::<Player>() else { return };
    match event {
        MediaControlEvent::Play => player.set_pause(false),
        MediaControlEvent::Pause => player.set_pause(true),
        MediaControlEvent::Toggle => player.toggle_pause(),
        MediaControlEvent::Next => {
            let _ = app.emit("media-key", "next");
        }
        MediaControlEvent::Previous => {
            let _ = app.emit("media-key", "previous");
        }
        MediaControlEvent::SetPosition(MediaPosition(position)) => player.seek_absolute(position.as_secs_f64()),
        MediaControlEvent::Seek(direction) | MediaControlEvent::SeekBy(direction, _) => {
            let delta = if direction == souvlaki::SeekDirection::Forward { 10.0 } else { -10.0 };
            player.seek_relative(delta);
        }
        MediaControlEvent::Raise => {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }
        _ => {}
    }
}
