# Linux port: source and dependency audit

2026-10-07, desktop 0.1.2. This supersedes broad confidence statements in the first
[feasibility study](LINUX-FEASIBILITY.md). The architecture supports a plausible port;
**the current application is not Linux-ready, and no Linux compile or runtime pass is claimed.**
The user will provide a working Linux environment for the final execution checks.

## Scope and evidence

The repeatable inventory, `node scripts/linux-port-audit.mjs`, currently covers 225 source,
configuration, script, fixture and SVG files across the frontend, native shell, core, mpv,
providers and experiment. It maps all **95 registered native commands** to declarations,
records 132 platform/process/desktop-UI references, and checks local TypeScript import spelling
against actual directory entries. Its evidence file is `target/linux-audit/source-audit.json`,
including source hashes and line locations. Generated/build folders, downloaded third-party
code, binary assets, secrets and documentation are excluded from that automated scan.

This is a coverage inventory and a targeted manual review of the portability-sensitive paths,
not a claim that every algorithm or third-party source line has been reviewed. Additional
manual reads cover the exact locked dependencies: Tauri alpha.4, runtime alpha.4, Wry 0.57.0,
Tao 0.37.1, gtk-rs 0.18.2, WebKit bindings 2.0.2, keyring 3.6.3, souvlaki 0.8.3,
discord-rich-presence 1.1.0 and btls-sys 0.5.6. Upstream documents are cited where API contracts
matter. No live provider resolution, account mutation, publication or installation was performed.

Observed checks on this Windows host:

- Frontend TypeScript/production build passed; Vite emitted its existing large-chunk advisory.
- Core library tests: **77 passed, 1 ignored** (the live DNS test was not run).
- mpv library version parser test: **1 passed**. It does not validate Linux loading/rendering.
- Desktop native unit tests: **25 passed**. Provider-kit tests: **9 passed**, including the
  shared title/season matching fixture. Total here: **112 passed, 1 ignored**, all on Windows.
- Frontend ESLint passed (`npx eslint src`).
- Linux dependency graph resolves with the committed lockfile, offline.
- Probe Rust formatting/parser, manifest/dependency resolution and JavaScript syntax checks pass.
- Linux preflight shell script passes Bash syntax checking and rejects a non-Linux host.
- GNU tar 1.35 on this host refuses a generated ZIP with exit code 2, reproducing the shader
  extraction problem without downloading or modifying shipped shader resources.

The existing changes in `src-tauri/src/catalog.rs` and `src/pages/anime.tsx` belong to the
working tree before this investigation and were not edited by this audit.

## Confirmed missing pieces and implementation requirements

| ID | Finding and source | Required treatment before a Linux beta |
| --- | --- | --- |
| L01 | `player.rs:158–182, 433–440`: startup requests a Win32/AppKit handle; Linux is rejected. The fallback video option is `vo=null`. | Introduce a Linux backend independent of HWND; an opening catalog window is not a playback pass. |
| L02 | `player.rs:465–489`, `fetch-libmpv.mjs:22–59`: Linux has no library pin, lookup chooses `.dylib`, fetcher refuses `linux-x64`. | Select/build and pin a Linux LGPL `.so` with provenance, correct paths, ABI 2.x and all required shared dependencies. |
| L03 | `tauri.conf.json`: NSIS plus Windows DLL resources; no Linux override. | Linux packaging config; inherited Windows resources must be removed explicitly. Keep providers, shaders, lists and icons. |
| L04 | `crates/mpv/src/ffi.rs:83–89`, `lib.rs:352–367`: GL render init has no X11/Wayland display parameters. | Add optional typed platform display data with lifetime guarantees for direct decoding paths; do not call software playback hardware-decoding parity. |
| L05 | Production `player_load`/`player_control` are main-thread synchronous client operations. Putting GTK drawing on that thread introduces a new render/client interaction. | Choose a command queue or async client API that preserves ordering and settings semantics while keeping blocking mpv client work separate from rendering. See the upstream threading contract below. |
| L06 | `core/update.rs:57` selects Windows for non-Mac targets; `updates.rs:72,107–111` retains EXE/DMG names and Windows installer flags. Publisher permits Windows/Mac only. | Explicit Linux/architecture selection and package-specific update ownership, or disable app updates on Linux until implemented. Preserve signed provider updates. |
| L07 | `notify.rs:54–57,67–68`: Linux logs rather than posts notifications; clear-group does nothing. | Desktop D-Bus implementation with replacement IDs, route actions, capability checks and graceful absence. |
| L08 | `adblock.rs:113–116`: the Linux request hook is a no-op. `embed-player.tsx:53` says “Popups and ads blocked” unconditionally. | Implement a documented Linux filter path or accurately expose limited filtering. Existing popup/navigation guards do not establish ad-request filtering. |
| L09 | Keyring features enable Windows and Apple stores only. Locked keyring defaults to its mock implementation on Linux. | Persistent Secret Service backend; handle locked/missing service and verify password survival across processes. Never accept mock storage as a successful persistent login. |
| L10 | `library.rs:656–662`: Linux browser opening does no work and returns success. `downloads.rs:543–548,693–696` chooses macOS `open`. | Linux browser/folder openers; FileManager1 selection with a folder fallback where unavailable. |
| L11 | `media.rs:34–39`: MPRIS-capable dependency is present, but startup occurs after the failing HWND lookup. | Start MPRIS independently with Linux configuration and test a real session bus. Missing controls should not prevent playback. |
| L12 | `fetch-shaders.mjs:29–30` extracts a ZIP using generic `tar` on non-Windows hosts. | Use a ZIP-capable extractor/library, declare the prerequisite and preserve checksum checking. GNU tar failure was reproduced. |
| L13 | `dev-go.sh`, `dev-kill.sh` treat every non-Mac host as Windows and invoke PowerShell/cygpath/taskkill/netstat conventions. | Add explicit Linux development helpers or document alternatives. Capture only the app/probe window. |
| L14 | `package.json:22,37`: Tauri JS API and CLI have caret ranges. Lockfile currently resolves the intended alpha versions. | Exact npm specs as the repository requires; use locked installs. Avoid a platform migration silently refreshing alpha packages. |
| L15 | `main.rs` does not enable clipboard access. Locked Tauri defaults it false; Wry's Linux setting is conditional. UI uses clipboard read/write and catches paste failures as empty text. | Verify actual user-gesture permission behavior; enable/manage access for the trusted main view or use a supported native path. Do not infer permission from API presence. |
| L16 | `player.rs:924–933`, `lib/captions.ts`: non-Mac subtitle families are Windows fonts. | Choose available Linux families or documented Fontconfig substitutions; test Latin, Arabic and CJK subtitles and preview consistency. The main UI's Geist font is bundled. |
| L17 | Settings notification text and folder placeholder reference Windows. | Platform-neutral or accurate Linux copy when those features are enabled. |

These are source-supported gaps. They are not all compilation errors: several compile into a
silent no-op or a runtime error, which is why compiling alone would not establish a complete port.

## Runtime contracts checked against locked sources

### Video, display systems and lifecycle

Locked Tauri exposes a native WebKitGTK widget through `with_webview` and runtime downcasting.
The Linux runtime uses GTK 3; WebKitGTK **API 4.1 is not GTK 4**. Wry's exact-version
`gtk_opengl` example demonstrates the desired overlay arrangement. Wry normally attaches
Tauri's webview to a GtkBox and uses GTK allocation; its fixed-parent path is separate.
Reparenting is a candidate integration, with input, sizing, focus and teardown still untested.

Tao explicitly implements both X11 and Wayland handles. Its position getter returns tracked
coordinates; it does not necessarily raise an unsupported-operation error. GTK explains that
Wayland has no global window coordinates, and that keep-above is a request the window manager
may ignore. Thus `use-mini-player.ts:22–40` cannot promise exact position restoration and
always-on-top behavior everywhere. This is a **desktop limitation to accommodate**, not evidence
that all mini-player entry must fail. [GTK positioning](https://docs.gtk.org/gtk3/method.Window.get_position.html),
[keep-above](https://docs.gtk.org/gtk3/method.Window.set_keep_above.html).

mpv's render API requires serialized render calls with the same current GL context, context
creation before video loading, and destruction before the core. Blocking client work must not
depend on the render thread. The probe initially violated that last rule; it now loads and handles
client commands/property reads on workers. Production needs its own integration preserving
ordering. [mpv render contract](https://raw.githubusercontent.com/mpv-player/mpv/master/include/mpv/render.h).

GtkGLArea owns its FBO; use the active draw FBO and physical viewport rather than assuming 0
or multiplying logical sizes blindly. Efficient Linux decoding has additional EGL/native-display
requirements. None of the software probe, a successful CLI mpv run, or a WSLg run establishes
native Linux GPU interop, HDR behavior or power efficiency. [GtkGLArea](https://docs.gtk.org/gtk3/class.GLArea.html),
[mpv GL interop](https://raw.githubusercontent.com/mpv-player/mpv/master/include/mpv/render_gl.h).

Test minimize/hide, context unrealize/recreate, resize, fullscreen, side panels, monitors,
suspend/resume and cleanup. There is no explicit Linux inhibition integration in the app;
do not assume `vo=libmpv` prevents display sleep. A desktop/portal inhibit path may be needed.
[Inhibit portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Inhibit.html).

### Browser, UI and providers

The main navigation guard already accepts the app's `tauri`/`asset` schemes and localhost;
it is not hard-coded solely to Windows's URL form. Popup denial is registered in native code.
Account windows intercept AniList/MAL localhost redirects through navigation callbacks.
Those callback APIs exist in the locked Linux runtime, but successful OAuth still needs a real
WebKit session, including cookie behavior and certificate redirects.

The Linux-resolved wreq feature set includes `webpki-roots`, and its default trust path uses
that bundled root set. Browser and FFmpeg HTTPS are separate stacks; a wreq request passing
does not prove their certificates, headers or streaming requests work.

Linux cookie APIs exist in Wry and use a nested GTK loop. Timeout, cancellation and reentrancy
need execution checks. The bridge uses in-page fetch, gzip CompressionStream and top-level
navigation to localhost; its Chromium client-hints identity completion waits up to five seconds
on engines with no `sec-ch-ua`. Clearance replay always selects an Edge TLS profile. A WebKit
user agent does not prove matching wire identity or challenge acceptance.

Tailwind v4's browser floor is newer than merely being able to link Wry's minimum WebKit ABI.
Do not equate “WebKitGTK >=2.38 links” with “our generated CSS works.” The probe now reports
OKLCH, color-mix, backdrop-filter, :has, registered-property and clipboard API presence; the
production React UI still needs visual/input checks. [Tailwind browser requirements](https://tailwindcss.com/docs/compatibility).

Direct providers have no host OS APIs in their Rust sources. Rift's Android identity fields are
service-protocol values, not an Android host requirement. WASM components use the shared WIT
contract, and the native host has its own platform-specific compiled cache. This establishes
structural reuse, not provider success. Test representative HLS, MP4, DASH and browser-backed
sources once using normal caching. Keep Rift's serialization/rate limits; do not stress its registration.

Iframe playback uses WebKit's codec/GStreamer stack, independently of libmpv. It needs its own
media test and package resources. Compiled WebKit content filters accept JSON rules; that is a
different interface from evaluating every request with our Brave filter engine. Exact filter parity
remains an implementation decision, not a proven translation. [WebKit filter API](https://webkitgtk.org/reference/webkit2gtk/stable/class.UserContentFilterStore.html).

### Accounts, data and desktop services

SQLite, HLS proxy, downloads, backup crypto, recommendations and link parsing have no
Windows-only runtime calls in the reviewed core paths. Core tests pass here, including shared
backup/link/spelling fixtures. UNIX free-space checking already uses `statvfs` and available
blocks. Recheck filesystem permissions, actual disk reserves, subtitle sidecars and interrupted
downloads on Linux. A Windows success is not a Linux filesystem pass.

Backups deliberately omit the machine's download directory and local background image.
The suspected cross-platform path transfer issue does **not** exist in that backup code.
App data/cache/resources use Tauri paths; logs have an XDG fallback. Pictures/Downloads
availability, Unicode paths and a writable install/data environment remain runtime checks.

Locked Discord IPC code searches XDG/temp directories and common Flatpak/Snap client socket
locations. Locked souvlaki implements MPRIS over D-Bus. These dependency implementations exist;
they still require live desktop services. Notification actions/replacement are capability-dependent.
[Desktop notification specification](https://specifications.freedesktop.org/notification/latest-single/).

Download taskbar progress is limited: Tao 0.37.1 loads libunity and checks for a Unity session.
The application does not supply a desktop filename with its progress state. Do not promise a
GNOME/KDE dock progress bar; in-app queue progress remains available. Linux deep-link registration
code exists and uses desktop files, `update-desktop-database` and `xdg-mime`; installation/default
handler behavior needs package-level tests. [FileManager1](https://wiki.freedesktop.org/www/Specifications/file-manager-interface/).

### Build, binary closure and release ownership

Linux dependency resolution includes libdbus-sys via both souvlaki and Tao, in addition to
GTK/WebKitGTK/libsoup. It requires development packages; a clean compile must exercise their
pkg-config probes. Native SQLite is bundled. btls-sys builds the prepared BoringSSL source;
the Linux toolchain needs C/C++, CMake, Go, Clang/libclang for bindgen, and appropriate assembly
tools. The Node preparation script has no inspected Windows-only path; that does not prove
the patched BoringSSL compiles on Linux.

Linux npm native bindings for the locked Tauri CLI, Rolldown and Tailwind are present in
`package-lock.json`. The frontend production build passes here, but a clean Linux `npm ci`
still needs verification. Use the project's exact Rust toolchain and Vite's declared Node range
(`^20.19.0 || >=22.12.0` in the installed package). Do not copy Windows node_modules, target output or
compiled WASM caches to the Linux machine.

No verified Linux LGPL libmpv archive/pin has been selected. mpv upstream supports an LGPL
build with `-Dgpl=false`; mpv v0.41.0's options include `libmpv` and `cplayer`. FFmpeg and
optional dependency choices must remain compatible. A checked, pinned self-build is an available
route, but its produced library, dependency closure, ABI and streaming features still require
execution checks. Do not infer the license from a distribution package name.
[mpv build options](https://raw.githubusercontent.com/mpv-player/mpv/v0.41.0/meson.options),
[FFmpeg build requirements](https://www.ffmpeg.org/legal.html).

Bundlers can miss a library loaded through dlopen. Inspect the supplied libmpv's NEEDED entries,
each dependency's own RPATH/RUNPATH and the packaged resource locations. Verify on a clean
machine without the development libraries or vendor checkout. Avoid assuming the executable's
RUNPATH will find every transitive dependency. AppImage is not a universal ABI/GPU compatibility
guarantee, and webview GStreamer bundling does not supply mpv automatically. Any bundled codec
modules need their own dependency/license inventory; an LGPL mpv build alone does not establish
the provenance of the webview's media stack.
[Tauri AppImage guidance](https://tauri.app/distribute/appimage/).

The repository remains private, local builds only. Any Linux installer eventually belongs to the
existing release, under separate explicit release authorization. No public source CI, fork,
alternate remote, update publication or installer upload is part of this investigation.

## Handoff and remaining work

- [Linux test matrix](LINUX-TEST-MATRIX.md) specifies compilation, probe and production acceptance gates.
- `bash scripts/linux-preflight.sh /absolute/path/to/verified/libmpv.so` collects local prerequisites
  without installation, account/provider calls or upload. It does not establish LGPL provenance.
- `experiments/linux-playback-probe/README.md` describes the isolated source-reviewed probe.

The source-supported implementation backlog is identified across the audited feature areas.
Native type-checking/linking, binary provenance/closure and runtime behavior remain open. The
earlier 3–5 week estimate is **provisional**, not a schedule established by this audit. A successful
Linux compile, playback probe and normal browser-backed resolution are the first evidence gates.
