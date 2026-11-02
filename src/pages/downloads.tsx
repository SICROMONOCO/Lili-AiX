// Downloads → Library: the saved anime as posters, as Android's Downloads screen and every other list
// in the app draws them, with how many episodes are here, the space they take and how far through
// them you are. Filters (watched or not), a sort and a title search above; while something is
// downloading, a strip pointing at the Queue. An anime opens its own page (downloads-series.tsx).

import { useQueries } from "@tanstack/react-query"
import { Link, useNavigate } from "@tanstack/react-router"
import { ArrowRightIcon, DownloadIcon, FolderOpenIcon, InfoIcon, PlayIcon, SearchIcon, Trash2Icon, XIcon } from "lucide-react"
import { useState } from "react"

import { CardSkeleton } from "@/components/anime/anime-row"
import { PosterTag } from "@/components/anime/poster-tag"
import { RemoveAllDialog } from "@/components/downloads/delete-dialog"
import { DownloadsHeader, Poster, ProgressRing, act, finished, nextToPlay, type SavedSeries, useDownloadsView } from "@/components/downloads/shared"
import { Button } from "@/components/ui/button"
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuSeparator, ContextMenuTrigger } from "@/components/ui/context-menu"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group"
import { Progress as Bar } from "@/components/ui/progress"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { api, keys } from "@/lib/api"
import { formatBytes } from "@/lib/downloads"
import { episodeLabel } from "@/lib/format"
import type { Progress } from "@/lib/types"
import { cn } from "@/lib/utils"

type View = "all" | "unwatched" | "watched"
type Sort = "recent" | "title" | "largest" | "episodes"

const VIEWS: { value: View; label: string }[] = [
  { value: "all", label: "All" },
  { value: "unwatched", label: "Unwatched" },
  { value: "watched", label: "Watched" },
]
const SORTS: { value: Sort; label: string }[] = [
  { value: "recent", label: "Recently saved" },
  { value: "title", label: "Title" },
  { value: "largest", label: "Largest" },
  { value: "episodes", label: "Most episodes" },
]

export function DownloadsPage() {
  const view = useDownloadsView()
  const [text, setText] = useState("")
  const [shown, setShown] = useState<View>("all")
  const [sort, setSort] = useState<Sort>("recent")
  // Each anime's progress, for the watched badges and the filter.
  const progressQueries = useQueries({
    queries: view.series.map((s) => ({ queryKey: keys.progress(s.animeId), queryFn: () => api.progressFor(s.animeId) })),
  })
  const progressOf = new Map(view.series.map((s, i) => [s.animeId, new Map((progressQueries[i]?.data ?? []).map((p) => [p.episode, p]))]))
  const watchedOf = (s: SavedSeries) => s.episodes.filter((e) => finished(progressOf.get(s.animeId)?.get(e.episode))).length
  // Watched through: every saved episode, and at least one saved (one still downloading isn't).
  const allWatched = (s: SavedSeries) => s.episodes.length > 0 && watchedOf(s) >= s.episodes.length

  const needle = text.trim().toLowerCase()
  const list = view.series
    .filter((s) => !needle || s.title.toLowerCase().includes(needle))
    .filter((s) => shown === "all" || (shown === "watched") === allWatched(s))
  const sorted =
    sort === "title"
      ? [...list].sort((a, b) => a.title.localeCompare(b.title))
      : sort === "largest"
        ? [...list].sort((a, b) => b.bytes - a.bytes)
        : sort === "episodes"
          ? [...list].sort((a, b) => b.episodes.length - a.episodes.length)
          : list

  return (
    <div className="flex flex-col gap-6 p-8">
      <DownloadsHeader tab="library" />
      <ActiveStrip />

      {view.loading ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
          {Array.from({ length: 12 }, (_, i) => (
            <CardSkeleton key={i} />
          ))}
        </div>
      ) : view.series.length === 0 ? (
        <Empty className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <DownloadIcon />
            </EmptyMedia>
            <EmptyTitle>Nothing saved yet</EmptyTitle>
            <EmptyDescription>
              {view.pending > 0
                ? "Episodes appear here as they finish downloading."
                : "Use “Download” on a title's page, or the arrow on an episode, to watch it without a connection."}
            </EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button nativeButton={false} render={<Link to="/search" />}>
              Browse anime
            </Button>
          </EmptyContent>
        </Empty>
      ) : (
        <>
          <div className="flex flex-wrap items-center gap-3">
            <InputGroup className="w-64">
              <InputGroupAddon>
                <SearchIcon />
              </InputGroupAddon>
              <InputGroupInput placeholder="Search downloads" aria-label="Search downloads" value={text} onChange={(e) => setText(e.target.value)} />
              {text && (
                <InputGroupAddon align="inline-end">
                  <InputGroupButton size="icon-xs" aria-label="Clear" onClick={() => setText("")}>
                    <XIcon />
                  </InputGroupButton>
                </InputGroupAddon>
              )}
            </InputGroup>
            <div className="flex gap-1.5" role="group" aria-label="Show">
              {VIEWS.map((v) => {
                const count = v.value === "all" ? view.series.length : view.series.filter((s) => (v.value === "watched") === allWatched(s)).length
                return (
                  <button
                    key={v.value}
                    type="button"
                    aria-pressed={shown === v.value}
                    onClick={() => setShown(v.value)}
                    className={cn(
                      "h-8 rounded-full border px-3.5 text-sm transition",
                      shown === v.value ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
                    )}
                  >
                    {v.label} <span className="opacity-60 tabular-nums">{count}</span>
                  </button>
                )
              })}
            </div>
            <Select items={SORTS} value={sort} onValueChange={(v) => v && setSort(v as Sort)}>
              <SelectTrigger className="ml-auto w-44" aria-label="Sort">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {SORTS.map((s) => (
                    <SelectItem key={s.value} value={s.value}>
                      {s.label}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>

          {sorted.length === 0 ? (
            <p className="py-10 text-center text-sm text-muted-foreground">Nothing here matches.</p>
          ) : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
              {sorted.map((s) => (
                <SeriesTile key={s.animeId} series={s} progress={progressOf.get(s.animeId) ?? new Map()} watched={watchedOf(s)} paused={!!view.queue?.paused} />
              ))}
            </div>
          )}
        </>
      )}
    </div>
  )
}

/** While something downloads: what, how far, and the way to the Queue. */
function ActiveStrip() {
  const { running, queued, queue } = useDownloadsView()
  const first = running[0] ?? queued[0]
  if (!first) return null
  const more = running.length + queued.length - 1
  const percent = running[0] ? Math.round(running[0].progress * 100) : 0
  return (
    <Link
      to="/downloads/queue"
      className="surface group flex items-center gap-3 rounded-xl p-2.5 pr-4 transition hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
    >
      <Poster src={first.cover} className="aspect-[2/3] w-9 shrink-0" />
      <div className="flex min-w-0 flex-1 flex-col gap-1.5">
        <span className="truncate text-sm">
          <span className="font-medium">{queue?.paused ? "Paused" : running.length ? "Downloading" : "Waiting"}</span>{" "}
          <span className="text-muted-foreground">
            {first.title} · {episodeLabel(first.episode)}
            {running[0] ? ` · ${percent}%` : ""}
            {more > 0 ? ` · ${more} more` : ""}
          </span>
        </span>
        {running[0] && <Bar value={percent} className="h-1" />}
      </div>
      <span className="flex items-center gap-1 text-sm text-muted-foreground group-hover:text-foreground">
        Queue
        <ArrowRightIcon className="size-4" />
      </span>
    </Link>
  )
}

function SeriesTile({ series, progress, watched, paused }: { series: SavedSeries; progress: Map<number, Progress>; watched: number; paused: boolean }) {
  const navigate = useNavigate()
  const [removing, setRemoving] = useState(false)
  const next = nextToPlay(series.episodes, progress)
  const all = series.episodes.length > 0 && watched >= series.episodes.length
  const downloading = series.pending.length > 0
  const play = () => next && void navigate({ to: "/offline/$id", params: { id: String(next.id) } })
  const open = () => void navigate({ to: "/downloads/$animeId", params: { animeId: String(series.animeId) } })
  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger className="contents" onContextMenu={(e) => e.stopPropagation()}>
          <div className="group flex min-w-0 flex-col gap-2">
            <div className="relative">
              <button
                type="button"
                onClick={open}
                className="block w-full overflow-hidden rounded-lg bg-muted ring-ring/60 transition outline-none group-hover:ring-2 focus-visible:ring-2"
                aria-label={`${series.title}: ${series.episodes.length} saved episodes${downloading ? `, ${series.pending.length} downloading` : ""}`}
              >
                <Poster src={series.cover} className="aspect-[2/3] w-full transition duration-300 group-hover:scale-105" />
              </button>
              <span className="pointer-events-none absolute top-1.5 left-1.5 flex gap-1">
                {series.episodes.length > 0 && (
                  <PosterTag label={`${series.episodes.length} saved episodes`}>
                    {series.episodes.length} ep{series.episodes.length === 1 ? "" : "s"}
                  </PosterTag>
                )}
              </span>
              {series.dub && (
                <PosterTag label="Some episodes saved in dub" className="pointer-events-none absolute bottom-1.5 left-1.5">
                  DUB
                </PosterTag>
              )}
              {downloading && (
                // In the middle, over a darkened poster: the card's news while it is still arriving.
                <span className="pointer-events-none absolute inset-0 flex items-center justify-center rounded-lg bg-black/35">
                  <ProgressRing fraction={series.fraction} paused={paused} size={64} className="shadow-xl" />
                </span>
              )}
              <button
                type="button"
                onClick={() => setRemoving(true)}
                aria-label={`Remove all downloads of ${series.title}`}
                title="Remove every episode of this anime from downloads"
                className="surface-chip absolute top-1.5 right-1.5 flex size-7.5 items-center justify-center rounded-full text-white opacity-0 shadow-lg outline-none transition group-hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-3 focus-visible:ring-ring/60 [&_svg]:size-4"
              >
                <Trash2Icon />
              </button>
              {all ? (
                <PosterTag tone="accent" outlined={false} className="pointer-events-none absolute right-1.5 bottom-2.5">
                  Watched
                </PosterTag>
              ) : (
                next && (
                  <button
                    type="button"
                    onClick={play}
                    aria-label={`Play ${episodeLabel(next.episode)}`}
                    title={`Play ${episodeLabel(next.episode)}`}
                    className="surface-chip absolute right-1.5 bottom-2.5 flex size-9 items-center justify-center rounded-full text-white opacity-0 shadow-lg outline-none transition group-hover:opacity-100 focus-visible:opacity-100 focus-visible:ring-3 focus-visible:ring-ring/60"
                  >
                    <PlayIcon className="size-5 translate-x-px fill-current" />
                  </button>
                )
              )}
              {watched > 0 && !all && (
                <div className="absolute inset-x-0 bottom-0 h-1 bg-background/60">
                  <div className="h-full bg-primary" style={{ width: `${(watched / series.episodes.length) * 100}%` }} />
                </div>
              )}
            </div>
            <button type="button" onClick={open} className="flex min-w-0 flex-col gap-0.5 text-left outline-none">
              <span className="line-clamp-2 text-sm leading-snug font-medium" title={series.title}>
                {series.title}
              </span>
              <span className="truncate text-xs text-muted-foreground">
                {series.episodes.length > 0 ? formatBytes(series.bytes) : `${series.pending.length} downloading`}
                {series.episodes.length > 0 && downloading ? ` · ${series.pending.length} more coming` : ""}
                {watched > 0 && !all && !downloading ? ` · ${watched} of ${series.episodes.length} watched` : ""}
              </span>
            </button>
          </div>
        </ContextMenuTrigger>
        <ContextMenuContent className="w-56">
          {next && (
            <ContextMenuItem onClick={play}>
              <PlayIcon />
              Play {episodeLabel(next.episode)}
            </ContextMenuItem>
          )}
          <ContextMenuItem onClick={open}>
            <DownloadIcon />
            Saved episodes
          </ContextMenuItem>
          <ContextMenuItem onClick={() => void navigate({ to: "/anime/$id", params: { id: String(series.animeId) } })}>
            <InfoIcon />
            Title page
          </ContextMenuItem>
          {series.episodes.length > 0 && (
            <ContextMenuItem onClick={() => act(api.downloadReveal(series.episodes[0].id))}>
              <FolderOpenIcon />
              Show in folder
            </ContextMenuItem>
          )}
          <ContextMenuSeparator />
          <ContextMenuItem variant="destructive" onClick={() => setRemoving(true)}>
            <Trash2Icon />
            Remove all downloads
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenu>
      {removing && (
        <RemoveAllDialog title={series.title} saved={series.episodes} pending={series.pending} failed={series.failed} onClose={() => setRemoving(false)} />
      )}
    </>
  )
}
