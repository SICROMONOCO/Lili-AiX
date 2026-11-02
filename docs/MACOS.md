# macOS: where it stands (2026-10-12)

Builds and runs on an arm64 Mac (macOS 26): the app opens, providers load, episodes resolve and
play with picture, sound and subtitles. The gaps left are in the table below. Since 2026-10-12 the
video goes through a new libmpv and renderer (below), **not yet run on a Mac**: the first build
there is its test.

## Getting it running

```bash
brew install go cmake                # BoringSSL's build needs both; plus full Xcode (libmpv's build needs Swift)
npm install
node scripts/prepare-boringssl.mjs
node scripts/fetch-libmpv.mjs        # builds the LGPL libmpv (scripts/build-libmpv-macos.sh); the first run takes a while
node scripts/fetch-shaders.mjs
node scripts/fetch-filters.mjs
rustup target add wasm32-wasip2 && node scripts/build-providers.mjs
npx tauri dev
```

`src-tauri/tauri.macos.conf.json` is merged over `tauri.conf.json` on a Mac: it bundles `.app` and
`.dmg` instead of NSIS, and the whole `vendor/libmpv/darwin-arm64/` folder (`libmpv.dylib`, its
licences and `BUILD-INFO.txt`) as `Resources/libmpv/`. An Intel build would need that path changed
to `darwin-x64` (the script builds both). macOS 12 or later: MPVKit's deployment target. Never swap
in Homebrew's libmpv or ffmpeg: both are GPL.

## The libmpv

Nobody publishes an LGPL libmpv for macOS with Vulkan, so `scripts/build-libmpv-macos.sh` makes
one: MPVKit (LGPL-3.0, pinned) builds mpv v0.41.0 with `-Dgpl=false` over FFmpeg n9.0.2,
libplacebo and MoltenVK as static libraries, with its MoltenVK context patch and ours
(`patches/mpv-moltenvk-host-layer.patch`) on top; the script links them into one `libmpv.dylib`
(only `mpv_*` exported, loaded at runtime as on Windows, so it can be replaced), refuses a GPL
configuration or a link to anything outside the system, and signs it ad hoc. It reruns MPVKit only
when the pin or the patch changes, else only relinks. `BUILD-INFO.txt` beside the dylib says what
it was built from (LGPL: those sources plus the patch).

The former build (media-kit's, mpv 0.36 over ffmpeg 6.0) needed four workarounds: unknown options
skipped, `loadfile` without an index, `sub-add` queued until `FileLoaded`, `http_persistent=0`.
They stay in the code, as they detect the version (a Linux distribution's mpv can be that old),
but no longer apply here.

## The video

mpv runs the renderer Windows does (`vo=gpu-next`, libplacebo), over Vulkan through MoltenVK
(`gpu-context=moltenvk`), and presents into a `CAMetalLayer` that backs a view under the WKWebView
(`player_mac.rs`), handed to mpv as `wid`. VideoToolbox frames reach it without a copy. The view
keeps the layer's drawable at its size in pixels; our patch makes mpv notice a new size (within
50 ms) and the Retina scale. MoltenVK's 1×1 drawable trick is filtered out, and its EDR switch is
moved to the main thread (the screen only enters EDR from there).

HDR: the view stores its screen's EDR headroom on the layer (`mpvEDRHeadroom`), which the patch
reports to mpv as the display's colour space when it is above 1. `player.rs` turns
`target-colorspace-hint` to `auto` only while an HDR file (`pq`, `hlg`) plays, with
`target-colorspace-hint-mode=source`: on an HDR screen the video goes out as HDR10/HLG and macOS fits
it to the display; on an SDR screen nothing is signalled and mpv tone-maps it, as before. SDR video
always keeps the plain sRGB surface (with the hint on, mpv would tag it BT.709 on HDR screens only).

To check on the Mac: the build script end to end; a picture at all (the log says
`video layer attached`, and mpv's `[vo/gpu-next]` lines appear with `ANILILI_MPV_LOG=v`); resizing
the window, the side panel (`video-margin-ratio-*`), fullscreen, and dragging between a Retina and a
non-Retina screen; an HDR10 sample on an XDR screen and on an SDR one; subtitles, styled and plain.

## What works differently

| Area | Windows | macOS now | To do |
| --- | --- | --- | --- |
| Video | `gpu-next` over Direct3D 11 paints the window through `wid`, under the transparent webview. | `gpu-next` over Vulkan (MoltenVK) into a `CAMetalLayer` view under the WKWebView (`player_mac.rs`), HDR to HDR screens; VideoToolbox decoding. **Not yet run** (see The video). | The checks listed under The video. |
| Window glass | CSS glass (mpv owns the surface). | Liquid Glass (vibrancy before macOS 26) behind the webview (`glass.rs`), untested. | Check the transparent webview really shows it. |
| Notifications | WinRT toasts with posters; a click opens the route. | Notification Center through `osascript`: shown, but posted as Script Editor, without the poster, and a click opens nothing. | `UNUserNotificationCenter` with a delegate (or tauri-plugin-notification once it supports Tauri 3) to get the app's name and click-through. |
| Embed ad blocking | WebView2 request hook (`adblock.rs`). | None: WKWebView has no request hook Tauri exposes. | WKContentRuleList, compiled from the same filter lists. |
| Free disk space (downloads) | `GetDiskFreeSpaceExW`. | `statvfs`. | — |
| Title bar colour | DWM paints it the sidebar's colour. | The system's. | Optionally `titleBarStyle: Overlay` with the sidebar under it. |
| Media keys | SMTC through souvlaki. | MPRemoteCommandCenter through souvlaki; untested. | Check it on the main thread. |
| Updates | NSIS installer from the signed channel. | The signed notice's `macos` entry (`Anilili-Desktop.dmg`); installing mounts the image, swaps its app in for the running one and reopens it, or shows the image if the app's folder isn't writable. Untested. | Publish a first `.dmg` build and check an update end to end. Unsigned by Apple: the first install needs Privacy & Security → Open Anyway; in-app updates don't (no quarantine flag). |
| Cloudflare clearance replay | Reads the bridge webview's cookies. | Same API; untested against WKWebView's cookie store. | Check `browser-fetch` providers (AnimePahe). |

Keyboard shortcuts say Ctrl in a few labels; on a Mac those should read ⌘.
