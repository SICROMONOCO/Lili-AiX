// Downloads the filter lists embeds are checked against into vendor/filters/, bundled with the
// installer as the starting copy (the app refreshes them weekly). Network rules only are used.
// EasyList and EasyPrivacy: GPLv3 / CC BY-SA 3.0. uBlock filters: GPLv3.
// Usage: node scripts/fetch-filters.mjs [--force]

import { existsSync, mkdirSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"

const root = resolve(import.meta.dirname, "..")
const outDir = join(root, "vendor", "filters")
// Keep in step with LISTS in src-tauri/src/adblock.rs.
const LISTS = {
  "easylist.txt": "https://easylist.to/easylist/easylist.txt",
  "easyprivacy.txt": "https://easylist.to/easylist/easyprivacy.txt",
  "ublock-filters.txt": "https://ublockorigin.github.io/uAssets/filters/filters.txt",
  "ublock-badware.txt": "https://ublockorigin.github.io/uAssets/filters/badware.txt",
}

mkdirSync(outDir, { recursive: true })
const force = process.argv.includes("--force")
for (const [name, url] of Object.entries(LISTS)) {
  const path = join(outDir, name)
  if (!force && existsSync(path)) {
    console.log(`${name} already present`)
    continue
  }
  const response = await fetch(url)
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`)
  const text = await response.text()
  // A list always opens with its `! Title:` header; anything else is an error page.
  if (!/^\s*(\[.*\]\s*)?! Title:/m.test(text.slice(0, 2000))) throw new Error(`${url}: not a filter list`)
  writeFileSync(path, text)
  console.log(`${name}: ${(text.length / 1024).toFixed(0)} KB`)
}
