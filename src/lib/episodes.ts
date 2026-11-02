// Android's episode-list rules (`EpisodeBrowser.kt`, `EpisodeArtwork.kt`, `PipeModels.kt`), shared by
// the lists that follow them. Keep in step with Android.

import type { Episode, Progress } from "@/lib/types"

/** Long series are split into blocks of this many, each labelled by its two ends. */
export const EPISODE_BLOCK_SIZE = 100

/** Past this many episodes the player's list draws a grid of numbers instead of artwork rows. */
export const EPISODE_GRID_THRESHOLD = 25

export type EpisodeLayout = "list" | "grid"

export type EpisodeBlock = { label: string; episodes: Episode[] }

/** "5", or "13.5" for a recap between two. */
export function displayNumber(n: number): string {
  return Number.isInteger(n) ? String(n) : n.toFixed(1)
}

/**
 * The episode's own title, or null when the source only echoed the number back ("Episode 6", "EP 6",
 * "6"): such a title must not be printed beside the number it repeats.
 */
export function distinctTitle(episode: Episode): string | null {
  const title = episode.title?.trim()
  if (!title) return null
  const n = displayNumber(episode.number)
  const echoes = [`episode ${n}`, `ep ${n}`, n].includes(title.toLowerCase())
  return echoes ? null : title
}

/**
 * Slices by position rather than by number, so gaps and decimal episodes still give even blocks, and
 * a season starting at 1051 is labelled from there.
 */
export function episodeBlocks(
  episodes: Episode[],
  size = EPISODE_BLOCK_SIZE
): EpisodeBlock[] {
  const blocks: EpisodeBlock[] = []
  for (let i = 0; i < episodes.length; i += size) {
    const chunk = episodes.slice(i, i + size)
    const first = displayNumber(chunk[0].number)
    const last = displayNumber(chunk[chunk.length - 1].number)
    blocks.push({
      label: first === last ? first : `${first} – ${last}`,
      episodes: chunk,
    })
  }
  return blocks
}

/** The block holding `number`, or the first: a resumed long-runner doesn't open on episode 1. */
export function blockIndexContaining(
  blocks: EpisodeBlock[],
  number: number | null | undefined
): number {
  if (number == null) return 0
  return Math.max(
    0,
    blocks.findIndex((b) => b.episodes.some((e) => e.number === number))
  )
}

/** The viewer's choice when there is one; otherwise a grid once a season gets long. */
export function showsEpisodeGrid(
  count: number,
  layout: EpisodeLayout | null
): boolean {
  return layout ? layout === "grid" : count > EPISODE_GRID_THRESHOLD
}

/**
 * The picture for episodes without their own: the banner, but only when some episode has art or the
 * list is a single entry. Twelve copies of one banner read as a list that failed to load; blank
 * frames honestly say there is no art yet.
 */
export function episodeArtFallback(
  episodes: Episode[],
  banner: string | null | undefined
): string | null {
  if (!banner) return null
  return episodes.length <= 1 || episodes.some((e) => e.thumbnail)
    ? banner
    : null
}

/** How much of an episode was watched, 0–1; one played past 90% counts as finished. */
export function watchFraction(progress: Progress | undefined): number {
  if (!progress || progress.duration <= 0) return 0
  const fraction = progress.position / progress.duration
  return fraction >= 0.9 ? 1 : Math.min(1, Math.max(0, fraction))
}

/** How long after it airs an episode stays locked while the servers post it (Rust
 * `catalog::WATCHABLE_AFTER_SECS`, Android's `EPISODE_WATCHABLE_AFTER_SEC`). */
export const WATCHABLE_AFTER_SECS = 2 * 60 * 60

/** A locked episode's clock (Android TV's counting card): to its broadcast, then, since an episode
 * airing is not yet an episode on the servers, to when they have it — a clock stopped at 00:00:00
 * over a card that still won't open would look broken. */
export type EpisodeLock = { label: string; remaining: number; aired: boolean }

/** The lock on an episode at `nowSec`, or null once it is open (or was never locked). */
export function episodeLock(episode: Episode, nowSec: number): EpisodeLock | null {
  if (episode.airsAt == null) return null
  if (nowSec < episode.airsAt) return { label: "Next episode in", remaining: episode.airsAt - nowSec, aired: false }
  const opensAt = episode.airsAt + WATCHABLE_AFTER_SECS
  if (nowSec < opensAt) return { label: "On servers in", remaining: opensAt - nowSec, aired: true }
  return null
}

/** `02:15:08`, or `6d 04:15` once a day or more is left: a countdown the width of an episode card
 * (Android's `formatCompactCountdown`). */
export function compactCountdown(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds))
  const days = Math.floor(total / 86_400)
  const pad = (n: number) => String(n).padStart(2, "0")
  const hours = pad(Math.floor((total % 86_400) / 3_600))
  const minutes = pad(Math.floor((total % 3_600) / 60))
  return days > 0 ? `${days}d ${hours}:${minutes}` : `${hours}:${minutes}:${pad(total % 60)}`
}

/** "Airs Sat, Oct 18, 22:30", in this computer's time: when a locked episode comes out. */
export function airsLabel(airsAt: number): string {
  const when = new Date(airsAt * 1000).toLocaleString(undefined, { weekday: "short", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })
  return airsAt > Date.now() / 1000 ? `Airs ${when}` : `Aired ${when}`
}
