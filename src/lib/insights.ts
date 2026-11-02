// What the Profile page shows, worked out from the account's list: the port of Android's
// `accountInsights` (ui/profile/AccountSheet.kt), so both apps describe an account the same way.

import type { AccountProfile, ListEntry } from "@/lib/types"

/** Where titles sit, in a fixed order with a fixed colour each (Android's palette). Rewatching
 * folds into Watching: a sixth sliver would be too thin to read. */
export const STATUS_SLICES = [
  { key: "watching", label: "Watching", color: "#3987E5", statuses: ["CURRENT", "REPEATING"] },
  { key: "completed", label: "Completed", color: "#D95926", statuses: ["COMPLETED"] },
  { key: "planning", label: "Planned", color: "#199E70", statuses: ["PLANNING"] },
  { key: "paused", label: "Paused", color: "#C98500", statuses: ["PAUSED"] },
  { key: "dropped", label: "Dropped", color: "#D55181", statuses: ["DROPPED"] },
] as const

export type Slice = (typeof STATUS_SLICES)[number]
export type Bar = { label: string; value: number }

export type Insights = {
  titles: number
  episodes: number
  minutes: number
  meanScore: number | null
  statusCounts: { slice: Slice; count: number }[]
  topGenres: Bar[]
  formats: Bar[]
  studios: Bar[]
  /** Per release year, or per decade once the list spans more than a dozen years. */
  releaseYears: Bar[]
  /** Titles per whole score 1..10; empty when nothing is scored. */
  scoreSpread: Bar[]
  /** Completed as a share of everything finished one way or the other; null before either. */
  completionRate: number | null
  /** Episodes left in what's being watched, where the length is known. */
  episodesToGo: number
  /** Every entry, most recently changed first. */
  recent: ListEntry[]
}

const MAX_RANKED = 5
const STARTED = new Set(["CURRENT", "REPEATING", "COMPLETED", "PAUSED", "DROPPED"])

export function accountInsights(profile: AccountProfile, list: ListEntry[]): Insights {
  const byId = new Map<number, ListEntry>()
  for (const entry of list) if (!byId.has(entry.anime.id)) byId.set(entry.anime.id, entry)
  const entries = [...byId.values()]
  const started = entries.filter((e) => STARTED.has(e.status))
  const statusCounts = STATUS_SLICES.map((slice) => ({
    slice,
    count: entries.filter((e) => (slice.statuses as readonly string[]).includes(e.status)).length,
  }))
  // Genres of what the viewer has started: a Planning list is a wish list, and letting it vote
  // would describe what they mean to watch rather than what they watch.
  const genreSource = started.length > 0 ? started : entries
  const years = entries.map((e) => e.anime.year ?? 0).filter((y) => y > 0)
  const scored = entries.filter((e) => e.score > 0)
  const completed = statusCounts.find((s) => s.slice.key === "completed")!.count
  const dropped = statusCounts.find((s) => s.slice.key === "dropped")!.count

  return {
    titles: profile.titles > 0 ? profile.titles : entries.length,
    episodes: profile.episodes > 0 ? profile.episodes : entries.reduce((sum, e) => sum + Math.max(0, e.progress), 0),
    minutes: profile.minutes,
    meanScore: profile.meanScore,
    statusCounts,
    topGenres: ranked(
      genreSource.flatMap((e) => e.anime.genres),
      MAX_RANKED,
    ),
    formats: ranked(entries.map((e) => formatName(e.anime.format)).filter((f): f is string => !!f), Infinity),
    studios: ranked(
      started.flatMap((e) => e.studios ?? []),
      MAX_RANKED,
    ),
    releaseYears: releaseYears(years),
    scoreSpread:
      scored.length === 0
        ? []
        : Array.from({ length: 10 }, (_, i) => ({
            label: String(i + 1),
            value: scored.filter((e) => Math.min(10, Math.max(1, Math.round(e.score))) === i + 1).length,
          })),
    completionRate: completed + dropped === 0 ? null : Math.round((completed * 100) / (completed + dropped)),
    episodesToGo: entries
      .filter((e) => e.status === "CURRENT" || e.status === "REPEATING")
      .reduce((sum, e) => sum + (e.anime.episodes ? Math.max(0, e.anime.episodes - e.progress) : 0), 0),
    recent: entries.filter((e) => e.updatedAt > 0).sort((a, b) => b.updatedAt - a.updatedAt),
  }
}

function ranked(values: string[], limit: number): Bar[] {
  const counts = new Map<string, number>()
  for (const v of values) counts.set(v, (counts.get(v) ?? 0) + 1)
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .slice(0, limit)
    .map(([label, value]) => ({ label, value }))
}

function releaseYears(years: number[]): Bar[] {
  if (years.length === 0) return []
  const min = Math.min(...years)
  const max = Math.max(...years)
  if (max - min <= 11) {
    // 2024 as '24: twelve labels fit across.
    return Array.from({ length: max - min + 1 }, (_, i) => ({
      label: `’${String((min + i) % 100).padStart(2, "0")}`,
      value: years.filter((y) => y === min + i).length,
    }))
  }
  const decades = years.map((y) => Math.floor(y / 10) * 10)
  const first = Math.min(...decades)
  const last = Math.max(...decades)
  return Array.from({ length: (last - first) / 10 + 1 }, (_, i) => ({
    label: `${String((first + i * 10) % 100).padStart(2, "0")}s`,
    value: decades.filter((d) => d === first + i * 10).length,
  }))
}

function formatName(format: string | null): string | null {
  switch (format) {
    case null:
      return null
    case "TV_SHORT":
      return "TV short"
    case "MOVIE":
      return "Movie"
    case "SPECIAL":
      return "Special"
    case "MUSIC":
      return "Music"
    case "TV":
    case "OVA":
    case "ONA":
      return format
    default:
      return format.charAt(0) + format.slice(1).toLowerCase()
  }
}

/** Watch time as the stat row prints it: hours once there are enough ("1,062 h"), else h and m. */
export function watchTime(minutes: number): string {
  if (minutes >= 600) return `${Math.floor(minutes / 60).toLocaleString()} h`
  const h = Math.floor(minutes / 60)
  const m = minutes % 60
  return h > 0 ? `${h} h ${m} min` : `${m} min`
}

/** Activity per day (unix seconds at UTC midnight → count): the account's own history when it
 * keeps one (AniList), else when list entries last changed (MyAnimeList keeps no history). */
export function activityDays(profile: AccountProfile, list: ListEntry[]): { days: Map<number, number>; fromAccount: boolean } {
  const days = new Map<number, number>()
  if (profile.activity.length > 0) {
    for (const d of profile.activity) days.set(d.day, (days.get(d.day) ?? 0) + d.amount)
    return { days, fromAccount: true }
  }
  for (const entry of list) {
    if (entry.updatedAt <= 0) continue
    const day = entry.updatedAt - (entry.updatedAt % 86_400)
    days.set(day, (days.get(day) ?? 0) + 1)
  }
  return { days, fromAccount: false }
}
