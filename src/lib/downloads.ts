// What the download dialog and the Downloads page share: Android's qualities and size estimates,
// the space verdict, and how sizes, speeds and times read.

import type { Audio, DownloadQuality, DownloadRecord } from "@/lib/types"

export const QUALITIES: { value: DownloadQuality; label: string }[] = [
  { value: "best", label: "Best" },
  { value: "1080", label: "1080p" },
  { value: "720", label: "720p" },
  { value: "480", label: "480p" },
  { value: "360", label: "360p" },
]

/** A quality as a height ceiling; `best` has none. */
export const qualityHeight = (quality: DownloadQuality): number | null => (quality === "best" ? null : Number(quality))

const MB = 1024 * 1024

/** Rough size of one episode, on the high side (Android's `DownloadStorage`, measured; Rust
 * `downloads::estimated_episode_bytes`). "Best" is unknown until a playlist is read, so middling. */
export function estimatedEpisodeBytes(maxHeight: number | null): number {
  if (maxHeight == null) return 400 * MB
  if (maxHeight <= 360) return 90 * MB
  if (maxHeight <= 480) return 150 * MB
  if (maxHeight <= 720) return 300 * MB
  if (maxHeight <= 1080) return 550 * MB
  return 400 * MB
}

/** How a batch compares with the room on its disk: it fits, it's tight (would leave less than the
 * headroom), or it won't fit. Unreadable free space is left unjudged. */
export function spaceVerdict(needed: number, free: number | null, headroom: number): "fits" | "tight" | "wontFit" {
  if (free == null || free <= 0) return "fits"
  if (needed > free) return "wontFit"
  if (free - needed < headroom) return "tight"
  return "fits"
}

export function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 4) return `${(bytes / 1024 ** 4).toFixed(2)} TB`
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(bytes >= 10 * 1024 ** 3 ? 0 : 1)} GB`
  if (bytes >= MB) return `${Math.round(bytes / MB)} MB`
  if (bytes > 0) return `${Math.max(1, Math.round(bytes / 1024))} KB`
  return "0 KB"
}

export const formatSpeed = (bytesPerSecond: number) => `${formatBytes(bytesPerSecond)}/s`

/** "4 min left", "1 h 20 min left". */
export function formatEta(seconds: number): string {
  if (seconds < 60) return "under a minute left"
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes} min left`
  return `${Math.floor(minutes / 60)} h ${minutes % 60} min left`
}

/** The download of an episode in an audio, if there is one. */
export function downloadFor(rows: DownloadRecord[] | undefined, animeId: number, episode: number, audio: Audio) {
  return rows?.find((r) => r.animeId === animeId && r.episode === episode && r.audio === audio)
}

export const isActive = (row: DownloadRecord) => row.status === "queued" || row.status === "downloading"
