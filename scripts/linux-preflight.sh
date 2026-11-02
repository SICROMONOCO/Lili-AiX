#!/usr/bin/env bash
# Collect local Linux port-test prerequisites; no installations, provider calls or uploads.
# Usage: bash scripts/linux-preflight.sh [absolute path to verified LGPL libmpv.so]
set -u
if [ "$(uname -s)" != Linux ]; then
  echo "This preflight must run on Linux."
  exit 1
fi
task_root="$(cd "$(dirname "$0")/.." && pwd)"
# Prevent rustup proxies from downloading a missing pinned toolchain during inspection.
export RUSTUP_AUTO_INSTALL=0
cd "$task_root"
mkdir -p "$task_root/target/linux-audit"
task_report="$task_root/target/linux-audit/preflight.txt"
{
  echo "Anilili Linux preflight (local only)"
  echo "Observed UTC time: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  uname -srmo
  if [ -f /etc/os-release ]; then
    sed -n '/^PRETTY_NAME=/p; /^VERSION_ID=/p' /etc/os-release
  fi
  getconf GNU_LIBC_VERSION 2>/dev/null || true
  echo "Session: ${XDG_SESSION_TYPE:-unknown}; desktop: ${XDG_CURRENT_DESKTOP:-unknown}"
  echo "DISPLAY: ${DISPLAY:+present}; WAYLAND_DISPLAY: ${WAYLAND_DISPLAY:+present}"
  echo "GDK_BACKEND: ${GDK_BACKEND:-automatic}"
  for task_tool in node npm cargo rustc pkg-config cmake ninja go clang cc tar unzip; do
    if command -v "$task_tool" >/dev/null 2>&1; then
      echo "Tool present: $task_tool"
      case "$task_tool" in
        node|npm|cargo|rustc|cmake|ninja|go|clang|cc|tar|unzip)
          case "$task_tool" in
            go) go version ;;
            unzip) unzip -v | head -n 1 ;;
            *) "$task_tool" --version 2>/dev/null | head -n 1 ;;
          esac
          ;;
      esac
    else
      echo "Tool missing: $task_tool"
    fi
  done
  if command -v rustup >/dev/null 2>&1; then
    rustup toolchain list
    rustup target list --installed
  fi
  if command -v pkg-config >/dev/null 2>&1; then
    for task_pkg in gtk+-3.0 webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0 dbus-1 egl gl libass fontconfig libva libpulse; do
      if pkg-config --exists "$task_pkg"; then
        echo "Package $task_pkg: $(pkg-config --modversion "$task_pkg")"
      else
        echo "Package unavailable to pkg-config: $task_pkg"
      fi
    done
  fi
  if command -v lspci >/dev/null 2>&1; then
    lspci -nn | sed -n '/VGA compatible controller/p; /3D controller/p; /Display controller/p'
  fi
  if command -v busctl >/dev/null 2>&1; then
    # Do not collect a whole bus inventory (process/user details); only relevant service names.
    busctl --user --no-pager --no-legend list 2>/dev/null |
      awk '$1 ~ /^(org\.freedesktop\.(secrets|Notifications|portal\.Desktop)|org\.mpris\.)/ { print "Desktop bus service: " $1 }'
  fi
  for task_command in xdg-open xdg-mime update-desktop-database desktop-file-validate readelf; do
    command -v "$task_command" >/dev/null 2>&1 && echo "Desktop tool present: $task_command" || echo "Desktop tool missing: $task_command"
  done
  if [ -n "${1:-}" ]; then
    case "$1" in
      /*)
        if [ -f "$1" ]; then
          echo "Supplied libmpv ELF metadata (does not establish LGPL provenance):"
          if command -v readelf >/dev/null 2>&1; then
            readelf -d "$1" | sed -n '/NEEDED/p; /SONAME/p; /RUNPATH/p; /RPATH/p'
          fi
          sha256sum "$1" | awk '{ print "Supplied libmpv SHA-256: " $1 }'
        else
          echo "Supplied libmpv file does not exist."
        fi
        ;;
      *) echo "Supply an absolute libmpv path." ;;
    esac
  fi
  echo "This report proves only observed prerequisites; it does not validate playback, permissions or packaging."
} > "$task_report" 2>&1
echo "Saved local report: $task_report"
