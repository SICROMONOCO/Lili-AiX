// What floats over the picture besides the controls, after Android's player: the clock chip, the
// billing a long pause shows, the skip action (with the auto-skip countdown sweeping across it) and
// the up-next card. The rules for when each appears are in src/lib/player-policy.ts.

import { useEffect, useRef, useState } from "react"

import { useAppearance } from "@/lib/api"
import {
  AUTO_SKIP_COUNTDOWN_SECONDS,
  NEXT_EPISODE_COUNTDOWN_SECONDS,
  PAUSE_BILLING_DELAY_MS,
  SKIP_POPUP_SECONDS,
  clockText,
} from "@/lib/player-policy"
import { clockTime } from "@/lib/theme"
import { cn } from "@/lib/utils"

/** The chip every readout over the picture wears, so it holds over a pale frame. */
export const CHIP = "rounded-[10px] border border-white/18 bg-black/46 px-2.5 py-0.5 text-xs font-medium text-white/92"

/** The series' own logo where it has one (and logos are on), else its title in type. */
export function SeriesName({ title, logo, className, fallback }: { title: string; logo?: string | null; className?: string; fallback: React.ReactNode }) {
  const showLogos = useAppearance().showSeriesLogos
  const [broken, setBroken] = useState<string | null>(null)
  if (!logo || !showLogos || broken === logo) return fallback
  return (
    <img
      src={logo}
      alt={title}
      className={cn("object-contain object-left drop-shadow-[0_2px_6px_rgb(0_0_0/0.6)]", className)}
      onError={() => setBroken(logo)}
    />
  )
}

/**
 * The time now and when the episode runs out at this speed: the answer to "how late will this
 * make me?" without the arithmetic. Ticks on its own, so a paused player's clock still moves.
 */
export function PlayerClock({ position, duration, speed, className }: { position: number; duration: number; speed: number; className?: string }) {
  const format = useAppearance().clockFormat
  const [now, setNow] = useState(() => new Date())
  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 1000)
    return () => window.clearInterval(timer)
  }, [])
  return (
    <div className={cn(CHIP, "tabular-nums", className)}>
      {clockText(now, position, duration, speed, (d) => clockTime(d.getTime() / 1000, format))}
    </div>
  )
}

/**
 * The episode's billing over a darkened frame, once a pause has held for fifteen seconds: what a
 * viewer coming back to the screen wants to know, and what tells a pause from a stall. Mounted
 * when the pause starts, so the controls coming and going don't restart the wait; `hidden` while
 * they're up, since the two sets of text would overlap.
 */
export function PauseBilling({
  hidden,
  seriesTitle,
  logo,
  episodeTitle,
  provider,
  description,
}: {
  hidden: boolean
  seriesTitle: string
  logo?: string | null
  episodeTitle?: string
  provider?: string
  description?: string
}) {
  const [due, setDue] = useState(false)
  useEffect(() => {
    const timer = window.setTimeout(() => setDue(true), PAUSE_BILLING_DELAY_MS)
    return () => window.clearTimeout(timer)
  }, [])
  if (!due || hidden) return null
  return (
    <div className="pointer-events-none absolute inset-0 animate-in bg-linear-to-r from-black/82 via-black/40 via-55% to-transparent duration-500 fade-in">
      <div className="absolute top-1/2 left-10 flex w-full max-w-lg -translate-y-1/2 flex-col gap-2 pr-6">
        {seriesTitle && (
          <SeriesName
            title={seriesTitle}
            logo={logo}
            className="mb-1 max-h-28 max-w-sm"
            fallback={<h2 className="mb-1 line-clamp-2 font-heading text-3xl font-bold text-white">{seriesTitle}</h2>}
          />
        )}
        {episodeTitle && <p className="truncate text-base text-white/92">{episodeTitle}</p>}
        {provider && <p className="truncate text-sm font-medium text-white/62">{provider}</p>}
        {description && <p className="line-clamp-4 text-sm text-white/78">{description}</p>}
      </div>
    </div>
  )
}

/** Played time, not wall time: a pause holds the count. Calls `onDone` once it runs out. */
function usePlayedCountdown(from: number, playing: boolean, onDone: () => void): number {
  const [left, setLeft] = useState(from)
  const live = useRef({ playing, onDone })
  useEffect(() => {
    live.current = { playing, onDone }
  }, [playing, onDone])
  useEffect(() => {
    const tick = 100
    let played = 0
    const timer = window.setInterval(() => {
      if (live.current.playing) played += tick
      const remaining = from * 1000 - played
      setLeft(Math.max(0, Math.ceil(remaining / 1000)))
      if (remaining <= 0) {
        window.clearInterval(timer)
        live.current.onDone()
      }
    }, tick)
    return () => window.clearInterval(timer)
  }, [from])
  return left
}

/**
 * Whether the recap or intro skip for `windowKey` is still in its pop-up: the first
 * SKIP_POPUP_SECONDS played inside the window ("" for none). Played time, so a stream still loading
 * at 0:00 inside an opening does not use it up. Leaving the window re-arms it.
 */
export function useSkipPopup(windowKey: string, playing: boolean): boolean {
  const [expired, setExpired] = useState<string | null>(null)
  const live = useRef(playing)
  useEffect(() => {
    live.current = playing
  }, [playing])
  useEffect(() => {
    if (!windowKey) return
    const tick = 100
    let played = 0
    const timer = window.setInterval(() => {
      if (live.current) played += tick
      if (played >= SKIP_POPUP_SECONDS * 1000) {
        window.clearInterval(timer)
        setExpired(windowKey)
      }
    }, tick)
    return () => {
      window.clearInterval(timer)
      setExpired(null)
    }
  }, [windowKey])
  return !!windowKey && expired !== windowKey
}

/** The skip action's frame: bottom right, an accent outline over a dark fill, as on Android. */
function SkipFrame({ label, onClick, sweep }: { label: string; onClick: () => void; sweep?: number }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="relative overflow-hidden rounded-[10px] border-[1.5px] border-primary/86 bg-black/50 px-3.5 py-1.5 text-sm font-bold tracking-wide text-white uppercase shadow-lg transition-colors outline-none hover:bg-black/70 focus-visible:ring-3 focus-visible:ring-primary/50"
    >
      {sweep !== undefined && (
        <span
          aria-hidden
          className="absolute inset-y-0 left-0 bg-primary/60 transition-[width] duration-1000 ease-linear"
          style={{ width: `${sweep * 100}%` }}
        />
      )}
      <span className="relative">{label}</span>
    </button>
  )
}

export function SkipAction({ label, onClick }: { label: string; onClick: () => void }) {
  return <SkipFrame label={label} onClick={onClick} />
}

/**
 * "Skipping Intro · 3s": an auto-skip announced before it jumps, so the picture never lurches with
 * nothing to say why and a viewer who wants this opening can take it (seeking out of the window
 * calls it off). Pressing it skips at once.
 */
export function AutoSkipCountdown({ segment, playing, onSkip }: { segment: string; playing: boolean; onSkip: () => void }) {
  const left = usePlayedCountdown(AUTO_SKIP_COUNTDOWN_SECONDS, playing, onSkip)
  // A second ahead of the label, so the fill reaches the edge as the count reaches zero.
  const sweep = Math.min(1, Math.max(0, 1 - (left - 1) / AUTO_SKIP_COUNTDOWN_SECONDS))
  return <SkipFrame label={`Skipping ${segment} · ${left}s`} onClick={onSkip} sweep={sweep} />
}

/**
 * The next episode, offered in the file's last half-minute with a countdown that plays it unless
 * refused. Counts in played time (and on at the very end, where mpv holds the last frame paused);
 * the file's end takes the next episode by itself (watch.tsx), as Android's does.
 */
export function UpNextCard({ label, playing, onPlay, onStay }: { label: string; playing: boolean; onPlay: () => void; onStay: () => void }) {
  const left = usePlayedCountdown(NEXT_EPISODE_COUNTDOWN_SECONDS, playing, onPlay)
  return (
    <div className="flex w-[22rem] max-w-full animate-in flex-col gap-2.5 rounded-2xl border border-white/16 bg-black/86 px-4 py-3.5 text-white shadow-2xl duration-300 fade-in slide-in-from-bottom-4">
      <span className="text-xs font-medium text-white/62">Up next</span>
      <span className="line-clamp-2 text-sm font-semibold">{label}</span>
      <div className="flex items-center gap-2.5">
        <button
          type="button"
          onClick={onPlay}
          className="rounded-[10px] bg-white/24 px-3.5 py-2 text-sm font-semibold outline-none hover:bg-white/34 focus-visible:ring-3 focus-visible:ring-white/40"
        >
          {left > 0 ? `Play now · ${left}s` : "Play now"}
        </button>
        <button
          type="button"
          onClick={onStay}
          className="rounded-[10px] border border-white/22 bg-white/12 px-3.5 py-2 text-sm font-semibold outline-none hover:bg-white/20 focus-visible:ring-3 focus-visible:ring-white/40"
        >
          Stay
        </button>
      </div>
    </div>
  )
}
