//! Discord Rich Presence over the desktop client's local IPC pipe.
//!
//! On desktop this needs only the application id. The Android app's blocker, the Social SDK
//! access gate, does not apply to local IPC, and neither does its sign-in: the card goes to
//! whichever account the Discord client on this machine has open. Discord may not be running, or
//! may start later, so a background thread owns the connection and retries on its own.
//!
//! The card is Android's (`DiscordPresencePayload.kt`; keep the two in step): the series, then the
//! episode as "Ep: 3/12 - Its Title" ("Ep: 3/12" without one), the cover, the app's mark as the
//! badge (the pause glyph while paused), and one button to the project's Discord server. Two things differ because IPC is not the gateway: a cover is sent as
//! its https URL (the client proxies it; no `external-assets` registration), and the uploaded art
//! is named by its key (the client resolves it; the gateway needs the asset id).
//!
//! Publishing follows Android's `DiscordPresence.kt`: a card goes out only when it changes (a
//! different episode or title, a pause, a seek that moves the clock more than ten seconds), at
//! most once every five seconds, the latest one winning, since Discord keeps a progress bar moving
//! on its own and rate-limits activity updates.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use discord_rich_presence::activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

/// The same Discord application the Android app uses ("Anilili").
const APPLICATION_ID: &str = "1545324837306634250";

/// Art uploaded to the application: the app's mark and the pause glyph (Android's badge keys).
const APP_ICON_ASSET: &str = "icon";
const PAUSED_ASSET: &str = "paused";

/// The card's one button: the project's Discord server, About's link (there are no per-title pages).
const WATCH_BUTTON_LABEL: &str = "Join the Anilili Discord";
const WATCH_BUTTON_URL: &str = "https://discord.gg/Anilili";

/// Discord truncates past this and drops a `details`/`state` shorter than two characters.
const MAX_TEXT: usize = 128;

/// Updates inside this window wait, and only the last is sent (a dragged seek bar is a burst).
const MIN_PUBLISH_INTERVAL: Duration = Duration::from_secs(5);
/// How far, in seconds, the clock may drift from where the card put it before republishing.
const SEEK_TOLERANCE: f64 = 10.0;
/// Between attempts to reach a Discord that isn't running.
const RECONNECT_INTERVAL: Duration = Duration::from_secs(30);
/// An unchanged card is sent again this often: the pipe can't tell a restarted Discord (which
/// dropped the card) from a live one until a write fails.
const REFRESH_INTERVAL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct Presence {
    pub anime_id: i64,
    pub title: String,
    pub episode: f64,
    pub episode_title: Option<String>,
    pub total: Option<i32>,
    pub cover: Option<String>,
    pub position: f64,
    /// 0 when unknown (an embedded player's, until its page reports the video): the card then
    /// counts up from `position` instead of drawing a bar.
    pub duration: f64,
    pub paused: bool,
    /// From an embedded (iframe) player rather than mpv: mpv stopping leaves it be.
    pub embedded: bool,
}

impl Presence {
    /// What the card shows, with the clock left out: two reports that differ only in position
    /// are the same card, which Discord keeps moving on its own.
    fn identity(&self) -> String {
        format!("{}/{}/{}/{:?}/{}", self.anime_id, self.title, self.episode, self.episode_title, self.paused)
    }
}

enum Message {
    Show(Presence),
    /// `players_only`: only a card mpv put up, so mpv stopping for an embed doesn't take the
    /// embed's card down after it went up.
    Clear { players_only: bool },
}

pub struct Discord {
    tx: Sender<Message>,
}

impl Discord {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Message>();
        std::thread::Builder::new().name("discord-presence".into()).spawn(move || run(rx)).expect("spawn discord thread");
        Self { tx }
    }

    pub fn show(&self, presence: Presence) {
        let _ = self.tx.send(Message::Show(presence));
    }

    pub fn clear(&self) {
        let _ = self.tx.send(Message::Clear { players_only: false });
    }

    /// Clears the card unless an embedded player put it up.
    pub fn clear_player(&self) {
        let _ = self.tx.send(Message::Clear { players_only: true });
    }
}

fn run(rx: Receiver<Message>) {
    let mut worker = Worker {
        client: DiscordIpcClient::new(APPLICATION_ID),
        connected: false,
        wanted: None,
        published: None,
        next_attempt: Instant::now(),
    };
    loop {
        let wait = worker.due().map_or(REFRESH_INTERVAL, |at| at.saturating_duration_since(Instant::now()));
        match rx.recv_timeout(wait) {
            Ok(Message::Show(presence)) => worker.wanted = Some((presence, Instant::now())),
            Ok(Message::Clear { players_only }) => {
                if !(players_only && worker.wanted.as_ref().is_some_and(|(p, _)| p.embedded)) {
                    worker.clear();
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if worker.due().is_some_and(|at| at <= Instant::now()) {
            worker.publish();
        }
    }
}

/// The card on Discord, and where its clock stood when it was sent.
struct Published {
    identity: String,
    position: f64,
    at: Instant,
}

struct Worker {
    client: DiscordIpcClient,
    connected: bool,
    /// The latest report, and when it arrived (its position is as of then).
    wanted: Option<(Presence, Instant)>,
    published: Option<Published>,
    /// No write before this: the rate-limit window after a send, or the wait after a refused
    /// connection.
    next_attempt: Instant,
}

impl Worker {
    /// When the wanted card should go out, or `None` when Discord already shows it.
    fn due(&self) -> Option<Instant> {
        let (presence, reported) = self.wanted.as_ref()?;
        let stale = match &self.published {
            None => true,
            Some(published) => {
                let drifted = !presence.paused && {
                    let projected = published.position + reported.saturating_duration_since(published.at).as_secs_f64();
                    (presence.position - projected).abs() > SEEK_TOLERANCE
                };
                published.identity != presence.identity() || drifted || published.at.elapsed() >= REFRESH_INTERVAL
            }
        };
        stale.then_some(self.next_attempt)
    }

    fn publish(&mut self) {
        let Some((presence, reported)) = self.wanted.clone() else { return };
        let now = Instant::now();
        // The report may have waited out the rate limit; bring its clock up to now.
        let position = if presence.paused { presence.position } else { presence.position + now.duration_since(reported).as_secs_f64() };
        let card = Presence { position, ..presence };
        // A second try, because the usual failure is a pipe that died between updates (Discord
        // restarted), and a fresh connection is what fixes it.
        for _ in 0..2 {
            if !self.connected {
                if self.client.connect().is_err() {
                    self.next_attempt = now + RECONNECT_INTERVAL;
                    return;
                }
                self.connected = true;
                tracing::info!("connected to Discord");
            }
            match self.client.set_activity(activity(&card, unix_ms())) {
                Ok(()) => {
                    self.published = Some(Published { identity: card.identity(), position, at: now });
                    self.next_attempt = now + MIN_PUBLISH_INTERVAL;
                    return;
                }
                Err(e) => {
                    tracing::debug!(error = %e, "Discord presence failed; reconnecting");
                    self.connected = false;
                    let _ = self.client.close();
                }
            }
        }
        self.next_attempt = now + RECONNECT_INTERVAL;
    }

    fn clear(&mut self) {
        self.wanted = None;
        self.published = None;
        if self.connected && self.client.clear_activity().is_err() {
            self.connected = false;
            let _ = self.client.close();
        }
    }
}

fn unix_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default()
}

/// `12`, or `12.5` for a half episode.
pub fn episode_text(episode: f64) -> String {
    if episode.fract() == 0.0 { format!("{}", episode as i64) } else { format!("{episode:.1}") }
}

/// "Ep: 3/12", or "Ep: 3" when the count isn't known.
fn episode_label(p: &Presence) -> String {
    let episode = episode_text(p.episode);
    match p.total {
        Some(total) if total > 0 => format!("Ep: {episode}/{total}"),
        _ => format!("Ep: {episode}"),
    }
}

fn episode_title(p: &Presence) -> Option<&str> {
    p.episode_title.as_deref().map(str::trim).filter(|t| !t.is_empty())
}

/// A line Discord will show: trimmed, cut to its limit, and `None` under two characters.
fn text(value: &str) -> Option<String> {
    let value = value.trim();
    (value.chars().count() >= 2).then(|| value.chars().take(MAX_TEXT).collect())
}

fn activity(p: &Presence, now_ms: i64) -> Activity<'static> {
    // "Ep: 3/12 - Its Title", or the number alone; Discord ellipsises a long title on one line.
    let label = episode_label(p);
    let state = episode_title(p).map_or_else(|| label.clone(), |title| format!("{label} - {title}"));

    // The badge says who is playing this, and swaps to the pause glyph when stopped. Its caption
    // only when paused: naming the app would repeat the card's header.
    let mut assets = Assets::new().small_image(if p.paused { PAUSED_ASSET } else { APP_ICON_ASSET });
    if p.paused {
        assets = assets.small_text("Paused");
    }
    if let Some(cover) = p.cover.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
        assets = assets.large_image(cover.to_owned());
    }

    let mut activity = Activity::new()
        .activity_type(ActivityType::Watching)
        // The member list reads "Watching <series>" rather than "Watching Anilili".
        .status_display_type(StatusDisplayType::Details)
        .assets(assets)
        .buttons(vec![Button::new(WATCH_BUTTON_LABEL, WATCH_BUTTON_URL)]);
    if let Some(details) = text(&p.title) {
        activity = activity.details(details);
    }
    if let Some(state) = text(&state) {
        activity = activity.state(state);
    }
    // A paused card has no bar: Discord can't hold one still, and one that keeps moving lies.
    // Start is back-dated by the position, so the bar lands where the viewer is.
    if !p.paused && p.duration > 0.0 {
        let start = now_ms - (p.position.clamp(0.0, p.duration) * 1000.0) as i64;
        activity = activity.timestamps(Timestamps::new().start(start).end(start + (p.duration * 1000.0) as i64));
    } else if !p.paused {
        // No length to measure against (an embedded player's): Discord counts up instead.
        activity = activity.timestamps(Timestamps::new().start(now_ms - (p.position.max(0.0) * 1000.0) as i64));
    }
    activity
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn watching() -> Presence {
        Presence {
            anime_id: 21,
            title: "One Piece".into(),
            episode: 3.0,
            episode_title: None,
            total: Some(12),
            cover: Some("https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx21.jpg".into()),
            position: 60.0,
            duration: 1440.0,
            paused: false,
            embedded: false,
        }
    }

    fn card(p: &Presence) -> Value {
        serde_json::to_value(activity(p, 1_000_000)).unwrap()
    }

    #[test]
    fn the_number_is_the_second_line_without_a_title() {
        let c = card(&watching());
        assert_eq!(c["details"], "One Piece");
        assert_eq!(c["state"], "Ep: 3/12");
        assert_eq!(c["assets"].get("large_text"), None);
        assert_eq!(c["assets"]["small_image"], APP_ICON_ASSET);
        assert_eq!(c["assets"].get("small_text"), None);
        assert_eq!(c["buttons"], json!([{ "label": WATCH_BUTTON_LABEL, "url": WATCH_BUTTON_URL }]));
        assert_eq!(c["timestamps"], json!({ "start": 940_000, "end": 2_380_000 }));
    }

    #[test]
    fn an_episode_title_follows_the_number() {
        let c = card(&Presence { episode_title: Some("  Enter the Great Swordsman ".into()), ..watching() });
        assert_eq!(c["state"], "Ep: 3/12 - Enter the Great Swordsman");
        assert_eq!(c["assets"].get("large_text"), None);
        let c = card(&Presence { episode_title: Some("Romance Dawn".into()), episode: 10.5, total: None, ..watching() });
        assert_eq!(c["state"], "Ep: 10.5 - Romance Dawn");
    }

    #[test]
    fn a_paused_card_has_the_pause_badge_and_no_bar() {
        let c = card(&Presence { paused: true, ..watching() });
        assert_eq!(c["assets"]["small_image"], PAUSED_ASSET);
        assert_eq!(c["assets"]["small_text"], "Paused");
        assert_eq!(c.get("timestamps"), None);
    }

    #[test]
    fn an_embedded_player_counts_up_without_a_bar() {
        let c = card(&Presence { position: 30.0, duration: 0.0, embedded: true, ..watching() });
        assert_eq!(c["timestamps"], json!({ "start": 970_000 }));
        assert_eq!(c["state"], "Ep: 3/12");
    }

    #[test]
    fn an_embedded_player_that_reported_its_video_gets_a_bar() {
        let c = card(&Presence { position: 60.0, duration: 1440.0, embedded: true, ..watching() });
        assert_eq!(c["timestamps"], json!({ "start": 940_000, "end": 2_380_000 }));
    }

    #[test]
    fn short_and_long_lines() {
        let c = card(&Presence { title: "K".into(), episode_title: Some("x".repeat(200)), total: None, ..watching() });
        assert_eq!(c.get("details"), None);
        assert_eq!(c["state"].as_str().unwrap().chars().count(), MAX_TEXT);
        assert!(c["state"].as_str().unwrap().starts_with("Ep: 3 - xxx"));
    }

    #[test]
    fn the_clock_is_not_part_of_the_card() {
        let a = watching();
        assert_eq!(a.identity(), Presence { position: 900.0, ..watching() }.identity());
        assert_ne!(a.identity(), Presence { paused: true, ..watching() }.identity());
        assert_ne!(a.identity(), Presence { episode_title: Some("Romance Dawn".into()), ..watching() }.identity());
    }
}
