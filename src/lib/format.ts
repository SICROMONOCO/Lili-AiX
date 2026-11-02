import type { AnimeCard } from "@/lib/types"

const FORMATS: Record<string, string> = {
  TV: "TV",
  TV_SHORT: "TV Short",
  MOVIE: "Movie",
  SPECIAL: "Special",
  OVA: "OVA",
  ONA: "ONA",
  MUSIC: "Music",
}

const STATUSES: Record<string, string> = {
  RELEASING: "Airing",
  FINISHED: "Finished",
  NOT_YET_RELEASED: "Upcoming",
  CANCELLED: "Cancelled",
  HIATUS: "Hiatus",
}

export const formatLabel = (format: string | null) => (format ? (FORMATS[format] ?? format) : null)
export const statusLabel = (status: string | null) => (status ? (STATUSES[status] ?? status) : null)

/** Whether a title is still to come out: AniList says so, or its first episode is ahead. */
export function notYetOut(card: Pick<AnimeCard, "status" | "nextAiring">, now = Date.now() / 1000): boolean {
  if (card.status === "NOT_YET_RELEASED") return true
  return card.nextAiring != null && card.nextAiring.episode <= 1 && card.nextAiring.airingAt > now
}

/** AniList's fuzzy date ("2026-10-20", "2026-10" or "2026") in words: "Tue, Oct 20, 2026",
 * "October 2026", "2026". */
export function fuzzyDateLabel(date: string): string {
  const [year, month, day] = date.split("-").map(Number)
  if (!month) return String(year)
  if (!day) return new Date(year, month - 1, 1).toLocaleDateString(undefined, { month: "long", year: "numeric" })
  return new Date(year, month - 1, day).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric", year: "numeric" })
}

export function seasonLabel(card: Pick<AnimeCard, "season" | "year">): string | null {
  if (!card.year) return null
  if (!card.season) return String(card.year)
  return `${card.season.charAt(0)}${card.season.slice(1).toLowerCase()} ${card.year}`
}

/** "in 2d 4h", "in 35m", "now". */
export function countdown(unixSeconds: number, now = Date.now() / 1000): string {
  const delta = Math.max(0, unixSeconds - now)
  if (delta < 60) return "now"
  const d = Math.floor(delta / 86400)
  const h = Math.floor((delta % 86400) / 3600)
  const m = Math.floor((delta % 3600) / 60)
  if (d > 0) return `in ${d}d ${h}h`
  if (h > 0) return `in ${h}h ${m}m`
  return `in ${m}m`
}

export function timeAgo(unixSeconds: number, now = Date.now() / 1000): string {
  const delta = Math.max(0, now - unixSeconds)
  if (delta < 60) return "just now"
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`
  if (delta < 86400 * 30) return `${Math.floor(delta / 86400)}d ago`
  return new Date(unixSeconds * 1000).toLocaleDateString()
}

export function episodeLabel(n: number): string {
  return Number.isInteger(n) ? `Episode ${n}` : `Episode ${n.toFixed(1)}`
}

/** AniList descriptions are light HTML; keep line breaks and emphasis, drop everything else. */
export function plainDescription(html: string | null, limit?: number): string {
  if (!html) return ""
  const text = html
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&quot;/g, '"')
    .replace(/&#039;/g, "'")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/\n{3,}/g, "\n\n")
    .trim()
  if (limit && text.length > limit) return `${text.slice(0, limit).trimEnd()}…`
  return text
}

export const GENRES = [
  "Action",
  "Adventure",
  "Comedy",
  "Drama",
  "Ecchi",
  "Fantasy",
  "Hentai",
  "Horror",
  "Mahou Shoujo",
  "Mecha",
  "Music",
  "Mystery",
  "Psychological",
  "Romance",
  "Sci-Fi",
  "Slice of Life",
  "Sports",
  "Supernatural",
  "Thriller",
]
