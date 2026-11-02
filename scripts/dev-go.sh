#!/usr/bin/env bash
# Development aid: navigate the running debug app, wait, and capture the window.
# Usage: scripts/dev-go.sh /anime/154587 [shot-name] [seconds]
root="$(cd "$(dirname "$0")/.." && pwd)"
printf '%s' "$1" > "$root/target/dev-route.txt"
sleep "${3:-4}"
if [ -n "$2" ]; then
  mkdir -p "$root/target/captures"
  if [ "$(uname)" = "Darwin" ]; then
    bash "$root/scripts/capture-window.sh" "$root/target/captures/$2.png"
  else
    powershell -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$root/scripts/capture-window.ps1")" "$(cygpath -w "$root/target/captures/$2.png")"
  fi
fi
