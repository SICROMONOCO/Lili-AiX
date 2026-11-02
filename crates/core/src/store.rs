//! SQLite: library, watch progress, settings, and a response cache.
//!
//! One connection behind a mutex; every call is short. WAL so a crash mid-write loses nothing
//! already committed.

use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::Result;
use crate::model::{AnimeCard, Audio, ContinueEntry, DownloadOptions, DownloadRecord, LibraryEntry, Notice, Progress};

const MIGRATIONS: &[&str] = &[
    // 1: initial schema
    "CREATE TABLE cache (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        stored_at INTEGER NOT NULL
     );
     CREATE TABLE anime (
        id INTEGER PRIMARY KEY,
        card TEXT NOT NULL,
        updated_at INTEGER NOT NULL
     );
     CREATE TABLE library (
        anime_id INTEGER PRIMARY KEY,
        status TEXT NOT NULL,
        updated_at INTEGER NOT NULL
     );
     CREATE TABLE progress (
        anime_id INTEGER NOT NULL,
        episode REAL NOT NULL,
        position REAL NOT NULL,
        duration REAL NOT NULL,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY (anime_id, episode)
     );
     CREATE INDEX progress_recent ON progress (updated_at DESC);
     CREATE TABLE settings (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL
     );",
    // 2: downloads
    "CREATE TABLE downloads (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        anime_id INTEGER NOT NULL,
        episode REAL NOT NULL,
        audio TEXT NOT NULL,
        title TEXT NOT NULL,
        status TEXT NOT NULL,
        progress REAL NOT NULL DEFAULT 0,
        bytes INTEGER NOT NULL DEFAULT 0,
        path TEXT,
        error TEXT,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL,
        UNIQUE (anime_id, episode, audio)
     );",
    // 3: the library mirrors the account (AniList or MAL). `synced`: the account has this entry as
    // it is here. `library_removed`: titles taken off here whose removal hasn't reached the account
    // yet, so the next pull doesn't put them back.
    "ALTER TABLE library ADD COLUMN synced INTEGER NOT NULL DEFAULT 0;
     CREATE TABLE library_removed (
        anime_id INTEGER PRIMARY KEY,
        removed_at INTEGER NOT NULL
     );",
    // 4: the download dialog's choices per episode, and what each download delivered; the app's
    // own notification log, and the ledger that lets each announcement happen once (Android's
    // EpisodeAlertLedger and its delivered AniList ids).
    "ALTER TABLE downloads ADD COLUMN provider TEXT;
     ALTER TABLE downloads ADD COLUMN max_height INTEGER;
     ALTER TABLE downloads ADD COLUMN match_quality INTEGER NOT NULL DEFAULT 0;
     ALTER TABLE downloads ADD COLUMN height INTEGER;
     ALTER TABLE downloads ADD COLUMN server TEXT;
     CREATE TABLE notices (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        kind TEXT NOT NULL,
        title TEXT NOT NULL,
        body TEXT,
        route TEXT,
        image TEXT,
        anime_id INTEGER,
        created_at INTEGER NOT NULL,
        read INTEGER NOT NULL DEFAULT 0
     );
     CREATE INDEX notices_recent ON notices (created_at DESC);
     CREATE TABLE announced (
        key TEXT PRIMARY KEY,
        at INTEGER NOT NULL
     );",
    // 5: the download dialog's "Hard subs only", kept per episode so a retry asks the same.
    "ALTER TABLE downloads ADD COLUMN hard_subs INTEGER NOT NULL DEFAULT 0;",
];

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or_default()
}

pub struct Store {
    db: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| crate::Error::Storage(e.to_string()))?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(db: Connection) -> Result<Self> {
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (index, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            db.execute_batch(sql)?;
            db.pragma_update(None, "user_version", (index + 1) as i64)?;
        }
        Ok(Self { db: Mutex::new(db) })
    }

    // ---- cache -------------------------------------------------------------------------------

    /// A cached value younger than `max_age_secs`, if any.
    pub fn cache_get<T: DeserializeOwned>(&self, key: &str, max_age_secs: i64) -> Option<T> {
        let db = self.db.lock().unwrap();
        let row: Option<(String, i64)> = db
            .query_row("SELECT value, stored_at FROM cache WHERE key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?)))
            .optional()
            .ok()
            .flatten();
        let (value, stored_at) = row?;
        if now() - stored_at > max_age_secs {
            return None;
        }
        serde_json::from_str(&value).ok()
    }

    /// The cached value regardless of age: the fallback when the network is down.
    pub fn cache_get_stale<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.cache_get(key, i64::MAX / 2)
    }

    pub fn cache_put<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let text = serde_json::to_string(value)?;
        self.db.lock().unwrap().execute(
            "INSERT INTO cache (key, value, stored_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, stored_at = excluded.stored_at",
            params![key, text, now()],
        )?;
        Ok(())
    }

    pub fn cache_delete(&self, key: &str) -> Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM cache WHERE key = ?1", [key])?;
        Ok(())
    }

    pub fn cache_clear(&self) -> Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM cache", [])?;
        Ok(())
    }

    // ---- anime cards -------------------------------------------------------------------------

    /// Remembers a title's card so the library and history render offline.
    pub fn remember(&self, card: &AnimeCard) -> Result<()> {
        let text = serde_json::to_string(card)?;
        self.db.lock().unwrap().execute(
            "INSERT INTO anime (id, card, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET card = excluded.card, updated_at = excluded.updated_at",
            params![card.id, text, now()],
        )?;
        Ok(())
    }

    pub fn card(&self, id: i64) -> Option<AnimeCard> {
        let db = self.db.lock().unwrap();
        let text: Option<String> = db.query_row("SELECT card FROM anime WHERE id = ?1", [id], |r| r.get(0)).optional().ok().flatten();
        text.and_then(|t| serde_json::from_str(&t).ok())
    }

    // ---- library -----------------------------------------------------------------------------

    /// A change made here: the account hasn't seen it yet (`synced` cleared).
    pub fn library_set(&self, card: &AnimeCard, status: Option<&str>) -> Result<()> {
        self.remember(card)?;
        let db = self.db.lock().unwrap();
        match status {
            Some(status) => {
                db.execute(
                    "INSERT INTO library (anime_id, status, updated_at, synced) VALUES (?1, ?2, ?3, 0)
                     ON CONFLICT(anime_id) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at, synced = 0",
                    params![card.id, status, now()],
                )?;
                db.execute("DELETE FROM library_removed WHERE anime_id = ?1", [card.id])?;
            }
            None => {
                db.execute("DELETE FROM library WHERE anime_id = ?1", [card.id])?;
            }
        };
        Ok(())
    }

    /// The account's entry, taken as this computer's: in step from here on.
    pub fn library_apply_remote(&self, card: &AnimeCard, status: &str, updated_at: i64) -> Result<()> {
        self.remember(card)?;
        self.db.lock().unwrap().execute(
            "INSERT INTO library (anime_id, status, updated_at, synced) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(anime_id) DO UPDATE SET status = excluded.status, updated_at = excluded.updated_at, synced = 1",
            params![card.id, status, updated_at],
        )?;
        Ok(())
    }

    pub fn library_mark_synced(&self, anime_id: i64) -> Result<()> {
        self.db.lock().unwrap().execute("UPDATE library SET synced = 1 WHERE anime_id = ?1", [anime_id])?;
        Ok(())
    }

    /// Taken off the account elsewhere: gone here too, with nothing left to push.
    pub fn library_drop(&self, anime_id: i64) -> Result<()> {
        let db = self.db.lock().unwrap();
        db.execute("DELETE FROM library WHERE anime_id = ?1", [anime_id])?;
        db.execute("DELETE FROM library_removed WHERE anime_id = ?1", [anime_id])?;
        Ok(())
    }

    /// Remembers a removal the account still has to hear about.
    pub fn library_tombstone(&self, anime_id: i64) -> Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT INTO library_removed (anime_id, removed_at) VALUES (?1, ?2) ON CONFLICT(anime_id) DO NOTHING",
            params![anime_id, now()],
        )?;
        Ok(())
    }

    pub fn library_tombstones(&self) -> Result<Vec<i64>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT anime_id FROM library_removed")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn library_clear_tombstone(&self, anime_id: i64) -> Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM library_removed WHERE anime_id = ?1", [anime_id])?;
        Ok(())
    }

    pub fn library_status(&self, anime_id: i64) -> Option<String> {
        self.db
            .lock()
            .unwrap()
            .query_row("SELECT status FROM library WHERE anime_id = ?1", [anime_id], |r| r.get(0))
            .optional()
            .ok()
            .flatten()
    }

    pub fn library(&self) -> Result<Vec<LibraryEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT a.card, l.status, l.updated_at, l.synced FROM library l JOIN anime a ON a.id = l.anime_id
             ORDER BY l.updated_at DESC",
        )?;
        let rows =
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, bool>(3)?)))?;
        Ok(rows
            .filter_map(|row| row.ok())
            .filter_map(|(card, status, updated_at, synced)| {
                Some(LibraryEntry { anime: serde_json::from_str(&card).ok()?, status, updated_at, synced })
            })
            .collect())
    }

    // ---- progress ----------------------------------------------------------------------------

    pub fn save_progress(&self, progress: &Progress) -> Result<()> {
        self.db.lock().unwrap().execute(
            "INSERT INTO progress (anime_id, episode, position, duration, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(anime_id, episode) DO UPDATE SET position = excluded.position,
                duration = excluded.duration, updated_at = excluded.updated_at",
            params![progress.anime_id, progress.episode, progress.position, progress.duration, progress.updated_at],
        )?;
        Ok(())
    }

    /// Every episode's progress for one title, by episode number.
    pub fn progress_for(&self, anime_id: i64) -> Result<Vec<Progress>> {
        let db = self.db.lock().unwrap();
        let mut stmt =
            db.prepare("SELECT anime_id, episode, position, duration, updated_at FROM progress WHERE anime_id = ?1 ORDER BY episode")?;
        let rows = stmt.query_map([anime_id], row_progress)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Every progress row, for a backup.
    pub fn all_progress(&self) -> Result<Vec<Progress>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT anime_id, episode, position, duration, updated_at FROM progress ORDER BY anime_id, episode")?;
        let rows = stmt.query_map([], row_progress)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// The most recent episode of each title touched lately, newest first.
    /// Every title with any saved progress.
    pub fn progress_titles(&self) -> Result<Vec<i64>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare("SELECT DISTINCT anime_id FROM progress")?;
        let ids = stmt.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
        Ok(ids)
    }

    pub fn continue_watching(&self, limit: usize) -> Result<Vec<ContinueEntry>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT p.anime_id, p.episode, p.position, p.duration, p.updated_at, a.card
             FROM progress p JOIN anime a ON a.id = p.anime_id
             WHERE p.updated_at = (SELECT MAX(updated_at) FROM progress q WHERE q.anime_id = p.anime_id)
             ORDER BY p.updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| Ok((row_progress(r)?, r.get::<_, String>(5)?)))?;
        Ok(rows
            .filter_map(|r| r.ok())
            .filter_map(|(progress, card)| Some(ContinueEntry { anime: serde_json::from_str(&card).ok()?, progress }))
            .collect())
    }

    pub fn clear_history(&self, anime_id: Option<i64>) -> Result<()> {
        let db = self.db.lock().unwrap();
        match anime_id {
            Some(id) => db.execute("DELETE FROM progress WHERE anime_id = ?1", [id])?,
            None => db.execute("DELETE FROM progress", [])?,
        };
        Ok(())
    }

    // ---- downloads ---------------------------------------------------------------------------

    /// Queues a download; `None` when that episode (and audio) is already queued or done. A
    /// failed or cancelled one is queued again, with the new choices.
    pub fn download_insert(&self, anime_id: i64, episode: f64, audio: Audio, title: &str, options: &DownloadOptions) -> Result<Option<i64>> {
        let db = self.db.lock().unwrap();
        let audio = audio_text(audio);
        let existing: Option<(i64, String)> = db
            .query_row(
                "SELECT id, status FROM downloads WHERE anime_id = ?1 AND episode = ?2 AND audio = ?3",
                params![anime_id, episode, audio],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let now = now();
        let provider = options.provider.as_deref().filter(|p| !p.is_empty());
        match existing {
            Some((_, status)) if status == "done" || status == "queued" || status == "downloading" => Ok(None),
            Some((id, _)) => {
                db.execute(
                    "UPDATE downloads SET status = 'queued', progress = 0, bytes = 0, error = NULL, provider = ?3,
                     max_height = ?4, match_quality = ?5, hard_subs = ?6, created_at = ?2, updated_at = ?2 WHERE id = ?1",
                    params![id, now, provider, options.max_height, options.match_quality, options.hard_subs],
                )?;
                Ok(Some(id))
            }
            None => {
                db.execute(
                    "INSERT INTO downloads (anime_id, episode, audio, title, status, created_at, updated_at, provider, max_height, match_quality, hard_subs)
                     VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?5, ?6, ?7, ?8, ?9)",
                    params![anime_id, episode, audio, title, now, provider, options.max_height, options.match_quality, options.hard_subs],
                )?;
                Ok(Some(db.last_insert_rowid()))
            }
        }
    }

    /// What a finished download turned out to be.
    pub fn download_delivered(&self, id: i64, height: Option<u32>, server: &str) -> Result<()> {
        self.db.lock().unwrap().execute("UPDATE downloads SET height = ?2, server = ?3 WHERE id = ?1", params![id, height, server])?;
        Ok(())
    }

    /// Every queued download marked cancelled (the one running is the caller's to stop).
    pub fn downloads_cancel_queued(&self) -> Result<usize> {
        Ok(self.db.lock().unwrap().execute("UPDATE downloads SET status = 'cancelled', updated_at = ?1 WHERE status = 'queued'", [now()])?)
    }

    /// Failed and cancelled downloads back in the queue.
    pub fn downloads_retry_failed(&self) -> Result<usize> {
        Ok(self.db.lock().unwrap().execute(
            "UPDATE downloads SET status = 'queued', progress = 0, bytes = 0, error = NULL, updated_at = ?1
             WHERE status IN ('failed', 'cancelled')",
            [now()],
        )?)
    }

    /// Forgets failed and cancelled downloads (their files were never finished).
    pub fn downloads_clear_unfinished(&self) -> Result<usize> {
        Ok(self.db.lock().unwrap().execute("DELETE FROM downloads WHERE status IN ('failed', 'cancelled')", [])?)
    }

    /// How many downloads wait or run.
    pub fn downloads_pending(&self) -> Result<i64> {
        Ok(self.db.lock().unwrap().query_row("SELECT COUNT(*) FROM downloads WHERE status IN ('queued', 'downloading')", [], |r| r.get(0))?)
    }

    pub fn download_update(&self, id: i64, status: &str, progress: f64, bytes: u64, path: Option<&str>, error: Option<&str>) -> Result<()> {
        self.db.lock().unwrap().execute(
            "UPDATE downloads SET status = ?2, progress = ?3, bytes = ?4, path = COALESCE(?5, path), error = ?6,
             updated_at = ?7 WHERE id = ?1",
            params![id, status, progress, bytes as i64, path, error, now()],
        )?;
        Ok(())
    }

    pub fn downloads(&self) -> Result<Vec<DownloadRecord>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(&format!("{DOWNLOAD_SELECT} ORDER BY d.created_at DESC, d.episode ASC"))?;
        let rows = stmt.query_map([], row_download)?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn download(&self, id: i64) -> Result<Option<DownloadRecord>> {
        let db = self.db.lock().unwrap();
        Ok(db.query_row(&format!("{DOWNLOAD_SELECT} WHERE d.id = ?1"), [id], row_download).optional()?)
    }

    /// The oldest queued download (episodes in order within a title).
    pub fn download_next_queued(&self) -> Result<Option<DownloadRecord>> {
        let db = self.db.lock().unwrap();
        Ok(db
            .query_row(
                &format!("{DOWNLOAD_SELECT} WHERE d.status = 'queued' ORDER BY d.created_at ASC, d.episode ASC LIMIT 1"),
                [],
                row_download,
            )
            .optional()?)
    }

    pub fn download_delete(&self, id: i64) -> Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM downloads WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Downloads that were running when the app closed go back in the queue.
    pub fn downloads_reset_interrupted(&self) -> Result<()> {
        self.db.lock().unwrap().execute("UPDATE downloads SET status = 'queued', progress = 0 WHERE status = 'downloading'", [])?;
        Ok(())
    }

    // ---- notices -----------------------------------------------------------------------------

    pub fn notice_add(
        &self,
        kind: &str,
        title: &str,
        body: Option<&str>,
        route: Option<&str>,
        image: Option<&str>,
        anime_id: Option<i64>,
    ) -> Result<i64> {
        let db = self.db.lock().unwrap();
        db.execute(
            "INSERT INTO notices (kind, title, body, route, image, anime_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![kind, title, body, route, image, anime_id, now()],
        )?;
        let id = db.last_insert_rowid();
        // The log is a recent history, not an archive.
        db.execute("DELETE FROM notices WHERE id NOT IN (SELECT id FROM notices ORDER BY created_at DESC, id DESC LIMIT 200)", [])?;
        Ok(id)
    }

    pub fn notices(&self, limit: usize) -> Result<Vec<Notice>> {
        let db = self.db.lock().unwrap();
        let mut stmt = db.prepare(
            "SELECT id, kind, title, body, route, image, anime_id, created_at, read FROM notices
             ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit as i64], |r| {
            Ok(Notice {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                body: r.get(3)?,
                route: r.get(4)?,
                image: r.get(5)?,
                anime_id: r.get(6)?,
                created_at: r.get(7)?,
                read: r.get::<_, i64>(8)? != 0,
            })
        })?;
        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    pub fn notices_unread(&self) -> Result<i64> {
        Ok(self.db.lock().unwrap().query_row("SELECT COUNT(*) FROM notices WHERE read = 0", [], |r| r.get(0))?)
    }

    pub fn notices_mark_read(&self) -> Result<()> {
        self.db.lock().unwrap().execute("UPDATE notices SET read = 1 WHERE read = 0", [])?;
        Ok(())
    }

    pub fn notices_clear(&self) -> Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM notices", [])?;
        Ok(())
    }

    /// True for the first caller with `key`, false for every later one, so an announcement made
    /// by two paths (a local alert and the AniList feed) happens once. Keys older than three
    /// weeks are forgotten.
    pub fn announce_claim(&self, key: &str) -> Result<bool> {
        let db = self.db.lock().unwrap();
        let now = now();
        db.execute("DELETE FROM announced WHERE at < ?1", [now - 21 * 86_400])?;
        Ok(db.execute("INSERT OR IGNORE INTO announced (key, at) VALUES (?1, ?2)", params![key, now])? == 1)
    }

    // ---- settings ----------------------------------------------------------------------------

    pub fn setting<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let db = self.db.lock().unwrap();
        let text: Option<String> = db.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional().ok().flatten();
        text.and_then(|t| serde_json::from_str(&t).ok())
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let text = serde_json::to_string(value)?;
        self.db.lock().unwrap().execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, text],
        )?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<()> {
        self.db.lock().unwrap().execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    }
}

const DOWNLOAD_SELECT: &str = "SELECT d.id, d.anime_id, d.episode, d.audio, d.title, d.status, d.progress, d.bytes,
    d.path, d.error, d.created_at, d.updated_at, a.card, d.provider, d.max_height, d.match_quality, d.height, d.server, d.hard_subs
    FROM downloads d LEFT JOIN anime a ON a.id = d.anime_id";

fn audio_text(audio: Audio) -> &'static str {
    match audio {
        Audio::Sub => "sub",
        Audio::Dub => "dub",
    }
}

fn row_download(r: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadRecord> {
    let card: Option<String> = r.get(12)?;
    let cover = card.and_then(|c| serde_json::from_str::<AnimeCard>(&c).ok()).and_then(|c| c.cover);
    Ok(DownloadRecord {
        id: r.get(0)?,
        anime_id: r.get(1)?,
        episode: r.get(2)?,
        audio: if r.get::<_, String>(3)? == "dub" { Audio::Dub } else { Audio::Sub },
        title: r.get(4)?,
        cover,
        status: r.get(5)?,
        progress: r.get(6)?,
        bytes: r.get::<_, i64>(7)? as u64,
        path: r.get(8)?,
        error: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
        provider: r.get(13)?,
        max_height: r.get(14)?,
        match_quality: r.get::<_, i64>(15)? != 0,
        height: r.get(16)?,
        server: r.get(17)?,
        hard_subs: r.get::<_, i64>(18)? != 0,
    })
}

fn row_progress(r: &rusqlite::Row<'_>) -> rusqlite::Result<Progress> {
    Ok(Progress { anime_id: r.get(0)?, episode: r.get(1)?, position: r.get(2)?, duration: r.get(3)?, updated_at: r.get(4)? })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: i64) -> AnimeCard {
        AnimeCard {
            id,
            title: format!("Anime {id}"),
            title_romaji: None,
            cover: None,
            color: None,
            banner: None,
            format: None,
            status: None,
            episodes: Some(12),
            score: None,
            year: None,
            season: None,
            genres: vec![],
            next_airing: None,
            is_adult: false,
        }
    }

    #[test]
    fn continue_watching_takes_latest_episode_per_title() {
        let store = Store::in_memory().unwrap();
        store.remember(&card(1)).unwrap();
        store.remember(&card(2)).unwrap();
        let p = |anime_id, episode, updated_at| Progress { anime_id, episode, position: 100.0, duration: 1400.0, updated_at };
        store.save_progress(&p(1, 1.0, 10)).unwrap();
        store.save_progress(&p(1, 2.0, 30)).unwrap();
        store.save_progress(&p(2, 5.0, 20)).unwrap();
        let rows = store.continue_watching(10).unwrap();
        let summary: Vec<_> = rows.iter().map(|r| (r.anime.id, r.progress.episode)).collect();
        assert_eq!(summary, [(1, 2.0), (2, 5.0)]);
    }

    #[test]
    fn library_tracks_what_the_account_has() {
        let store = Store::in_memory().unwrap();
        // A change made here is pending until the account takes it.
        store.library_set(&card(1), Some("watching")).unwrap();
        assert!(!store.library().unwrap()[0].synced);
        store.library_mark_synced(1).unwrap();
        assert!(store.library().unwrap()[0].synced);
        // The account's own entry comes in already in step, and a later local change unsyncs it.
        store.library_apply_remote(&card(2), "completed", 100).unwrap();
        let two = store.library().unwrap().into_iter().find(|e| e.anime.id == 2).unwrap();
        assert!(two.synced && two.status == "completed" && two.updated_at == 100);
        store.library_set(&card(2), Some("dropped")).unwrap();
        assert!(!store.library().unwrap().into_iter().find(|e| e.anime.id == 2).unwrap().synced);
        // A removal waits as a tombstone; saving the title again cancels it.
        store.library_set(&card(1), None).unwrap();
        store.library_tombstone(1).unwrap();
        assert_eq!(store.library_tombstones().unwrap(), [1]);
        store.library_set(&card(1), Some("planning")).unwrap();
        assert!(store.library_tombstones().unwrap().is_empty());
        // Removed elsewhere: dropped here, with nothing pending.
        store.library_drop(2).unwrap();
        assert!(store.library().unwrap().iter().all(|e| e.anime.id != 2));
    }

    #[test]
    fn library_and_settings_round_trip() {
        let store = Store::in_memory().unwrap();
        store.library_set(&card(7), Some("watching")).unwrap();
        assert_eq!(store.library_status(7).as_deref(), Some("watching"));
        assert_eq!(store.library().unwrap().len(), 1);
        store.library_set(&card(7), None).unwrap();
        assert!(store.library().unwrap().is_empty());
        store.set_setting("volume", &80).unwrap();
        assert_eq!(store.setting::<i32>("volume"), Some(80));
    }

    #[test]
    fn cache_respects_age() {
        let store = Store::in_memory().unwrap();
        store.cache_put("k", &vec![1, 2]).unwrap();
        assert_eq!(store.cache_get::<Vec<i32>>("k", 60), Some(vec![1, 2]));
        assert_eq!(store.cache_get::<Vec<i32>>("k", -1), None);
        assert_eq!(store.cache_get_stale::<Vec<i32>>("k"), Some(vec![1, 2]));
    }

    #[test]
    fn announcements_happen_once() {
        let store = Store::in_memory().unwrap();
        assert!(store.announce_claim("episode:1:5").unwrap());
        assert!(!store.announce_claim("episode:1:5").unwrap());
        assert!(store.announce_claim("episode:1:6").unwrap());
    }

    #[test]
    fn notices_are_newest_first_and_marked_read() {
        let store = Store::in_memory().unwrap();
        store.notice_add("download", "Downloaded 2 episodes", None, Some("/downloads"), None, None).unwrap();
        store.notice_add("airing", "Frieren", Some("Episode 5 is out"), Some("/anime/1"), None, Some(1)).unwrap();
        let all = store.notices(10).unwrap();
        assert_eq!(all[0].title, "Frieren");
        assert_eq!(store.notices_unread().unwrap(), 2);
        store.notices_mark_read().unwrap();
        assert_eq!(store.notices_unread().unwrap(), 0);
    }

    #[test]
    fn requeueing_a_failed_download_takes_the_new_choices() {
        let store = Store::in_memory().unwrap();
        let options = DownloadOptions { provider: Some("animepahe".into()), max_height: Some(720), match_quality: true, hard_subs: true };
        let id = store.download_insert(1, 1.0, Audio::Sub, "A", &DownloadOptions::default()).unwrap().unwrap();
        assert_eq!(store.download_insert(1, 1.0, Audio::Sub, "A", &options).unwrap(), None);
        store.download_update(id, "failed", 0.0, 0, None, Some("x")).unwrap();
        assert_eq!(store.download_insert(1, 1.0, Audio::Sub, "A", &options).unwrap(), Some(id));
        let row = store.download(id).unwrap().unwrap();
        assert_eq!((row.status.as_str(), row.max_height, row.provider.as_deref(), row.match_quality), ("queued", Some(720), Some("animepahe"), true));
        assert!(row.hard_subs);
        assert_eq!(store.downloads_pending().unwrap(), 1);
    }
}
