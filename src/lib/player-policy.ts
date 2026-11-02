// When the player offers a skip, counts one down, and offers the next episode: ports of Android's
// `OutroWindowPolicy.kt`, `NextEpisodeCard.kt`, `AutoSkipCountdown.kt`, `ManualSkipButtonPolicy.kt`
// and `PlayerPauseOverlay.kt`, in seconds rather than milliseconds. Keep the two in step.

import type { SeekMarker } from "@/components/player/seek-bar"

/** An auto-skip announces itself on the skip button this long before it jumps. */
export const AUTO_SKIP_COUNTDOWN_SECONDS = 3
/**
 * "Skip Recap" and "Skip Intro" show by themselves for this long (played) once the playhead enters
 * their window, then only with the controls: a window can start at 0:00 and last ninety seconds.
 * Android's TIMED_SKIP_POPUP_MS.
 */
export const SKIP_POPUP_SECONDS = 10
/** The up-next card counts this long before it plays the next episode itself. */
export const NEXT_EPISODE_COUNTDOWN_SECONDS = 8
/** The last stretch of a file in which the next episode is offered. */
export const NEXT_EPISODE_TAIL_SECONDS = 30
/** Offered here even over credits that haven't finished. */
export const NEXT_EPISODE_FINAL_SECONDS = 5
/** An outro has to reach this far into the file to be the credits. */
export const OUTRO_CREDITS_TAIL_FRACTION = 0.75
/** A pause has to hold this long before the episode's billing appears: most pauses are about the
 * picture (a line to re-read, a frame to capture), and the billing would cover it. */
export const PAUSE_BILLING_DELAY_MS = 15_000

/** Inside `[start, end)`; a window that doesn't run forwards locates nothing. */
export function inWindow(t: number, start: number, end: number): boolean {
  return end > start && t >= start && t < end
}

/**
 * Whether an outro is the credits of the file actually playing: it has to reach the last quarter.
 * AniSkip files windows made against other encodes (Bleach 274's "ending" sits in its fifth
 * minute), and every use of an outro says the episode is nearly over.
 */
export function outroIsCredits(outro: { start: number; end: number } | undefined, duration: number): boolean {
  if (!outro || outro.start < 0 || outro.end <= outro.start || duration <= 0) return false
  return outro.end >= duration * OUTRO_CREDITS_TAIL_FRACTION
}

/**
 * Whether the next episode should be offered now: only in the file's last half-minute, and while
 * highlighted credits still roll, not until they finish (or five seconds are all that's left), so
 * a preview or post-credits scene after them still plays.
 */
export function nextEpisodeDue(position: number, duration: number, outro: SeekMarker | undefined): boolean {
  if (duration <= 0 || position < 0) return false
  const remaining = duration - position
  if (remaining < 0 || remaining > NEXT_EPISODE_TAIL_SECONDS) return false
  if (!outro || !outroIsCredits(outro, duration)) return true
  return !(position < outro.end && remaining > NEXT_EPISODE_FINAL_SECONDS)
}

/** Where skipping a window lands: an outro filed past the end lands on the end. */
export function skipTarget(window: SeekMarker, duration: number): number {
  return window.kind === "outro" && duration > 0 ? Math.min(window.end, duration) : window.end
}

/**
 * The window the playhead is in that may be acted on: the recap first (it runs before the
 * opening, and the two can abut), then the opening, then credits that reach the end of the file.
 */
export function activeSkipWindow(markers: SeekMarker[], position: number, duration: number): SeekMarker | null {
  for (const kind of ["recap", "intro", "outro"] as const) {
    const m = markers.find((w) => w.kind === kind)
    if (!m || !inWindow(position, m.start, m.end)) continue
    if (kind === "outro" && !outroIsCredits(m, duration)) continue
    return m
  }
  return null
}

/** "10:42 PM — Ends at 11:03 PM": the time now and when the episode runs out at this speed. */
export function clockText(now: Date, position: number, duration: number, speed: number, time: (d: Date) => string): string {
  const clock = time(now)
  const remaining = duration - position
  if (duration <= 0 || remaining <= 0) return clock
  const rate = speed > 0 ? speed : 1
  return `${clock} — Ends at ${time(new Date(now.getTime() + (remaining / rate) * 1000))}`
}
