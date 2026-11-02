// What the settings search knows (Android's SettingsSearchIndex.kt): each setting's name, the
// category holding it, and the words someone might type instead. Nobody searches "Audio"; they
// search "dub". Hand-written, as Android's is: a `title` must match the row's label (or a group's
// heading) exactly, since that is how the page finds the row to point at.

import type { SectionId } from "@/components/settings/section"

export type SearchEntry = { section: SectionId; title: string; keywords?: string[] }

export const SEARCH_ENTRIES: SearchEntry[] = [
  { section: "player", title: "Audio", keywords: ["dub", "dubbed", "sub", "subbed", "english", "japanese", "language"] },
  { section: "player", title: "Resume where you left off", keywords: ["continue", "position", "restart"] },
  { section: "player", title: "Autoplay next episode", keywords: ["next", "countdown", "binge", "up next"] },
  { section: "player", title: "Skip intros and recaps automatically", keywords: ["opening", "op", "intro", "recap", "aniskip"] },
  { section: "player", title: "Skip endings automatically", keywords: ["ending", "ed", "outro", "credits", "aniskip"] },
  { section: "player", title: "Skip button", keywords: ["jump", "85", "seconds"] },
  { section: "player", title: "Skip button length", keywords: ["jump", "85", "seconds"] },
  { section: "player", title: "Screen size", keywords: ["fit", "crop", "fill", "stretch", "zoom", "aspect", "black bars"] },
  { section: "player", title: "Pause details", keywords: ["logo", "synopsis", "overlay", "paused"] },
  { section: "player", title: "Clock and finishing time", keywords: ["time", "ends at", "clock"] },
  { section: "player", title: "Upscaling (Anime4K)", keywords: ["anime4k", "sharpen", "upscale", "gpu", "quality"] },
  { section: "player", title: "Buffer size", keywords: ["buffering", "stutter", "stall", "lag", "network", "connection"] },
  { section: "player", title: "Hardware decoding", keywords: ["gpu", "cpu", "artefacts", "artifacts", "green", "decoder"] },

  { section: "subtitles", title: "Show subtitles by default", keywords: ["captions", "cc", "subs", "off", "on", "hide"] },
  { section: "subtitles", title: "Also on dubbed audio", keywords: ["dub", "dubbed", "captions", "subs"] },
  { section: "subtitles", title: "Preferred language", keywords: ["subtitle language", "captions", "cc", "language", "subs"] },
  { section: "subtitles", title: "OpenSubtitles", keywords: ["opensubtitles", "search subtitles", "download subtitles", "srt", "account", "sign in"] },
  { section: "subtitles", title: "Second choice", keywords: ["secondary", "fallback", "subtitle language", "captions"] },
  {
    section: "subtitles",
    title: "Caption appearance",
    keywords: ["subtitle size", "font", "text size", "bigger", "smaller", "colour", "color", "background", "box", "outline", "shadow", "position", "bold", "opacity", "style", "left", "right", "across", "align"],
  },

  { section: "downloads", title: "Folder", keywords: ["path", "location", "directory", "disk", "save", "where"] },
  { section: "downloads", title: "Quality", keywords: ["resolution", "1080p", "720p", "480p", "download quality"] },

  {
    section: "servers",
    title: "Server order",
    keywords: ["preferred server", "priority", "provider", "source", "miruro", "first", "order", "enable", "disable", "extensions", "plugins", "update"],
  },
  {
    section: "servers",
    title: "DNS over HTTPS",
    keywords: ["dns", "doh", "secure dns", "blocked", "isp", "cloudflare", "google", "quad9", "adguard", "sources not loading"],
  },
  { section: "servers", title: "Server languages", keywords: ["language", "tags", "filter", "arabic", "english", "spanish", "source language"] },

  { section: "accounts", title: "AniList", keywords: ["sign in", "login", "log in", "account", "sync", "sign out"] },
  { section: "accounts", title: "Update AniList automatically", keywords: ["sync", "progress", "scrobble"] },
  { section: "accounts", title: "MyAnimeList", keywords: ["mal", "sign in", "login", "account", "sync"] },

  { section: "notifications", title: "New episode alerts", keywords: ["release", "airing", "toast", "alerts", "feed"] },
  { section: "notifications", title: "Finished downloads", keywords: ["download", "done", "complete"] },
  { section: "notifications", title: "Test", keywords: ["send a test", "not working", "do not disturb"] },

  { section: "content", title: "Discord status", keywords: ["discord", "rich presence", "activity", "profile"] },
  { section: "content", title: "Show adult titles", keywords: ["18+", "adult", "nsfw", "hentai", "mature"] },

  { section: "appearance", title: "Theme presets", keywords: ["theme", "preset", "colours", "colors"] },
  { section: "appearance", title: "Background", keywords: ["amoled", "black", "dark", "oled"] },
  { section: "appearance", title: "Accent colour", keywords: ["accent", "color", "colour", "custom", "highlight"] },
  { section: "appearance", title: "Colour style", keywords: ["vivid", "tonal", "neutral", "vibrant", "color style"] },
  { section: "appearance", title: "Contrast", keywords: ["accessibility", "low vision", "glare", "outlines"] },
  { section: "appearance", title: "Colour anime pages by the show", keywords: ["dynamic", "artwork", "color"] },
  { section: "appearance", title: "Accent in the player", keywords: ["player", "white", "controls", "color"] },
  { section: "appearance", title: "Background image", keywords: ["wallpaper", "backdrop", "picture", "image"] },
  { section: "appearance", title: "Liquid glass", keywords: ["glass", "all glass effects", "blur", "transparency", "translucent", "solid"] },
  { section: "appearance", title: "Interface size", keywords: ["zoom", "scale", "text size", "font size", "bigger", "smaller"] },
  { section: "appearance", title: "Time format", keywords: ["12-hour", "24-hour", "clock", "am pm"] },
  { section: "appearance", title: "Arrange Home", keywords: ["home", "rows", "order", "hide rows"] },
  { section: "appearance", title: "Card details", keywords: ["badges", "poster", "score", "dub tag", "captions"] },
  { section: "appearance", title: "Blur episode images", keywords: ["spoilers", "thumbnails", "blur"] },
  { section: "appearance", title: "Series logos", keywords: ["wordmark", "logo", "title art"] },

  { section: "data", title: "Video cache", keywords: ["cache", "disk", "seeking", "memory"] },
  { section: "data", title: "Clear cache", keywords: ["cache", "reset", "storage", "free space"] },
  { section: "data", title: "Clear watch history", keywords: ["history", "progress", "continue watching", "reset"] },
  { section: "data", title: "Backup", keywords: ["export", "restore", "import", "android", "transfer", "move"] },

  { section: "app", title: "Check for updates on launch", keywords: ["update", "automatic", "startup"] },
  { section: "app", title: "Check for updates", keywords: ["update", "new version", "upgrade"] },
  { section: "app", title: "Send diagnostics", keywords: ["report", "bug", "problem", "support", "logs"] },
  { section: "app", title: "Log folder", keywords: ["logs", "debug"] },

  { section: "about", title: "Version", keywords: ["version", "build", "about"] },
  { section: "about", title: "Discord server", keywords: ["discord", "help", "support", "community", "news"] },
  { section: "about", title: "Keyboard shortcuts", keywords: ["keys", "hotkeys", "keyboard", "shortcuts"] },
]

/**
 * Entries holding every word typed, best first: a name that starts with the text, then a name that
 * holds it, then the rest (a keyword or the category).
 */
export function searchSettings(query: string, sectionTitle: (id: SectionId) => string): SearchEntry[] {
  const text = query.trim().toLowerCase()
  if (!text) return []
  const words = text.split(/\s+/)
  const scored: { entry: SearchEntry; rank: number }[] = []
  for (const entry of SEARCH_ENTRIES) {
    const title = entry.title.toLowerCase()
    const haystack = [title, sectionTitle(entry.section).toLowerCase(), ...(entry.keywords ?? [])].join(" ")
    if (!words.every((w) => haystack.includes(w))) continue
    scored.push({ entry, rank: title.startsWith(text) ? 0 : title.includes(text) ? 1 : 2 })
  }
  return scored.sort((a, b) => a.rank - b.rank).map((s) => s.entry)
}
