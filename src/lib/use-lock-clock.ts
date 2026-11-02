import { useEffect, useState } from "react"

import { WATCHABLE_AFTER_SECS } from "@/lib/episodes"
import type { Episode } from "@/lib/types"

/** The time a list's locks are judged by, in Unix seconds: it moves on when the soonest locked
 * episode opens, so its card unlocks on the minute without the page ticking every second (each
 * counting card ticks its own face). */
export function useLockClock(episodes: Episode[] | undefined): number {
  const [now, setNow] = useState(() => Date.now() / 1000)
  let opensAt = Infinity
  for (const e of episodes ?? []) {
    if (e.airsAt != null && e.airsAt + WATCHABLE_AFTER_SECS > now) opensAt = Math.min(opensAt, e.airsAt + WATCHABLE_AFTER_SECS)
  }
  useEffect(() => {
    if (!Number.isFinite(opensAt)) return
    // setTimeout overflows past ~24.8 days; a far-off airing just checks again then.
    const wait = Math.min(Math.max(0, opensAt * 1000 - Date.now()) + 500, 2_000_000_000)
    const timer = window.setTimeout(() => setNow(Date.now() / 1000), wait)
    return () => window.clearTimeout(timer)
  }, [opensAt])
  return now
}
