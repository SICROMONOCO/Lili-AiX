#!/usr/bin/env bash
# macOS: captures the dev app's window only (never the whole screen: it holds the user's other
# windows), composited as shown, video included, without bringing it to the front.
# Usage: bash scripts/capture-window.sh out.png [process name, default anilili-desktop]
# The first run asks for Screen Recording permission for the terminal.
set -euo pipefail
out="${1:?usage: capture-window.sh out.png [process]}"
name="${2:-anilili-desktop}"
id=$(swift - "$name" <<'SWIFT'
import CoreGraphics
let name = CommandLine.arguments[1]
let windows = CGWindowListCopyWindowInfo([.optionAll, .excludeDesktopElements], kCGNullWindowID) as? [[String: Any]] ?? []
// The largest normal-layer window the process owns: the main window, not the bridge.
let best = windows
    .filter { ($0[kCGWindowOwnerName as String] as? String) == name && ($0[kCGWindowLayer as String] as? Int) == 0 }
    .max { a, b in
        func area(_ w: [String: Any]) -> Double {
            let r = w[kCGWindowBounds as String] as? [String: Double] ?? [:]
            return (r["Width"] ?? 0) * (r["Height"] ?? 0)
        }
        return area(a) < area(b)
    }
if let id = best?[kCGWindowNumber as String] as? Int { print(id) } else { exit(1) }
SWIFT
) || { echo "no window for $name" >&2; exit 1; }
screencapture -x -o -l "$id" "$out"
echo "$out"
