//! The error catalogue: every failure a viewer can see, with a stable code.
//!
//! The code is what a bug report quotes and what the diagnostics log records as the event name,
//! so it must never change meaning. The desktop app has its own namespace: every prefix starts
//! with `D` (`DNET-01`), so a code never collides with the Android app's (`NET-01`), whose numbers
//! mean different things.
//!
//! Rules:
//! - Numbers are assigned once and never reused, even when an entry is retired.
//! - `message` is what the viewer reads: no hosts, file names or library errors. Those go in the
//!   failure's `detail`, which the UI shows behind **Details**.
//! - A code may carry a second half from the layer below: `DNET-03/h503` (HTTP status),
//!   `DPLAY-02/m-13` (mpv error number).
//! - `docs/ERROR_CODES.md` is generated from [`ALL`] by this module's test. Don't edit it by hand.

use serde::Serialize;

/// What the viewer can do about a failure. The UI turns it into a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Next {
    /// Nothing useful: the message is the whole story.
    Nothing,
    Retry,
    /// Pick another server or audio.
    OtherServer,
    SignIn,
    /// Something only a diagnostics bundle can explain.
    Diagnostics,
    Reinstall,
}

impl Next {
    pub fn label(self) -> &'static str {
        match self {
            Next::Nothing => "—",
            Next::Retry => "Try again",
            Next::OtherServer => "Try another server",
            Next::SignIn => "Sign in",
            Next::Diagnostics => "Save diagnostics",
            Next::Reinstall => "Reinstall",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Net,
    Api,
    Src,
    Play,
    Acc,
    Dl,
    Upd,
    Db,
    Bak,
    Link,
    App,
}

impl Domain {
    pub fn prefix(self) -> &'static str {
        match self {
            Domain::Net => "DNET",
            Domain::Api => "DAPI",
            Domain::Src => "DSRC",
            Domain::Play => "DPLAY",
            Domain::Acc => "DACC",
            Domain::Dl => "DDL",
            Domain::Upd => "DUPD",
            Domain::Db => "DDB",
            Domain::Bak => "DBAK",
            Domain::Link => "DLINK",
            Domain::App => "DAPP",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Domain::Net => "Network",
            Domain::Api => "Server responses",
            Domain::Src => "Sources",
            Domain::Play => "Playback",
            Domain::Acc => "Accounts and sync",
            Domain::Dl => "Downloads",
            Domain::Upd => "Updates",
            Domain::Db => "Storage",
            Domain::Bak => "Backup and restore",
            Domain::Link => "Links",
            Domain::App => "App",
        }
    }
}

/// One catalogue entry.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub domain: Domain,
    pub number: u8,
    pub message: &'static str,
    pub next: Next,
}

impl Entry {
    pub fn code(&self) -> String {
        format!("{}-{:02}", self.domain.prefix(), self.number)
    }
}

macro_rules! catalogue {
    ($($name:ident = $domain:ident $number:literal, $next:ident, $message:literal;)*) => {
        $(pub const $name: &Entry = &Entry { domain: Domain::$domain, number: $number, message: $message, next: Next::$next };)*
        /// Every entry, in catalogue order.
        pub const ALL: &[&Entry] = &[$($name),*];
    };
}

catalogue! {
    NET_OFFLINE = Net 1, Retry, "No connection.";
    NET_TIMEOUT = Net 2, Retry, "The server took too long to answer.";
    NET_SERVER = Net 3, Retry, "A server answered with an error.";
    NET_RATE_LIMITED = Net 4, Retry, "A server is limiting requests. Wait a minute, then try again.";
    NET_CHALLENGE = Net 5, Retry, "A server's browser check didn't clear.";
    NET_CERTIFICATE = Net 6, Diagnostics, "This computer couldn't verify a server's certificate.";
    NET_HOME = Net 7, Retry, "Couldn't load the home page.";
    NET_SEARCH = Net 8, Retry, "Search didn't come back.";
    NET_TITLE = Net 9, Retry, "Couldn't load this title.";
    NET_SCHEDULE = Net 10, Retry, "Couldn't load the schedule.";
    NET_LIBRARY = Net 11, Retry, "Couldn't load your list.";
    NET_DICE = Net 12, Retry, "Couldn't roll a title. Try again.";
    NET_RECS = Net 13, Retry, "Couldn't load your recommendations.";
    NET_NOTIFICATIONS = Net 14, Retry, "Couldn't load your notifications.";

    API_UNREADABLE = Api 1, Diagnostics, "A server sent something the app couldn't read.";
    API_NOT_FOUND = Api 2, Nothing, "That isn't there any more.";

    SRC_NONE_FOR_TITLE = Src 1, Nothing, "No streams are known for this title yet.";
    SRC_NONE_FOR_EPISODE = Src 2, OtherServer, "No playable server for this episode. Try the other audio, or later.";
    SRC_EMPTY_SERVER = Src 3, OtherServer, "This server has no stream.";
    SRC_PROVIDER_FAILED = Src 4, Nothing, "One of the extra providers failed.";
    SRC_NONE_DOWNLOADABLE = Src 5, Nothing, "This episode can't be downloaded: its servers are embedded players only.";
    SRC_SUBTITLE_SEARCH = Src 6, Retry, "Couldn't search OpenSubtitles.";
    SRC_SUBTITLE_ALLOWANCE = Src 7, Nothing, "You've used today's OpenSubtitles downloads. They reset within a day.";
    SRC_SUBTITLE_UNAVAILABLE = Src 8, Nothing, "OpenSubtitles isn't available in this build of the app.";
    SRC_NOT_AIRED = Src 9, Nothing, "This episode hasn't aired yet. Its card on the title page counts down to it.";

    PLAY_NO_LIBMPV = Play 1, Reinstall, "The video player is missing from the app. Reinstall Anilili.";
    PLAY_STREAM_FAILED = Play 2, OtherServer, "Playback stopped: the stream failed.";
    PLAY_FILE_FAILED = Play 3, Nothing, "Couldn't open this file.";
    PLAY_ALL_FAILED = Play 4, Retry, "Every server failed for this episode.";
    PLAY_NO_TRACK = Play 5, Nothing, "That track isn't available.";
    PLAY_WINDOW = Play 6, Diagnostics, "The player couldn't attach to the window.";
    PLAY_UNAVAILABLE = Play 7, Diagnostics, "The video player couldn't start on this computer.";

    ACC_SIGNED_OUT = Acc 1, SignIn, "You're not signed in.";
    ACC_ANILIST_LOGIN = Acc 2, Retry, "AniList sign-in didn't finish.";
    ACC_MAL_LOGIN = Acc 3, Retry, "MyAnimeList sign-in didn't finish.";
    ACC_SYNC = Acc 4, Retry, "Couldn't sync your progress.";
    ACC_OPENSUBTITLES_LOGIN = Acc 5, SignIn, "OpenSubtitles didn't accept that username and password.";
    ACC_SIMKL_LOGIN = Acc 6, Retry, "Simkl sign-in didn't finish.";
    ACC_SIMKL_SYNC = Acc 7, Retry, "Couldn't update Simkl.";

    DL_NO_FILE = Dl 1, Nothing, "This download has no file yet.";
    DL_MISSING = Dl 2, Nothing, "That download no longer exists.";
    DL_UNFINISHED = Dl 3, Nothing, "This download hasn't finished.";
    DL_FILE_GONE = Dl 4, Nothing, "The file is gone (moved or deleted).";
    DL_FAILED = Dl 5, Retry, "A download failed.";
    DL_NO_SPACE = Dl 6, Nothing, "Not enough free space for this download. Free some space, or choose a lower quality.";
    DL_FOLDER = Dl 7, Nothing, "Couldn't use the download folder.";

    UPD_CHECK = Upd 1, Retry, "Couldn't check for updates.";
    UPD_SIGNATURE = Upd 2, Diagnostics, "An update didn't pass its signature check, so it was ignored.";
    UPD_INSTALL = Upd 3, Retry, "Couldn't install the update.";
    UPD_PROVIDERS = Upd 4, Retry, "Couldn't update the providers.";

    DB_FAILED = Db 1, Diagnostics, "Couldn't read or write the app's data.";

    BAK_UNREADABLE = Bak 1, Nothing, "That file isn't an Anilili backup.";
    BAK_NEWER = Bak 2, Nothing, "That backup comes from a newer version of Anilili. Update, then try again.";
    BAK_WRITE = Bak 3, Retry, "Couldn't save the backup.";
    BAK_PASSPHRASE = Bak 4, Retry, "That passphrase doesn't open the logins in this backup.";

    LINK_UNKNOWN = Link 1, Nothing, "That Anilili link isn't one this app understands.";

    APP_GENERIC = App 1, Diagnostics, "Something went wrong.";
    APP_PAGE = App 2, Retry, "Something went wrong on this page.";
    APP_DIAGNOSTICS = App 3, Nothing, "Couldn't save the diagnostics file.";
}

/// The catalogue as Markdown, for `docs/ERROR_CODES.md`.
pub fn markdown() -> String {
    let mut out = String::from(
        "# Error codes\n\nGenerated from `crates/core/src/catalogue.rs` by its test. Do not edit by hand.\n\n\
         Every failure the app can show carries a code. Quote it in a bug report: it is also the event\n\
         name in the diagnostics file (Settings → Diagnostics), so the two can be matched.\n\n\
         Desktop codes start with `D` and are separate from the Android app's: `DNET-01` and `NET-01`\n\
         are unrelated.\n\n\
         A code may carry a second half from the layer below, which is more precise than anything the\n\
         app invents:\n\n\
         | Prefix | Source |\n| --- | --- |\n| `h` | HTTP response status |\n| `m` | mpv error number |\n",
    );
    let mut domain = None;
    for entry in ALL {
        if domain != Some(entry.domain) {
            domain = Some(entry.domain);
            out.push_str(&format!(
                "\n## {} — `{}`\n\n| Code | What it means | Next step |\n| --- | --- | --- |\n",
                entry.domain.title(),
                entry.domain.prefix()
            ));
        }
        out.push_str(&format!("| `{}` | {} | {} |\n", entry.code(), entry.message, entry.next.label()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_unique_and_never_renumbered() {
        let mut seen = std::collections::HashSet::new();
        for entry in ALL {
            assert!(seen.insert(entry.code()), "duplicate code {}", entry.code());
            assert!(entry.number > 0);
        }
    }

    #[test]
    fn messages_carry_no_technical_detail() {
        for entry in ALL {
            for word in ["http", "://", ".dll", "error:", "mpv", "wreq"] {
                assert!(!entry.message.to_lowercase().contains(word), "{} mentions {word}", entry.code());
            }
        }
    }

    /// Regenerates docs/ERROR_CODES.md, like the Android app's ErrorCatalogTest.
    #[test]
    fn writes_the_catalogue_document() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/ERROR_CODES.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, markdown()).unwrap();
    }
}
