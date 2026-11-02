// The library: one list, as on Android. Titles saved on this computer and the signed-in account's
// list are the same list, kept in step both ways (src-tauri/src/library.rs `reconcile`): a change
// here goes to AniList or MyAnimeList straight away, and one made there shows up here.

import { Link } from "@tanstack/react-router"
import { useQueryClient } from "@tanstack/react-query"
import { CheckIcon, CloudOffIcon, HistoryIcon, LibraryIcon, RefreshCwIcon } from "lucide-react"
import { useState } from "react"
import { toast } from "sonner"

import { AnimeCard } from "@/components/anime/anime-card"
import { CardSkeleton } from "@/components/anime/anime-row"
import { ErrorNotice } from "@/components/layout/error-notice"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { api, keys, useContinue, useLibrary, useMalUser, useViewer } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import { episodeLabel, timeAgo } from "@/lib/format"
import type { AnimeCard as Card, LibraryEntry } from "@/lib/types"
import { cn } from "@/lib/utils"

/** Android's lists (LibraryView), in its order: any number can be on at once, each its own section,
 * and sections keep this order however they were picked. */
const VIEWS = [
  { key: "watchlist", label: "My watchlist", chip: "Watchlist", match: (e: LibraryEntry) => e.status === "planning" },
  {
    key: "watching",
    label: "Watching",
    chip: "Watching",
    match: (e: LibraryEntry) => e.status === "watching" && e.accountStatus !== "REPEATING",
  },
  { key: "rewatching", label: "Re-watching", chip: "Re-watching", match: (e: LibraryEntry) => e.accountStatus === "REPEATING" },
  { key: "paused", label: "Paused", chip: "Paused", match: (e: LibraryEntry) => e.status === "paused" },
  { key: "completed", label: "Completed", chip: "Completed", match: (e: LibraryEntry) => e.status === "completed" },
  { key: "dropped", label: "Dropped", chip: "Dropped", match: (e: LibraryEntry) => e.status === "dropped" },
] as const

const VIEWS_KEY = "library.views"
const DEFAULT_VIEWS = ["watching", "watchlist"]

type View = "library" | "history"

export function LibraryPage({ initialView }: { initialView?: View }) {
  const viewer = useViewer().data
  const malUser = useMalUser().data
  const signedIn = !!viewer || !!malUser
  const [view, setView] = useState<View>(initialView ?? "library")

  return (
    <div className="flex flex-col gap-6 p-8">
      <div className="flex flex-wrap items-center gap-4">
        <h1 className="font-heading text-2xl font-bold">Library</h1>
        <ToggleGroup variant="outline" size="sm" value={[view]} onValueChange={(v) => v[0] && setView(v[0] as View)}>
          <ToggleGroupItem value="library">Library</ToggleGroupItem>
          <ToggleGroupItem value="history">History</ToggleGroupItem>
        </ToggleGroup>
        {signedIn && (
          <Link
            to="/profile"
            className="surface-control ml-auto flex items-center gap-2 rounded-full py-1 pr-3 pl-1 text-sm transition hover:bg-muted"
          >
            <Avatar className="size-7">
              <AvatarImage src={(viewer?.avatar ?? malUser?.picture) ?? undefined} alt="" />
              <AvatarFallback>{(viewer?.name ?? malUser?.name ?? "?").slice(0, 1)}</AvatarFallback>
            </Avatar>
            <span className="font-medium">{viewer?.name ?? malUser?.name}</span>
            <span className="text-muted-foreground">· Profile</span>
          </Link>
        )}
      </div>
      {view === "library" ? (
        <Library service={viewer ? "AniList" : malUser ? "MyAnimeList" : null} />
      ) : (
        <History />
      )}
    </div>
  )
}

function Grid({ items, loading }: { items: { anime: Card; caption?: string; progress?: number }[]; loading: boolean }) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
      {loading
        ? Array.from({ length: 12 }, (_, i) => <CardSkeleton key={i} />)
        : items.map(({ anime, caption, progress }) => (
            <AnimeCard key={anime.id} anime={anime} caption={caption} progress={progress} className="w-full" />
          ))}
    </div>
  )
}

function caption(entry: LibraryEntry): string {
  const parts: string[] = []
  if (entry.progress != null && entry.progress > 0) {
    parts.push(`${entry.progress}${entry.anime.episodes ? ` / ${entry.anime.episodes}` : ""} watched`)
  }
  if (entry.score) parts.push(`★ ${entry.score}`)
  if (parts.length === 0) parts.push(timeAgo(entry.updatedAt))
  return parts.join(" · ")
}

/** The list, and how it stands with the account. */
function Library({ service }: { service: string | null }) {
  const library = useLibrary()
  const client = useQueryClient()
  const [syncing, setSyncing] = useState(false)
  const [chosen, setChosen] = useState<string[]>(() => {
    const stored = localStorage.getItem(VIEWS_KEY)
    return stored ? (JSON.parse(stored) as string[]) : DEFAULT_VIEWS
  })
  const all = library.data ?? []
  const pending = all.filter((e) => !e.synced).length
  const lists = VIEWS.map((v) => ({ ...v, entries: all.filter(v.match) }))
    // Re-watching only exists on an account that has some.
    .filter((v) => v.key !== "rewatching" || v.entries.length > 0)
  const shown = lists.filter((v) => chosen.includes(v.key))
  const toggle = (key: string) => {
    const next = chosen.includes(key) ? chosen.filter((k) => k !== key) : [...chosen, key]
    localStorage.setItem(VIEWS_KEY, JSON.stringify(next))
    setChosen(next)
  }

  const sync = async () => {
    setSyncing(true)
    try {
      await api.accountRefresh()
      await Promise.all([
        client.invalidateQueries({ queryKey: keys.library }),
        client.invalidateQueries({ queryKey: keys.anilistList }),
        client.invalidateQueries({ queryKey: keys.account }),
      ])
      toast.success(`Synced with ${service}`)
    } catch (e) {
      toastFailure(e)
    } finally {
      setSyncing(false)
    }
  }

  if (library.isError) return <ErrorNotice error={library.error} onRetry={() => void library.refetch()} />

  return (
    <div className="flex flex-col gap-4">
      {service ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          {pending > 0 ? <CloudOffIcon className="size-4" /> : <CheckIcon className="size-4" />}
          {pending > 0
            ? `${pending} change${pending === 1 ? "" : "s"} waiting to reach ${service}`
            : `In sync with ${service}: changes here show up there, and theirs here`}
          <Button variant="ghost" size="sm" loading={syncing} onClick={() => void sync()}>
            <RefreshCwIcon data-icon="inline-start" />
            Sync now
          </Button>
        </div>
      ) : (
        <p className="text-sm text-muted-foreground">
          Saved on this computer.{" "}
          <Link to="/profile" className="underline underline-offset-4 hover:text-foreground">
            Sign in
          </Link>{" "}
          to keep it in step with AniList or MyAnimeList.
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        {lists.map((v) => {
          const on = chosen.includes(v.key)
          return (
            <button
              key={v.key}
              type="button"
              aria-pressed={on}
              onClick={() => toggle(v.key)}
              className={cn(
                "flex h-8 items-center gap-1.5 rounded-lg border px-3 text-sm transition",
                on ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
              )}
            >
              {on && <CheckIcon className="size-4" />}
              {v.chip}
              <span className={cn("tabular-nums", on ? "text-primary-foreground/75" : "text-muted-foreground")}>
                {library.isPending ? "…" : v.entries.length}
              </span>
            </button>
          )
        })}
      </div>
      {!library.isPending && all.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <LibraryIcon />
            </EmptyMedia>
            <EmptyTitle>Nothing here yet</EmptyTitle>
            <EmptyDescription>Add titles from their page with “Add to library”.</EmptyDescription>
          </EmptyHeader>
          <EmptyContent>
            <Button nativeButton={false} render={<Link to="/search" />}>
              Browse anime
            </Button>
          </EmptyContent>
        </Empty>
      ) : shown.length === 0 ? (
        <p className="text-sm text-muted-foreground">Pick one or more lists above.</p>
      ) : (
        shown.map((v) => (
          <section key={v.key} className="flex flex-col gap-3">
            <h2 className="flex items-baseline gap-2 font-heading text-lg font-semibold">
              {v.label}
              <span className="text-sm font-normal text-muted-foreground tabular-nums">{library.isPending ? "" : v.entries.length}</span>
            </h2>
            {!library.isPending && v.entries.length === 0 ? (
              <p className="text-sm text-muted-foreground">Nothing on this list.</p>
            ) : (
              <Grid
                loading={library.isPending}
                items={v.entries.map((e) => ({
                  anime: e.anime,
                  caption: caption(e),
                  progress: e.progress && e.anime.episodes ? e.progress / e.anime.episodes : undefined,
                }))}
              />
            )}
          </section>
        ))
      )}
    </div>
  )
}

function History() {
  const history = useContinue()
  const client = useQueryClient()
  if (!history.isPending && (history.data ?? []).length === 0) {
    return (
      <Empty>
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <HistoryIcon />
          </EmptyMedia>
          <EmptyTitle>No history yet</EmptyTitle>
          <EmptyDescription>Episodes you watch show up here, ready to resume.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  }
  return (
    <div className="flex flex-col gap-4">
      <Button
        variant="outline"
        size="sm"
        className="self-start"
        onClick={() =>
          api
            .clearHistory(null)
            .then(() => Promise.all([history.refetch(), client.invalidateQueries({ queryKey: ["progress"] })]))
            .then(() => toast.success("History cleared"))
        }
      >
        Clear history
      </Button>
      <Grid
        loading={history.isPending}
        items={(history.data ?? []).map(({ anime, progress }) => ({
          anime,
          caption: `${episodeLabel(progress.episode)} · ${timeAgo(progress.updatedAt)}`,
          progress: progress.duration ? progress.position / progress.duration : 0,
        }))}
      />
    </div>
  )
}
