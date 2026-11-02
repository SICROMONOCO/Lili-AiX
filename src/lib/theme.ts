// What Android's Appearance choices look like here (the stored values are Rust's `Appearance`, shared
// with Android through backups). Android generates a Material 3 scheme from the seed; the desktop's
// shadcn tokens are fewer, so each choice is applied as the nearest equivalent:
// - the background is the canvas (AMOLED black, or Material's softer dark grey);
// - the accent is `--primary` and the focus ring;
// - the style decides how the accent is drawn: Vivid keeps the exact colour and tints surfaces
//   lightly, Tonal softens it to a pastel, Neutral keeps surfaces grey and the accent muted, Vibrant
//   tints surfaces strongly;
// - contrast pushes secondary text and outlines further from the canvas.

import type { Appearance } from "@/lib/types"

export const ACCENTS: { value: string; label: string; seed: string | null }[] = [
  { value: "monochrome", label: "Monochrome", seed: null },
  { value: "purple", label: "Purple", seed: "#8b5cf6" },
  { value: "neon_green", label: "Neon green", seed: "#39ff14" },
  { value: "blood_red", label: "Blood red", seed: "#d1102b" },
  { value: "ocean_blue", label: "Ocean blue", seed: "#3b82f6" },
  { value: "cyan", label: "Cyan", seed: "#22d3ee" },
  { value: "sakura_pink", label: "Sakura pink", seed: "#f472b6" },
  { value: "sunset_orange", label: "Sunset orange", seed: "#f97316" },
  { value: "gold", label: "Gold", seed: "#facc15" },
  { value: "emerald", label: "Emerald", seed: "#10b981" },
  { value: "custom", label: "Custom", seed: null },
]

/** Android's ThemePreset: canvas, accent, style and contrast chosen together. */
export const PRESETS: { label: string; background: string; accent: string; style: string; contrast?: string }[] = [
  { label: "Midnight", background: "amoled", accent: "monochrome", style: "vivid" },
  { label: "Crimson Night", background: "amoled", accent: "blood_red", style: "vivid" },
  { label: "Toxic", background: "amoled", accent: "neon_green", style: "vibrant" },
  { label: "Royal", background: "amoled", accent: "purple", style: "vivid" },
  { label: "Deep Sea", background: "amoled", accent: "ocean_blue", style: "vivid" },
  { label: "Cyber", background: "amoled", accent: "cyan", style: "vibrant" },
  { label: "Sakura", background: "dark", accent: "sakura_pink", style: "tonal" },
  { label: "Ember", background: "amoled", accent: "sunset_orange", style: "vivid" },
  { label: "Gilded", background: "amoled", accent: "gold", style: "neutral" },
  { label: "Forest", background: "dark", accent: "emerald", style: "tonal" },
  { label: "High contrast", background: "amoled", accent: "monochrome", style: "vivid", contrast: "high" },
]

export const DEFAULT_APPEARANCE: Appearance = {
  background: "amoled",
  accent: "monochrome",
  customSeed: "#8b5cf6",
  style: "vivid",
  contrast: "standard",
  playerAccent: false,
  showColourPages: true,
  backdropSource: "none",
  backdropAnimeUrl: null,
  backdropAnimeTitle: null,
  backdropImage: null,
  backdropDim: "medium",
  backdropBlur: true,
  uiScale: "medium",
  clockFormat: "system",
  hiddenPosterElements: [],
  blurEpisodeImages: false,
  showSeriesLogos: true,
  glassNavigation: true,
  glassHome: true,
  glassDetails: true,
  glassBrowse: true,
  glassPopups: true,
  glassRefresh: true,
  glassPages: true,
  homeLayout: null,
}

/** Monochrome style matches regardless of style: it has no hue for a style to spread. */
export function presetMatches(p: (typeof PRESETS)[number], a: Appearance): boolean {
  return (
    p.background === a.background &&
    p.accent === a.accent &&
    (p.contrast ?? "standard") === a.contrast &&
    (a.accent === "monochrome" || p.style === a.style)
  )
}

export function seedOf(a: Pick<Appearance, "accent" | "customSeed">): string | null {
  if (a.accent === "custom") return a.customSeed
  return ACCENTS.find((x) => x.value === a.accent)?.seed ?? null
}

const CANVAS = {
  amoled: { background: "oklch(0 0 0)", card: "oklch(0.17 0 0)", muted: "oklch(0.245 0 0)", popover: "oklch(0.19 0 0)" },
  dark: { background: "oklch(0.18 0 0)", card: "oklch(0.225 0 0)", muted: "oklch(0.29 0 0)", popover: "oklch(0.24 0 0)" },
}

const TINT = { vivid: 4, tonal: 7, neutral: 0, vibrant: 13 } as Record<string, number>

/** Black or white, whichever reads on `hex`. */
function onColour(hex: string): string {
  const n = Number.parseInt(hex.slice(1), 16)
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255].map((c) => {
    const v = c / 255
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.35 ? "oklch(0.145 0 0)" : "oklch(0.985 0 0)"
}

/** The accent's own variables, for a subtree (a title's page, the player) or the whole app. */
export function accentVars(seed: string | null, style = "vivid"): Record<string, string> {
  if (!seed) {
    return {
      "--primary": "oklch(0.922 0 0)",
      "--primary-foreground": "oklch(0.205 0 0)",
      "--ring": "oklch(0.556 0 0)",
      "--sidebar-primary": "oklch(0.922 0 0)",
    }
  }
  const primary =
    style === "tonal"
      ? `color-mix(in oklab, ${seed} 62%, white)`
      : style === "neutral"
        ? `color-mix(in oklab, ${seed} 60%, oklch(0.7 0 0))`
        : seed
  const foreground = style === "tonal" ? "oklch(0.145 0 0)" : onColour(seed)
  return { "--primary": primary, "--primary-foreground": foreground, "--ring": primary, "--sidebar-primary": primary }
}

/** Every variable the appearance sets on the document. */
export function themeVars(a: Appearance): Record<string, string> {
  const canvas = CANVAS[a.background as keyof typeof CANVAS] ?? CANVAS.amoled
  const seed = seedOf(a)
  const tint = seed ? (TINT[a.style] ?? 4) : 0
  const tinted = (base: string, extra = 0) => (tint + extra > 0 ? `color-mix(in oklab, ${seed} ${tint + extra}%, ${base})` : base)
  const contrast = { standard: [0.708, 10], medium: [0.78, 16], high: [0.87, 26] }[a.contrast] ?? [0.708, 10]
  return {
    "--background": tinted(canvas.background),
    "--card": tinted(canvas.card),
    "--popover": tinted(canvas.popover),
    "--muted": tinted(canvas.muted, 2),
    "--secondary": tinted(canvas.muted, 2),
    "--accent": tinted(canvas.muted, 2),
    "--muted-foreground": `oklch(${contrast[0]} 0 0)`,
    "--border": `oklch(1 0 0 / ${contrast[1]}%)`,
    "--input": `oklch(1 0 0 / ${contrast[1] + 5}%)`,
    "--foreground": a.contrast === "high" ? "oklch(1 0 0)" : "oklch(0.985 0 0)",
    ...accentVars(seed, a.style),
  }
}

const SCALE: Record<string, number> = { small: 0.9, medium: 1, large: 1.15 }

/** Android's Liquid glass parts (Appearance → Liquid glass), as the desktop has them. Android's
 * pull-to-refresh spinner has no desktop counterpart; its switch is kept for backups only. */
export const GLASS_PARTS = [
  { key: "glassNavigation", label: "Sidebar", description: "The navigation bar down the side" },
  { key: "glassHome", label: "Home", description: "The spotlight's tags and buttons, and the cards' badges" },
  { key: "glassDetails", label: "Details", description: "Panels and badges on a title's page" },
  { key: "glassBrowse", label: "Browse", description: "The search and filters over the results" },
  { key: "glassPages", label: "Other pages", description: "Library, Schedule, Downloads, Notifications, Profile and Settings" },
  { key: "glassPopups", label: "Popups", description: "Dialogs, menus and lists, and blurring the page behind them" },
] as const satisfies readonly { key: keyof Appearance; label: string; description: string }[]

export type GlassPart = (typeof GLASS_PARTS)[number]["key"]

/** The glass part a route's page belongs to. */
export function glassPartFor(pathname: string): GlassPart {
  if (pathname === "/") return "glassHome"
  if (pathname.startsWith("/anime/")) return "glassDetails"
  if (pathname.startsWith("/search")) return "glassBrowse"
  return "glassPages"
}

/** Applies the appearance to the document: colours, the interface size (rem-based sizes), and the
 * popups' glass (they render outside the pages, so the switch lives on the document). */
export function applyAppearance(a: Appearance) {
  const root = document.documentElement
  for (const [name, value] of Object.entries(themeVars(a))) root.style.setProperty(name, value)
  root.style.fontSize = `${16 * (SCALE[a.uiScale] ?? 1)}px`
  root.dataset.glassPopups = a.glassPopups ? "on" : "off"
}

/** A time of day in the viewer's chosen clock. */
export function clockTime(unixSeconds: number, format: string): string {
  return new Date(unixSeconds * 1000).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
    hour12: format === "12" ? true : format === "24" ? false : undefined,
  })
}

/** Android's poster elements (PosterElement). The new-episode marker isn't drawn on desktop cards
 * yet; its switch keeps its value so a backup carries it back to Android. */
export const POSTER_ELEMENTS = [
  { value: "score", label: "Score", description: "The star rating on the artwork" },
  { value: "progress", label: "Episode progress", description: "How far into a title you are" },
  { value: "adult", label: "Adult marker", description: "The 18+ tag on adult titles" },
  { value: "dub", label: "Dub marker", description: "DUB on titles with a known English dub" },
  { value: "title", label: "Title", description: "The name under the artwork" },
  { value: "details", label: "Details line", description: "Format, year and episode count under the title" },
] as const

/** Home's rows, under Android's keys where it has them (`HomeRow`). */
export const HOME_ROWS = [
  { key: "continue", label: "Continue watching" },
  { key: "account", label: "Watching on your account" },
  { key: "for_you", label: "For you" },
  { key: "because", label: "Because you watched…" },
  { key: "newest", label: "New episodes" },
  { key: "trending", label: "Trending now" },
  { key: "popular", label: "This season" },
  { key: "all_time", label: "All-time popular" },
  { key: "top_rated", label: "Top rated" },
  { key: "movies", label: "Movies" },
  { key: "upcoming", label: "Coming soon" },
] as const

export type HomeRowKey = (typeof HOME_ROWS)[number]["key"]
export type HomeLayout = { order: HomeRowKey[]; hidden: Set<HomeRowKey>; foreign: string[] }

/** Android's encoding ("continue,-because,…"). Rows it doesn't mention go back where the default
 * has them; keys this app doesn't know are kept, so a backup returns them intact. */
export function decodeHomeLayout(stored: string | null): HomeLayout {
  const known = new Set<string>(HOME_ROWS.map((r) => r.key))
  const order: HomeRowKey[] = []
  const hidden = new Set<HomeRowKey>()
  const foreign: string[] = []
  for (const token of (stored ?? "").split(",").map((t) => t.trim()).filter(Boolean)) {
    const key = token.replace(/^-/, "")
    if (!known.has(key)) {
      foreign.push(token)
      continue
    }
    if (order.includes(key as HomeRowKey)) continue
    order.push(key as HomeRowKey)
    if (token.startsWith("-")) hidden.add(key as HomeRowKey)
  }
  HOME_ROWS.forEach((row, index) => {
    if (!order.includes(row.key)) order.splice(Math.min(index, order.length), 0, row.key)
  })
  return { order, hidden, foreign }
}

/** `row` taken out and put back just before or after `anchor` (Android's `placeBeside`): a drag
 * on Home passes only the rows drawn, and hidden or empty ones keep their places among the rest. */
export function placeBeside(layout: HomeLayout, row: HomeRowKey, anchor: HomeRowKey, after: boolean): HomeLayout {
  if (row === anchor) return layout
  const order = layout.order.filter((k) => k !== row)
  order.splice(order.indexOf(anchor) + (after ? 1 : 0), 0, row)
  return { ...layout, order }
}

export function encodeHomeLayout(layout: HomeLayout): string {
  return [...layout.order.map((k) => (layout.hidden.has(k) ? `-${k}` : k)), ...layout.foreign].join(",")
}
