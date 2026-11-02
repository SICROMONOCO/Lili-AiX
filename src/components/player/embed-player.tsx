import { ArrowLeftIcon, ShieldCheckIcon } from "lucide-react"
import { useEffect } from "react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { api } from "@/lib/api"
import type { Server } from "@/lib/types"

/**
 * A server only an iframe player can play. No `sandbox` attribute: these hosts detect it and
 * refuse to play. The guards are native instead (see `create_main_window` in main.rs): every
 * popup is denied and top-level navigation away from the app is refused.
 *
 * The page inside says nothing about what it plays, so Discord is told from here: the episode, for
 * as long as the embed is open (`playing`; the dev page that opens any URL has none), and where its
 * video is once a frame of the page reports it.
 */

/** A video's clock as a frame of the embed reports it (`src-tauri/src/embed-progress.js`). */
type EmbedClock = { position: number; duration: number; paused: boolean }

/** A frame not heard from in this long has paused, or is gone. */
const PAUSED_AFTER_SILENCE_MS = 4_000
/** ...and after this long, its video no longer counts (an ad's frame that was taken out). */
const FORGET_AFTER_MS = 30_000
/** An unchanged clock still goes to Discord this often, so a seek reaches the card. */
const RESEND_MS = 5_000

export function EmbedPlayer({
  server,
  title,
  playing,
  onBack,
  onClose,
}: {
  server: Server
  title: string
  playing?: { animeId: number; episode: number; title: string | null; episodeTitle: string | null }
  onBack: () => void
  onClose: () => void
}) {
  const animeId = playing?.animeId
  const episode = playing?.episode
  const series = playing?.title ?? null
  const episodeTitle = playing?.episodeTitle ?? null
  useEffect(() => {
    if (animeId == null || episode == null) return
    const embed = { animeId, episode, title: series, episodeTitle }
    void api.embedPresence(embed).catch(() => {})

    // Frames with a video post their clock each second (`embed-progress.js`). Ads play videos too,
    // so the episode is the longest video heard from lately; one that went quiet (its frame left,
    // or the page stopped reporting) is paused where it was last heard.
    const heard = new Map<MessageEventSource, { clock: EmbedClock; at: number }>()
    const onMessage = (event: MessageEvent) => {
      const clock = (event.data as { aniliEmbedProgress?: Partial<EmbedClock> } | null)?.aniliEmbedProgress
      if (!event.source || typeof clock?.position !== "number" || typeof clock.duration !== "number") return
      heard.set(event.source, { clock: { position: clock.position, duration: clock.duration, paused: !!clock.paused }, at: Date.now() })
    }
    window.addEventListener("message", onMessage)

    // Discord's worker sends a card only when it changed; this only spares it the ticks in between,
    // resending often enough for a seek to reach it.
    let sent: { duration: number; paused: boolean; at: number } | null = null
    const report = window.setInterval(() => {
      const now = Date.now()
      let best: { clock: EmbedClock; at: number } | null = null
      for (const [source, entry] of heard) {
        if (now - entry.at > FORGET_AFTER_MS) heard.delete(source)
        else if (!best || entry.clock.duration > best.clock.duration) best = entry
      }
      if (!best) return
      const paused = best.clock.paused || now - best.at > PAUSED_AFTER_SILENCE_MS
      const { position, duration } = best.clock
      if (sent && sent.duration === duration && sent.paused === paused && now - sent.at < RESEND_MS) return
      sent = { duration, paused, at: now }
      void api.embedPresence({ ...embed, position, duration, paused }).catch(() => {})
    }, 1000)

    return () => {
      window.removeEventListener("message", onMessage)
      window.clearInterval(report)
      void api.embedPresence(null).catch(() => {})
    }
  }, [animeId, episode, series, episodeTitle])
  return (
    <div className="absolute inset-0 flex flex-col bg-black">
      <div className="flex items-center gap-3 p-3">
        <Button variant="ghost" size="icon" aria-label="Back" onClick={onBack}>
          <ArrowLeftIcon />
        </Button>
        <div className="flex min-w-0 flex-col">
          <span className="truncate font-heading font-semibold">{title}</span>
          <span className="truncate text-sm text-muted-foreground">
            {server.label} · {server.name} · embedded player
          </span>
        </div>
        <Badge variant="outline" className="ml-auto gap-1">
          <ShieldCheckIcon data-icon="inline-start" />
          {/Linux/.test(navigator.userAgent) ? "Popups blocked" : "Popups and ads blocked"}
        </Badge>
        <Button variant="secondary" onClick={onClose}>
          Back to the built-in player
        </Button>
      </div>
      <iframe
        key={server.embed ?? ""}
        src={server.embed ?? undefined}
        title={`${server.label} player`}
        className="min-h-0 flex-1 border-0"
        allow="autoplay; fullscreen; encrypted-media; picture-in-picture"
        allowFullScreen
        referrerPolicy="origin"
      />
    </div>
  )
}
