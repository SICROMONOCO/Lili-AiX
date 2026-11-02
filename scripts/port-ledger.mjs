// The port ledger: Android commits since the last sync that touch code the desktop mirrors,
// grouped by the desktop code to look at. Reads the Android repo, never writes to it.
//
// Usage:
//   node scripts/port-ledger.mjs                 what is left to port since docs/android-sync.json
//   node scripts/port-ledger.mjs --all           also Android-only areas (other providers, UI)
//   node scripts/port-ledger.mjs --since <rev>   from another Android revision
//   node scripts/port-ledger.mjs --mark [rev] [--note text]   record a sync point (default: Android's HEAD)
// The Android repo is ../miruro-apk, or ANILILI_ANDROID.

import { execFileSync } from "node:child_process"
import { existsSync, readFileSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"

const root = resolve(import.meta.dirname, "..")
const android = resolve(process.env.ANILILI_ANDROID ?? join(root, "..", "miruro-apk"))
const syncFile = join(root, "docs", "android-sync.json")
const src = "app/src/main/java/com/anilili/"

// Android path (prefix, under `src` unless it starts with "app/" or "docs/") → what mirrors it.
// First match wins, so specific entries come before their directory's.
const AREAS = [
  ["data/remote/NativeProviderParsers.kt", "providers/kit (title and season matching, HTML helpers)"],
  ["data/remote/AnivexaClient.kt", "providers/megaplay, providers/anizone"],
  ["data/remote/MegaPlay", "providers/megaplay"],
  ["data/remote/AniPm", "providers/anipm"],
  ["data/remote/UniqueStream", "providers/uniquestream, crates/core/src/proxy.rs (media-id key)"],
  ["data/remote/Pahe", "providers/animepahe, crates/core/src/proxy.rs (Kwik IVs)"],
  ["data/remote/Zoko", "providers/zoko"],
  ["data/remote/AnimeOnsen", "providers/animeonsen"],
  ["data/remote/AnimeHeaven", "providers/animeheaven"],
  ["data/remote/RiftProvider.kt", "providers/rift"],
  ["data/remote/Miruro", "crates/core/src/miruro.rs"],
  ["data/remote/Konoha", "crates/core/src/media_index.rs"],
  ["data/remote/AniSkip", "crates/core/src/media_index.rs (skip times)"],
  ["data/remote/AniListClient.kt", "crates/core/src/anilist.rs"],
  ["data/remote/MalClient.kt", "crates/core/src/mal.rs"],
  ["data/auth/MalAuthManager.kt", "crates/core/src/mal.rs (login)"],
  ["data/auth/AuthManager.kt", "src-tauri/src/library.rs (AniList login)"],
  ["data/backup/", "crates/core/src/backup.rs, src-tauri/src/backup.rs (the shared file format)"],
  ["data/update/", "crates/core/src/update.rs (signed channel)"],
  ["ui/watch/EmbedWebView.kt", "crates/core/src/adblock.rs (ORIGINS, if EMBED_BLOCKED_ORIGINS changed)"],
  ["data/recs/", "crates/core/src/recs.rs, src-tauri/src/recs.rs (taste engine, graph)"],
  ["ui/random/", "src/lib/dice.ts, src/components/anime/dice-dialog.tsx (the dice)"],
  ["ui/schedule/", "src/pages/schedule.tsx"],
  ["ui/profile/AccountSheet.kt", "src/lib/insights.ts, src/pages/profile.tsx (account stats)"],
]
// Areas the desktop has no counterpart for; listed only with --all.
const ANDROID_ONLY = [
  ["data/remote/", "providers not on desktop"],
  ["diagnostics/", "Android diagnostics (the desktop's codes are separate: D-prefixed)"],
  ["ui/", "Android UI"],
]

const args = process.argv.slice(2)
const flag = (name) => args.includes(name)
const value = (name) => {
  const i = args.indexOf(name)
  return i >= 0 && args[i + 1] && !args[i + 1].startsWith("--") ? args[i + 1] : null
}
const git = (...a) => execFileSync("git", ["-C", android, ...a], { encoding: "utf8", maxBuffer: 64 << 20 }).trim()

if (!existsSync(join(android, ".git"))) {
  console.error(`No Android repo at ${android} (set ANILILI_ANDROID).`)
  process.exit(1)
}

if (flag("--mark")) {
  const rev = git("rev-parse", "--short=8", value("--mark") ?? "HEAD")
  const date = git("log", "-1", "--format=%cs", rev)
  writeFileSync(syncFile, JSON.stringify({ android: rev, date, note: value("--note") ?? "Android changes up to here are ported or judged Android-only." }, null, 2) + "\n")
  console.log(`Sync point recorded: Android ${rev} (${date}) in docs/android-sync.json`)
  process.exit(0)
}

const since = value("--since") ?? JSON.parse(readFileSync(syncFile, "utf8")).android
const all = flag("--all")
const areaOf = (path) => {
  const rel = path.startsWith(src) ? path.slice(src.length) : null
  if (rel === null) return null
  const hit = AREAS.find(([prefix]) => rel.startsWith(prefix))
  if (hit) return { desktop: hit[1], ported: true }
  const other = all && ANDROID_ONLY.find(([prefix]) => rel.startsWith(prefix))
  return other ? { desktop: other[1], ported: false } : null
}

// One record per commit: hash, date, subject, then the files it touched.
const log = git("log", "--reverse", "--no-merges", "--name-only", "--format=%x00%h%x09%cs%x09%s", `${since}..HEAD`)
const commits = log
  .split("\0")
  .filter(Boolean)
  .map((chunk) => {
    const [head, ...files] = chunk.trim().split("\n")
    const [hash, date, subject] = head.split("\t")
    return { hash, date, subject, files: files.filter(Boolean) }
  })

const byArea = new Map()
for (const commit of commits) {
  for (const file of commit.files) {
    const area = areaOf(file)
    if (!area) continue
    const key = (area.ported ? "" : "~") + area.desktop
    if (!byArea.has(key)) byArea.set(key, new Map())
    const entry = byArea.get(key).get(commit.hash) ?? { ...commit, touched: [] }
    entry.touched.push(file.slice(src.length))
    byArea.get(key).set(commit.hash, entry)
  }
}

const head = git("rev-parse", "--short=8", "HEAD")
console.log(`Android ${since}..${head}: ${commits.length} commit(s), ${byArea.size} area(s) to look at.\n`)
for (const key of [...byArea.keys()].sort()) {
  console.log(key.startsWith("~") ? `${key.slice(1)} [Android-only]` : key)
  for (const c of byArea.get(key).values()) {
    console.log(`  ${c.hash} ${c.date} ${c.subject}`)
    for (const f of c.touched) console.log(`      ${f}`)
  }
  console.log()
}
if (byArea.size === 0) console.log("Nothing to port.")
else console.log("When ported (or judged Android-only): node scripts/port-ledger.mjs --mark")
