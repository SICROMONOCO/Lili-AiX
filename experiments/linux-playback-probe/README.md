# Linux playback probe

**Experimental and not yet compiled or run on Linux.** This isolates the highest-risk port
question: can the project's pinned Tauri/Wry webview be reparented above a GTK OpenGL video
surface while keeping native commands, transparency, input and resizing?

It uses the existing `crates/mpv` binding, software decoding, a local video and a separate
app identifier. It has no production data, provider requests or update functionality.
No production source changes are necessary to run it. The nested Cargo workspace avoids
compiling Wasmtime and BoringSSL for this first experiment.

Install the pinned Rust 1.99.0 toolchain and Linux development dependencies, including
`build-essential`, `pkg-config`, `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `libayatana-appindicator3-dev`
and OpenGL/EGL libraries on Ubuntu. Dependency names differ on other distributions.
Supply an independently verified **LGPL** libmpv `.so` and its shared-library dependencies.
Do not substitute an unverified distribution/GPL build. Use a local video you already have.

From this directory, on a Linux desktop:

```bash
export ANILILI_PROBE_LIBMPV=/absolute/path/to/lgpl/libmpv.so.2
export ANILILI_PROBE_VIDEO=/absolute/path/to/test-video.mp4
cargo run
```

Repeat under X11/XWayland (`GDK_BACKEND=x11 cargo run`) and native Wayland
(`GDK_BACKEND=wayland cargo run`) where those backends are available. A Wayland desktop
running an X11 client through XWayland is not evidence of native Wayland support.

Check:

1. Picture appears beneath the translucent bars; controls receive clicks and heartbeat keeps moving.
2. Play/pause and seek commands work. Resize repeatedly, toggle fullscreen and move between displays.
3. Side panel moves video clear of it. Test a video with subtitle tracks and mixed DPI.
4. Close/reopen cleanly; inspect stderr for GL, mpv, widget or lifetime errors.
5. Read the displayed WebKit user agent, CSS/clipboard API presence and CompressionStream result.
   API presence does not establish clipboard permission or correct rendering of the production UI.

Record distribution, session/backend, GPU/driver, GTK/WebKit versions, mpv build/version,
observations and logs. Capture **only the probe window** if visual evidence is collected.

Limitations: this does not test hardware decoding, provider clearance, embedded server codecs,
production React/CSS performance or packaging. The existing render binding omits native display
parameters; hardware decoding is deliberately disabled. The probe does not supply presentation
feedback because a GTK render callback is not proof of physical presentation. It uses mpv's
nonblocking render path and audio timing. A passing run justifies integrating a Linux backend;
it does not establish full feature parity.

Synchronous mpv client commands and property reads run on workers, separate from GTK drawing.
The mpv callback only marks a dirty flag; GTK's frame clock schedules the actual render. This
follows the upstream render API's threading rules. This design has source review only until
the Linux executable is compiled and exercised.
