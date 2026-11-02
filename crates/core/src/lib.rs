//! Anilili's engine: everything that is not the window.
//!
//! - [`net`]: one HTTP client that looks like Chrome on the wire.
//! - [`adblock`]: which requests an embedded player's page may make (filter lists).
//! - [`anilist`]: browse, search, details, schedule, and the signed-in user's list.
//! - [`miruro`]: Miruro's catalog API, which maps an AniList title to episodes and playable servers.
//! - [`proxy`]: a localhost HLS proxy for streams the player cannot take as served (Kwik's IVs).
//! - [`recs`]: recommendations from the published neighbour graph (Android's taste engine).
//! - [`store`]: SQLite for the library, watch progress, settings and response caches.
//! - [`mal`]: MyAnimeList login, list and progress sync.
//! - [`download`]: episode downloads (HLS/MP4) to local files.
//! - [`plugin`]: providers as sandboxed WebAssembly components (wasmtime).
//! - [`update`]: the signed release channel (Nostr event, mirrored as a file).
//! - [`backup`]: the backup file shared with the Android app (format, sealed logins).
//! - [`franchise`]: a title's whole franchise as a watch guide (Android's buildWatchOrder).
//! - [`links`]: `anilili://` links, shared with the Android app (`docs/LINKS.md`).
//! - [`catalogue`]: every failure a viewer can see, with a stable code.
//! - [`media_index`]: the project's static media index (episode art, logos, skip times, schedule).
//! - [`spelling`]: "Did you mean" for title searches (Android's `TitleSpelling.kt`).
//! - [`storyboard`]: a server's seek-bar previews (WebVTT of sprite-sheet tiles).

pub mod adblock;
pub mod anilist;
pub mod aniskip;
pub mod backup;
pub mod catalogue;
pub mod dns;
pub mod download;
pub mod error;
pub mod franchise;
pub mod links;
pub mod mal;
pub mod media_index;
pub mod miruro;
pub mod model;
pub mod net;
pub mod opensubtitles;
pub mod plugin;
pub mod proxy;
pub mod recs;
pub mod simkl;
pub mod spelling;
pub mod store;
pub mod storyboard;
pub mod update;

pub use error::{Error, Failure, Result};
