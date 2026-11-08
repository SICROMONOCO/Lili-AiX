# Standalone Nix package for Anilili Desktop.
#
#   nix-build default.nix
#   nix build .#default
#
# Upstream ships package-lock.json only, so npm is the authoritative package
# manager here. pnpm is available in the flake's devShell, but using
# pnpmConfigHook would require a second, unmaintained lockfile.
{
  lib,
  stdenv,
  rustPlatform,
  fetchNpmDeps,
  npmHooks,
  nodejs,
  pkg-config,
  wrapGAppsHook3,
  copyDesktopItems,
  makeDesktopItem,
  cmake,
  ninja,
  go,
  nasm,
  perl,
  git,
  openssl,
  gtk3,
  webkitgtk_4_1,
  libsoup_3,
  glib,
  glib-networking,
  dbus,
  libappindicator-gtk3,
  alsa-lib,
  mpv-unwrapped,
  gst_all_1,
  librsvg,
  webp-pixbuf-loader,
  gdk-pixbuf,
}:

let
  version = "0.1.6";
  src = ./.;

  # tauri::generate_context! embeds the frontend at compile time, so Vite must
  # finish before Cargo compiles the application.
  frontend = stdenv.mkDerivation {
    pname = "anilili-desktop-frontend";
    inherit version src;

    npmDeps = fetchNpmDeps {
      inherit src;
      hash = "sha256-vvZEN2faQBGEVGOz/0ARieLWtvC/c6/I2R2jjXp6Aq8=";
    };

    nativeBuildInputs = [
      nodejs
      npmHooks.npmConfigHook
    ];

    buildPhase = "npm run build";
    installPhase = "mkdir -p $out && cp -r dist $out/";
  };

  gstPlugins = with gst_all_1; [
    gstreamer
    gst-plugins-base
    gst-plugins-good
    gst-plugins-bad
    gst-libav
  ];
in
rustPlatform.buildRustPackage {
  pname = "anilili-desktop";
  inherit version src;

  cargoDeps = rustPlatform.importCargoLock {
    lockFile = ./Cargo.lock;
  };

  cargoBuildFlags = [
    "-p"
    "anilili-desktop"

    # Tauri's custom-protocol feature embeds frontendDist. Without it
    # tauri::is_dev() stays true and generate_context! bakes in devUrl
    # (localhost:5173), producing a connection-refused window.
    "--features"
    "tauri/custom-protocol"
  ];

  # ninja is only needed by BoringSSL's CMake build; its setup hook must not
  # take over Cargo's build/check/install phases.
  dontUseNinjaBuild = true;
  dontUseNinjaCheck = true;
  dontUseNinjaInstall = true;

  # These tests read host machine-id/DMI/locale, which do not exist in the
  # Nix build sandbox.
  checkFlags = [
    "--skip=usage::tests::this_machine_has_an_id"
    "--skip=usage::tests::this_machine_describes_itself"
  ];

  nativeBuildInputs = [
    gdk-pixbuf
    pkg-config
    wrapGAppsHook3
    copyDesktopItems
    cmake
    ninja
    go
    nasm
    perl
    git
    nodejs
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    openssl
    gtk3
    webkitgtk_4_1
    libsoup_3
    glib
    glib-networking
    dbus
    libappindicator-gtk3
    alsa-lib
    mpv-unwrapped
    librsvg
    webp-pixbuf-loader
    gdk-pixbuf
  ] ++ gstPlugins;

  preBuild = ''
    cp -r ${frontend}/dist ./dist

    # btls-sys builds the BoringSSL vendored in its crate. The store is
    # read-only, so make the copied tree writable before git applies patches.
    substituteInPlace scripts/prepare-boringssl.mjs \
      --replace-fail 'cpSync(join(sysDir, "deps", "boringssl"), outDir, { recursive: true })' \
                     'cpSync(join(sysDir, "deps", "boringssl"), outDir, { recursive: true }); execFileSync("chmod", ["-R", "u+w", outDir])'

    node scripts/prepare-boringssl.mjs
  '';

  # libmpv is dlopen()ed and GStreamer is used by WebKitGTK's embed player, so
  # both must be on the runtime loader paths. wrapGAppsHook3 supplies the
  # dconf GIO module; append glib-networking so TLS-backed remote images work
  # without dropping dconf.
  preFixup = ''
    # Build one GDK pixbuf cache containing the core, SVG and WebP loaders.
    # gdk-pixbuf-query-loaders takes explicit module paths; a colon-separated
    # GDK_PIXBUF_MODULEDIR produces an empty cache.
    GDK_PIXBUF_MODULE_FILE="$out/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
    mkdir -p "$out/lib/gdk-pixbuf-2.0/2.10.0"
    gdk-pixbuf-query-loaders \
      $(find \
        "${gdk-pixbuf}/lib/gdk-pixbuf-2.0/2.10.0/loaders" \
        "${librsvg}/lib/gdk-pixbuf-2.0/2.10.0/loaders" \
        "${webp-pixbuf-loader}/lib/gdk-pixbuf-2.0/2.10.0/loaders" \
        -maxdepth 1 -name '*.so' -print) \
      > "$GDK_PIXBUF_MODULE_FILE"

    gappsWrapperArgs+=(
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [ mpv-unwrapped ]}"
      --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "${lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins}"
      --set-default __NV_DISABLE_EXPLICIT_SYNC 1
      --prefix GIO_EXTRA_MODULES : "${glib-networking}/lib/gio/modules"
      --set GDK_PIXBUF_MODULE_FILE "$GDK_PIXBUF_MODULE_FILE"
    )
  '';

  desktopItems = [
    (makeDesktopItem {
      name = "anilili";
      desktopName = "Anilili";
      exec = "anilili-desktop %U";
      icon = "anilili";
      comment = "Anilili desktop client";
      categories = [ "AudioVideo" "Video" ];
      mimeTypes = [ "x-scheme-handler/anilili" ];
      startupNotify = true;
    })
  ];

  postInstall = ''
    install -Dm644 src-tauri/icons/32x32.png $out/share/icons/hicolor/32x32/apps/anilili.png
    install -Dm644 src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/anilili.png

    # Tauri's resource directory on Linux is ../lib/<productName> relative to
    # bin/. The vendor tree is generated (gitignored) and absent in a clean
    # checkout; install it when the caller fetched it first.
    for dir in shaders filters providers; do
      if [ -d "vendor/$dir" ]; then
        install -d "$out/lib/Anilili"
        cp -r "vendor/$dir" "$out/lib/Anilili/"
      fi
    done
  '';

  meta = with lib; {
    description = "Anilili desktop client (Linux port)";
    homepage = "https://github.com/sanilili/desktoprepo1";
    license = licenses.mit0;
    platforms = [ "x86_64-linux" ];
    mainProgram = "anilili-desktop";
  };
}
