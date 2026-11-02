#!/usr/bin/env bash
# Builds the Linux libmpv into vendor/libmpv/linux-x64/: libmpv.so.2 plus the FFmpeg
# shared libraries it links, all LGPL or more permissive.
#
# mpv is built with -Dgpl=false and FFmpeg without --enable-gpl/--enable-nonfree (and
# without x264/x265 or other GPL/nonfree parts), from the pinned sources below.
# The result is loaded dynamically by the app, so it stays replaceable, as the LGPL asks.
# Never substitute a distribution libmpv/ffmpeg for a release: their build flags are
# unknown and may make the combination GPL.
#
# It must load on more than the distribution it was built on, so what it links from the
# system is kept to libraries whose names every distribution shares (libass.so.9, libz.so.1,
# libgnutls.so.30, the audio and display libraries). libplacebo, whose soname changes with
# every release (an Ubuntu 24.04 build wanted libplacebo.so.338 and failed to load on a newer
# distribution), is built as mpv's subproject and linked in statically. mpv's optional
# features with distribution-specific names (Lua, JavaScript, rubberband, sixel, sndio,
# Blu-ray, libarchive, libjpeg, uchardet) are off: the app uses none of them, and
# screenshots are encoded by FFmpeg instead of libjpeg.
#
# FFmpeg has no TLS of its own and detects none: without --enable-gnutls it has no `https`
# protocol and no HTTPS stream plays (the first Linux build shipped like that).
#
# Needs: gcc/g++, make, meson, ninja, pkg-config, nasm, git, patchelf, python3, and the libass
# and GnuTLS development packages (`libass-dev`, `libgnutls28-dev` on Debian/Ubuntu).
#
# Usage: bash scripts/build-libmpv-linux.sh [--force]
set -euo pipefail

MPV_TAG=v0.40.0
FFMPEG_TAG=n7.1.1
LIBPLACEBO_TAG=v7.351.0

if [ "$(uname -s)" != Linux ]; then
    echo "build-libmpv-linux.sh runs on Linux." >&2
    exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
work="$root/vendor/libmpv-build-linux"
out="$root/vendor/libmpv/linux-x64"
force=0
[ "${1:-}" = "--force" ] && force=1

sha() { sha256sum "$1" | cut -d' ' -f1; }
stamp="mpv=$MPV_TAG ffmpeg=$FFMPEG_TAG libplacebo=$LIBPLACEBO_TAG script=$(sha "$0")"
if [ "$force" = 0 ] && [ "$(head -1 "$out/BUILD-INFO.txt" 2>/dev/null || true)" = "$stamp" ]; then
    echo "libmpv is up to date in $out"
    exit 0
fi
# A different stamp means different flags: rebuild everything rather than mix two builds.
if [ "$(head -1 "$out/BUILD-INFO.txt" 2>/dev/null || true)" != "$stamp" ]; then
    force=1
fi

for tool in gcc g++ make meson ninja pkg-config git nasm patchelf python3; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
done
pkg-config --exists libass || { echo "missing libass development package (libass-dev)" >&2; exit 1; }
pkg-config --exists gnutls || { echo "missing GnuTLS development package (libgnutls28-dev)" >&2; exit 1; }

mkdir -p "$work"
jobs="$(nproc)"
if [ "$force" = 1 ]; then
    rm -rf "$out"
    rm -f "$work/ffmpeg-done" "$work/mpv-done"
fi
mkdir -p "$out"

# 1. FFmpeg, LGPL: no GPL, no nonfree, no programs (the app only needs the libraries). GnuTLS
# (LGPL) for https. No libavdevice (mpv doesn't need it) and no bzip2 (Debian calls it
# libbz2.so.1.0, Fedora libbz2.so.1). Encoders: AAC for downloads' remux, PNG and MJPEG for
# screenshots.
ffmpeg_src="$work/FFmpeg-$FFMPEG_TAG"
if [ ! -d "$ffmpeg_src/.git" ]; then
    rm -rf "$ffmpeg_src"
    git clone -q --depth 1 --branch "$FFMPEG_TAG" https://github.com/FFmpeg/FFmpeg.git "$ffmpeg_src"
fi
if [ ! -f "$work/ffmpeg-done" ]; then
    (cd "$ffmpeg_src" && make distclean >/dev/null 2>&1 || true)
    (cd "$ffmpeg_src" && ./configure \
        --prefix="$out" \
        --enable-shared --disable-static --disable-programs --disable-doc \
        --disable-gpl --disable-nonfree --disable-avdevice --disable-bzlib \
        --enable-gnutls \
        --disable-encoders --disable-muxers \
        --enable-encoder=aac,png,mjpeg --enable-muxer=null \
        --extra-cflags="-O2" && make -j"$jobs" && make install)
    touch "$work/ffmpeg-done"
fi

# 2. mpv, LGPL: -Dgpl=false, client library only (no cplayer binary), libplacebo from its
# subproject, static.
mpv_src="$work/mpv-$MPV_TAG"
if [ ! -d "$mpv_src/.git" ]; then
    rm -rf "$mpv_src"
    git clone -q --depth 1 --branch "$MPV_TAG" https://github.com/mpv-player/mpv.git "$mpv_src"
fi
if [ ! -d "$mpv_src/subprojects/libplacebo/.git" ]; then
    rm -rf "$mpv_src/subprojects/libplacebo"
    git clone -q --depth 1 --branch "$LIBPLACEBO_TAG" --recurse-submodules --shallow-submodules \
        https://code.videolan.org/videolan/libplacebo.git "$mpv_src/subprojects/libplacebo"
fi
export PKG_CONFIG_PATH="$out/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$out/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
if [ ! -f "$work/mpv-done" ]; then
    (cd "$mpv_src" && meson setup build --wipe -Dgpl=false -Dlibmpv=true -Dcplayer=false \
        -Dprefix="$out" \
        --force-fallback-for=libplacebo \
        -Dlibplacebo:default_library=static -Dlibplacebo:demos=false -Dlibplacebo:tests=false \
        -Dlibplacebo:vulkan=disabled -Dlibplacebo:unwind=disabled -Dlibplacebo:xxhash=disabled \
        -Dlibplacebo:libdovi=disabled \
        -Dlua=disabled -Djavascript=disabled -Drubberband=disabled -Dsixel=disabled -Dsndio=disabled \
        -Dlibbluray=disabled -Dlibarchive=disabled -Djpeg=disabled -Duchardet=disabled \
        -Dlibavdevice=disabled -Dcdda=disabled -Ddvdnav=disabled -Dcaca=disabled -Dvapoursynth=disabled \
        -Dsdl2=disabled -Djack=disabled -Dopenal=disabled -Dvulkan=disabled -Dshaderc=disabled \
        -Dspirv-cross=disabled \
        && meson compile -C build -j"$jobs" && meson install -C build)
    touch "$work/mpv-done"
fi
grep -q '"gpl", "value": false' "$mpv_src/build/meson-info/intro-buildoptions.json" 2>/dev/null \
    || meson configure "$mpv_src/build" | grep -E '^  gpl ' | grep -q false \
    || { echo "mpv was not configured with gpl=false: refusing to ship it." >&2; exit 1; }

so="$out/lib/libmpv.so.2"
[ -f "$so" ] || so="$(find "$out" -name 'libmpv.so.2*' | head -1)"
[ -n "$so" ] && [ -f "$so" ] || { echo "build produced no libmpv.so.2 under $out" >&2; exit 1; }
cp -f "$so" "$out/libmpv.so.2"
# The FFmpeg libraries libmpv links must sit beside it in the bundle, found via $ORIGIN.
cp -f "$out"/lib/libavcodec.so.* "$out"/lib/libavfilter.so.* \
    "$out"/lib/libavformat.so.* "$out"/lib/libavutil.so.* "$out"/lib/libswresample.so.* \
    "$out"/lib/libswscale.so.* "$out/"
for lib in "$out"/libmpv.so.2 "$out"/libav*.so.* "$out"/libsw*.so.*; do
    [ -L "$lib" ] || patchelf --set-rpath '$ORIGIN' "$lib"
done

# Checks: LGPL configuration only (into a file first, as grep -q under pipefail lies).
strings "$out/libmpv.so.2" > "$work/strings.txt"
if grep -q -- '--enable-gpl' "$work/strings.txt"; then
    echo "FFmpeg was configured with --enable-gpl: refusing to ship it." >&2
    exit 1
fi
needed="$(for lib in "$out"/libmpv.so.2 "$out"/libav*.so.?? "$out"/libsw*.so.?; do readelf -d "$lib"; done \
    | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' | sort -u)"
echo "NEEDED of the built libraries:"
echo "$needed"
if echo "$needed" | grep -E '^lib(placebo|mujs|lua|rubberband|sixel|sndio|bluray|archive|jpeg|uchardet|bz2)[.-]'; then
    echo "a distribution-specific library is linked (above): this build would not load elsewhere." >&2
    exit 1
fi
echo "$needed" | grep -q '^libgnutls' || { echo "FFmpeg has no TLS: HTTPS streams would not play." >&2; exit 1; }

{
    echo "$stamp"
    echo "Built $(date -u +%Y-%m-%dT%H:%M:%SZ) by scripts/build-libmpv-linux.sh"
    echo "mpv $MPV_TAG (-Dgpl=false, libmpv only), libplacebo $LIBPLACEBO_TAG (static, inside libmpv) and FFmpeg $FFMPEG_TAG (--disable-gpl --disable-nonfree --enable-gnutls, shared libraries beside libmpv)."
    echo "System libraries linked (every distribution's names):"
    echo "$needed" | sed 's/^/  /'
    echo "LGPL: the sources are those tags; libmpv.so.2 may be replaced by another build."
} > "$out/BUILD-INFO.txt"
rm -rf "$out/lib" "$out/include" "$out/share"
echo "libmpv ready: $out/libmpv.so.2 ($(du -h "$out/libmpv.so.2" | cut -f1))"
