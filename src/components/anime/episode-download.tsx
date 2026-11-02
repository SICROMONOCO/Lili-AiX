// An episode's download control, as Android's EpisodeDownloadButton: quiet until something is
// happening to the episode. One press queues it with the usual choices; then it is a progress ring
// (a menu stops it), an error (a menu retries it), or a tick (a menu plays, shows or removes it).

import { useNavigate } from "@tanstack/react-router"
import { CircleAlertIcon, CircleCheckIcon, DownloadIcon } from "lucide-react"
import { useState } from "react"

import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api } from "@/lib/api"
import { formatBytes } from "@/lib/downloads"
import { toastFailure } from "@/lib/failure"
import { episodeLabel } from "@/lib/format"
import type { AnimeCard, Audio, DownloadRecord } from "@/lib/types"
import { cn } from "@/lib/utils"

/** The copy an episode's control speaks for: the one in the audio shown, else a finished copy in
 * the other audio (the viewer has the episode either way). */
export function episodeDownload(rows: DownloadRecord[] | undefined, animeId: number, episode: number, audio: Audio) {
  const mine = rows?.find((r) => r.animeId === animeId && r.episode === episode && r.audio === audio)
  if (mine && mine.status !== "cancelled") return mine
  return rows?.find((r) => r.animeId === animeId && r.episode === episode && r.audio !== audio && r.status === "done")
}

export function EpisodeDownloadButton({
  anime,
  episode,
  audio,
  row,
  className,
}: {
  anime: AnimeCard
  episode: number
  audio: Audio
  row: DownloadRecord | undefined
  className?: string
}) {
  const navigate = useNavigate()
  // The press is taken before the queue has a row to show for it.
  const [pending, setPending] = useState(false)
  const act = (promise: Promise<unknown>) => promise.catch((e) => toastFailure(e))
  const base = cn(
    "surface-chip flex size-8 items-center justify-center rounded-full text-foreground/70 transition outline-none hover:bg-background hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring",
    className,
  )

  if (!row || row.status === "cancelled") {
    return (
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              aria-label={`Download ${episodeLabel(episode).toLowerCase()}`}
              className={base}
              disabled={pending}
              onClick={(e) => {
                e.stopPropagation()
                setPending(true)
                api
                  .download(anime, [episode], audio)
                  .catch((error) => toastFailure(error))
                  .finally(() => setPending(false))
              }}
            />
          }
        >
          {pending ? <Ring /> : <DownloadIcon className="size-4" />}
        </TooltipTrigger>
        <TooltipContent>Download</TooltipContent>
      </Tooltip>
    )
  }

  const active = row.status === "queued" || row.status === "downloading"
  const label =
    row.status === "done"
      ? `Downloaded${row.audio !== audio ? ` (${row.audio})` : ""}${row.bytes ? ` · ${formatBytes(row.bytes)}` : ""}`
      : row.status === "failed"
        ? "Download failed"
        : row.status === "queued"
          ? "Queued"
          : `Downloading ${Math.round(row.progress * 100)}%`
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={<button type="button" aria-label={label} className={base} onClick={(e) => e.stopPropagation()} />}
      >
        {row.status === "done" ? (
          <CircleCheckIcon className="size-4 text-primary" />
        ) : row.status === "failed" ? (
          <CircleAlertIcon className="size-4 text-destructive" />
        ) : (
          <Ring value={row.status === "downloading" ? row.progress : null} />
        )}
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" onClick={(e) => e.stopPropagation()}>
        <DropdownMenuGroup>
          <DropdownMenuLabel>{label}</DropdownMenuLabel>
          {row.status === "failed" && row.error && <DropdownMenuLabel className="font-normal text-destructive">{row.error}</DropdownMenuLabel>}
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          {row.status === "done" && (
            <>
              <DropdownMenuItem onClick={() => void navigate({ to: "/offline/$id", params: { id: String(row.id) } })}>Play offline</DropdownMenuItem>
              <DropdownMenuItem onClick={() => act(api.downloadReveal(row.id))}>Show in folder</DropdownMenuItem>
            </>
          )}
          {row.status === "failed" && <DropdownMenuItem onClick={() => act(api.downloadRetry(row.id))}>Try again</DropdownMenuItem>}
          {active && <DropdownMenuItem onClick={() => act(api.downloadCancel(row.id))}>Stop download</DropdownMenuItem>}
          {!active && (
            <DropdownMenuItem variant="destructive" onClick={() => act(api.downloadRemove(row.id, row.status === "done"))}>
              {row.status === "done" ? "Remove download" : "Forget"}
            </DropdownMenuItem>
          )}
          <DropdownMenuItem onClick={() => void navigate({ to: "/downloads" })}>Open Downloads</DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}

/** A progress ring; spinning while the amount is unknown (queued, or waiting on a server). */
export function Ring({ value = null, className }: { value?: number | null; className?: string }) {
  const r = 7
  const circumference = 2 * Math.PI * r
  return (
    <svg viewBox="0 0 18 18" className={cn("size-[18px] -rotate-90", value == null && "animate-spin", className)} aria-hidden>
      <circle cx="9" cy="9" r={r} fill="none" strokeWidth="2" className="stroke-muted-foreground/25" />
      <circle
        cx="9"
        cy="9"
        r={r}
        fill="none"
        strokeWidth="2"
        strokeLinecap="round"
        className="stroke-primary transition-[stroke-dashoffset]"
        strokeDasharray={circumference}
        strokeDashoffset={value == null ? circumference * 0.7 : circumference * (1 - Math.max(0.03, value))}
      />
    </svg>
  )
}
