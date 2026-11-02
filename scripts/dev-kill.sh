#!/usr/bin/env bash
# Stops a dev session completely: the app and the Vite server Tauri started.
# (Stopping `tauri dev` alone can leave Vite holding port 5173.)
# The dev build only: an installed copy that's running is the user's.
if [ "$(uname)" = "Darwin" ]; then
  pkill -f "/target/debug/anilili-desktop" || true
  for pid in $(lsof -ti tcp:5173 -sTCP:LISTEN); do kill "$pid"; done
  exit 0
fi
powershell -NoProfile -Command "Get-Process anilili-desktop -ErrorAction SilentlyContinue | Where-Object { \$_.Path -like '*\target\*' } | Stop-Process -Force"
for pid in $(netstat -ano | grep ":5173 " | grep LISTENING | awk '{print $5}' | sort -u); do
  taskkill //PID "$pid" //F //T
done
