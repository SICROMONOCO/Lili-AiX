// What the three Downloads pages share: the queue and the saved anime worked out from the download
// records, the header with its two tabs and the space used, and small pieces of each page.

import { Link } from "@tanstack/react-router"
import { EllipsisIcon, FolderOpenIcon, HardDriveIcon, LibraryIcon, ListOrderedIcon, PauseIcon, PlayIcon } from "lucide-react"
import { useMemo } from "react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, useDownloads, useProgress, useQueueState } from "@/lib/api"
import { formatBytes } from "@/lib/downloads"
import { toastFailure } from "@/lib/failure"
import type { DownloadRecord, Progress } from "@/lib/types"
import { cn } from "@/lib/utils"

export const act = (promise: Promise<unknown>) => promise.catch((e) => toastFailure(e))

/** One anime's downloads: what's saved, and what is still on its way. */
export type SavedSeries = {
  animeId: number
  title: string
  cover: string | null
  /** Done, in episode order. */
  episodes: DownloadRecord[]
  /** Downloading or waiting, in episode order. */
  pending: DownloadRecord[]
  /** Failed or stopped, in episode order. */
  failed: DownloadRecord[]
  bytes: number
  /** When the latest episode was saved or asked for (unix seconds): Recent sorts by it. */
  savedAt: number
  dub: boolean
  /** How much of what was asked for is here: saved episodes, plus the running ones' progress. */
  fraction: number
}

/** The download records, worked out for the pages. */
export function useDownloadsView() {
  const downloads = useDownloads()
  const queue = useQueueState().data
  return useMemo(() => {
    const rows = downloads.data ?? []
    // The worker runs two at a time.
    const running = rows.filter((r) => r.status === "downloading")
    // Oldest first, as the worker takes them.
    const queued = rows.filter((r) => r.status === "queued").sort((a, b) => a.createdAt - b.createdAt || a.episode - b.episode)
    const failed = rows.filter((r) => r.status === "failed" || r.status === "cancelled")
    // Every anime with something saved or on its way; failures alone don't make a card.
    const byAnime = new Map<number, DownloadRecord[]>()
    for (const row of rows) {
      byAnime.set(row.animeId, [...(byAnime.get(row.animeId) ?? []), row])
    }
    const byEpisode = (a: DownloadRecord, b: DownloadRecord) => a.episode - b.episode
    const series: SavedSeries[] = [...byAnime.values()].flatMap((list) => {
      const episodes = list.filter((r) => r.status === "done").sort(byEpisode)
      const pending = list.filter((r) => r.status === "downloading" || r.status === "queued").sort(byEpisode)
      if (episodes.length === 0 && pending.length === 0) return []
      const latest = list.reduce((a, b) => (b.updatedAt > a.updatedAt ? b : a))
      const asked = episodes.length + pending.length
      const progress = episodes.length + pending.reduce((sum, r) => sum + (r.status === "downloading" ? r.progress : 0), 0)
      return [
        {
          animeId: latest.animeId,
          title: latest.title,
          cover: list.find((r) => r.cover)?.cover ?? null,
          episodes,
          pending,
          failed: list.filter((r) => r.status === "failed" || r.status === "cancelled").sort(byEpisode),
          bytes: episodes.reduce((sum, r) => sum + r.bytes, 0),
          savedAt: latest.updatedAt,
          dub: list.some((e) => e.audio === "dub"),
          fraction: asked > 0 ? progress / asked : 1,
        },
      ]
    })
    series.sort((a, b) => b.savedAt - a.savedAt)
    return {
      loading: downloads.isPending,
      rows,
      running,
      queued,
      failed,
      series,
      pending: running.length + queued.length,
      savedBytes: series.reduce((sum, s) => sum + s.bytes, 0),
      savedEpisodes: series.reduce((sum, s) => sum + s.episodes.length, 0),
      /** Anime with something saved (the Library's own count). */
      savedSeries: series.filter((s) => s.episodes.length > 0).length,
      queue,
    }
  }, [downloads.data, downloads.isPending, queue])
}

/** Watched through, as the app counts it everywhere (Rust `Progress::finished`). */
export const finished = (p: Progress | undefined) => !!p && p.duration > 0 && p.position >= p.duration * 0.9

/** An anime's progress by episode. */
export function useEpisodeProgress(animeId: number) {
  const progress = useProgress(animeId).data
  return useMemo(() => new Map((progress ?? []).map((p) => [p.episode, p])), [progress])
}

/** The saved episode to play next: the first not watched through, else the first. */
export function nextToPlay(episodes: DownloadRecord[], progress: Map<number, Progress>): DownloadRecord | undefined {
  return episodes.find((e) => !finished(progress.get(e.episode))) ?? episodes[0]
}

/** "7–10, 12": episode numbers as runs. */
export function episodeRanges(numbers: number[]): string {
  const sorted = [...new Set(numbers)].sort((a, b) => a - b)
  const runs: string[] = []
  for (let i = 0; i < sorted.length; ) {
    let j = i
    while (j + 1 < sorted.length && sorted[j + 1] === sorted[j] + 1) j++
    runs.push(j > i ? `${sorted[i]}–${sorted[j]}` : String(sorted[i]))
    i = j + 1
  }
  return runs.join(", ")
}

/**
 * How much of an anime's download is done, as a ring: the saved share, the running episodes' progress
 * included. Paused, it says so in the middle.
 */
export function ProgressRing({ fraction, paused, size = 40, className }: { fraction: number; paused?: boolean; size?: number; className?: string }) {
  const stroke = size >= 56 ? 5 : 3.5
  const r = (size - stroke) / 2
  const c = 2 * Math.PI * r
  const percent = Math.round(Math.min(1, Math.max(0, fraction)) * 100)
  return (
    <span
      role="img"
      aria-label={`${percent}% downloaded${paused ? ", paused" : ""}`}
      title={`${percent}% downloaded${paused ? " · paused" : ""}`}
      className={cn("relative inline-flex shrink-0 items-center justify-center rounded-full bg-black/78 text-white", className)}
      style={{ width: size, height: size }}
    >
      <svg width={size} height={size} className="absolute inset-0 -rotate-90" aria-hidden>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="currentColor" strokeOpacity={0.2} strokeWidth={stroke} />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke="var(--primary)"
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={c}
          strokeDashoffset={c * (1 - percent / 100)}
          className="transition-[stroke-dashoffset] duration-500"
        />
      </svg>
      {paused ? (
        <PauseIcon className={cn("relative", size >= 56 ? "size-5" : "size-3.5")} />
      ) : (
        <span className={cn("relative font-bold tabular-nums", size >= 56 ? "text-sm" : "text-[0.6rem]")}>
          {percent}
          {size >= 56 && <span className="text-[0.6rem] opacity-70">%</span>}
        </span>
      )}
    </span>
  )
}

export function Poster({ src, className }: { src: string | null; className?: string }) {
  return src ? (
    <img src={src} alt="" loading="lazy" className={cn("rounded-lg bg-muted object-cover", className)} />
  ) : (
    <div className={cn("rounded-lg bg-muted", className)} />
  )
}

export function IconAction({ label, onClick, children }: { label: string; onClick: () => unknown; children: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger render={<Button variant="ghost" size="icon-sm" aria-label={label} onClick={onClick} />}>{children}</TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

/**
 * The top of the Library and Queue pages: the title, the two tabs (each with its count), the queue's
 * pause and the page's actions, and the space downloads take beside what is left on the disk.
 */
export function DownloadsHeader({ tab }: { tab: "library" | "queue" }) {
  const view = useDownloadsView()
  const { queue, pending, failed } = view
  const used = view.savedBytes
  const free = queue?.freeBytes ?? null
  const share = free != null && used + free > 0 ? used / (used + free) : null
  return (
    <header className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <h1 className="font-heading text-2xl font-bold">Downloads</h1>
        <nav className="surface-control ml-2 flex rounded-xl border p-1" aria-label="Downloads">
          <TabLink to="/downloads" active={tab === "library"} icon={<LibraryIcon />} label="Library" count={view.savedEpisodes} />
          <TabLink to="/downloads/queue" active={tab === "queue"} icon={<ListOrderedIcon />} label="Queue" count={pending + failed.length} />
        </nav>
        <div className="ml-auto flex items-center gap-2">
          {pending > 0 && queue && (
            <Button variant="outline" onClick={() => act(api.downloadsPause(!queue.paused))}>
              {queue.paused ? <PlayIcon data-icon="inline-start" /> : <PauseIcon data-icon="inline-start" />}
              {queue.paused ? "Resume" : "Pause"}
            </Button>
          )}
          <Button variant="outline" onClick={() => act(api.downloadsOpenFolder())}>
            <FolderOpenIcon data-icon="inline-start" />
            Open folder
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button variant="outline" size="icon" aria-label="More" />}>
              <EllipsisIcon />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuGroup>
                <DropdownMenuItem disabled={failed.length === 0} onClick={() => act(api.downloadsRetryFailed())}>
                  Retry failed and stopped
                </DropdownMenuItem>
                <DropdownMenuItem disabled={failed.length === 0} onClick={() => act(api.downloadsClearUnfinished())}>
                  Forget failed and stopped
                </DropdownMenuItem>
              </DropdownMenuGroup>
              <DropdownMenuSeparator />
              <DropdownMenuGroup>
                <DropdownMenuItem variant="destructive" disabled={pending === 0} onClick={() => act(api.downloadsCancelAll())}>
                  Stop everything in the queue
                </DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      {queue && (
        <div className="flex flex-col gap-1.5">
          <div className="flex items-center gap-2 text-xs text-muted-foreground">
            <HardDriveIcon className="size-3.5 shrink-0" />
            <span>
              <span className="font-medium text-foreground">{formatBytes(used)}</span> in downloads
              {free != null && ` · ${formatBytes(free)} free`}
            </span>
            <span className="min-w-0 truncate">· {queue.folder}</span>
            <Button variant="link" size="xs" className="px-0" nativeButton={false} render={<Link to="/settings" search={{ section: "downloads" }} />}>
              Change
            </Button>
          </div>
          {share != null && (
            <div className="h-1.5 w-full max-w-md overflow-hidden rounded-full bg-muted" title="Downloads beside the space left on this disk">
              <div className="h-full rounded-full bg-primary" style={{ width: `${Math.max(1, Math.round(share * 100))}%` }} />
            </div>
          )}
        </div>
      )}
    </header>
  )
}

function TabLink({ to, active, icon, label, count }: { to: "/downloads" | "/downloads/queue"; active: boolean; icon: React.ReactNode; label: string; count: number }) {
  return (
    <Link
      to={to}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-8 items-center gap-1.5 rounded-lg px-3 text-sm font-medium transition [&_svg]:size-4",
        active ? "bg-primary text-primary-foreground" : "text-muted-foreground hover:bg-muted hover:text-foreground",
      )}
    >
      {icon}
      {label}
      {count > 0 && (
        <Badge variant={active ? "outline" : "secondary"} className={cn("h-5 min-w-5 px-1.5 tabular-nums", active && "border-primary-foreground/40 text-primary-foreground")}>
          {count}
        </Badge>
      )}
    </Link>
  )
}
