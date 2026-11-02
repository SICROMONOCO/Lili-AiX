#!/usr/bin/env bash
# Builds the macOS libmpv into vendor/libmpv/darwin-arm64/ and darwin-x64/: one libmpv.dylib
# holding mpv 0.41 (gpu-next over Vulkan, MoltenVK onto a CAMetalLayer the app owns), FFmpeg 9
# with VideoToolbox, libplacebo, libass and the rest, all LGPL or more permissive.
#
# MPVKit (LGPL-3.0, pinned below) does the building: it fetches its own prebuilt static
# dependencies, builds FFmpeg and mpv with -Dgpl=false and its MoltenVK context patch, and we
# add ours (patches/mpv-moltenvk-host-layer.patch) after it. Its output is static libraries;
# this script links them into the one dylib the app loads at runtime (so it stays replaceable,
# as the LGPL asks), checks it is not a GPL build and depends only on the system, and signs it
# ad hoc. A release build signs it again with the app.
#
# The first run takes a while (FFmpeg and mpv, for both architectures). Later runs reuse
# MPVKit's build until the pin or our patch changes, and only relink.
#
# Needs: Xcode (full, for Swift), Homebrew. Installs meson, ninja, pkg-config and wget if
# missing. Never use Homebrew's libmpv or ffmpeg instead: both are GPL builds.
#
# Usage: bash scripts/build-libmpv-macos.sh [--force]   (--force rebuilds MPVKit's part too)
set -euo pipefail

# Tag 1.1.0-n9.0.2: mpv v0.41.0, FFmpeg n9.0.2, libplacebo 7.360.1, MoltenVK 1.4.2.
MPVKIT_REPO=https://github.com/mpvkit/MPVKit.git
MPVKIT_REV=98c3453e252922e3136422da3fbb2e341ed9edc8
# MPVKit's macOS deployment target; tauri.macos.conf.json's minimumSystemVersion matches it.
MIN_MACOS=12.0

if [ "$(uname -s)" != Darwin ]; then
    echo "build-libmpv-macos.sh runs on a Mac." >&2
    exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
patch="$root/patches/mpv-moltenvk-host-layer.patch"
work="$root/vendor/libmpv-build"
mpvkit="$work/MPVKit"
dist="$mpvkit/dist"
force=0
[ "${1:-}" = "--force" ] && force=1

sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
# What MPVKit's build depends on, and what the linked result depends on.
build_stamp="mpvkit=$MPVKIT_REV patch=$(sha "$patch")"
link_stamp="$build_stamp script=$(sha "$0")"

up_to_date=1
for arch in arm64 x86_64; do
    out="$root/vendor/libmpv/darwin-$([ "$arch" = arm64 ] && echo arm64 || echo x64)"
    if [ "$(cat "$out/BUILD-INFO.txt" 2>/dev/null | head -1)" != "$link_stamp" ]; then
        up_to_date=0
    fi
done
if [ "$up_to_date" = 1 ] && [ "$force" = 0 ]; then
    echo "libmpv is up to date in vendor/libmpv/darwin-*"
    exit 0
fi

xcrun --find swiftc >/dev/null 2>&1 || {
    echo "Swift is missing: install Xcode (the command line tools alone are not enough)." >&2
    exit 1
}
command -v brew >/dev/null || {
    echo "Homebrew is missing: https://brew.sh" >&2
    exit 1
}
for tool in meson ninja pkg-config wget; do
    command -v "$tool" >/dev/null || brew install "$tool"
done

# 1. MPVKit at the pinned revision, with our patch after its own.
mkdir -p "$mpvkit"
if [ "$(git -C "$mpvkit" rev-parse HEAD 2>/dev/null || true)" != "$MPVKIT_REV" ]; then
    git -C "$mpvkit" init -q
    git -C "$mpvkit" fetch -q --depth 1 "$MPVKIT_REPO" "$MPVKIT_REV"
    git -C "$mpvkit" checkout -q --force FETCH_HEAD
    rm -f "$work/build-stamp"
fi
cp "$patch" "$mpvkit/Sources/BuildScripts/patch/libmpv/0100-anilili-moltenvk-host-layer.patch"

# 2. MPVKit's build (LGPL: no enable-gpl), unless this exact one is done.
if [ "$force" = 1 ] || [ "$(cat "$work/build-stamp" 2>/dev/null)" != "$build_stamp" ]; then
    echo "Building FFmpeg and mpv through MPVKit (this takes a while)..."
    (cd "$mpvkit" && make build platform=macos)
    echo "$build_stamp" > "$work/build-stamp"
fi

sdk="$(xcrun --sdk macosx --show-sdk-path)"
toolchain_usr="$(dirname "$(dirname "$(xcrun --find swiftc)")")"
exports="$work/exports.txt"
echo "_mpv_*" > "$exports"
# zlib and libxml2 come from the system (linked below), which ships no .pc files for them; these
# stubs answer freetype2's, libbluray's and mpv's Requires without letting Homebrew's in.
system_pc="$work/system-pc"
mkdir -p "$system_pc"
printf 'Name: zlib\nDescription: system zlib\nVersion: 1.2.12\nLibs: -lz\n' > "$system_pc/zlib.pc"
printf 'Name: libxml2\nDescription: system libxml2\nVersion: 2.9.13\nLibs: -lxml2\n' > "$system_pc/libxml-2.0.pc"

# 3. One dylib per architecture.
for arch in arm64 x86_64; do
    out="$root/vendor/libmpv/darwin-$([ "$arch" = arm64 ] && echo arm64 || echo x64)"
    libmpv_a="$dist/libmpv/macos/thin/$arch/lib/libmpv.a"
    [ -f "$libmpv_a" ] || { echo "MPVKit built no $libmpv_a" >&2; exit 1; }

    # Only MPVKit's own .pc files: a Homebrew one (its ffmpeg is GPL) must never be picked up.
    pc_path="$(find "$dist" -path "*/macos/thin/$arch/lib/pkgconfig" -type d | paste -sd: -):$system_pc"
    # gmp too: FFmpeg's and hogweed's .pc say -lgmp without requiring gmp, so nothing gives its -L.
    libs="$(PKG_CONFIG_PATH="$pc_path" PKG_CONFIG_LIBDIR="$pc_path" pkg-config --static --libs mpv gmp)"
    # libmpv itself goes in whole (-force_load); the rest only as far as it is used.
    libs="$(printf '%s\n' $libs | grep -vx -- '-lmpv' | paste -sd' ' -)"

    echo "Linking libmpv.dylib for $arch..."
    rm -rf "$out"
    mkdir -p "$out"
    # shellcheck disable=SC2086 # $libs is a list of linker arguments.
    xcrun clang -dynamiclib -arch "$arch" -isysroot "$sdk" -mmacosx-version-min="$MIN_MACOS" \
        -install_name @rpath/libmpv.dylib \
        -Wl,-exported_symbols_list,"$exports" \
        -Wl,-force_load,"$libmpv_a" \
        $libs \
        -L"$toolchain_usr/lib/swift/macosx" -L"$sdk/usr/lib/swift" -Wl,-rpath,/usr/lib/swift \
        -framework AppKit -framework AudioToolbox -framework AVFoundation -framework Cocoa \
        -framework CoreAudio -framework CoreFoundation -framework CoreGraphics -framework CoreMedia \
        -framework CoreServices -framework CoreText -framework CoreVideo -framework Foundation \
        -framework IOKit -framework IOSurface -framework MediaPlayer -framework Metal \
        -framework QuartzCore -framework Security -framework VideoToolbox \
        -lbz2 -lc++ -lexpat -liconv -lresolv -lxml2 -lz \
        -o "$out/libmpv.dylib"

    # Checks: an LGPL build, and nothing outside the system. (Into a file first: under
    # pipefail, `strings | grep -q` fails when grep stops reading early, i.e. on a match.)
    strings "$out/libmpv.dylib" > "$work/strings-$arch.txt"
    if grep -q -- '--enable-gpl' "$work/strings-$arch.txt"; then
        echo "FFmpeg was configured with --enable-gpl: refusing to ship it." >&2
        exit 1
    fi
    grep -q -- '-Dgpl=false' "$work/strings-$arch.txt" || {
        echo "mpv's configuration does not say -Dgpl=false: refusing to ship it." >&2
        exit 1
    }
    foreign="$(otool -L "$out/libmpv.dylib" | tail -n +2 | awk '{print $1}' |
        grep -vE '^(/usr/lib/|/System/Library/|@rpath/libmpv\.dylib$)' || true)"
    if [ -n "$foreign" ]; then
        echo "libmpv.dylib links libraries outside the system:" >&2
        echo "$foreign" >&2
        exit 1
    fi
    codesign --force --sign - "$out/libmpv.dylib"

    # The licences the binary is under, and where it came from.
    for license in "$dist"/libmpv-*/LICENSE.LGPL "$dist"/libmpv-*/Copyright "$dist"/FFmpeg-*/COPYING.LGPLv3 \
        "$dist"/FFmpeg-*/COPYING.LGPLv2.1 "$dist"/FFmpeg-*/LICENSE.md "$mpvkit/LICENSE"; do
        [ -f "$license" ] && cp "$license" "$out/$(basename "$(dirname "$license")" | cut -d- -f1)-$(basename "$license")"
    done
    {
        echo "$link_stamp"
        echo "Built $(date -u +%Y-%m-%dT%H:%M:%SZ) by scripts/build-libmpv-macos.sh: MPVKit $MPVKIT_REV"
        echo "(mpv v0.41.0, FFmpeg n9.0.2, libplacebo 7.360.1, MoltenVK 1.4.2) and patches/mpv-moltenvk-host-layer.patch."
        echo "LGPL: the sources are those revisions plus that patch; libmpv.dylib may be replaced by another build."
    } > "$out/BUILD-INFO.txt"
    size="$(du -h "$out/libmpv.dylib" | cut -f1)"
    echo "libmpv ready: $out/libmpv.dylib ($size)"
done
