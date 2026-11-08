{
  description = "Anilili Desktop - Linux packaging";

  # Pinned to the same nixpkgs-unstable revision used by the working
  # nixos-config package. Rust 1.99 is required by rust-toolchain.toml;
  # nixos-26.05 only ships 1.95.
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/5adc50831c259f87faeb09ddf930d43597197473";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs:
        let
          pkg = pkgs.callPackage ./default.nix { };
        in
        {
          default = pkg;
          anilili-desktop = pkg;
        });

      devShells = forAllSystems (pkgs:
        let
          gstPlugins = with pkgs.gst_all_1; [
            gstreamer
            gst-plugins-base
            gst-plugins-good
            gst-plugins-bad
            gst-libav
          ];
          runtimeLibs = with pkgs; [
            webkitgtk_4_1
            gtk3
            libsoup_3
            openssl
            glib
            glib-networking
            librsvg
            gdk-pixbuf
            mpv-unwrapped
          ];
        in
        {
          default = pkgs.mkShell {
            packages = (with pkgs; [
              cargo
              rustc
              cargo-tauri
              nodejs
              pnpm
              pkg-config
              wrapGAppsHook3
              gobject-introspection
              gtk3
              webkitgtk_4_1
              libsoup_3
              openssl
              glib
              glib-networking
              librsvg
              webp-pixbuf-loader
              gdk-pixbuf
              libappindicator-gtk3
              alsa-lib
              mpv-unwrapped
              cmake
              ninja
              go
              nasm
              perl
              git
            ]) ++ gstPlugins;

            shellHook = ''
              export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
              export GST_PLUGIN_SYSTEM_PATH_1_0="${pkgs.lib.makeSearchPath "lib/gstreamer-1.0" gstPlugins}''${GST_PLUGIN_SYSTEM_PATH_1_0:+:$GST_PLUGIN_SYSTEM_PATH_1_0}"
              export GIO_EXTRA_MODULES="${pkgs.glib-networking}/lib/gio/modules''${GIO_EXTRA_MODULES:+:$GIO_EXTRA_MODULES}"
              export GDK_PIXBUF_MODULE_FILE="${pkgs.librsvg}/lib/gdk-pixbuf-2.0/2.10.0/loaders.cache"
            '';
          };
        });

      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.programs.desktoprepo1;
          package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        in
        {
          options.programs.desktoprepo1 = {
            enable = lib.mkEnableOption "the Anilili desktop client";
            package = lib.mkOption {
              type = lib.types.package;
              default = package;
              description = "The Anilili package to install.";
            };
          };

          config = lib.mkIf cfg.enable {
            environment.systemPackages = [ cfg.package ];
            programs.dconf.enable = true;
            xdg.mime.defaultApplications."x-scheme-handler/anilili" = "anilili.desktop";
          };
        };
    };
}
