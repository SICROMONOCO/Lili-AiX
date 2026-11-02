# Error codes

Generated from `crates/core/src/catalogue.rs` by its test. Do not edit by hand.

Every failure the app can show carries a code. Quote it in a bug report: it is also the event
name in the diagnostics file (Settings → Diagnostics), so the two can be matched.

Desktop codes start with `D` and are separate from the Android app's: `DNET-01` and `NET-01`
are unrelated.

A code may carry a second half from the layer below, which is more precise than anything the
app invents:

| Prefix | Source |
| --- | --- |
| `h` | HTTP response status |
| `m` | mpv error number |

## Network — `DNET`

| Code | What it means | Next step |
| --- | --- | --- |
| `DNET-01` | No connection. | Try again |
| `DNET-02` | The server took too long to answer. | Try again |
| `DNET-03` | A server answered with an error. | Try again |
| `DNET-04` | A server is limiting requests. Wait a minute, then try again. | Try again |
| `DNET-05` | A server's browser check didn't clear. | Try again |
| `DNET-06` | This computer couldn't verify a server's certificate. | Save diagnostics |
| `DNET-07` | Couldn't load the home page. | Try again |
| `DNET-08` | Search didn't come back. | Try again |
| `DNET-09` | Couldn't load this title. | Try again |
| `DNET-10` | Couldn't load the schedule. | Try again |
| `DNET-11` | Couldn't load your list. | Try again |
| `DNET-12` | Couldn't roll a title. Try again. | Try again |
| `DNET-13` | Couldn't load your recommendations. | Try again |
| `DNET-14` | Couldn't load your notifications. | Try again |

## Server responses — `DAPI`

| Code | What it means | Next step |
| --- | --- | --- |
| `DAPI-01` | A server sent something the app couldn't read. | Save diagnostics |
| `DAPI-02` | That isn't there any more. | — |

## Sources — `DSRC`

| Code | What it means | Next step |
| --- | --- | --- |
| `DSRC-01` | No streams are known for this title yet. | — |
| `DSRC-02` | No playable server for this episode. Try the other audio, or later. | Try another server |
| `DSRC-03` | This server has no stream. | Try another server |
| `DSRC-04` | One of the extra providers failed. | — |
| `DSRC-05` | This episode can't be downloaded: its servers are embedded players only. | — |
| `DSRC-06` | Couldn't search OpenSubtitles. | Try again |
| `DSRC-07` | You've used today's OpenSubtitles downloads. They reset within a day. | — |
| `DSRC-08` | OpenSubtitles isn't available in this build of the app. | — |
| `DSRC-09` | This episode hasn't aired yet. Its card on the title page counts down to it. | — |

## Playback — `DPLAY`

| Code | What it means | Next step |
| --- | --- | --- |
| `DPLAY-01` | The video player is missing from the app. Reinstall Anilili. | Reinstall |
| `DPLAY-02` | Playback stopped: the stream failed. | Try another server |
| `DPLAY-03` | Couldn't open this file. | — |
| `DPLAY-04` | Every server failed for this episode. | Try again |
| `DPLAY-05` | That track isn't available. | — |
| `DPLAY-06` | The player couldn't attach to the window. | Save diagnostics |
| `DPLAY-07` | The video player couldn't start on this computer. | Save diagnostics |

## Accounts and sync — `DACC`

| Code | What it means | Next step |
| --- | --- | --- |
| `DACC-01` | You're not signed in. | Sign in |
| `DACC-02` | AniList sign-in didn't finish. | Try again |
| `DACC-03` | MyAnimeList sign-in didn't finish. | Try again |
| `DACC-04` | Couldn't sync your progress. | Try again |
| `DACC-05` | OpenSubtitles didn't accept that username and password. | Sign in |
| `DACC-06` | Simkl sign-in didn't finish. | Try again |
| `DACC-07` | Couldn't update Simkl. | Try again |

## Downloads — `DDL`

| Code | What it means | Next step |
| --- | --- | --- |
| `DDL-01` | This download has no file yet. | — |
| `DDL-02` | That download no longer exists. | — |
| `DDL-03` | This download hasn't finished. | — |
| `DDL-04` | The file is gone (moved or deleted). | — |
| `DDL-05` | A download failed. | Try again |
| `DDL-06` | Not enough free space for this download. Free some space, or choose a lower quality. | — |
| `DDL-07` | Couldn't use the download folder. | — |

## Updates — `DUPD`

| Code | What it means | Next step |
| --- | --- | --- |
| `DUPD-01` | Couldn't check for updates. | Try again |
| `DUPD-02` | An update didn't pass its signature check, so it was ignored. | Save diagnostics |
| `DUPD-03` | Couldn't install the update. | Try again |
| `DUPD-04` | Couldn't update the providers. | Try again |

## Storage — `DDB`

| Code | What it means | Next step |
| --- | --- | --- |
| `DDB-01` | Couldn't read or write the app's data. | Save diagnostics |

## Backup and restore — `DBAK`

| Code | What it means | Next step |
| --- | --- | --- |
| `DBAK-01` | That file isn't an Anilili backup. | — |
| `DBAK-02` | That backup comes from a newer version of Anilili. Update, then try again. | — |
| `DBAK-03` | Couldn't save the backup. | Try again |
| `DBAK-04` | That passphrase doesn't open the logins in this backup. | Try again |

## Links — `DLINK`

| Code | What it means | Next step |
| --- | --- | --- |
| `DLINK-01` | That Anilili link isn't one this app understands. | — |

## App — `DAPP`

| Code | What it means | Next step |
| --- | --- | --- |
| `DAPP-01` | Something went wrong. | Save diagnostics |
| `DAPP-02` | Something went wrong on this page. | Try again |
| `DAPP-03` | Couldn't save the diagnostics file. | — |
