//! The window's own glass, under the webview's.
//!
//! On macOS the window takes the system's material: Liquid Glass on macOS 26 (`NSGlassEffectView`),
//! and below it the under-window vibrancy Tauri falls back to. The webview is transparent, so where
//! a part of the interface has glass on, its CSS lets that material through rather than imitating
//! it with a blur of its own (`src/index.css`, `html[data-native-glass]`). With every part off the
//! window has no material and the pages are a solid fill, as on Android.
//!
//! Windows keeps the CSS glass alone: mpv paints the main window's surface under the webview (see
//! AGENTS.md, "How the video works"), and Mica or Acrylic on that same window would be painted
//! over by the video, or fight it for the surface.

use tauri::{AppHandle, Manager, Runtime};

use crate::app::Appearance;

/// Whether any part of the window's pages wants glass (popups float above, and don't need the
/// window's material).
pub fn wanted(a: &Appearance) -> bool {
    a.glass_navigation || a.glass_home || a.glass_details || a.glass_browse || a.glass_pages
}

/// Puts the material behind the window, or takes it away.
pub fn apply<R: Runtime>(app: &AppHandle<R>, appearance: &Appearance) {
    let Some(window) = app.get_webview_window("main") else { return };
    #[cfg(target_os = "macos")]
    {
        use tauri::window::{Effect, EffectState, EffectsBuilder};
        let effects = wanted(appearance).then(|| {
            EffectsBuilder::new()
                .effect(Effect::LiquidGlassRegular)
                .effect(Effect::UnderWindowBackground)
                .state(EffectState::FollowsWindowActiveState)
                .build()
        });
        if let Err(e) = window.set_effects(effects) {
            tracing::warn!(error = %e, "window glass failed");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, appearance);
}

/// Whether this platform draws the window's glass natively, for the frontend's CSS.
#[tauri::command]
pub fn native_glass() -> bool {
    cfg!(target_os = "macos")
}
