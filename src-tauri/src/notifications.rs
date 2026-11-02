//! The notification system, after Android's (`data/reminder/`, `ui/notifications/`).
//!
//! - **The AniList feed** is polled every 15 minutes while signed in (AniList has no push; this is
//!   Android's `AiringPollWorker`). Unread items not seen before become Windows notifications, at
//!   most eight a poll; the ids are remembered so a poll never repeats one.
//! - **Local release alerts** cover what the feed doesn't: library titles for a viewer signed out
//!   or on MyAnimeList, and, for an AniList viewer, library titles not being watched (AniList only
//!   announces airings for CURRENT and REPEATING entries). Refreshed every three hours; "Episode N
//!   is out" at airing time (Android's `AutomaticReleaseManager`).
//! - **"Airs soon" reminders** the viewer sets on the schedule, ten minutes before airing
//!   (`ReminderManager`).
//! - **One alert per episode** whichever path sees it first: the feed and the local alert claim
//!   `episode:<id>:<n>` in the store's ledger, and the loser stays silent (`EpisodeAlertLedger`).
//!   All three post under the same tag, so "is out" replaces "airs soon".
//!
//! Besides the feed, the app keeps its own log (store `notices`): releases, reminders, finished
//! downloads. The Notifications page shows both; the bell's count is AniList's unread count plus
//! the log's.

use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use anilili_core::anilist::FeedItem;
use anilili_core::catalogue as c;
use anilili_core::model::{AiringEntry, Notice};
use anilili_core::store::now;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, Runtime, State};
use tokio::sync::Notify;

use crate::app::{AppState, CmdResult, CommandError, as_entry};
use crate::notify::{self, Toast};

const FEED_EVERY: Duration = Duration::from_secs(15 * 60);
const ALERTS_EVERY: Duration = Duration::from_secs(3 * 60 * 60);
/// An alert that comes due while the app was closed still counts within a day (Android's
/// `oldestUseful`).
const LATE_ALERT_SECS: i64 = 24 * 60 * 60;
const REMINDER_LEAD_SECS: i64 = 10 * 60;
const MAX_PUSHES_PER_POLL: usize = 8;
const REMINDERS_KEY: &str = "notifications.reminders";

#[derive(Default)]
pub struct Notifications {
    /// AniList's unread count, as last read.
    feed_unread: AtomicI64,
    alerts: Mutex<Vec<Alert>>,
    feed_polled: Mutex<Option<Instant>>,
    alerts_synced: Mutex<Option<Instant>>,
    wake: Notify,
}

/// A library title's next episode, announced when it airs.
#[derive(Debug, Clone)]
struct Alert {
    anime_id: i64,
    episode: i32,
    title: String,
    cover: Option<String>,
    airing_at: i64,
}

/// An "airs soon" reminder set from the schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reminder {
    pub anime_id: i64,
    pub episode: i32,
    pub airing_at: i64,
    pub title: String,
    pub cover: Option<String>,
}

impl Reminder {
    fn key(&self) -> String {
        format!("{}:{}:{}", self.anime_id, self.episode, self.airing_at)
    }
}

pub fn start<R: Runtime>(app: &AppHandle<R>) {
    app.manage(Notifications::default());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Launch has enough to do; the first look waits a little.
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            tick(&app).await;
            let notifications = app.state::<Notifications>();
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                _ = notifications.wake.notified() => {}
            }
        }
    });
}

/// Runs the feed poll and the alert refresh now (after signing in, or turning alerts on).
pub fn poke<R: Runtime>(app: &AppHandle<R>) {
    if let Some(n) = app.try_state::<Notifications>() {
        *n.feed_polled.lock().unwrap() = None;
        *n.alerts_synced.lock().unwrap() = None;
        n.wake.notify_one();
    }
}

fn due(at: &Mutex<Option<Instant>>, every: Duration) -> bool {
    at.lock().unwrap().is_none_or(|t| t.elapsed() >= every)
}

async fn tick<R: Runtime>(app: &AppHandle<R>) {
    let state = app.state::<AppState>();
    let notifications = app.state::<Notifications>();
    let enabled = state.settings().release_notifications;

    if state.anilist.has_token() && due(&notifications.feed_polled, FEED_EVERY) {
        *notifications.feed_polled.lock().unwrap() = Some(Instant::now());
        match state.anilist.notifications(false).await {
            Ok((items, unread)) => {
                notifications.feed_unread.store(unread, Ordering::Relaxed);
                if enabled {
                    push_feed(app, &state, &items).await;
                }
                changed(app);
            }
            Err(e) => tracing::info!(error = %e, "AniList notification poll failed"),
        }
    } else if !state.anilist.has_token() && notifications.feed_unread.swap(0, Ordering::Relaxed) != 0 {
        changed(app);
    }

    if !enabled {
        notifications.alerts.lock().unwrap().clear();
        return;
    }
    if due(&notifications.alerts_synced, ALERTS_EVERY) {
        match sync_alerts(&state).await {
            Ok(alerts) => {
                tracing::info!(count = alerts.len(), "release alerts refreshed");
                *notifications.alerts.lock().unwrap() = alerts;
                *notifications.alerts_synced.lock().unwrap() = Some(Instant::now());
            }
            // Tried again on the next tick but one, not every minute.
            Err(e) => {
                tracing::info!(error = %e, "refreshing release alerts failed");
                *notifications.alerts_synced.lock().unwrap() = Some(Instant::now() - ALERTS_EVERY + Duration::from_secs(10 * 60));
            }
        }
    }
    fire_alerts(app, &state).await;
    fire_reminders(app, &state).await;
}

/// The library titles to announce locally: everything airing, less what AniList announces itself.
async fn sync_alerts(state: &AppState) -> anilili_core::Result<Vec<Alert>> {
    let library = state.store.library()?;
    let feed_covers_watching = state.anilist.has_token();
    let ids: Vec<i64> = library
        .iter()
        .filter(|e| !(feed_covers_watching && e.status == "watching" && e.synced))
        .filter(|e| e.status != "completed" && e.status != "dropped")
        .map(|e| e.anime.id)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let cutoff = now() - LATE_ALERT_SECS;
    Ok(state
        .anilist
        .cards_by_ids(&ids)
        .await?
        .into_iter()
        .filter_map(|pick| {
            let next = pick.card.next_airing.as_ref()?;
            (next.airing_at >= cutoff).then(|| Alert {
                anime_id: pick.card.id,
                episode: next.episode,
                title: pick.card.title.clone(),
                cover: pick.card.cover.clone(),
                airing_at: next.airing_at,
            })
        })
        .collect())
}

fn episode_key(anime_id: i64, episode: i64) -> String {
    format!("episode:{anime_id}:{episode}")
}

async fn fire_alerts<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    let now = now();
    let notifications = app.state::<Notifications>();
    let fired: Vec<Alert> = {
        let mut alerts = notifications.alerts.lock().unwrap();
        let (due, waiting): (Vec<Alert>, Vec<Alert>) = alerts.drain(..).partition(|a| a.airing_at <= now);
        *alerts = waiting;
        due
    };
    for alert in &fired {
        if alert.airing_at < now - LATE_ALERT_SECS {
            continue;
        }
        // The feed may have announced it already: then stay silent.
        if !state.store.announce_claim(&episode_key(alert.anime_id, alert.episode as i64)).unwrap_or(false) {
            continue;
        }
        let body = format!("Episode {} is out", alert.episode);
        announce_episode(app, state, alert.anime_id, alert.episode as i64, &alert.title, &body, alert.cover.as_deref(), "airing").await;
    }
    if !fired.is_empty() {
        // The next episode's time is known now (Android re-syncs after each alarm).
        *notifications.alerts_synced.lock().unwrap() = None;
        changed(app);
    }
}

async fn fire_reminders<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    let now = now();
    let mut reminders: Vec<Reminder> = state.store.setting(REMINDERS_KEY).unwrap_or_default();
    let (due, waiting): (Vec<Reminder>, Vec<Reminder>) = reminders.drain(..).partition(|r| r.airing_at - REMINDER_LEAD_SECS <= now);
    if due.is_empty() {
        return;
    }
    let _ = state.store.set_setting(REMINDERS_KEY, &waiting);
    for reminder in due.iter().filter(|r| r.airing_at > now - LATE_ALERT_SECS) {
        let body = if reminder.airing_at > now { format!("Episode {} airs soon", reminder.episode) } else { format!("Episode {} is out", reminder.episode) };
        announce_episode(app, state, reminder.anime_id, reminder.episode as i64, &reminder.title, &body, reminder.cover.as_deref(), "reminder").await;
    }
    changed(app);
}

/// One episode's notification: in the log, and as a toast under the episode's own tag.
#[allow(clippy::too_many_arguments)]
async fn announce_episode<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    anime_id: i64,
    episode: i64,
    title: &str,
    body: &str,
    cover: Option<&str>,
    kind: &str,
) {
    let route = format!("/anime/{anime_id}");
    let _ = state.store.notice_add(kind, title, Some(body), Some(&route), cover, Some(anime_id));
    let image = notify::artwork(app, cover).await;
    notify::show(
        app,
        &Toast {
            tag: &episode_key(anime_id, episode),
            group: notify::EPISODES,
            title,
            body,
            image,
            route: Some(&route),
            action: Some("View anime"),
        },
    );
}

/// Unread feed items not pushed before, oldest of them first so the newest ends on top.
async fn push_feed<R: Runtime>(app: &AppHandle<R>, state: &AppState, items: &[FeedItem]) {
    let fresh: Vec<&FeedItem> =
        items.iter().filter(|i| i.unread && state.store.announce_claim(&format!("anilist:{}", i.id)).unwrap_or(false)).collect();
    for item in fresh.into_iter().take(MAX_PUSHES_PER_POLL).rev() {
        let route = item.media_id.map(|id| format!("/anime/{id}")).unwrap_or_else(|| "/notifications".into());
        let text = feed_text(item);
        if item.kind == "airing"
            && let (Some(media), Some(episode)) = (item.media_id, item.episode)
        {
            if !state.store.announce_claim(&episode_key(media, episode)).unwrap_or(false) {
                continue;
            }
            let image = notify::artwork(app, item.image.as_deref()).await;
            notify::show(
                app,
                &Toast { tag: &episode_key(media, episode), group: notify::EPISODES, title: &item.title, body: &text, image, route: Some(&route), action: Some("View anime") },
            );
            continue;
        }
        let image = notify::artwork(app, item.image.as_deref()).await;
        let action = if item.media_id.is_some() { "View anime" } else { "Open notifications" };
        notify::show(
            app,
            &Toast { tag: &format!("anilist:{}", item.id), group: notify::ANILIST, title: &item.title, body: &text, image, route: Some(&route), action: Some(action) },
        );
    }
}

/// The title carries the anime, so the body only says what happened to it.
fn feed_text(item: &FeedItem) -> String {
    if item.kind == "airing" {
        return item.episode.map(|e| format!("Episode {e} is out")).unwrap_or_else(|| "A new episode is out".into());
    }
    if let Some(detail) = &item.detail {
        return detail.clone();
    }
    match item.badge.as_deref() {
        Some(badge) => {
            let lower = badge.to_lowercase();
            let mut chars = lower.chars();
            chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
        }
        None => "Open to see more".into(),
    }
}

/// Something new to show: the page refetches, and the bell takes the new count.
pub fn changed<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit("notifications-changed", unread_count(app));
}

/// Only the count moved (the page read or marked the list itself). A separate event, because the
/// page refetches on `notifications-changed`, and a fetch that announced that would fetch again
/// for ever (it did: AniList answered 429 within seconds).
fn unread_changed<R: Runtime>(app: &AppHandle<R>) {
    let _ = app.emit("notifications-unread", unread_count(app));
}

fn unread_count<R: Runtime>(app: &AppHandle<R>) -> i64 {
    let feed = app.try_state::<Notifications>().map(|n| n.feed_unread.load(Ordering::Relaxed)).unwrap_or_default();
    feed + app.state::<AppState>().store.notices_unread().unwrap_or_default()
}

/// The app's own notification, posted from elsewhere (downloads, updates): logged, and shown
/// when `toast` (the caller has checked its setting).
pub async fn post<R: Runtime>(app: &AppHandle<R>, post: Post<'_>) {
    let state = app.state::<AppState>();
    let _ = state.store.notice_add(post.kind, post.title, Some(post.body), post.route, post.cover, post.anime_id);
    if post.toast {
        let image = notify::artwork(app, post.cover).await;
        notify::show(
            app,
            &Toast { tag: post.tag, group: post.group, title: post.title, body: post.body, image, route: post.route, action: None },
        );
    }
    changed(app);
}

pub struct Post<'a> {
    pub kind: &'a str,
    pub tag: &'a str,
    pub group: &'a str,
    pub title: &'a str,
    pub body: &'a str,
    pub route: Option<&'a str>,
    pub cover: Option<&'a str>,
    pub anime_id: Option<i64>,
    pub toast: bool,
}

// ---- commands -------------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationsPage {
    /// The AniList feed; empty when signed out of AniList.
    pub feed: Vec<FeedItem>,
    pub feed_error: Option<CommandError>,
    pub anilist: bool,
    pub notices: Vec<Notice>,
}

/// The page's contents. Opening it clears the feed's toasts from the notification centre (the
/// viewer is looking at the same items, as on Android); `mark_read` also resets the counts.
#[tauri::command]
pub async fn notifications_list<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>, mark_read: bool) -> CmdResult<NotificationsPage> {
    notify::clear_group(&app, notify::ANILIST);
    let anilist = state.anilist.has_token();
    let (feed, feed_error) = if anilist {
        match state.anilist.notifications(mark_read).await {
            Ok((items, unread)) => {
                // Shown now, so not pushed later.
                for item in items.iter().filter(|i| i.unread) {
                    let _ = state.store.announce_claim(&format!("anilist:{}", item.id));
                }
                app.state::<Notifications>().feed_unread.store(if mark_read { 0 } else { unread }, Ordering::Relaxed);
                (items, None)
            }
            Err(e) => (Vec::new(), Some(as_entry(c::NET_NOTIFICATIONS)(e))),
        }
    } else {
        (Vec::new(), None)
    };
    let notices = state.store.notices(200)?;
    if mark_read {
        state.store.notices_mark_read()?;
    }
    unread_changed(&app);
    Ok(NotificationsPage { feed, feed_error, anilist, notices })
}

#[tauri::command(async)]
pub fn notifications_unread<R: Runtime>(app: AppHandle<R>) -> i64 {
    unread_count(&app)
}

/// Empties the app's own log (the AniList feed is the account's).
#[tauri::command(async)]
pub fn notifications_clear<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CmdResult<()> {
    state.store.notices_clear()?;
    changed(&app);
    Ok(())
}

/// The schedule's reminders, as `animeId:episode:airingAt` keys.
#[tauri::command(async)]
pub fn reminders_list(state: State<'_, AppState>) -> Vec<String> {
    state.store.setting::<Vec<Reminder>>(REMINDERS_KEY).unwrap_or_default().iter().map(Reminder::key).collect()
}

/// Sets or clears the "airs soon" reminder for a schedule entry; true when it is now set.
#[tauri::command]
pub fn reminder_toggle(state: State<'_, AppState>, entry: AiringEntry) -> CmdResult<bool> {
    let reminder = Reminder {
        anime_id: entry.anime.id,
        episode: entry.episode,
        airing_at: entry.airing_at,
        title: entry.anime.title.clone(),
        cover: entry.anime.cover.clone(),
    };
    let mut all: Vec<Reminder> = state.store.setting(REMINDERS_KEY).unwrap_or_default();
    let before = all.len();
    all.retain(|r| r.key() != reminder.key());
    let set = all.len() == before;
    if set {
        all.push(reminder);
    }
    state.store.set_setting(REMINDERS_KEY, &all)?;
    Ok(set)
}

/// A sample notification, so the viewer can see Windows lets them through.
#[tauri::command]
pub async fn notification_test<R: Runtime>(app: AppHandle<R>) -> CmdResult<()> {
    notify::show(
        &app,
        &Toast {
            tag: "test",
            group: notify::APP,
            title: "Notifications are on",
            body: "New episodes and finished downloads will show up like this.",
            image: None,
            route: Some("/notifications"),
            action: None,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(kind: &str, badge: Option<&str>, detail: Option<&str>, episode: Option<i64>) -> FeedItem {
        FeedItem {
            id: 1,
            kind: kind.into(),
            created_at: 0,
            title: "T".into(),
            badge: badge.map(Into::into),
            detail: detail.map(Into::into),
            episode,
            media_id: Some(1),
            image: None,
            banner: None,
            unread: true,
        }
    }

    #[test]
    fn feed_texts_read_as_androids() {
        assert_eq!(feed_text(&item("airing", Some("EP 5"), None, Some(5))), "Episode 5 is out");
        assert_eq!(feed_text(&item("media", Some("NEW RELATED"), None, None)), "New related");
        assert_eq!(feed_text(&item("social", None, Some("Someone liked your activity."), None)), "Someone liked your activity.");
    }
}
