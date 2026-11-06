# Anilili Desktop — NixOS Runtime Diagnostics

Date: 2026-11-06

Scope: diagnose the Linux runtime failures around remote image loading and
related media/resource paths. This report is diagnostic only: no application
source, Tauri configuration, CSP, or adblock changes were made.

The artifact inspected was the previous Nix package built from `e1c84db`:

```
/nix/store/qxmq02rq47ml6x74rc69gzn7qkp6b3v2-anilili-desktop-0.1.6
```

That build included the Tauri `custom-protocol` fix, so it is the correct
baseline for runtime behavior.

## Bottom line

1. **The tested wrapper has no `glib-networking` in its runtime closure.**
   `GIO_EXTRA_MODULES` contains only dconf. WebKitGTK/libsoup use GIO's TLS
   backend for HTTPS; without `glib-networking`, HTTPS page and image requests
   from the webview fail. This is the most likely packaging cause of broken
   remote images when the app's own Rust HTTP client still works.
2. **The GDK pixbuf cache has no WebP or AVIF loader.** The wrapper points
   `GDK_PIXBUF_MODULE_FILE` at librsvg's cache, which contains the default
   loaders plus SVG, but no `webp`/`avif` entries. `webp-pixbuf-loader` is not
   in the closure. This affects GDK-decoded images (tray, notifications, file
   icons); WebKit content images use WebKit's own decoders.
3. **CSP is not the cause.** `tauri.conf.json` has `app.security.csp: null`,
   `index.html` has no CSP meta tag, and Tauri 3 alpha has no v1 `allowlist`.
   Nothing is blocking `https:`, `asset:`, `data:`, or `blob:` images in the
   current configuration.
4. **Bundled resources are missing from the package.** The output contains
   only `bin/` and `share/`; there is no `lib/Anilili` resource directory and
   the clean clone has no `vendor/` tree. Filters, providers, and Anime4K
   shaders are therefore unavailable at first run. This does not explain the
   remote image failure, but it is a real functional gap.
5. **The API `DNET-07` seen in one test is separate.** Shell HTTPS to AniList
   and TMDB worked during this diagnosis. The app-level failure needs a live
   retest before it is treated as a root cause.

## Evidence

### Runtime wrapper

`bin/anilili-desktop` is a `makeBinaryWrapper` launcher. Its embedded arguments
contain:

```
GIO_EXTRA_MODULES=:...-dconf-0.49.0-lib/lib/gio/modules
GDK_PIXBUF_MODULE_FILE=...-librsvg-2.62.3/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache
GST_PLUGIN_SYSTEM_PATH_1_0=...gstreamer...gst-plugins-base...gst-plugins-good...gst-plugins-bad...gst-libav
LD_LIBRARY_PATH=...mpv-0.41.0/lib
__NV_DISABLE_EXPLICIT_SYNC=1
```

The actual binary is `bin/.anilili-desktop-wrapped`; it is wrapped by
`wrapGAppsHook3`.

### Missing TLS backend

The package closure contains no `glib-networking`:

```sh
nix-store -q --requisites /nix/store/qxmq02rq47ml6x74rc69gzn7qkp6b3v2-anilili-desktop-0.1.6 \
  | grep -E 'glib-networking|webp-pixbuf-loader'
# no output
```

`GIO_EXTRA_MODULES` lists only dconf, not a `glib-networking/lib/gio/modules`
path. WebKitGTK 2.54's closure was also checked and has no glib-networking.

Consequence: the webview's HTTPS stack has no GIO TLS module. Local UI can
render, and Rust-side HTTP (which uses BoringSSL through `wreq`/`btls-sys`) can
still work, while remote `<img src="https://...">` requests fail. That split is
the signature to look for when the shell UI loads but posters/banners do not.

### Pixbuf loaders

The cache at the wrapper's `GDK_PIXBUF_MODULE_FILE` contains:

- BMP, GIF, ICNS, ICO, PNM, QTIF, TGA, TIFF, XBM, XPM
- SVG (from librsvg)

It contains no WebP or AVIF loader:

```sh
grep -Ei 'webp|avif' /nix/store/8cqgh75f63kcsgskg9fswm4n15h97d1g-librsvg-2.62.3/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache
# no output
```

`libwebp` and `libavif` are present as libraries (also in WebKitGTK's closure),
but the separate `webp-pixbuf-loader` package is absent. GDK therefore cannot
decode WebP/AVIF through the pixbuf API on this artifact.

WebKit's own closure includes:

```
gdk-pixbuf-2.44.8
libavif-1.4.2
libjpeg-turbo-3.2.0
libpng-apng-1.6.59
libwebp-1.6.0
```

So WebKit content images can be decoded *if* the HTTPS request reaches the
decoder. Do not treat the pixbuf gap as the cause of every remote image failure;
it is a separate, confirmed packaging gap.

### CSP and Tauri protocol

`src-tauri/tauri.conf.json`:

```
app.security.csp = null
app.windows = []
build.frontendDist = "../dist"
bundle.targets = ["nsis"]
```

`index.html` contains no Content-Security-Policy meta element. The Tauri 3
alpha configuration has no Tauri v1 `allowlist`; the v1 `allowlist.protocol`
check does not apply. There is no `asset:` protocol registration and no CSP
`img-src` restriction in this artifact.

Conclusion: CSP, `asset:` scope, and allowlist are **not** the cause of the
image failure. Adding a restrictive CSP now would be a new change, not a
diagnosis.

### Resources and generated assets

The package output has no resource tree:

```sh
find /nix/store/qxmq02rq47ml6x74rc69gzn7qkp6b3v2-anilili-desktop-0.1.6 -maxdepth 2 -type d
# bin, share, share/icons, share/applications
```

The clean clone has no `vendor/` directory. `vendor/` is gitignored and is
generated by:

- `node scripts/fetch-shaders.mjs`
- `node scripts/fetch-filters.mjs`
- `node scripts/build-providers.mjs`

Runtime lookups that expect bundled resources:

- `src-tauri/src/adblock.rs`: `resource_dir()/filters`
- `crates/core/src/plugin.rs`: bundled `providers/`
- Anime4K shader loading under `vendor/shaders/`

The app can fetch updated filters/providers later, but first-run bundled
resources are absent. This is a functional gap for ad blocking, extra providers,
and Anime4K, not for decoding a remote JPEG in the webview.

Tauri's `bundle.resources` config is Windows-oriented
(`libmpv-2.dll`, `vulkan-1.dll`) and is not applied by this Nix recipe. The Nix
package installs the binary, desktop file, and icons only.

### Network check

From the host shell during diagnosis:

```
GET https://graphql.anilist.co        -> 404 (reachable; root GET is not an API call)
GET https://image.tmdb.org/...jpg      -> 200, image/jpeg
```

The earlier runtime log had:

```
Couldn't load the home page ... code=DNET-07 ... network error: error sending request
```

That is a Rust-side request failure, not a webview image-load failure. It may
have been transient, DNS/proxy-related, or app-specific. It should be retested
with the fixed package before being reported as the image root cause.

### GStreamer, tray, and graphics

- The wrapper's `GST_PLUGIN_SYSTEM_PATH_1_0` includes gstreamer, base, good,
  bad, and libav. GStreamer is not missing.
- `libappindicator-gtk3` is in the tested buildInputs; no tray-specific runtime
  test was performed, so tray behavior is unverified rather than diagnosed.
- `LD_LIBRARY_PATH` includes mpv, and the wrapper sets
  `__NV_DISABLE_EXPLICIT_SYNC=1`.
- No `WEBKIT_DISABLE_DMABUF_RENDERER` or `WEBKIT_DISABLE_COMPOSITING_MODE` is
  set. DMABUF/WebGL behavior on Wayland was not instrumented in this pass.
- The previous runtime screenshot showed the app UI and logs showed an Intel
  Mesa renderer; there is no evidence that hardware acceleration causes the
  image failure.

## Missing dependencies / environment / config

| Item | Status in tested artifact | Effect |
|---|---|---|
| `glib-networking` | missing from closure; `GIO_EXTRA_MODULES` only dconf | HTTPS webview requests, including remote images, cannot use GIO TLS |
| `webp-pixbuf-loader` | missing | GDK pixbuf cannot decode WebP |
| AVIF pixbuf loader | no loader entry | GDK pixbuf cannot decode AVIF |
| `librsvg` | present; SVG loader in pixbuf cache | SVG works through GDK |
| `GDK_PIXBUF_MODULE_FILE` | points at librsvg cache | default + SVG only; no WebP/AVIF |
| `CSP` | `null`; no meta CSP | not blocking images |
| Tauri v1 `allowlist` | absent in Tauri 3 alpha | not applicable |
| `asset:` protocol | not configured | not used by remote `<img>` |
| `vendor/filters` | absent | no first-run adblock lists |
| `vendor/providers` | absent | no bundled extra providers |
| `vendor/shaders` | absent | no bundled Anime4K shaders |
| GStreamer plugins | present | video stack is covered |
| mpv runtime | present via `LD_LIBRARY_PATH` | video playback covered |

## Prioritized bottlenecks

1. **`glib-networking` missing (P0).** Add it to `buildInputs` so
   `wrapGAppsHook3` appends its `lib/gio/modules` to `GIO_EXTRA_MODULES`.
   Rebuild, then verify the wrapper contains the GIO TLS module and test an
   HTTPS image in the webview.
2. **WebP/AVIF pixbuf loaders (P1).** Add `webp-pixbuf-loader` and ensure the
   generated `GDK_PIXBUF_MODULE_FILE` actually includes it; adding the package
   alone may not change the selected cache. Verify with
   `grep -Ei 'webp|avif'` on the final cache.
3. **Bundled resources (P1).** Run the shader/filter/provider fetchers before
   packaging and install `vendor/{shaders,filters,providers}` into the Tauri
   resource directory (`$out/lib/Anilili` for this product name). Verify
   `resource_dir()` resolves.
4. **API connectivity/DNS (P2).** Retest `DNET-07` with the fixed package.
   Capture whether it is DNS, proxy, or a transient network failure; do not
   conflate it with image decoding.
5. **Linux bundle metadata (P2).** `bundle.targets` is `["nsis"]` (Windows
   only). The Nix package bypasses Tauri bundling, which is acceptable, but the
   config should not be mistaken for a Linux installer definition.
6. **Wayland/hardware acceleration (P3).** Instrument WebKitGTK DMABUF/WebGL
   only if rendering artifacts remain after the TLS and loader fixes.

## Packaging changes made in this branch

`default.nix` now adds `glib-networking`, `librsvg`, `webp-pixbuf-loader`, and
`gdk-pixbuf` to the build, keeps the `tauri/custom-protocol` feature, and
conditionally installs `vendor/` resources. The flake exposes the package,
devShell, and `nixosModules.default`.

These are packaging changes, not application fixes. The new artifact still
needs a rebuild and runtime retest to confirm that `wrapGAppsHook3` adds the
GIO TLS module and that the pixbuf cache contains WebP.

## Verification performed

- `nix flake check --no-build`: all checks passed for `packages`, `devShells`,
  and `nixosModules`.
- `nix-store --realise` of the frontend derivation succeeded: npm dependencies
  installed from the pinned lock and `tsc -b && vite build` produced the
  expected `index.html` and assets.
- The full Rust package was not rebuilt in this pass. It is the same
  `buildRustPackage` recipe that previously produced
  `/nix/store/qxmq02rq47ml6x74rc69gzn7qkp6b3v2-anilili-desktop-0.1.6`, with the
  dependency additions above.
