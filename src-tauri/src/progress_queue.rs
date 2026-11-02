//! Finished episodes on their way to the list service, kept until the service has them.
//!
//! The player used to post "watched up to N" once, the moment an episode finished, and report a
//! failure. With no connection — the usual state for someone watching their downloads — that one
//! post failed and the episode was gone: nothing retried it, and the account only moved again on
//! the *next* episode, so the last one of a session never arrived at all. Android has queued these
//! since 0.1.77 (`ListProgressSyncWorker`); this is the desktop's equivalent.
//!
//! Every finished episode goes through here. It is written to the store first, so it outlives a
//! closed app, then sent at once; a failure that says nothing about the push itself (no
//! connection, a rate limit, a server error) leaves it queued, and the queue is retried every
//! minute and at launch. One entry per title, the highest episode winning: "watched up to N"
//! makes every lower number redundant.

use std::collections::BTreeMap;
use std::time::Duration;

use anilili_core::store::now;
use anilili_core::Error;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::app::AppState;

const QUEUE_KEY: &str = "pending_progress";
const RETRY_EVERY: Duration = Duration::from_secs(60);
/// Pushes the service answered and refused. Each one blocks nothing else, but one that will never
/// succeed should not be retried for ever.
const MAX_REFUSALS: u32 = 5;

/// One title's unsent progress.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pending {
    pub episode: i32,
    pub queued_at: i64,
    /// Set once a send has failed. A push that waited may be older than what the account now
    /// says (the episode was watched on the phone meanwhile), so a deferred one is checked first.
    #[serde(default)]
    pub deferred: bool,
    #[serde(default)]
    pub refusals: u32,
}

/// The queue with `episode` recorded for `anime_id`; a lower episode than one already waiting
/// changes nothing.
pub fn with_episode(mut queue: BTreeMap<i64, Pending>, anime_id: i64, episode: i32, at: i64) -> BTreeMap<i64, Pending> {
    match queue.get_mut(&anime_id) {
        Some(pending) if pending.episode >= episode => {}
        Some(pending) => {
            pending.episode = episode;
            pending.queued_at = at;
            pending.refusals = 0;
        }
        None => {
            queue.insert(anime_id, Pending { episode, queued_at: at, deferred: false, refusals: 0 });
        }
    }
    queue
}

/// Whether a failed send should stay queued without counting against the push: it never reached
/// the service, or the service turned it away for reasons of its own.
pub fn is_transient(error: &Error) -> bool {
    match error {
        Error::Network(_) | Error::RateLimited { .. } => true,
        Error::Http { status, .. } => *status >= 500,
        _ => false,
    }
}

/// Queues a finished episode and tries to send it now.
pub fn push<R: Runtime>(app: &AppHandle<R>, anime_id: i64, episode: i32) {
    if anime_id <= 0 || episode <= 0 {
        return;
    }
    let state = app.state::<AppState>();
    let queue = state.store.setting::<BTreeMap<i64, Pending>>(QUEUE_KEY).unwrap_or_default();
    if let Err(e) = state.store.set_setting(QUEUE_KEY, &with_episode(queue, anime_id, episode, now())) {
        tracing::warn!(anime_id, episode, error = %e, "queueing progress failed");
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { flush(&app).await });
}

/// Retries whatever is queued, at launch and then every minute.
pub fn start<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            flush(&app).await;
            tokio::time::sleep(RETRY_EVERY).await;
        }
    });
}

/// One pass over the queue. Serialised, so a finished episode and the timer never send the same
/// entry twice.
async fn flush<R: Runtime>(app: &AppHandle<R>) {
    static FLUSHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = FLUSHING.lock().await;
    let state = app.state::<AppState>();
    let queue = state.store.setting::<BTreeMap<i64, Pending>>(QUEUE_KEY).unwrap_or_default();
    if queue.is_empty() {
        return;
    }
    if !state.signed_in().await {
        // As on Android: progress belongs to the account that watched it, not the next one.
        tracing::info!(titles = queue.len(), "signed out; dropping queued progress");
        let _ = state.store.delete_setting(QUEUE_KEY);
        return;
    }
    for (anime_id, pending) in queue {
        let outcome = state.deliver_progress(anime_id, pending.episode, pending.deferred).await;
        // Re-read per entry: an episode finished during the send may have raised this title.
        let mut current = state.store.setting::<BTreeMap<i64, Pending>>(QUEUE_KEY).unwrap_or_default();
        let still_same = current.get(&anime_id).is_some_and(|p| p.episode == pending.episode);
        match outcome {
            Ok(()) => {
                if still_same {
                    current.remove(&anime_id);
                }
            }
            Err(error) if is_transient(&error) => {
                tracing::info!(anime_id, episode = pending.episode, error = %error, "progress kept for later");
                if let Some(entry) = current.get_mut(&anime_id) {
                    entry.deferred = true;
                }
            }
            Err(error) => {
                tracing::warn!(anime_id, episode = pending.episode, error = %error, "progress refused");
                if let Some(entry) = current.get_mut(&anime_id).filter(|_| still_same) {
                    entry.deferred = true;
                    entry.refusals += 1;
                    if entry.refusals >= MAX_REFUSALS {
                        current.remove(&anime_id);
                        crate::app::report(app, crate::app::as_entry(anilili_core::catalogue::ACC_SYNC)(error));
                    }
                }
            }
        }
        if let Err(e) = state.store.set_setting(QUEUE_KEY, &current) {
            tracing::warn!(error = %e, "saving the progress queue failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highest_episode_per_title_wins() {
        let queue = with_episode(BTreeMap::new(), 21, 5, 100);
        let queue = with_episode(queue, 21, 3, 200);
        assert_eq!(queue[&21], Pending { episode: 5, queued_at: 100, deferred: false, refusals: 0 });
        let queue = with_episode(queue, 21, 6, 300);
        assert_eq!(queue[&21].episode, 6);
        assert_eq!(queue[&21].queued_at, 300);
        let queue = with_episode(queue, 30, 1, 400);
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn a_later_episode_resets_refusals_but_stays_deferred() {
        let mut queue = with_episode(BTreeMap::new(), 21, 5, 100);
        let entry = queue.get_mut(&21).unwrap();
        entry.deferred = true;
        entry.refusals = 3;
        let queue = with_episode(queue, 21, 6, 200);
        assert_eq!(queue[&21].refusals, 0);
        assert!(queue[&21].deferred);
    }

    #[test]
    fn only_failures_outside_the_push_are_transient() {
        assert!(is_transient(&Error::Network("dns error".into())));
        assert!(is_transient(&Error::RateLimited { service: "AniList", retry_after: 60 }));
        assert!(is_transient(&Error::Http { service: "AniList", status: 503, detail: String::new() }));
        assert!(!is_transient(&Error::Http { service: "AniList", status: 400, detail: String::new() }));
        assert!(!is_transient(&Error::NotFound("Media".into())));
    }
}
