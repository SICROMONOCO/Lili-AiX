// Downloads → Queue: what is downloading now (two at a time), what waits, grouped by anime so a
// season reads as one line rather than thirteen, and what failed, with why.

import { Link } from "@tanstack/react-router"
import { ChevronDownIcon, ListOrderedIcon, PauseIcon, PlayIcon, RotateCwIcon, Trash2Icon, XIcon } from "lucide-react"
import { useState } from "react"

import { DownloadsHeader, IconAction, Poster, act, episodeRanges, useDownloadsView } from "@/components/downloads/shared"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { Progress } from "@/components/ui/progress"
import { Skeleton } from "@/components/ui/skeleton"
import { api } from "@/lib/api"
import { formatBytes, formatEta, formatSpeed } from "@/lib/downloads"
import { episodeLabel } from "@/lib/format"
import type { DownloadRecord } from "@/lib/types"
import { cn } from "@/lib/utils"

export function DownloadsQueuePage() {
  const view = useDownloadsView()
  const { running, queued, failed, queue } = view
  const paused = !!queue?.paused
  // Queued episodes as runs of one anime and audio, in the order the worker takes them.
  const groups: DownloadRecord[][] = []
  for (const row of queued) {
    const last = groups[groups.length - 1]
    if (last && last[0].animeId === row.animeId && last[0].audio === row.audio) last.push(row)
    else groups.push([row])
  }

  return (
    <div className="flex flex-col gap-6 p-8">
      <DownloadsHeader tab="queue" />

      {view.loading ? (
        <Skeleton className="h-40 rounded-xl" />
      ) : view.pending === 0 && failed.length === 0 ? (
        <Empty className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <ListOrderedIcon />
            </EmptyMedia>
            <EmptyTitle>Nothing downloading</EmptyTitle>
            <EmptyDescription>Episodes you download wait here, two at a time, until they're saved.</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button variant="outline" nativeButton={false} render={<Link to="/downloads" />}>
              Saved episodes
            </Button>
          </EmptyContent>
        </Empty>
      ) : (
        <>
          {view.pending > 0 && (
            <div className={cn("surface flex items-center gap-3 rounded-xl px-4 py-3", paused && "border-primary/40")}>
              {paused ? <PauseIcon className="size-4 text-primary" /> : <span className="size-2 animate-pulse rounded-full bg-primary" />}
              <span className="flex-1 text-sm">
                {paused
                  ? `Paused · ${view.pending} episode${view.pending === 1 ? "" : "s"} waiting. It pauses itself when the disk is nearly full.`
                  : `Downloading ${running.length} · ${queued.length} waiting`}
              </span>
              {paused && (
                <Button size="sm" onClick={() => act(api.downloadsPause(false))}>
                  <PlayIcon data-icon="inline-start" />
                  Resume
                </Button>
              )}
            </div>
          )}

          {running.length > 0 && (
            <Section title="Now downloading">
              <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
                {running.map((row) => (
                  <Running key={row.id} row={row} paused={paused} />
                ))}
              </div>
            </Section>
          )}

          {groups.length > 0 && (
            <Section title="Up next" count={queued.length}>
              <div className="surface flex flex-col divide-y rounded-2xl">
                {groups.map((rows) => (
                  <QueuedGroup key={rows[0].id} rows={rows} />
                ))}
              </div>
            </Section>
          )}

          {failed.length > 0 && (
            <Section title="Failed or stopped" count={failed.length}>
              <div className="surface flex flex-col divide-y rounded-2xl">
                {failed.map((row) => (
                  <FailedRow key={row.id} row={row} />
                ))}
              </div>
            </Section>
          )}
        </>
      )}
    </div>
  )
}

function Section({ title, count, children }: { title: string; count?: number; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <h2 className="flex items-center gap-2 font-heading text-sm font-semibold text-muted-foreground">
        {title}
        {count != null && <Badge variant="secondary">{count}</Badge>}
      </h2>
      {children}
    </section>
  )
}

/** The dialog's choices a download carries, when they differ from "any server, best quality". */
function Choices({ row }: { row: DownloadRecord }) {
  return (
    <>
      {row.audio === "dub" && <Badge variant="outline">Dub</Badge>}
      {row.maxHeight != null && <Badge variant="outline">≤ {row.maxHeight}p</Badge>}
    </>
  )
}

/** One episode being fetched: the one thing on the page that moves. */
function Running({ row, paused }: { row: DownloadRecord; paused: boolean }) {
  const percent = Math.round(row.progress * 100)
  const details = [
    row.bytes > 0 ? formatBytes(row.bytes) : null,
    row.speed ? formatSpeed(row.speed) : null,
    row.eta != null ? `${formatEta(row.eta)} left` : null,
    row.server,
  ].filter(Boolean)
  return (
    <article className="surface flex items-center gap-4 rounded-xl p-4">
      <Link to="/anime/$id" params={{ id: String(row.animeId) }} className="shrink-0">
        <Poster src={row.cover} className="aspect-[2/3] w-16" />
      </Link>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <div className="flex min-w-0 items-baseline gap-2">
          <span className="truncate font-heading font-semibold" title={row.title}>
            {row.title}
          </span>
          <span className="shrink-0 text-sm text-muted-foreground">{episodeLabel(row.episode)}</span>
          <Choices row={row} />
          <span className="ml-auto shrink-0 font-heading text-lg font-bold tabular-nums">{percent}%</span>
        </div>
        <Progress value={percent} className="h-2" />
        <span className="truncate text-xs text-muted-foreground tabular-nums">
          {row.progress === 0 && !row.bytes ? "Finding a server…" : details.join(" · ")}
          {paused && " · finishing this one, then pausing"}
        </span>
      </div>
      <IconAction label="Stop" onClick={() => act(api.downloadCancel(row.id))}>
        <XIcon />
      </IconAction>
    </article>
  )
}

/** A run of one anime's episodes: one line, opened to see each. */
function QueuedGroup({ rows }: { rows: DownloadRecord[] }) {
  const [open, setOpen] = useState(rows.length === 1)
  const first = rows[0]
  const removeAll = () => act(Promise.all(rows.map((r) => api.downloadCancel(r.id))))
  return (
    <div className="flex flex-col">
      <div className="flex items-center gap-3 px-4 py-2.5">
        <Poster src={first.cover} className="aspect-[2/3] w-9 shrink-0" />
        <button type="button" onClick={() => setOpen(!open)} className="flex min-w-0 flex-1 flex-col text-left outline-none" aria-expanded={open}>
          <span className="flex min-w-0 items-center gap-2 text-sm">
            <span className="truncate font-medium" title={first.title}>
              {first.title}
            </span>
            <Choices row={first} />
          </span>
          <span className="text-xs text-muted-foreground">
            {rows.length === 1 ? episodeLabel(first.episode) : `Episodes ${episodeRanges(rows.map((r) => r.episode))} · ${rows.length} episodes`}
          </span>
        </button>
        {rows.length > 1 && (
          <IconAction label={open ? "Hide episodes" : "Show episodes"} onClick={() => setOpen(!open)}>
            <ChevronDownIcon className={cn("transition-transform", open && "rotate-180")} />
          </IconAction>
        )}
        <IconAction label={rows.length === 1 ? "Remove from the queue" : `Remove these ${rows.length} from the queue`} onClick={removeAll}>
          <XIcon />
        </IconAction>
      </div>
      {open && rows.length > 1 && (
        <div className="flex flex-col pb-1.5 pl-16">
          {rows.map((row) => (
            <div key={row.id} className="flex items-center gap-2 py-1 pr-4 text-sm">
              <span className="flex-1 text-muted-foreground">{episodeLabel(row.episode)}</span>
              <IconAction label="Remove from the queue" onClick={() => act(api.downloadCancel(row.id))}>
                <XIcon />
              </IconAction>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

function FailedRow({ row }: { row: DownloadRecord }) {
  const stopped = row.status === "cancelled"
  return (
    <div className="flex items-center gap-3 px-4 py-2.5">
      <Poster src={row.cover} className="aspect-[2/3] w-9 shrink-0" />
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex min-w-0 items-center gap-2 text-sm">
          <span className="truncate font-medium" title={row.title}>
            {row.title}
          </span>
          <span className="shrink-0 text-muted-foreground">{episodeLabel(row.episode)}</span>
          <Choices row={row} />
        </div>
        <span className={cn("truncate text-xs", stopped ? "text-muted-foreground" : "text-destructive")} title={row.error ?? undefined}>
          {stopped ? "Stopped" : (row.error ?? "Failed")}
        </span>
      </div>
      <IconAction label="Try again" onClick={() => act(api.downloadRetry(row.id))}>
        <RotateCwIcon />
      </IconAction>
      <IconAction label="Forget" onClick={() => act(api.downloadRemove(row.id, false))}>
        <Trash2Icon />
      </IconAction>
    </div>
  )
}
