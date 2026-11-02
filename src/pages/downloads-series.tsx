// Downloads → one anime: every episode of it the app holds or is fetching, in episode order. Saved
// ones with their stills and titles, how far through each you are and what was fetched; the ones
// downloading or waiting with their progress or place in the queue; failed ones with why. As a list
// or a grid. Play picks up at the first saved episode not watched through; saved episodes can be
// selected and deleted together, as on Android, and Remove all takes the whole anime out of
// downloads at once.

import { Link, useNavigate } from "@tanstack/react-router"
import {
  ArrowLeftIcon,
  CheckIcon,
  ClockIcon,
  FolderOpenIcon,
  InfoIcon,
  LayoutGridIcon,
  ListIcon,
  PlayIcon,
  RotateCwIcon,
  Trash2Icon,
  TriangleAlertIcon,
  XIcon,
} from "lucide-react"
import { useState } from "react"

import { DeleteDialog, RemoveAllDialog } from "@/components/downloads/delete-dialog"
import { IconAction, Poster, ProgressRing, act, finished, nextToPlay, useDownloadsView, useEpisodeProgress } from "@/components/downloads/shared"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { Progress as Bar } from "@/components/ui/progress"
import { Skeleton } from "@/components/ui/skeleton"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { api, useAppearance, useDetail, useEpisodes } from "@/lib/api"
import { formatBytes, formatEta, formatSpeed } from "@/lib/downloads"
import { episodeLabel } from "@/lib/format"
import type { DownloadRecord, Episode, Progress } from "@/lib/types"
import { cn } from "@/lib/utils"

const LAYOUT_KEY = "downloads.episodes.layout"
type Layout = "list" | "grid"

/** What a row needs to draw any kind of episode. */
type Item = {
  row: DownloadRecord
  info?: Episode
  progress?: Progress
  /** 1-based place in the queue, for a waiting episode. */
  place?: number
  paused: boolean
}

export function DownloadSeriesPage({ animeId }: { animeId: number }) {
  const navigate = useNavigate()
  const view = useDownloadsView()
  const series = view.series.find((s) => s.animeId === animeId)
  const failed = view.failed.filter((r) => r.animeId === animeId)
  const detail = useDetail(animeId).data
  // Stills and titles, when the catalogue answers; offline, the rows go without.
  const episodeInfo = useEpisodes(animeId).data
  const infoOf = new Map((episodeInfo?.episodes ?? []).map((e) => [e.number, e]))
  const progress = useEpisodeProgress(animeId)
  const [layout, setLayout] = useState<Layout>(() => (localStorage.getItem(LAYOUT_KEY) === "grid" ? "grid" : "list"))
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [deleting, setDeleting] = useState<DownloadRecord[] | null>(null)
  const [removingAll, setRemovingAll] = useState(false)

  if (view.loading) return <Skeleton className="m-8 h-80 rounded-2xl" />
  if (!series && failed.length === 0) {
    return (
      <div className="flex flex-col gap-6 p-8">
        <BackLink />
        <Empty className="border">
          <EmptyHeader>
            <EmptyTitle>No saved episodes</EmptyTitle>
            <EmptyDescription>Everything saved for this title was deleted.</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button variant="outline" nativeButton={false} render={<Link to="/downloads" />}>
              Downloads
            </Button>
          </EmptyContent>
        </Empty>
      </div>
    )
  }

  const saved = series?.episodes ?? []
  const pending = series?.pending ?? []
  const title = series?.title ?? failed[0].title
  const cover = series?.cover ?? failed.find((r) => r.cover)?.cover ?? null
  const paused = !!view.queue?.paused
  const placeOf = new Map(view.queued.map((r, i) => [r.id, i + 1]))
  // Saved, on its way, or failed: one list in episode order.
  const items: Item[] = [...saved, ...pending, ...failed]
    .sort((a, b) => a.episode - b.episode || order(a) - order(b))
    .map((row) => ({ row, info: infoOf.get(row.episode), progress: progress.get(row.episode), place: placeOf.get(row.id), paused }))

  const next = nextToPlay(saved, progress)
  const watched = saved.filter((e) => finished(progress.get(e.episode))).length
  const play = (row: DownloadRecord) => void navigate({ to: "/offline/$id", params: { id: String(row.id) } })
  const chosen = saved.filter((e) => selected.has(e.id))
  const toggle = (id: number) => {
    const next = new Set(selected)
    if (next.has(id)) next.delete(id)
    else next.add(id)
    setSelected(next)
  }
  const switchLayout = (next: Layout) => {
    localStorage.setItem(LAYOUT_KEY, next)
    setLayout(next)
  }
  const heights = [...new Set(saved.map((e) => e.height).filter(Boolean))]
  const art = detail?.banner ?? cover
  const facts = [
    saved.length > 0 ? `${saved.length} episode${saved.length === 1 ? "" : "s"} saved · ${formatBytes(series?.bytes ?? 0)}` : null,
    pending.length > 0 ? `${pending.length} downloading` : null,
    failed.length > 0 ? `${failed.length} failed` : null,
    heights.length > 0 ? heights.map((h) => `${h}p`).join(", ") : null,
    watched > 0 ? `${watched} watched` : null,
  ].filter(Boolean)
  const actions = {
    onToggle: toggle,
    onPlay: play,
    onDelete: (row: DownloadRecord) => setDeleting([row]),
  }

  return (
    <div className="flex flex-col gap-6 p-8 pb-24">
      <BackLink />

      <section className="surface relative overflow-hidden rounded-2xl">
        {art && (
          <img
            src={art}
            alt=""
            aria-hidden
            className={cn("absolute inset-0 size-full object-cover opacity-35 [mask-image:linear-gradient(to_right,transparent,black_60%)]", !detail?.banner && "blur-2xl")}
          />
        )}
        <div className="relative flex items-end gap-5 p-5">
          <Link to="/anime/$id" params={{ id: String(animeId) }} className="relative shrink-0">
            <Poster src={cover} className="aspect-[2/3] w-28 shadow-xl" />
            {pending.length > 0 && series && <ProgressRing fraction={series.fraction} paused={paused} size={44} className="absolute -right-3 -bottom-3 shadow-lg" />}
          </Link>
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <h1 className="line-clamp-2 font-heading text-2xl font-bold">{title}</h1>
            <p className="text-sm text-muted-foreground">{facts.join(" · ")}</p>
            <div className="flex flex-wrap items-center gap-2 pt-1">
              {next && (
                <Button onClick={() => play(next)}>
                  <PlayIcon data-icon="inline-start" className="fill-current" />
                  {watched === 0 || watched >= saved.length ? "Play" : "Continue"} {episodeLabel(next.episode)}
                </Button>
              )}
              <Button variant="outline" nativeButton={false} render={<Link to="/anime/$id" params={{ id: String(animeId) }} />}>
                <InfoIcon data-icon="inline-start" />
                Title page
              </Button>
              {saved.length > 0 && (
                <Button variant="outline" onClick={() => act(api.downloadReveal(saved[0].id))}>
                  <FolderOpenIcon data-icon="inline-start" />
                  Show in folder
                </Button>
              )}
              <Button variant="ghost" className="text-destructive" onClick={() => setRemovingAll(true)}>
                <Trash2Icon data-icon="inline-start" />
                Remove all
              </Button>
            </div>
          </div>
        </div>
      </section>

      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-3 px-1">
          {saved.length > 0 && (
            <Checkbox
              aria-label="Select every saved episode"
              checked={chosen.length === saved.length}
              indeterminate={chosen.length > 0 && chosen.length < saved.length}
              onCheckedChange={(on) => setSelected(on ? new Set(saved.map((e) => e.id)) : new Set())}
            />
          )}
          <h2 className="font-heading text-sm font-semibold text-muted-foreground">Episodes</h2>
          <ToggleGroup variant="outline" size="sm" className="ml-auto" value={[layout]} onValueChange={(v) => v[0] && switchLayout(v[0] as Layout)}>
            <ToggleGroupItem value="list" aria-label="List">
              <ListIcon />
            </ToggleGroupItem>
            <ToggleGroupItem value="grid" aria-label="Grid">
              <LayoutGridIcon />
            </ToggleGroupItem>
          </ToggleGroup>
        </div>
        {layout === "list" ? (
          <div className="surface flex flex-col divide-y rounded-2xl">
            {items.map((item) => (
              <EpisodeRow key={item.row.id} item={item} selected={selected.has(item.row.id)} {...actions} />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-3">
            {items.map((item) => (
              <EpisodeCard key={item.row.id} item={item} selected={selected.has(item.row.id)} {...actions} />
            ))}
          </div>
        )}
      </section>

      {chosen.length > 0 && (
        <div className="surface-bar fixed inset-x-0 bottom-6 z-20 mx-auto flex w-fit items-center gap-3 rounded-2xl border px-4 py-2.5 shadow-2xl">
          <span className="text-sm">
            <span className="font-semibold">{chosen.length}</span> selected · {formatBytes(chosen.reduce((sum, e) => sum + e.bytes, 0))}
          </span>
          <Button variant="ghost" size="sm" onClick={() => setSelected(new Set())}>
            Clear
          </Button>
          <Button variant="destructive" size="sm" onClick={() => setDeleting(chosen)}>
            <Trash2Icon data-icon="inline-start" />
            Delete
          </Button>
        </div>
      )}

      {deleting && (
        <DeleteDialog
          episodes={deleting}
          title={title}
          onClose={() => setDeleting(null)}
          onDeleted={() => setSelected(new Set([...selected].filter((id) => !deleting.some((d) => d.id === id))))}
        />
      )}
      {removingAll && (
        <RemoveAllDialog title={title} saved={saved} pending={pending} failed={failed} onClose={() => setRemovingAll(false)} />
      )}
    </div>
  )
}

/** Within one episode number: saved first, then on its way, then failed. */
function order(row: DownloadRecord): number {
  return row.status === "done" ? 0 : row.status === "downloading" ? 1 : row.status === "queued" ? 2 : 3
}

function BackLink() {
  return (
    <Link to="/downloads" className="flex w-fit items-center gap-1.5 text-sm text-muted-foreground hover:text-foreground">
      <ArrowLeftIcon className="size-4" />
      Downloads
    </Link>
  )
}

type Actions = {
  selected: boolean
  onToggle: (id: number) => void
  onPlay: (row: DownloadRecord) => void
  onDelete: (row: DownloadRecord) => void
}

/** The episode's still (saved ones play from it), with its watch or download progress along the foot. */
function Still({ item, onPlay, className }: { item: Item; onPlay: (row: DownloadRecord) => void; className?: string }) {
  const { blurEpisodeImages } = useAppearance()
  const { row, info, progress } = item
  const saved = row.status === "done"
  const done = finished(progress)
  const watchedFraction = saved && progress && progress.duration > 0 ? Math.min(1, progress.position / progress.duration) : 0
  const image = info?.thumbnail ? (
    <img src={info.thumbnail} alt="" loading="lazy" className={cn("aspect-video w-full object-cover", blurEpisodeImages && !done && "blur-md", !saved && "opacity-50")} />
  ) : (
    <div className="flex aspect-video w-full items-center justify-center bg-muted text-xs text-muted-foreground">{episodeLabel(row.episode)}</div>
  )
  const foot =
    row.status === "downloading" ? (
      <span className="absolute inset-x-0 bottom-0 h-1 bg-black/50">
        <span className="block h-full bg-primary transition-[width]" style={{ width: `${row.progress * 100}%` }} />
      </span>
    ) : watchedFraction > 0 && !done ? (
      <span className="absolute inset-x-0 bottom-0 h-1 bg-black/50">
        <span className="block h-full bg-primary" style={{ width: `${watchedFraction * 100}%` }} />
      </span>
    ) : null
  if (!saved) {
    return (
      <div className={cn("relative overflow-hidden rounded-lg", className)}>
        {image}
        {foot}
      </div>
    )
  }
  return (
    <button
      type="button"
      onClick={() => onPlay(row)}
      className={cn("group/still relative overflow-hidden rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-ring", className)}
      aria-label={`Play ${episodeLabel(row.episode)}`}
    >
      {image}
      <span className="absolute inset-0 flex items-center justify-center bg-black/40 opacity-0 transition group-hover/still:opacity-100">
        <PlayIcon className="size-7 fill-white text-white" />
      </span>
      {foot}
    </button>
  )
}

/** What the episode is doing, in a line: saved details, download progress, its place, or the failure. */
function Status({ item }: { item: Item }) {
  const { row, progress, place, paused } = item
  if (row.status === "done") {
    const details = [row.height ? `${row.height}p` : null, row.server, formatBytes(row.bytes)].filter(Boolean).join(" · ")
    return (
      <span className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
        {details}
        {finished(progress) && (
          <Badge variant="secondary" className="gap-1">
            <CheckIcon />
            Watched
          </Badge>
        )}
      </span>
    )
  }
  if (row.status === "downloading") {
    const percent = Math.round(row.progress * 100)
    const details = [`${percent}%`, row.speed ? formatSpeed(row.speed) : null, row.eta != null ? `${formatEta(row.eta)} left` : null].filter(Boolean)
    return (
      <span className="flex flex-col gap-1">
        <Bar value={percent} className="h-1.5" />
        <span className="text-xs text-primary tabular-nums">
          {row.progress === 0 && !row.bytes ? "Finding a server…" : details.join(" · ")}
          {paused && " · pausing after this"}
        </span>
      </span>
    )
  }
  if (row.status === "queued") {
    return (
      <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <ClockIcon className="size-3.5" />
        {paused ? "Waiting · the queue is paused" : place ? `Waiting · ${ordinal(place)} in the queue` : "Waiting"}
      </span>
    )
  }
  return (
    <span className="flex items-center gap-1.5 text-xs text-destructive" title={row.error ?? undefined}>
      <TriangleAlertIcon className="size-3.5 shrink-0" />
      <span className="truncate">{row.status === "cancelled" ? "Stopped" : (row.error ?? "Failed")}</span>
    </span>
  )
}

/** The buttons an episode has in its state. */
function EpisodeActions({ row, onPlay, onDelete }: { row: DownloadRecord; onPlay: (row: DownloadRecord) => void; onDelete: (row: DownloadRecord) => void }) {
  if (row.status === "done") {
    return (
      <>
        <IconAction label="Play" onClick={() => onPlay(row)}>
          <PlayIcon />
        </IconAction>
        <IconAction label="Show in folder" onClick={() => act(api.downloadReveal(row.id))}>
          <FolderOpenIcon />
        </IconAction>
        <IconAction label="Delete file" onClick={() => onDelete(row)}>
          <Trash2Icon />
        </IconAction>
      </>
    )
  }
  if (row.status === "downloading" || row.status === "queued") {
    return (
      <IconAction label={row.status === "downloading" ? "Stop" : "Remove from the queue"} onClick={() => act(api.downloadCancel(row.id))}>
        <XIcon />
      </IconAction>
    )
  }
  return (
    <>
      <IconAction label="Try again" onClick={() => act(api.downloadRetry(row.id))}>
        <RotateCwIcon />
      </IconAction>
      <IconAction label="Forget" onClick={() => act(api.downloadRemove(row.id, false))}>
        <Trash2Icon />
      </IconAction>
    </>
  )
}

function EpisodeRow({ item, selected, onToggle, onPlay, onDelete }: { item: Item } & Actions) {
  const { row, info } = item
  const saved = row.status === "done"
  return (
    <div className={cn("group flex items-center gap-4 px-4 py-3 transition", selected && "bg-primary/5")}>
      {saved ? (
        <Checkbox checked={selected} onCheckedChange={() => onToggle(row.id)} aria-label={`Select ${episodeLabel(row.episode)}`} />
      ) : (
        <span className="size-4 shrink-0" aria-hidden />
      )}
      <Still item={item} onPlay={onPlay} className="w-40 shrink-0" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="flex items-center gap-2">
          <span className={cn("font-medium", !saved && "text-muted-foreground")}>{episodeLabel(row.episode)}</span>
          {row.audio === "dub" && <Badge variant="outline">Dub</Badge>}
        </span>
        {info?.title && (
          <span className="truncate text-sm text-muted-foreground" title={info.title}>
            {info.title}
          </span>
        )}
        <Status item={item} />
      </div>
      <div className="flex items-center gap-1 opacity-60 transition group-hover:opacity-100">
        <EpisodeActions row={row} onPlay={onPlay} onDelete={onDelete} />
      </div>
    </div>
  )
}

function EpisodeCard({ item, selected, onToggle, onPlay, onDelete }: { item: Item } & Actions) {
  const { row, info } = item
  const saved = row.status === "done"
  return (
    // The title page's episode cards: still with its badges and a quiet action in the corner,
    // name and state under, no box around the whole thing.
    <div className="group relative flex flex-col gap-2">
      <div className="relative">
        <Still item={item} onPlay={onPlay} className={cn("w-full", selected && "ring-2 ring-primary")} />
        {saved && (
          <span className="absolute top-2 left-2 rounded bg-black/60 p-0.5">
            <Checkbox checked={selected} onCheckedChange={() => onToggle(row.id)} aria-label={`Select ${episodeLabel(row.episode)}`} />
          </span>
        )}
        {row.audio === "dub" && (
          <span className={cn("absolute top-2 rounded-md bg-black/78 px-1.5 py-0.5 text-[0.7rem] font-semibold text-white", saved ? "left-8" : "left-2")}>
            DUB
          </span>
        )}
        <div className="absolute top-1.5 right-1.5 flex gap-0.5 rounded-lg bg-black/60 p-0.5 opacity-0 transition group-hover:opacity-100 group-focus-within:opacity-100">
          <EpisodeActions row={row} onPlay={onPlay} onDelete={onDelete} />
        </div>
      </div>
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className={cn("truncate text-sm font-medium", !saved && "text-muted-foreground")}>{episodeLabel(row.episode)}</span>
        {info?.title && (
          <span className="truncate text-xs text-muted-foreground" title={info.title}>
            {info.title}
          </span>
        )}
        <Status item={item} />
      </div>
    </div>
  )
}

/** 1st, 2nd, 3rd, 4th… */
function ordinal(n: number): string {
  const tens = n % 100
  const suffix = tens >= 11 && tens <= 13 ? "th" : ({ 1: "st", 2: "nd", 3: "rd" } as Record<number, string>)[n % 10] ?? "th"
  return `${n}${suffix}`
}
