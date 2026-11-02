# Linux execution checklist

Use this after the source/dependency audit in [LINUX-PORT-AUDIT.md](LINUX-PORT-AUDIT.md).
All Linux results below are **pending**. Source/dependency checks performed on Windows must
not be filled in as Linux passes. The production app still needs the audit's implementation fixes.

**Update 2026-10-12 (WSL2/WSLg, Ubuntu 24.04, this PC):** the implementation fixes have landed
(`player_linux.rs` and the L02/L06/L07/L09/L10/L11/L12/L16 treatments) and the WSL column of
the result table at the bottom is filled from live runs. WSLg is **not** a native desktop:
Gate B on GNOME/KDE, X11 and native Wayland, hardware decoding, keyring reboot persistence
and a real notification daemon remain open and are recorded as scoped limitations, not passes.

Record for each run: commit/working-tree diff, distro/version, CPU architecture, desktop,
actual display backend, GPU/driver, WebKitGTK/GTK, mpv/FFmpeg build and versions, package type,
result and evidence. `GDK_BACKEND=x11` on a Wayland desktop tests XWayland, not native Wayland.
Do not capture the whole screen or run repeated live provider/device-registration tests.

## Gate A: prerequisites and compilation

| ID | Check | Passing evidence |
| --- | --- | --- |
| A01 | Run `bash scripts/linux-preflight.sh /absolute/path/to/verified/libmpv.so`. | Local report in `target/linux-audit/preflight.txt`; session/backend, system packages and available tools identified. Missing services/packages are resolved or explicitly scoped out. |
| A02 | Establish LGPL libmpv provenance. | Source tags/checksums, build configuration for mpv/FFmpeg/dependencies, notices and output checksums recorded; dynamic `.so`, client ABI 2.x and required render exports. An archive label/package name is insufficient. |
| A03 | Clean frontend install/build. | `npm ci`, `npm run build`, `npx eslint src` pass on Linux with locked versions and Linux native bindings. No copied Windows node_modules. |
| A04 | Compile the isolated probe. | From `experiments/linux-playback-probe`: `cargo build --locked`. Native GTK/WebKit/GL dependencies resolve and actual Linux Rust type-check/link completes. Fix errors in this experiment before proceeding. |
| A05 | Prepare full app build inputs, after Linux configuration/backend fixes exist. | Run the documented BoringSSL preparation, fetch/build approved LGPL mpv resources, shaders and filters, and build WASM providers. No generic GNU tar ZIP failure or Windows resource references. Keep signing keys outside this repo. |
| A06 | Compile/check production on Linux. | `cargo clippy --workspace --all-targets --locked` and `cargo test --workspace --locked` pass, with ignored live tests identified. Shared parity fixtures remain intact. This tests native Linux code, not just a Linux Cargo dependency tree. |

The probe deliberately omits the production BoringSSL and Wasmtime build. A probe compilation
pass does not fulfill A05/A06. Its package identifier and data are separate from the app.

## Gate B: native video probe

Set absolute paths to a verified LGPL libmpv and a local video as the probe README describes.
Repeat on X11/XWayland and native Wayland when available. Run on a real desktop rather than
calling a container's software renderer proof of the target machine's GPU behavior.

| ID | Check | Passing evidence |
| --- | --- | --- |
| B01 | Layering/transparency and input. | Moving picture beneath translucent bars, readable overlays, working buttons, no video window floating separately, and a moving UI heartbeat. |
| B02 | Pause, repeated seek and fullscreen. | Controls continue responding; correct video/audio state; no render timeout/deadlock warnings. |
| B03 | Resize and side panel. | Correct aspect, no black/stale portions, video moves clear of the panel, controls remain aligned. |
| B04 | Scale/monitor changes. | Correct physical framebuffer size at 100%, fractional scaling if supported, and across mixed-DPI monitors. No upside-down video or clipped viewport. |
| B05 | Hide/minimize, restore, close/reopen and suspend/resume. | Video redraws/reinitializes correctly, no CPU spin or stale frame loop, clean teardown and audio shutdown. |
| B06 | Browser feature observations. | Record displayed user agent, CSS support flags and gzip CompressionStream result. Clipboard API presence is only an observation; permission gets a separate production test. |
| B07 | Software-decoding baseline. | Local MP4/MKV and subtitle tracks render; no claim of hardware decoding. The probe explicitly uses `hwdec=no`. |

A B-series pass establishes whether to integrate this GTK approach. It does not validate
production React performance, decoder interop, accounts, providers, filtering or packaging.

## Gate C: production feature coverage

Run after the known source gaps have been fixed or deliberately scoped out. A documented
feature limitation can be accepted; an unnoticed no-op, wrong-platform installer or inaccurate
UI promise cannot be recorded as a pass.

| ID | Check | Passing evidence |
| --- | --- | --- |
| C01 | Home, search, detail, library, schedule, downloads, dice and settings. | Correct colors/layout/fonts, menus/dialogs, scroll and keyboard focus; no missing assets or unsupported CSS breaking essential controls. |
| C02 | Main-window playback layout. | Logos, subtitles, hover previews, skip markers, hidden/paused overlays, side panel and next-episode controls remain positioned correctly over native video. |
| C03 | Hardware decoder path and fallback. | With acceleration enabled, logs/property `hwdec-current` identify the decoder actually used on the tested GPU; with it disabled or unavailable, software fallback plays correctly. Separately record EGL/GLX and GPU selection. |
| C04 | Codec/source coverage. | One representative HLS stream, MP4, DASH stream, local MKV/MP4, dub/audio switch and soft-subtitle track work. Test H.264 8-bit, supported HEVC/AV1, and CPU fallback for formats the tested GPU cannot decode. |
| C05 | Streaming recovery. | Seeks and episode transitions work, a simulated short network outage recovers, HLS segment failures do not skip content, and the buffer/cache limits remain effective. Avoid creating CDN retry/rate-limit storms. |
| C06 | Subtitle appearance. | External and embedded subtitles, offsets, preferred languages, caption positions, Latin/Arabic/CJK glyphs and Linux font substitutions match the intended settings. |
| C07 | Anime4K and screenshots. | Fast/quality shader chains load from packaged resources, disable cleanly, preserve subtitles and save a screenshot to the expected writable Pictures location. |
| C08 | Display sleep and suspend. | While playing, the intended desktop sleep inhibition works; after pausing/stopping/quitting it is released according to policy. Resume does not leave a dead GL context or stale audio device. |
| C09 | Mini player and window state. | Size/decorations/restore work; record compositor handling of position and keep-above. Unsupported exact positioning/topmost behavior is handled without failed promises or a stuck borderless window. |
| C10 | Direct providers/WASM cache. | Modules load from package and user directory, identify themselves, use a per-host compiled cache and serve a normal resolution; no copied Windows compiled cache. |
| C11 | Browser-backed provider. | A normal cached AnimePahe resolution completes through a real WebKit bridge; challenge readiness, cookie retrieval, localhost return, reset, timeout/cancellation and a subsequent request work. Do not infer success from `cf_clearance` alone. |
| C12 | Rift, if included in the agreed test. | One normal cache-respecting request; sub/dub serialization retained. No registration loop. If remote service denies access, record it without repeatedly retrying. |
| C13 | Embedded servers and filtering. | One actual iframe server plays with required WebKit/GStreamer codecs; fullscreen and popup/navigation guards work. Test known ad requests and essential media/CDN exceptions separately. The UI states the filtering actually implemented. |
| C14 | Clipboard and context menu. | Copy an app link/selection, paste into a field using the app menu, cut, and keyboard shortcuts work under real user gestures. Read permissions and denied/unavailable behavior are surfaced appropriately. |
| C15 | Browser/file openers. | Settings websites open in the default browser; Downloads opens the correct directory; reveal selects a file where FileManager1 supports it, otherwise opens its folder. |
| C16 | Deep links/single instance and local file launch. | Browser `anilili://anime`, watch with time, and search launch correct routes cold and while running. Repeated launch does not create a second player. Test a path containing spaces/Unicode. |
| C17 | AniList and MAL login/sync. | Redirect interception completes with real WebKit cookies, restart/session refresh works, state mismatch is rejected, library/progress changes reconcile correctly and logout completes. Use a test account where available; avoid redundant remote mutations. |
| C18 | OpenSubtitles credentials. | Consumer key is configured for the build; password is in a persistent Secret Service store and survives process restart. Locked/missing service produces an explicit failure. Nothing silently uses the mock backend. |
| C19 | Downloads/offline filesystem. | Queue persistence/pause/cancel/retry, two-job disk reservation, low-space pause, episode/subtitle files and offline playback work. Test a writable custom directory and a denied path. |
| C20 | Backup/data/diagnostics. | Shared sealed fixture and a real export/restore round-trip work; progress/library merge as intended; local-only paths/images do not travel. Logs honor XDG locations; Save diagnostics produces a scrubbed local file. |
| C21 | Notifications and routes. | Episode/download/sample notifications identify the app; replacement/group clearing and click actions work where the desktop supports them. Missing server/actions are handled without affecting playback. |
| C22 | Media keys and Discord. | MPRIS play/pause/seek/metadata works on the actual session bus; absence is harmless. Native Discord IPC works where installed, and reconnects when Discord starts later. Test sandboxed Discord separately if relevant. |
| C23 | Updates. | Linux selects only its own supported artifact/architecture; signed notice and checksum rejection remain effective. Package-managed installs are not overwritten by app updates. If deferred, app update controls/checks are accurately disabled; provider updates remain separate. |
| C24 | Dock/taskbar progress. | Treat as optional until explicitly implemented for the target desktop. The current libunity-based dependency path is not GNOME/KDE parity; in-app queue progress must always work. |

## Gate D: packaging and regression

| ID | Check | Passing evidence |
| --- | --- | --- |
| D01 | Clean `.deb` installation. | Native runtime dependencies declared, desktop entry/icon/URL scheme correct, app launches without a source checkout or development libraries. No Windows DLLs/stale providers shipped. |
| D02 | Dynamic-library closure. | Correct `.so`/SONAME, required libraries and per-library search paths resolve from the package. LGPL library remains replaceable. No accidental GPL/nonfree dependency build. |
| D03 | AppImage, if selected. | Launch on every declared supported distro baseline, resources and dlopen dependencies found, webview codecs present, host openers/desktop services work. Identify minimum glibc/runtime expectations. |
| D04 | Environment matrix. | At least the declared first distro/desktop; then GNOME/KDE, X11/native Wayland and representative Intel/AMD/NVIDIA as support expands. Unsupported environments are named, not implied supported. |
| D05 | Previous-platform regression. | Windows and macOS retain video, commands, resources, account/data contracts and updater selection after shared code changes. No accidental framework-version upgrade. |

Installer builds/testing are local. Publishing installers or signed manifests still needs the
repository's explicit release instruction; a successful checklist is not publishing authorization.

## Result template

Copy a row per case into a local result document:

| Case | Environment/build | Pass / fail / scoped limitation | Evidence | Follow-up |
| --- | --- | --- | --- | --- |
| A01 | Ubuntu 24.04 WSL2, preflight script | Pass | `target/linux-audit/preflight.txt`: GTK 3.24, WebKitGTK 2.52, libsoup, dbus, EGL/GL present | node/cmake/go added since |
| A03 | Ubuntu 24.04, Node 22 | Pass | `npm ci`, frontend build, `tsc -b`, `eslint src` all pass in WSL | — |
| A04 | Probe, WSL | Pass | `cargo build --locked` in `experiments/linux-playback-probe` | — |
| A06 | Production, WSL | Pass | `cargo test --workspace` 130 passed (88 core + 41 desktop + 1 mpv); clippy 0 errors | — |
| B01–B03, B07 | Probe + production, WSLg (Weston) | Pass (WSLg only) | User-operated probe: play/pause/seek/fullscreen/panel/resize all work; production `VO: [libmpv] 1280x720`, external subtitle track loaded, seek landed at exactly 30 s (MPRIS Position) | Native GNOME/KDE still required |
| C04 (partial) | Live catalog run, WSL | Pass | `anilili-core --example live`: native streams resolved across providers incl. Kwik/AnimePahe; HLS proxy playlist 479 lines | Embed iframe codecs untested |
| C10 | Production, WSL | Pass | All 9 provider modules loaded from package/user dirs in app log | — |
| C16 | Production, WSL | Pass | Second launch `anilili://anime/154587` exited 0; running instance logged `opening a link` (needs >25 s in WSL for D-Bus registration) | Cold-launch/mime handler on native desktop open |
| C18 | Keyring probe, WSL | Partial | App now uses `linux-native-sync-persistent` + `crypto-rust` (keyutils cache + Secret Service store); cross-process set/get passes with keyutils, Secret Service daemon runs in WSL but its prompt/collection creation needs a desktop prompt agent (dismissed in WSLg) | Retest on GNOME/KDE login session |
| Settings persistence | Production, WSL | Pass | Set volume 42 via player control, killed app, `settings` row in `anilili.db` holds full Settings JSON with `volume: 42.0` | — |
| B (X11) | Probe, WSLg XWayland | Pass (XWayland) | `GDK_BACKEND=x11`: render context created, `VO: [libmpv] 1280x720` | Native X11 desktop still open |
| D03 | AppImage, WSL | Pass | `Anilili_0.1.5_amd64.AppImage` (157 MiB) launches; log shows it loading its bundled `/usr/lib/Anilili/libmpv.so.2` from the extracted image and creating the render context | Other distros/glibc baselines untested |
| C19 (engine) | Live download, WSL | Pass | `anilili-core --example download`: full 157-segment HLS episode (52.2 MB, Kwik AES) saved in 24 s | App queue/low-disk paths are unit-tested, not live-driven |
| C07 | Anime4K + screenshot, WSL | Pass | `upscaler: fast` applied with no shader errors, playback continues; screenshot saved a real 43 KB JPEG frame (full byte variance, not a blank) | Screenshot landed in CWD because WSL has no XDG Pictures dir; native path untested |
| C22 | Production, WSL | Pass | `org.mpris.MediaPlayer2.anilili` on session bus; Metadata title/length, PlaybackStatus Playing/Paused; MPRIS Pause/Play control mpv | Discord untested |
| C21 | Production mechanism, WSLg + dunst | Pass | With a daemon present (`dunst` claimed `org.freedesktop.Notifications`), `notify-send` succeeds and `dbus-monitor` captured the `Notify` call (`Anilili`, title, body) — the exact command the app's Linux branch spawns. Stock WSLg ships no daemon, so nothing shows there | Click-actions/replacement IDs need a desktop daemon with those features (documented limitation in notify.rs) |
| D01–D02 | `.deb`, WSL | Pass | `Anilili_0.1.5_amd64.deb` (42 MiB) installs; no Windows files; desktop entry registers `x-scheme-handler/anilili` | — |
| A02 | Self-built libmpv, WSL (first build, 2026-10-12) | **Fail** (found 2026-10-13) | `scripts/build-libmpv-linux.sh` builds mpv v0.40.0 (`-Dgpl=false`) + FFmpeg n7.1.1 (`--disable-gpl --disable-nonfree`) from pinned tags with `BUILD-INFO.txt`; installed app logs `libmpv 2.5 (mpv v0.40.0)` loading `/usr/lib/Anilili/libmpv/libmpv.so.2` (FFmpeg libs beside it, `$ORIGIN` rpath, `ldd` fully resolved); probe plays with it | Field reports: FFmpeg had no TLS (no `https` protocol, so no stream played), and it needed Ubuntu 24.04's `libplacebo.so.338`, `libmujs.so.3`, `liblua5.2`…, so it would not load on a newer distribution (the player never started, and settings saves failed with it). The probe passed on local files only. |
| A02 | Self-built libmpv, WSL (rebuilt 2026-10-14) | Pass | FFmpeg `--enable-gnutls` (protocol list has `https`, `tls`); libplacebo v7.351.0 static inside libmpv; Lua/JS/rubberband/sixel/sndio/Blu-ray/libarchive/libjpeg/uchardet/libavdevice/bzip2 off. NEEDED is only names every distribution shares (libass.so.9, libgnutls.so.30, libva, wayland, X11, pipewire/pulse/alsa, lcms2, zimg, z, lzma); the script refuses a build linking a distribution-specific one or lacking TLS. An HTTPS HLS test stream loads (the old build: `END_FILE error`), and the app plays it in WSLg (`VO: [libmpv] 1920x1080`) | Load on Arch/Fedora still to be confirmed; when the bundled build won't load, the app now falls back to the distribution's `libmpv.so.2` |
| B/C hwdec, C03 | WSLg | Scoped limitation | `/dev/dri` inaccessible in WSLg; software decode only (`hwdec` falls back safely) | Native GPU test required |
| L08 adblock | Linux | Scoped limitation | Popup/navigation guards active; per-request Brave filtering is Windows-only, embed badge says "Popups blocked" on Linux | WebKit content-filter translation is future work |

Preserve the compiler errors, relevant scrubbed log excerpts and app-window captures for failures.
Collect observable facts first; do not convert dependency support into a runtime pass.
