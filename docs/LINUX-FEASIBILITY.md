# Linux port feasibility

Assessment date: 2026-10-07. Baseline: desktop 0.1.2, Tauri
3.0.0-alpha.4 / tauri-build alpha.3, Wry 0.57.0, Rust 1.99.0.

For the completed source/dependency coverage, confirmed backlog and execution gates, read
[LINUX-PORT-AUDIT.md](LINUX-PORT-AUDIT.md) and [LINUX-TEST-MATRIX.md](LINUX-TEST-MATRIX.md).
These refine this initial assessment; the current application is not yet Linux-ready.

## Decision

**Feasible, with a moderate platform port and a substantial playback validation task.**
Keep React, Tauri, the Rust core, the WASM providers and the existing mpv command/event
interface. A new frontend or provider rewrite is unnecessary. The main uncertainty is
compositing native video beneath the transparent WebKitGTK UI across X11 and Wayland.

Recommendation: spend 3–5 engineering days on a Linux playback proof of concept before
committing to a release. Target x86_64 first, one Ubuntu LTS build baseline, and test GNOME
and KDE. Prefer a GTK OpenGL render backend that can serve both display protocols; reserve
an X11/XWayland-only first release as a fallback if the rendering experiment fails.

This is a source and dependency assessment, **not a successful Linux build or runtime test**.
No application source was uploaded, no release was made, and existing working changes were
not altered. Estimates below are engineering judgments, not measured delivery times.

## What can carry over

| Area | Assessment |
| --- | --- |
| React UI, routes, query state, themes and player policies | Reuse; test WebKitGTK CSS, fonts, transparency and clipboard behavior. |
| Catalog, AniList/MAL, library, SQLite, backups, recommendations | Mostly platform independent; SQLite is bundled. Existing parity fixtures remain applicable. |
| Downloads, HLS proxy, subtitle and storyboard processing | Reuse; UNIX disk-space checks already use `statvfs`. Folder opening needs fixes. |
| WASM providers and signed provider updates | Components can be reused across desktop hosts; native Wasmtime still needs a Linux build. Browser-backed providers need separate testing. |
| libmpv binding | Already dynamically loads libraries and exposes the OpenGL render API used on macOS. Linux needs a host surface and correct library lookup. |
| Discord and media controls | Dependencies have UNIX support. Souvlaki 0.8.3 supports Linux MPRIS, but current startup ties it to acquiring a Windows/macOS window handle. |
| Diagnostics | XDG log location and `xdg-open` reveal fallback already exist. |

The BoringSSL preparation script is written in Node and locates btls-sys through Cargo
metadata. It has no obvious Windows-only execution path. Linux must still build the patched
TLS stack with the required native toolchain. Retain the pinned Tauri versions for the first
experiment; changing framework versions and platforms together would obscure failures.

## Concrete gaps in the current tree

| File | Current Linux behavior | Required change |
| --- | --- | --- |
| `src-tauri/src/player.rs` | `window_id` accepts only Win32/AppKit; other handles fail before mpv starts. Linux video branch is `vo=null`. Library lookup chooses `libmpv.dylib`. | Separate platform initialization; add Linux video surface, `.so` lookup and Linux resource paths. |
| `scripts/fetch-libmpv.mjs` | No Linux pin; exits with “No libmpv pin”. | Reproducible, pinned LGPL Linux library build/fetch, including its dependency closure. |
| `src-tauri/tauri.conf.json` | NSIS target and Windows DLL resources. | Linux override for package targets, library resources, dependencies, icons and desktop integration. |
| `crates/core/src/update.rs` | Every non-macOS target selects `windows`. | Explicit platform/architecture selection, Linux signed asset metadata, or disable app updates on Linux initially. |
| `src-tauri/src/updates.rs` | Accepts EXE/DMG names; every non-Mac install runs Windows installer flags. | Linux download naming and package-specific update behavior. Never execute the Windows branch on Linux. |
| `src-tauri/src/notify.rs` | Logs notifications without displaying them; clearing is a no-op. | Freedesktop notifications over D-Bus, replacement IDs and route actions where supported. |
| `src-tauri/src/adblock.rs` | Request interception is installed only on Windows. | Investigate WebKit content filters/web extensions; preserve CDN/media exceptions. Popup/navigation guards already exist, but do not replace request filtering. |
| `src-tauri/Cargo.toml`, `opensubtitles.rs` | Keyring enables Windows and Apple stores only. Keyring 3.6.3 falls back to its mock store on Linux. | Enable a persistent Secret Service backend; handle a locked or unavailable keyring explicitly. |
| `library.rs`, `downloads.rs` | Browser opener has no Linux branch; download folder/reveal paths choose macOS `open`. | Linux opener/file-manager integration, with a practical `xdg-open` fallback. |
| `media.rs` | MPRIS-capable dependency is present, but initialization follows the failing window-handle lookup. | Decouple media controls from HWND; pass Linux configuration without a Windows handle. |

The shell may open without a player because `main.rs` logs player setup failures on
non-Windows targets. That would demonstrate shell startup, not Linux playback support.

## Playback architecture

The best candidate is **GTK `Overlay` with a `GLArea` below the transparent WebKitGTK
webview**, using mpv `vo=libmpv` and the existing render bindings. The locally installed source
of the exact pinned Wry 0.57.0 includes `examples/gtk_opengl.rs`, demonstrating this arrangement.
The pinned Tauri runtime exposes the WebKitGTK webview and GTK window/container access.
This is strong evidence that the building blocks exist, not proof that reparenting Tauri's
managed webview preserves its resizing and lifecycle behavior.

Implement a Linux counterpart to `player_mac.rs`. GTK/OpenGL operations must stay on the
UI thread; mpv update callbacks should schedule rendering there. Draw to GTK's active
framebuffer at physical pixel dimensions, retain GL loader libraries, and release the render
context while its GL context is current. Verify hardware-decoding integration separately:
VA-API/other GPU paths, required display parameters and fallback copies can affect performance.

An X11 alternative is a native child drawable passed through `wid`. mpv documents X11
embedding, but transparent webview stacking/input must still be proven. Native Wayland
cannot be handled by simply passing its surface pointer as an X11 window ID. XWayland can
support an X11 implementation only where it is installed and available.

Preserve subtitles, Anime4K, video margins for the side panel, seek previews and React controls.
Do not assume GPU-next's Windows HDR behavior carries over to an OpenGL render backend.
Treat HDR equivalence as later work. Test display sleep inhibition explicitly with the render
API; it must not be assumed to work just because mpv is running.

## Providers and embedded playback

Direct providers should need little or no provider code change. The browser path is riskier:
Linux uses WebKitGTK, while clearance replay uses `Http::edge()` with a fingerprint measured
against Windows WebView2. Capturing a WebKit user agent does not make that TLS profile match.
Start by validating requests inside the actual bridge page; optimize replay only after measuring
the Linux browser. Check cookie APIs, challenge readiness, redirects back to the origin,
`CompressionStream`, and cancellation/timeouts on the supported WebKitGTK baseline.

Test AnimePahe once through the normal app/cache path rather than repeatedly invoking live
resolvers. Preserve Rift's cache and serialization rules; avoid repeated device registrations.
Embed-only servers need separate codec/GStreamer testing because their video plays in the
webview rather than libmpv. WebKit request-filter parity is a distinct task and could materially
extend the estimate if exact Brave filter-engine behavior is required.

## Packaging and distribution

Use a native Linux host or VM with GUI/GPU access for development and release validation.
WSL or cross-compilation can help compile checks, but cannot establish normal desktop behavior.
Install the exact Rust toolchain, Node, GTK/WebKitGTK development dependencies, D-Bus headers,
and the C/C++/CMake/Go toolchain needed by BoringSSL. Resolve the requirements from the pinned
crates rather than assuming all current Tauri v2 documentation applies to Tauri 3 alpha.

Start with a `.deb` for the chosen Ubuntu baseline; add an AppImage after the native build works.
Build on the oldest supported baseline to limit glibc compatibility problems. AppImage is not a
guarantee of compatibility with every distribution or GPU stack. Account for dynamically loaded
libmpv explicitly: bundlers may not discover its dependencies through ordinary executable linkage.

Ship a verified LGPL mpv build (`-Dgpl=false`) and an appropriate FFmpeg/dependency build,
dynamically replaceable under the repository's rules. Do not assume a distribution's libmpv
package meets those rules. Pin inputs/checksums and document dependency licenses and provenance.

Register `anilili://` through desktop/MIME integration; test cold and already-running launches.
Keep local builds and the existing private origin workflow. Linux binaries would join the same
existing release only with explicit release authorization. Never introduce hosted source builds
or another repository. Defer Flatpak: filesystem access, GPU libraries, credential services,
Discord sockets and update ownership add a separate sandbox integration project.

For the first beta, package-manager/manual updates are simpler than automatic replacement.
If AppImage self-updates are added, design verified staging, executable permissions, writable-path
checks, atomic replacement/restart and failure recovery. Do not overwrite an installed `.deb`
from inside the application.

## Work estimate and decision gates

Assumes one engineer familiar with this repository, a Linux test machine, x86_64 only and
reuse of the existing UI. Stages are sequential estimates, not commitments.

| Stage | Estimate | Acceptance gate |
| --- | --- | --- |
| Build and playback experiment | 3–5 days | Pinned stack builds; local video and subtitles appear beneath usable React controls on X11 and native Wayland. |
| Integrate usable beta | 5–10 additional days | Streaming/offline playback, GPU decoding/fallback, sources, account flows, keyring, openers, notifications, MPRIS and installable `.deb`. App updater disabled or correctly Linux-specific. |
| Release hardening | 5–10 additional days | GNOME/KDE, Intel/AMD/NVIDIA where available, scaling/fullscreen, package resources, long playback, diagnostics and Windows/macOS regression checks. |

**Total planning range: approximately 3–5 engineering weeks.** Full embed-blocking parity,
hard GPU/compositor failures, Flatpak, ARM64 or seamless automatic updates may exceed it.

Stop and reassess if the first experiment cannot preserve UI input and video composition in
the same window, or requires a broad runtime fork. An explicitly X11/XWayland-only beta is
a possible reduced scope; it must be labeled accurately.

Before calling the port ready, test seek/pause/next episode, HLS retries, DASH, subtitles,
Anime4K, fullscreen, resizing, mixed DPI, side-panel margins, screen locking/suspend/resume,
two representative desktop environments and hardware/software decode. Test clean installation
without development libraries, restart persistence, deep links, account reconciliation, backup
parity, low disk handling and notification actions. Run frontend checks, Linux workspace tests
and clippy, provider/parity tests, and the existing Windows/macOS regression checks.

## External references

- [Tauri Linux prerequisites](https://tauri.app/start/prerequisites/) — general dependency guidance; verify against pinned alpha crates.
- [Tauri AppImage distribution](https://tauri.app/distribute/appimage/) — baseline compatibility and webview media packaging.
- [Wry GTK OpenGL example](https://docs.rs/crate/wry/0.57.0/source/examples/gtk_opengl.rs) — exact-version source also inspected in the local Cargo registry.
- [mpv manual](https://mpv.io/manual/stable/) — `wid`, rendering and hardware-decoding behavior; validate against the selected Linux mpv version.
- [mpv upstream](https://github.com/mpv-player/mpv) — LGPL build option and build requirements.
- [Keyring 3.6.3](https://docs.rs/keyring/3.6.3/keyring/) — platform features, persistent stores and mock fallback.
- [Souvlaki 0.8.3](https://docs.rs/souvlaki/0.8.3/souvlaki/) — Linux MPRIS and D-Bus configuration.

## Follow-up investigation: exact APIs and local test environment

Further investigation on 2026-10-07 strengthens the architectural case, but does not remove
the requirement for an actual Linux build and playback run.

| Question | Evidence and conclusion |
| --- | --- |
| Is the pinned stack really GTK 3, or would it require GTK 4? | `tauri-runtime-wry` alpha.4 enables Tauri's `gtk3` feature and uses gtk-rs 0.18. Wry 0.57.0 uses WebKitGTK API 4.1, bindings 2.0.2 with the `v2_38` feature. Use **GTK 3 + WebKitGTK 4.1** for this port; a GTK 4 migration is unnecessary. |
| Can we reach Tauri's existing native webview? | `with_webview` supports downcasting to `tauri_runtime_wry::Webview`; its Linux `inner()` returns a cloned `webkit2gtk::WebView`. Window APIs also expose the GTK window and default box. No runtime fork is indicated merely to obtain widgets. |
| Can the webview be layered over OpenGL? | Wry's exact-version GTK OpenGL example does so. Wry's normal Tauri path packs the webview in a GtkBox and lets GTK allocate it; its fixed-parent sizing path is separate. Reparenting the retained native widget into GtkOverlay is a plausible narrow integration. Resize, input and unrealize behavior still need runtime validation. |
| Does the pinned window runtime support native Wayland? | Tao 0.37.1 has Wayland window/display handle branches and obtains `wl_display` from GDK. Wry's GTK-container path is used by Tauri. The fact that Wry also enables `x11` is not proof that Tauri is X11-only. |
| Is our current mpv render binding sufficient for efficient hardware decoding? | Not fully. `create_render_context_gl` supplies the GL loader but no X11/Wayland display. It also lacks render-parameter constants 8 and 9. Upstream documents EGL plus a native display for Intel/Linux direct decoding. Extend the binding with platform display data and explicit lifetime requirements before claiming hardware-decoding parity. |
| Can GTK framebuffer 0 be assumed? | No. GtkGLArea owns a framebuffer. Use the active draw framebuffer and physical viewport dimensions from the current context. Creation/render/destruction must follow that context's lifetime. |
| Can we use the existing cookies API? | Wry implements Linux cookies through WebKit's cookie manager and a nested GTK iteration loop. The API exists; responsiveness/reentrancy under the app's timeout wrapper still need testing. The existing 5-second identity wait for absent Chromium client hints also needs a Linux-specific completion condition. |
| Does app-update publishing already support Linux? | No. In addition to the client gaps, `scripts/publish-update.mjs` permits only Windows EXE and Mac DMG assets. Future Linux updates need a deliberate signed-channel contract and publisher extension, with compatibility tests. No publisher was executed. |
| Do dependencies resolve for Linux? | Yes. `cargo tree --locked --offline --target x86_64-unknown-linux-gnu -p anilili-desktop -e features --depth 1` resolves. It confirms the missing Linux keyring backend. This is **dependency resolution**, not compilation/linking. |

The highest remaining provider uncertainty is challenge acceptance by the actual Linux
WebKit browser. A bridge request uses the correct browser engine, but success is controlled
by each remote site's challenge policy. Neither matching HTTP headers nor supported JavaScript
APIs proves a site will accept it. Test one normal cached resolution on Linux; do not infer
provider parity from the video experiment.

### This PC

- WSL is present, but its runnable distribution inventory lists only `docker-desktop`.
- A registry entry named Ubuntu remains, pointing at the standard Ubuntu package's LocalState
  directory. That directory/package was not found in the checked standard locations, and
  `wsl --distribution Ubuntu --exec uname -a` reports `WSL_E_DISTRO_NOT_FOUND`.
- Docker Desktop's files are installed, but startup fails because its installation registry
  entry is missing. The Docker WSL launch separately fails because its `ext4.vhdx` path is missing.
- Hyper-V's CIM inventory reports the host and no guest VMs. Direct access to its protected
  configuration directory was denied, so that directory was not inspected. No VirtualBox/VMware
  installation or standard user VM folders were found.
- Windows reports AMD Radeon graphics and an NVIDIA RTX 4060. A future native Linux run on this
  machine would be useful for hybrid-GPU testing; WSLg results alone would not prove native
  VA-API/NVIDIA interop.

**No usable local Linux environment was confirmed.** These findings do not exclude an
unregistered VM disk stored elsewhere. No WSL registration, registry setting or OS installation
was changed, and no Docker/WSL distribution was deleted.

### Prepared experiment

`experiments/linux-playback-probe/` is a separate Cargo workspace using the exact Tauri/Wry
versions and this repository's dynamic mpv binding. It reparents the managed webview above
GtkGLArea, renders a local video with software decoding, and offers pause/seek/fullscreen and
side-panel checks. It also reports the webview user agent and gzip CompressionStream support.
It uses a separate app identifier and has no provider or update access.

Verification completed here: Rust formatting/parser checks, Node JavaScript syntax checks,
Cargo manifest parsing and Linux dependency graph resolution. **Linux type-checking, linking,
picture/input validation and hardware decoding remain unverified.** The experiment is a runnable
test candidate, not a validated backend. Its README defines the run commands and acceptance checks.

Next gate: provide a working Linux installation, a verified LGPL libmpv shared library and a
local video; compile and run the probe on X11 and native Wayland. If it passes, integrate the
same layer into production before doing broader packaging work. Keep the original 3–5 week
estimate provisional until those runs and the first browser-backed provider resolution pass.

Additional primary references:

- [Tauri 3 runtime documentation](https://v3.tauri.app/develop/webview-runtime/) — Wry/system-webview architecture.
- [GtkGLArea](https://docs.gtk.org/gtk3/class.GLArea.html) — context and framebuffer ownership.
- [mpv OpenGL render API](https://raw.githubusercontent.com/mpv-player/mpv/master/include/mpv/render_gl.h) — Linux decoding interop requirements.
- [mpv render parameters](https://raw.githubusercontent.com/mpv-player/mpv/master/include/mpv/render.h) — native display types/lifetimes and threading rules.
- [WebKit content filter store](https://webkitgtk.org/reference/webkit2gtk/stable/class.UserContentFilterStore.html) — compiled JSON filters, distinct from the existing per-request Brave engine.
- [Cloudflare browser support](https://developers.cloudflare.com/cloudflare-challenges/reference/supported-browsers/) — challenge support depends on browser behavior and policy.
