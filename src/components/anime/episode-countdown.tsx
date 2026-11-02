// The clock on a locked episode's card (Android TV's `TvEpisodeCountdownFace`): drawn over the
// artwork, which it dims to stay legible over any still, ticking on its own so the list around it
// doesn't re-render every second.

import { useEffect, useState } from "react"

import { compactCountdown, episodeLock } from "@/lib/episodes"
import type { Episode } from "@/lib/types"
import { cn } from "@/lib/utils"

export function EpisodeCountdown({ episode, compact, className }: { episode: Episode; compact?: boolean; className?: string }) {
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now() / 1000), 1000)
    return () => window.clearInterval(timer)
  }, [])
  const lock = episodeLock(episode, now)
  if (!lock) return null
  return (
    <div className={cn("absolute inset-0 flex flex-col items-center justify-center bg-black/72 text-white", className)}>
      {!compact && <span className="text-[0.65rem] font-semibold tracking-[0.12em] text-white/78 uppercase">{lock.label}</span>}
      <span className={cn("font-bold tabular-nums", compact ? "text-[0.7rem]" : "text-2xl")}>{compactCountdown(lock.remaining)}</span>
    </div>
  )
}


/** A premiere's clock for a title with no episodes yet ("Airing in 7d 02:15"): the same
 * ticking face as a locked episode's, as a line of text for the episodes empty state. */
export function AiringCountdown({ airingAt }: { airingAt: number }) {
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now() / 1000), 1000)
    return () => window.clearInterval(timer)
  }, [])
  const remaining = airingAt - now
  if (remaining <= 0) return <span className="tabular-nums">Airing now</span>
  return (
    <span className="tabular-nums">
      Airing in <span className="font-semibold text-foreground">{compactCountdown(remaining)}</span>
    </span>
  )
}

/** The same clock as a line of text ("Next episode in 6d 04:15"), for the player's notice. */
export function EpisodeCountdownText({ episode }: { episode: Episode }) {
  const [now, setNow] = useState(() => Date.now() / 1000)
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now() / 1000), 1000)
    return () => window.clearInterval(timer)
  }, [])
  const lock = episodeLock(episode, now)
  if (!lock) return null
  return (
    <span className="tabular-nums">
      {lock.label} {compactCountdown(lock.remaining)}
    </span>
  )
}
