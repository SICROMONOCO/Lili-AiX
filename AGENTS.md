# Working in this repository

Anilili Desktop: the Windows/macOS app. A **separate product** from the Android app in
`../miruro-apk`. Providers are written twice on purpose (Kotlin there, Rust here); the two share
*contracts* (backup JSON, pairing protocol, `anilili://` links, AniList/MAL sync, the signed update
channel's key), not code.

The design and the decisions behind it are in
`../miruro-apk/docs/investigations/desktop-tauri-rust-deep-dive-2026-10-04.md` (§12–14 are final).

## Rules

- **One private remote, nowhere else.** The code is shared between the developer's PC and Mac
  through one private GitHub repository, `origin` (set up 2026-10-05; its name says nothing about
  the app, and nothing in this repo names the account). Pushing there is fine; nothing else: no
  other remotes, no public repository, no pull requests or forks, no CI service, no uploading the
  source, a bundle or an archive anywhere else. The local `pre-push` hook refuses any other remote;
  never bypass it with `--no-verify`. `../miruro-apk` stays local only.
- **Stage explicit paths.** Never `git add -A`; check `git status` before every commit.
- **Commit as `sanilili <338052299+sanilili@users.noreply.github.com>`, and only that** (since
  2026-10-05; earlier commits are `kompoti`). The rule across every repo is that each commits as
  the account that owns it: this one is `sanilili`, `../miruro-apk` is `kompoti`, the metadata
  dataset is `pommieyewear`, so the identity that is right next door is wrong here. No other name
  or address, ever. Check
  `git config --show-origin user.name` and `user.email` before the first commit; each clone sets
  the identity in its own `.git/config` (a fresh clone has none, and neither the identity nor the
  `pre-push` hook travels with it). Never let git fall back to a machine default: on 2026-10-04
  that happened here, and ten commits had to be rewritten and purged.
- **One concern per commit**, and a message that accounts for the whole diff and says *why*.
- **No GPL code.** Seanime and L-MPV (GPL-3.0) are reference reading only. Nothing from them,
  not even type definitions, may be copied here. tauri-plugin-libmpv (MPL-2.0) may be read.
- **libmpv is LGPL and loaded dynamically** (`crates/mpv`, via `libloading`), so users can
  replace it. Never link it statically, and never ship a GPL build.
- **Tauri 3 is alpha.** Every `tauri*` crate and `@tauri-apps/*` package is pinned to an exact
  version. Move them together, deliberately.
- **A plain (not `async`) command runs on the main thread**, the one driving the window and the
  webview, so a slow one freezes the UI. Mark any command that touches SQLite, the disk or a
  large payload `#[tauri::command(async)]` (the worker pool). Keep it plain only when the main
  thread's ordering matters: a read-modify-write of one setting (`continue_hide`,
  `reminder_toggle`, `dice_feedback`, `downloads_pause`, `settings_set`; the store's lock covers
  one call, not the pair), or work on the window itself (`glass`, the player).
- **Secrets never live here.** The Nostr update key, signing keys and the runbook are in
  `../anilili-private/`. `scripts/publish-update.mjs` reads the key from a path you give it.
- **Publishing is a human step.** Never run `publish-update.mjs` or upload to the mirror unless
  the user asks for that specific release.

## Layout

| Path | What |
| --- | --- |
| `crates/mpv` | Our libmpv binding: dynamic load, commands/properties, typed event thread. No Tauri. |
| `crates/core` | The engine, no Tauri. See the table below. |
| `wit/provider.wit` | The WebAssembly provider interface: AniList-keyed title in, servers out. Host imports: `http`, `browser-fetch` (the same request made inside a webview, for challenge-gated sites), `log`, and a small private `cache-get`/`cache-set`. |
| `providers/` | Providers compiled to `wasm32-wasip2` components; a separate Cargo workspace. Bundled (2026-10-04, ports of the Android providers): `megaplay` (AniKoto's player, direct), `anipm` (ani.pm), `uniquestream` (Crunchyroll mirror, sub+dub), `anizone`, `animepahe` (id `pahe-direct`, through `browser-fetch`), `zoko`, `animeonsen` (DASH, nine subtitle languages), `animeheaven` (hard-subbed MP4), and since 2026-10-06 `rift` (AnimeRift, the Anime Rift app's private API: Arabic sub burned in, Arabic dub on some titles; last in `PRIORITY`, as the desktop has no per-provider language yet). **Rift registers a fake device per session (kept 30 days) and rate-limits each endpoint tightly**, so it is in `ONE_AT_A_TIME` (sub, then dub), and testing it in a loop gets the IP refused (`ACCESS_DENIED` on everything; it happened on 2026-10-06 after about 16 registrations in five minutes): test it sparingly, with the app's own cache. `kit` is not a provider: the shared helpers (requests, cache, HTML, title and season matching) whose bindings every provider reuses through wit-bindgen's `with`. `anichan` is dormant (its API returns no servers); AnimeKai is not ported (every mirror was down, Cloudflare 522, on 2026-10-04). **A provider id must not equal a Miruro provider key** (`anikoto`, `animepahe`, `kickassanime`, `animegg`, `allmanga`, `anidbapp`, `animedao`, `aniwaves`, `icarus`), or the preferred-provider choice and server ids collide; a site Miruro already serves is only worth porting for when Miruro is down. |
| `src-tauri/src/main.rs` | Window (built in code: popup and navigation guards; the title bar painted the sidebar's colour through DWM), plugins, command list, dev hooks. `with_browser_args` is the one browser-argument string every webview must share (WebView2's smooth scrolling is on it): webviews share a data directory, and WebView2 refuses an environment whose arguments differ from the running browser process's. |
| `src-tauri/src/app.rs` | `AppState` (core services, settings, short-lived caches), `Settings`, errors for the UI. |
| `src-tauri/src/player.rs` | mpv under the webview; progress saving (only once the new file has loaded: the old file's end arrives after `load` names the next episode); `time-pos` paced for the UI (`Clock`), its held position flushed when the playhead stops or jumps, so the UI's clock is never left where playback was; Android's Screen size (`keepaspect`/`panscan`); AniList sync at 90%; Anime4K; snapshot replay. Android's Buffer size and Video cache map to `cache-secs`/`demuxer-max-bytes`/`cache-pause-wait` and `cache-on-disk`/`demuxer-max-back-bytes` (`apply_buffer`); Automatic is the shipped tuning. |
| `src-tauri/src/player_mac.rs` | macOS video: a `CAMetalLayer` view placed under the WKWebView, handed to mpv as `wid` (`gpu-next` over MoltenVK); keeps the drawable at the view's pixel size and reports the screen's EDR headroom; shown only while a file is open. |
| `src-tauri/src/watch.rs` | Episode → server choice (audio, preferred provider, soft subs in language) → playback. |
| `src-tauri/src/catalog.rs` | Home/search/detail/schedule/episodes/sources, cached in SQLite with a stale fallback. Sources race Miruro against the providers: once Miruro answers, providers get 4 s (25 s if Miruro failed); late ones keep running and merge into the sources cache for the next request. Browse takes Android's `DiscoverFilters` (tags, exclusions, decades, country, source, length, score); "Dubbed only" and "Hide watched" travel to AniList as `id_in`/`id_not_in` lists worked out here (the dub index; the library and local progress). |
| `src-tauri/src/library.rs` | Library, history, settings, AniList and MAL login (login windows catch the redirects). **The library mirrors the account** (Android's philosophy): `library_set` pushes at once (removal deletes the account entry), and `reconcile` runs on every account-list read and in the background (`start_sync`, at launch and every 10 min): the account wins, except a newer change here it hasn't seen; entries carry a `synced` flag and removals wait in `library_removed` until the account takes them. Drop the cached account lists after any push, or a pull undoes it. |
| `src-tauri/src/downloads.rs` | Persisted download queue worker (two episodes at a time, `PARALLEL_JOBS`; the disk check reserves space for each running one), taskbar progress (their average), offline playback. Each job carries the download dialog's choices (Android's `SeasonDownloadDialog`: first-choice server, a height ceiling, switch servers per episode); `download_check` says what a server really offers for a sample episode before anything is queued. The queue pauses (persisted), pauses itself when the disk is nearly full (Android's size estimates and 1 GB headroom), and when it stops posts one notification for the batch it ran. |
| `src-tauri/src/notifications.rs` | Android's notification system: the AniList feed polled every 15 min (unread items pushed once, eight a poll at most), local "is out" alerts for library titles the feed doesn't cover (refreshed every 3 h), the schedule's "airs soon" reminders, one alert per episode through the store's ledger. The app's own log (`notices`) sits beside the feed on the Notifications page; the bell counts both. **`notifications-changed` makes the page refetch; a fetch only ever emits `notifications-unread`**, or it refetches for ever (it did, and AniList answered 429). |
| `src-tauri/src/notify.rs` | Windows toasts (WinRT): a tag replaces, a group clears, a click opens the route (`open-route`). Posters are fetched into the cache folder (toasts take local files). Release builds post as the installer's shortcut (AUMID = bundle identifier); debug builds borrow PowerShell's id. |
| `src-tauri/src/discord.rs`, `media.rs`, `updates.rs` | Discord presence (IPC), OS media controls (SMTC), app and provider updates. |
| `src-tauri/src/clearance.rs` | `browser-fetch`: clearance replay first, the in-page bridge as fallback; dev experiments (see below). |
| `src-tauri/src/adblock.rs` | Embed ad blocking: a WebView2 `WebResourceRequested` hook on the main window (iframes included, through the source-kinds filter) asks `anilili_core::adblock` about every request, and a refused one gets an empty 200. Filter lists: the bundled copy (`filters/`), replaced by one fetched into app data when older than a week. Each refused host is logged once a session. |
| `src-tauri/src/recs.rs` | Recommendations and the dice: the neighbour graph kept in app data (`recs/`, refreshed daily from the media index), taste from the account list, this PC's library and progress, and dice swipes (`taste.dice`, 300 kept). Home's "For you" and "Because you watched" rows (cached 10 min); the dice's pool pages (AniList, popularity > 10,000, cached a day) and taste picks. The deck itself is `src/lib/dice.ts` (a port of Android's `RandomAnimeDeck`). |
| `src-tauri/src/backup.rs` | Backup export/restore in Android's file: shared settings under Android's key names (the desktop's own under `desktop.`, which Android ignores), library, progress, and the AniList login sealed with a passphrase. A restore replaces settings and **merges** the rest; logins are opened before anything is written. A MAL hand-off made on Android is redeemed here; the desktop doesn't make one yet. |
| `src-tauri/src/diagnostics.rs` | The daily log file (seven kept, in `%LOCALAPPDATA%/app.anilili.desktop/logs`) and **Save diagnostics**: one scrubbed text file in Downloads, never uploaded. |
| `src` | React 19 + TypeScript. TanStack Router/Query, shadcn/ui (`base-nova`), Tailwind v4, lucide. |
| `src/lib/theme.ts` | Appearance (Settings → Appearance and Layout): Android's choices stored under its values in Rust `Settings.appearance` (so backups carry them both ways, under Android's keys), applied as shadcn colour variables on the document; also the clock format, poster elements and Home's row order (Android's `-hidden` encoding, keeping rows only Android has). |
| `src/index.css` (Surfaces) | **One look for every panel**: `surface` (panels, cards, `Card`, `Alert`), `surface-bar` (sticky bars), `surface-chip` (badges and round buttons over art), `surface-control` (tiles and toggle chips; tinted, never blurred). Don't paint panels with `bg-card`/`border` by hand. Android's Liquid glass switches decide them per part: the sidebar and `<main>` carry `data-glass` (the page's part from `glassPartFor`), popups follow `html[data-glass-popups]`; off is a solid card fill. |
| `src/components/player/seek-bar.tsx` | The player's timeline, in layers inside the slider's track (as siblings under it, the track hid them): buffered ahead, skip windows in Android's marker amber (`--player-marker`), played in the accent; hover shows the time and the window, under a preview picture when the server has a storyboard (`seek-preview.tsx`: the cues and each sprite sheet fetched by the app with the server's headers, a tile cut out as a CSS background; a scaled `<img>` of a whole sheet painted the bottom rows black). |
| `src/components/player/player-surface.tsx`, `player-overlays.tsx`, `src/lib/player-policy.ts` | The player laid out as Android's: logo title card, clock chip ("Ends at"), center transport cluster, time chips and labelled pills; controls hide while paused too, and a pause held 15 s bills the episode (logo, episode, server, synopsis). The policy file ports Android's rules (keep in step): one action in the corner (auto-skip counting down 3 s of *played* time, Skip Recap/Intro with the controls only, Skip Outro only for credits reaching the last quarter, `+Ns`), and the Up-next card only in the last 30 s, after the credits, 8 s countdown. With auto-play on, a file that ends (played out or skipped to its end) goes on to the next episode at once, as Android's `onEnded`. |
| `src/components/anime/anime-actions.tsx`, `continue-card.tsx` | Right-click on any anime card: Android's long-press list menu (Details, the list destinations with the current one marked, removal with a confirm when an account would lose it, the link); a row adds its own actions first. Continue watching cards are Android's: 16:9 art with a play mark, Details and Remove in the corners, progress along the foot. Removing hides a title from the row only (`continue_hide`, undone by watching it again); progress and the account stay. |
| `src/components/layout/app-context-menu.tsx` | The app's right-click menu, replacing the webview's: Refresh (every query and the route, not a reload, so playback goes on), Keyboard shortcuts, About; Cut/Copy/Paste on text fields and selections. A card's menu stops the click first. |
| `src/components/ui/button.tsx` | **A click handler that returns a promise makes the button busy** (spinner, disabled) until it settles; `loading` forces it. Return the promise rather than `void`-ing it. The shell's activity bar sweeps while any query loads for the first time or a mutation runs; quiet polls set `meta: { background: true }`. |
| `src-tauri/src/glass.rs` | The window's native glass: macOS 26 Liquid Glass (older macOS: under-window vibrancy) behind the transparent webview while any page part has glass on; the CSS then lets it through (`html[data-native-glass]`). Not on Windows: mpv paints that window's surface. **Untested on a Mac.** |
| `src/lib/types.ts`, `src/lib/player.ts` | Hand-written TS mirrors of the Rust types. tauri-specta has no Tauri 3 support yet, so **keep these in step with the Rust structs by hand.** |
| `scripts/` | Fetchers (libmpv, Anime4K shaders), `prepare-boringssl.mjs`, dev aids, `publish-update.mjs`. |
| `patches/` | Our patches to third-party sources: BoringSSL (ML-DSA signature schemes), applied by `scripts/prepare-boringssl.mjs`; mpv's MoltenVK context (resizes, HiDPI, EDR headroom), applied by `scripts/build-libmpv-macos.sh`. |
| `.cargo/config.toml` | Points btls-sys at `vendor/boringssl` (`BORING_BSSL_SOURCE_PATH`, `ASSUME_PATCHED`). |
| `vendor/` | Fetched binaries and shaders (gitignored). |

`crates/core`:

| Module | What |
| --- | --- |
| `net` | One `wreq` client emulating Chrome 149 (BoringSSL TLS + HTTP/2 fingerprint), `Http::edge()`, which matches WebView2 for clearance replays, and `Http::plain()` (no emulation) for CDNs that refuse a browser fingerprint without a browser behind it (UniqueStream's). **Redirects are enabled explicitly**: wreq follows none by default. |
| `miruro` | Miruro catalog API: mirror failover; `octet-stream` bodies are XOR `miruro/catalog` then gzip; queries are literal (else `400 Unsupported catalog request`): `limit=100` for the title lookup, `limit=10000` alone for episodes (since 2026-10-05 `kind=regular` is refused; specials arrive in the list and are filtered out). When Miruro changes, its own bundle (`limits` object, `listEpisodes` call in the Seasons chunk) says what it now sends. |
| `anilist` | GraphQL: home rows, search, detail, schedule, viewer, list, progress/status mutations. |
| `proxy` | Localhost HLS proxy for playlists mpv can't take as served: Kwik's filename-derived AES IVs, and UniqueStream's key (the media id in the URL; the served `key.bin` is wrapped and header-gated). `transform_for` decides by provider and host. |
| `recs` | Android's taste engine (`data/recs/`): the published "liked this, also liked" graph (`recs/neighbours-v1.tsv.gz`, CSR arrays), `signals` (scores read against the viewer's own scale, recency halving yearly), `recommend` per franchise with diversity, `featured_seed`. **Change it only together with `TasteEngine.kt`/`NeighbourGraph.kt`.** |
| `store` | SQLite (WAL, migrations): cache, cards, library, progress, settings, downloads, the app's notification log (`notices`) and the announcement ledger (`announced`: `episode:<id>:<n>`, `anilist:<id>`). |
| `download` | HLS (TS/fMP4) and MP4 downloads with AES-128 (Kwik IVs), retry-with-backoff, subtitles beside the file. A master playlist's variant is the best at or under the ceiling (else the shortest); `hls_heights` lists what a master offers. |
| `aniskip` | AniSkip's live API (Android's `AniSkipClient.kt`): intro, outro and recap, the submission nearest the playing stream's length, the core-types retry, relation-rule redirects. `watch::skip_times` asks it only when the server and the media index's mirror had nothing, and caches found answers a week, "none" a day. |
| `mal` | MyAnimeList: PKCE `plain` login, rotating refresh tokens (store each new pair; drop the session only when the old token is dead), list, progress/status updates. AniList stays the metadata source. |
| `plugin` | wasmtime host for provider components: per-call store, 64 MB memory cap, epoch deadline (45 s per call, network waits included), `http`/`browser-fetch` limited to each provider's declared hosts, cache keys namespaced `plugin:<id>:`. Bundled + user directory; newer version wins. Servers come back in `PRIORITY` order, the tiebreak after Miruro's servers when the app picks one itself; a new provider id belongs in that list. |
| `update` | Signed release channel (and the provider channel, d-tag `anilili-desktop-providers`): Nostr kind 30078, d-tag `anilili-desktop-update`, project key shared with Android; NIP-01 id + BIP-340 verified; installer SHA-256 checked. A 404 from the mirror means nothing is published on that channel yet: no update, not a failure. |
| `media_index` | The project's static media index on jsDelivr (`pommieyewear/media-index`, Android's "Konoha"): episode stills/titles/synopses, the title's logo and banner, the dub index behind the cards' DUB tag (`dub-index.json` plus `dub-episodes.json`, as Android's `DubIndex`) (and an AniList snapshot for when AniList refuses), AniSkip's skip-time mirror, filler and dub indexes, backdrops, and the monthly schedule. `catalog.rs` reads it through its own cache and only ever adds to a page: a 404 or a failure is "nothing", never an error. |
| `adblock` | Which requests an embed page may make. Android's list of ad/analytics origins (`EmbedWebView.kt`; keep the two in step) always applies; EasyList, EasyPrivacy and uBlock's filters and badware lists (network rules, Brave's `adblock` engine) join once loaded. **Never refused:** the app's own requests, the embed's own host and subdomains, media and subtitles, and the player CDNs in `NEVER`. Refusing a player's CDN takes the video with it. |
| `backup` | The `anilili-backup` v1 document Android writes (every field defaulted, unknown ones ignored, `format` required) and its login sealing: PBKDF2-HMAC-SHA256, 210,000 rounds, then AES-256-GCM with `format/version` as associated data. Tested against an independent implementation; **change it only together with Android's `AppBackup.kt`/`BackupCrypto.kt`.** |
| `links` | `anilili://anime/<id>`, `watch/<id>/<episode>[?t=]`, `search?q=`: parsed into app routes. The contract is `docs/LINKS.md`, written for Android to adopt (it reads none yet). The installer registers the scheme from `plugins.deep-link` in `tauri.conf.json`; links arrive as arguments, at launch (`initial_route`) or through single-instance (`open-route` event). |
| `franchise` | A title's whole franchise as a watch guide (Android's `buildWatchOrder`): the season chain from the media index's `series` (else a PREQUEL/SEQUEL walk), plus whatever its members' franchise relations reach, shelved Seasons, Movies, OVAs, Specials, Other. Every entry carries two franchise-wide numbers: `position` (airing order) and `chronological` (the story's own, a topological order of the PREQUEL/SEQUEL edges with airing order breaking ties; a cycle in the data falls back to airing for all). The title page shows the guide as one Watch order card grid (numbered chips, the current title marked) that offers the story's order beside the release order when the two disagree. |
| `storyboard` | Seek-bar previews: a server's `thumbnails` WebVTT (cues naming a sprite sheet and a `#xywh=` tile) parsed with sheet paths made absolute, and the sheets fetched, both with the server's headers from a **plain** client (Icarus's CDN refuses the Chrome fingerprint). Sources (2026-10-06): Miruro's per-provider `thumbnails.vtt` (KickAssAnime, Icarus), AniZone's `storyboard`, Zoko's `sprite_vtt` (always empty so far), MegaPlay's `kind: thumbnails` track (none sent so far). |
| `catalogue` | Every failure the UI can show: a `D`-prefixed code (`DNET-07`, never renumbered; separate from Android's `NET-01`), a plain message and the next step. A lower layer may add its own half (`/h404` HTTP status, `/m-13` mpv error). `docs/ERROR_CODES.md` is generated by its test. Commands fail with `fail(entry, detail)` or `as_entry(entry)`; a bare `String` lands on the generic `DAPP-01`, so prefer an entry. |

## How the video works

mpv receives the main window's own HWND as `wid`, so it paints the window surface. WebView2 is a
child window of that window and starts transparent (`WEBVIEW2_DEFAULT_BACKGROUND_COLOR=0`, set in
`main.rs` before the webview exists; `html, body, #root` are transparent in `index.css`), so the
UI floats over the video. Pages outside the player are opaque (`bg-background`). The video is
moved with `video-margin-ratio-*` (side panel), never covered. On macOS the renderer is the same
(`gpu-next`), over Vulkan through MoltenVK: `wid` is a `CAMetalLayer` under the WKWebView
(`player_mac.rs`), to the same effect, and HDR files go out as HDR on HDR screens. The Mac's LGPL
libmpv is built, not fetched (`scripts/build-libmpv-macos.sh`: MPVKit, mpv 0.41, FFmpeg 9, plus our
patch); `Mpv::loadfile`, deferred `sub-add` and `http_persistent=0` remain for older builds (a
Linux distribution's). **`docs/MACOS.md`** lists every macOS gap.

Playback tuning that matters (`player.rs`): `cache-secs=120` (the default prefetches the whole
episode and trips CDN 429s); `demuxer-lavf-o=seg_max_retry=8` (ffmpeg otherwise *skips* a failed
HLS segment).

Embed-only servers play in an iframe (`components/player/embed-player.tsx`). No `sandbox`
attribute: hosts detect it and refuse to play. The main window's `on_new_window` denies every
popup, and `on_navigation` refuses top-level navigation away from the app.

## Cloudflare clearance: where it stands (2026-10-04)

`browser-fetch` replays a webview-earned `cf_clearance` from `wreq`, and falls back to the
in-page bridge (below) when the replay is challenged. Measured with `clearance_experiment`
(its `classical` flag is the control), `fingerprint_compare`, and `--example ja4`:

- Don't override the webview's user agent: WebView2 keeps sending its real client hints
  (Edge WebView2 154), so the client looks inconsistent. Solve as the webview, then replay its
  exact `User-Agent`, `sec-ch-ua*` and `Accept-Language` headers.
- `Http::edge()` matches WebView2 on the wire: same HTTP/2 fingerprint, and **the same JA4**
  (`t13d1516h2_8daaf6152771_806a8c22fdea`). Chromium 154 offers the ML-DSA signature schemes
  (`0x0904–0x0906`) first; btls's BoringSSL did not know them, so we patch it
  (`patches/boringssl-mldsa-sigalgs.patch`: real TLS 1.3 support, verification included, not
  just the code points). `Http::new()` (Chrome 149) keeps the profile's classical list.
- **Unexplained:** replays got 403 earlier on 2026-10-04 and pass now *with either signature
  list* (4/4 alternating trials). So ML-DSA was not what decided it then, or Cloudflare's rules
  for the site changed. The JA4 match is kept so the replay stays indistinguishable; the bridge
  fallback covers a site that refuses replays anyway.
- Replays read cookies from the bridge window (the main window's view of the cookie store did
  not include `cf_clearance`), on a blocking thread with a timeout: the read waits on the UI
  thread.
- Measured: AnimePahe's whole lookup (search → title page → episodes → play page → two Kwik
  embeds, sub and dub) in 0.6 s by replay, against about 30 s through the bridge page.

**The browser bridge** (`clearance::browser_fetch`, a provider's `browser-fetch`):

- A 160×120 undecorated window at the screen corner (label `browser-bridge`), kept on the origin.
  It must be *visible*: a hidden webview throttles the timers a challenge script needs and never
  clears.
- Readiness is the page's own word: `readyState` complete and a title that is no longer "Just a
  moment…". A cookie alone is not proof.
- The request runs as `fetch()` inside the page, script re-sent every ~0.8 s until it answers
  (it does nothing while the page is still challenging, and runs once per page).
- The body is gzipped in the page (`CompressionStream`) and handed back by navigating to the
  proxy's `/bridge?id=…&d=…`: a public page may not `fetch` localhost (Local Network Access),
  but top-level navigations are allowed.
- After each answer the page is sent back to the origin, by `navigate` *and* `location.replace`,
  repeated: a lone `navigate` off the bridge page was observed being dropped, which stranded every
  later request.
- Requests are serialised. After a failure, requests for that origin fail at once for 60 s
  rather than each waiting out 45 s.

## Commands

```bash
npm install
node scripts/fetch-libmpv.mjs       # once (again after it changes): LGPL libmpv, and on Windows the Vulkan loader, into vendor/ (a Mac builds it: scripts/build-libmpv-macos.sh)
node scripts/fetch-shaders.mjs      # once: Anime4K shaders into vendor/
node scripts/fetch-filters.mjs      # once: embed filter lists into vendor/ (`--force` for fresh ones)
node scripts/prepare-boringssl.mjs  # once, before any cargo build: patched BoringSSL into vendor/ (then `cargo clean -p btls-sys` if it changed)
node scripts/build-providers.mjs    # once (and after provider changes): needs `rustup target add wasm32-wasip2`
npx tauri dev                       # run (Vite on :5173 + the app)
bash scripts/dev-kill.sh            # stop everything; `tauri dev` can leave Vite holding :5173
npx tsc -b && npx eslint src        # frontend checks
cargo clippy --workspace --all-targets && cargo test --workspace
npx tauri build                     # NSIS installer in target/release/bundle/nsis/
cargo run -p anilili-core --example live            # live catalog + proxy smoke test
cargo run -p anilili-core --example download        # live Kwik download test
cargo run -p anilili-core --example verify_channel  # verify the live Android channel event (`desktop` for ours)
cargo run -p anilili-core --example plugins         # resolve an episode with the built providers; args: [module] [anilist mal episode "Title|Alt"]; PLAY=1 also plays each in libmpv
cargo run -p anilili-core --example ja4             # the TLS/HTTP2 fingerprint of both clients
cargo run -p anilili-core --example adblock         # the real filter lists against ad hosts and player dependencies; args: [url source kind]...
node scripts/build-providers.mjs                    # build WebAssembly providers into vendor/providers/
```

**Checking the app without clicking** (debug builds only; the screen-control tool cannot drive
the dev exe):

- `ANILILI_ROUTE=/anime/154587` opens a route at launch. From Git Bash, prefix
  `MSYS_NO_PATHCONV=1`: Bash otherwise rewrites the value into a Windows path, which the app
  ignores (it opened on "Nothing here" before it did);
- write a route to `target/dev-route.txt` to navigate (`scroll:600` scrolls the page, `row-scroll:0:end` scrolls a page row
  (top first; `end` or pixels) to check a shelf's lazy loading, `dice` opens the dice, `hover-seek:0.4` holds the pointer 40% along the player's seek bar,
  `click:Download` clicks the button, link, menu item (radio items too), switch or tab with that text or `aria-label`), or `{"cmd": "...", "args": {...}}` to
  `target/dev-invoke.json` to call any command;
- `bash scripts/dev-go.sh <route> <shot-name> <seconds>` navigates, waits and captures;
- `powershell -File scripts/dev-keys.ps1 "<SendKeys>"` types into the app;
- `powershell -File scripts/capture-window.ps1 out.png` captures the dev window without taking focus
  (the webview only; add `-Screen` for the composited screen with mpv's video, which brings it to
  the front);
- on macOS, `bash scripts/capture-window.sh out.png` captures the dev window as composited, video
  included, without bringing it forward (the app running the command needs Screen Recording
  permission); `dev-go.sh` and `dev-kill.sh` work there too;
- an installed copy that's running takes over a dev launch (single-instance) and the dev copy exits.
  Run `npx tauri dev --config "{\"identifier\":\"app.anilili.desktop.dev\"}"` beside it (its own
  lock and data folder), with `ANILILI_BACKGROUND=1` so it opens without taking focus;
- `target/debug/anilili-desktop.exe anilili://anime/154587` opens a link in the running dev app, as
  a clicked link would (dev builds aren't registered for the scheme);
- `ANILILI_SPIKE_URL` / `ANILILI_SPIKE_SUB` (absolute path) autoplay at launch; `/dev/embed?url=`
  opens any embed URL;
- `ANILILI_MPV_LOG=v` (or `info`, `debug`) raises mpv's log level from `warn`, to see what it opens
  and decodes;
- after each navigation the page logs (target `ui`) how long until no skeleton is left and how many
  frames took over 50 ms; read it in the dev log to tell a slow page from a slow command. A window
  behind others draws at 30 fps, so judge smoothness with it in front.

Capture the app window only, never the whole screen: it holds the user's other windows.

Lint rules to know: the React Compiler lint forbids `setState` in an effect body. Use React
Query, event subscriptions, a component that mounts fresh, or state tagged with the episode it
belongs to. Add shadcn components with `npx shadcn@latest add <name>`, and follow the shadcn
skill's rules (semantic colours, `gap-*`, `data-icon` in buttons, `nativeButton={false}` when
rendering a link).

## Keeping in step with Android

- `node scripts/port-ledger.mjs` lists the Android commits since the last sync point
  (`docs/android-sync.json`) that touch code the desktop mirrors, grouped by the desktop file to
  look at (`--all` adds Android-only areas). After porting, or judging a change Android-only,
  record the new point with `--mark`. It only reads `../miruro-apk`.
- `fixtures/parity/` holds cases both apps must answer alike (title and season matching,
  `anilili://` links, a sealed backup), read by the desktop tests named in its README. A failing
  case is a disagreement between the apps. Settle it, don't delete the case.

## Releasing (only when told to, each time)

**"Bump build and push" means a release, not `git push` to `origin`.** Only the installer file and
the desktop changelog go into the release: the **same GitHub release the Android APKs use**, as
two more files. The repo,
release tag and `gh` account are in `../anilili-private/RELEASE-RUNBOOK.md` → "Desktop
releases"; hosts never go in this repo. Saying the phrase is permission for exactly this sequence;
no code goes into the release.

1. **Bump.** Desktop has no separate build number: every published build raises the last number of
   `version` (`0.1.0` → `0.1.1`) in `src-tauri/tauri.conf.json`, `package.json` and
   `src-tauri/Cargo.toml`, together. The updater compares versions numerically. Commit only that
   ("Release v<v>").
2. **Build.** `node scripts/fetch-filters.mjs --force`, then `npx tauri build`. Check the installer
   holds `libmpv-2.dll` and `vulkan-1.dll` beside it, the 39 shaders, `providers/*.wasm` and
   `filters/*.txt` (four lists), and nothing stale (a removed provider left in `target/release/providers/`).
3. **Upload the installer** to that GitHub release as **`Anilili-Desktop-Setup.exe`**. Use that
   exact name, replacing the previous one (`--clobber`).
   - **Never create a new release for it.** Android's update manifest downloads from GitHub's
     `releases/latest/download/…`; a new release would become "latest" and break every Android
     download.
   - **The name must contain no Android ABI token** (`arm64`, `armeabi`, `v7a`, `v8a`, `x86`,
     `x86_64`) and must not end in `.apk`. Old Android updaters pick the first asset whose name
     contains their ABI.
   - **macOS:** on the Mac, `npx tauri build` and upload `target/release/bundle/dmg/*.dmg` to the
     same release as **`Anilili-Desktop.dmg`** (`--clobber`). Both platforms share the version:
     build both from the same "Release v<v>" commit.
4. **Update the desktop changelog** (since v0.1.4): the release also carries
   **`Anilili-Desktop-Changelog.txt`**, every desktop version's notes, newest first, so the release
   page always says what each version changed. Write this version's notes (rules below) as
   `releases/desktop/<v>/changelog.txt` in the private folder, add them on top of the running
   file there as `v<v> (<YYYY-MM-DD>)`, and upload that file (`--clobber`). Paths and commands are
   in the runbook, step 4. Android's `changelog.txt` is Android's; never touch it from here. The
   notes stay out of this repo, like the hosts.
5. **Publish the signed manifest**, once, after every installer for this version is uploaded:
   `node scripts/publish-update.mjs --key <key path from the runbook> --version <v> --windows
   <.exe asset URL> [--macos <.dmg asset URL>] --changelog-file <this version's notes> [--relays]`,
   the same notes the changelog file got. It downloads each
   installer to hash it, so it runs on either machine. A platform left out gets no update from
   this release (its copies stay where they are; nothing errors). Dry-read the output before
   `--relays`.
   **Release notes:** write readable, user-facing bullet points, one change per line beginning
   with `- `. Use short sentences, group related changes, and avoid a dense paragraph or commit
   titles. Pass a UTF-8 text file with `--changelog-file <path>` to preserve real newlines.
   The update dialog renders the bullets and scrolls long notes independently of the buttons.
6. **Mirror the manifest:** upload `target/release/desktop.json` as `latest/desktop.json`, as the
   runbook says.
7. **Verify:** `cargo run -p anilili-core --example verify_channel desktop` shows the new version with a
   valid signature and an entry per platform you published.

Provider modules ship separately, without an app release: bump the provider's version in its
`Cargo.toml`, run `node scripts/build-providers.mjs`, upload the `.wasm`, then
`node scripts/publish-update.mjs --key … --providers <list.json>` (entries `{"id","version","url","file"}`)
and upload `target/release/providers.json` as `latest/providers.json`.
