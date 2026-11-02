import { Link, useCanGoBack, useNavigate, useRouter } from "@tanstack/react-router"
import {
  ArrowDownWideNarrowIcon,
  ArrowLeftIcon,
  ArrowUpNarrowWideIcon,
  BookmarkIcon,
  CheckIcon,
  ChevronDownIcon,
  CalendarClockIcon,
  ClapperboardIcon,
  DownloadIcon,
  Grid3x3Icon,
  LayoutGridIcon,
  LinkIcon,
  LockIcon,
  PlayIcon,
  RefreshCwIcon,
} from "lucide-react"
import { useMemo, useState } from "react"

import { AnimeCard } from "@/components/anime/anime-card"
import { AnimeRow } from "@/components/anime/anime-row"
import { DownloadDialog } from "@/components/anime/download-dialog"
import { AiringCountdown, EpisodeCountdown } from "@/components/anime/episode-countdown"
import { EpisodeDownloadButton, Ring, episodeDownload } from "@/components/anime/episode-download"
import { TrailerCard, TrailerDialog, trailerSources } from "@/components/anime/trailer"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuRadioGroup,
  DropdownMenuRadioItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { Skeleton } from "@/components/ui/skeleton"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import {
  api,
  useAnilistList,
  useAppearance,
  useDetail,
  useDownloads,
  useEpisodes,
  useQueueState,
  useLibraryStatus,
  useMalUser,
  useProgress,
  useSetLibrary,
  useSettings,
  useViewer,
  useWatchOrder,
} from "@/lib/api"
import { formatEta, isActive } from "@/lib/downloads"
import { airsLabel, episodeLock } from "@/lib/episodes"
import { countdown, episodeLabel, formatLabel, fuzzyDateLabel, notYetOut, plainDescription, seasonLabel, statusLabel } from "@/lib/format"
import type { AnimeCard as AnimeCardData, AnimeDetail, Audio, DownloadRecord, Episode, LibraryStatus, Progress, WatchOrderEntry } from "@/lib/types"
import { errorMessage } from "@/lib/types"
import { cn } from "@/lib/utils"
import { toastFailure } from "@/lib/failure"
import { animeLink, copyLink } from "@/lib/links"
import { accentVars } from "@/lib/theme"
import { useLockClock } from "@/lib/use-lock-clock"
import { ErrorNotice } from "@/components/layout/error-notice"

const STATUSES: { value: LibraryStatus; label: string }[] = [
  { value: "watching", label: "Watching" },
  { value: "planning", label: "Plan to watch" },
  { value: "completed", label: "Completed" },
  { value: "paused", label: "On hold" },
  { value: "dropped", label: "Dropped" },
]

const RANGE = 100
/** The compact grid shows more at once: a long series in a few pages instead of a dozen. */
const COMPACT_RANGE = 500

export function AnimePage({ id }: { id: number }) {
  const detail = useDetail(id)
  return (
    <>
      <BackButton />
      {detail.isPending ? (
        <DetailSkeleton />
      ) : detail.isError ? (
        <div className="flex h-full items-center justify-center">
          <ErrorNotice error={detail.error} onRetry={() => void detail.refetch()} />
        </div>
      ) : (
        <Detail anime={detail.data} />
      )}
    </>
  )
}

/** Back to wherever the title was opened from (Home when it was opened by a link), kept in the
 * corner while the page scrolls. */
function BackButton() {
  const router = useRouter()
  const canGoBack = useCanGoBack()
  const navigate = useNavigate()
  return (
    <div className="sticky top-0 z-20 h-0">
      <Tooltip>
        <TooltipTrigger
          render={
            <Button
              size="icon-lg"
              variant="ghost"
              aria-label="Back"
              className="surface-chip absolute top-4 left-4 rounded-full text-white hover:text-white"
              onClick={() => (canGoBack ? router.history.back() : void navigate({ to: "/" }))}
            />
          }
        >
          <ArrowLeftIcon />
        </TooltipTrigger>
        <TooltipContent side="right">Back</TooltipContent>
      </Tooltip>
    </div>
  )
}

function Detail({ anime }: { anime: AnimeDetail }) {
  const navigate = useNavigate()
  const episodes = useEpisodes(anime.id)
  const progress = useProgress(anime.id)
  const status = useLibraryStatus(anime.id)
  const setLibrary = useSetLibrary()
  const [expanded, setExpanded] = useState(false)
  const appearance = useAppearance()
  const watchOrder = useWatchOrder(anime.id)
  const guide = useMemo(() => watchGuide(anime, watchOrder.data), [anime, watchOrder.data])
  const suggestions = useMemo(() => {
    const franchise = new Set(guide.map((e) => e.anime.id))
    return anime.recommendations.filter((r) => r.id !== anime.id && !franchise.has(r.id))
  }, [anime, guide])
  const viewer = useViewer().data
  const malUser = useMalUser().data
  const signedIn = !!viewer || !!malUser
  const [confirmRemove, setConfirmRemove] = useState(false)
  const [downloadOpen, setDownloadOpen] = useState(false)
  const [trailerOpen, setTrailerOpen] = useState(false)
  const trailer = trailerSources(anime.trailer) ? anime.trailer : null
  const downloads = useDownloads().data
  const audio: Audio = useSettings().data?.audio ?? "sub"

  // Episodes finished on the account, wherever they were watched; held to the title's length
  // (a larger count is the two services disagreeing about the season, as Android's clamp says).
  const account = useAnilistList(signedIn).data?.find((e) => e.anime.id === anime.id)
  const watchedElsewhere = account ? Math.min(account.progress, anime.episodes || Infinity) : 0
  const merged = useMemo(() => withAccount(progress.data ?? [], anime.id, watchedElsewhere), [progress.data, anime.id, watchedElsewhere])
  const watchedByEpisode = useMemo(() => new Map(merged.map((p) => [p.episode, p])), [merged])
  // The episodes that can be played now: a locked one (the next, counting down in the grid) is
  // neither offered to resume nor to download.
  const lockNow = useLockClock(episodes.data?.episodes)
  const openEpisodes = useMemo(() => (episodes.data?.episodes ?? []).filter((e) => !episodeLock(e, lockNow)), [episodes.data, lockNow])
  const resume = useMemo(
    () => nextToWatch(openEpisodes, progress.data ?? [], watchedElsewhere),
    [openEpisodes, progress.data, watchedElsewhere],
  )

  // Before the premiere there is nothing to play: the Watch button counts down instead, or
  // gives the date when the schedule has no clock yet (a movie, often).
  const notAired = openEpisodes.length === 0 && notYetOut(anime, lockNow)
  const movie = anime.format === "MOVIE"
  const premiereLabel = anime.nextAiring
    ? `${movie ? "Out" : `Episode ${anime.nextAiring.episode} airing`} ${countdown(anime.nextAiring.airingAt, lockNow)}`
    : anime.startDate
      ? `Out ${fuzzyDateLabel(anime.startDate)}`
      : "Not out yet"
  const play = (episode: number) =>
    void navigate({ to: "/watch/$id/$episode", params: { id: String(anime.id), episode: String(episode) } })

  const description = plainDescription(anime.description)
  const meta = [
    formatLabel(anime.format),
    seasonLabel(anime),
    statusLabel(anime.status),
    anime.episodes ? `${anime.episodes} episodes` : null,
    anime.duration ? `${anime.duration} min` : null,
  ].filter(Boolean)

  return (
    // "Colour anime pages by the show": the page takes its accent from the artwork.
    <div
      className="flex flex-col pb-12"
      style={appearance.showColourPages && anime.color ? (accentVars(anime.color) as React.CSSProperties) : undefined}
    >
      <section className="relative h-72 w-full overflow-hidden" style={{ backgroundColor: anime.color ?? undefined }}>
        {anime.banner && <img src={anime.banner} alt="" className="size-full object-cover opacity-70" />}
        <div className="absolute inset-0 bg-linear-to-t from-background via-background/40 to-transparent" />
      </section>

      <div className="relative -mt-40 flex gap-8 px-8">
        <img
          src={anime.cover ?? undefined}
          alt=""
          className="aspect-[2/3] w-52 shrink-0 rounded-xl object-cover shadow-2xl"
          style={{ backgroundColor: anime.color ?? undefined }}
        />
        <div className="flex min-w-0 flex-1 flex-col justify-end gap-3 pt-28">
          {anime.logo && appearance.showSeriesLogos ? (
            <h1>
              <img src={anime.logo} alt={anime.title} className="max-h-28 max-w-md object-contain object-left drop-shadow-lg" />
            </h1>
          ) : (
            <h1 className="font-heading text-3xl font-bold text-balance">{anime.title}</h1>
          )}
          {anime.titleRomaji && anime.titleRomaji !== anime.title && (
            <p className="text-muted-foreground">{anime.titleRomaji}</p>
          )}
          <div className="flex flex-wrap items-center gap-2">
            {anime.score != null && <Badge>{anime.score}%</Badge>}
            {meta.map((m) => (
              <Badge key={m} variant="secondary">
                {m}
              </Badge>
            ))}
            {anime.nextAiring && (
              <Badge variant="outline">
                {anime.format === "MOVIE" ? "Out" : `Episode ${anime.nextAiring.episode}`} {countdown(anime.nextAiring.airingAt)}
              </Badge>
            )}
          </div>
          <div className="flex flex-wrap gap-2 pt-1">
            <Button size="lg" onClick={() => play(resume.episode)} disabled={episodes.isError || notAired}>
              {notAired ? <CalendarClockIcon data-icon="inline-start" /> : <PlayIcon data-icon="inline-start" />}
              {notAired ? premiereLabel : resume.label}
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button size="lg" variant="secondary" />}>
                {status.data ? <CheckIcon data-icon="inline-start" /> : <BookmarkIcon data-icon="inline-start" />}
                {STATUSES.find((s) => s.value === status.data)?.label ?? "Add to library"}
                <ChevronDownIcon data-icon="inline-end" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="start">
                <DropdownMenuGroup>
                  <DropdownMenuRadioGroup
                    value={status.data ?? ""}
                    onValueChange={(value) =>
                      setLibrary.mutate(
                        { anime, status: value as LibraryStatus },
                        { onError: (e) => toastFailure(e) },
                      )
                    }
                  >
                    {STATUSES.map((s) => (
                      <DropdownMenuRadioItem key={s.value} value={s.value}>
                        {s.label}
                      </DropdownMenuRadioItem>
                    ))}
                  </DropdownMenuRadioGroup>
                </DropdownMenuGroup>
                {status.data && (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuGroup>
                      <DropdownMenuItem onClick={() => (signedIn ? setConfirmRemove(true) : setLibrary.mutate({ anime, status: null }))}>
                        Remove from library
                      </DropdownMenuItem>
                    </DropdownMenuGroup>
                  </>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
            {episodes.data && episodes.data.episodes.length > 0 && (
              <Button size="lg" variant="secondary" onClick={() => setDownloadOpen(true)}>
                <DownloadIcon data-icon="inline-start" />
                Download
              </Button>
            )}
            {trailer && (
              <Button size="lg" variant="secondary" onClick={() => setTrailerOpen(true)}>
                <ClapperboardIcon data-icon="inline-start" />
                Trailer
              </Button>
            )}
            <Tooltip>
              <TooltipTrigger
                render={<Button size="icon-lg" variant="secondary" aria-label="Copy link" onClick={() => copyLink(animeLink(anime.id))} />}
              >
                <LinkIcon />
              </TooltipTrigger>
              <TooltipContent>Copy an anilili:// link to this title</TooltipContent>
            </Tooltip>
          </div>
          <BatchStatus rows={(downloads ?? []).filter((d) => d.animeId === anime.id)} />
        </div>
      </div>

      <div className="grid grid-cols-1 gap-8 px-8 pt-8 xl:grid-cols-[1fr_18rem]">
        <div className="flex min-w-0 flex-col gap-6">
          {description && (
            <div className="flex flex-col gap-1">
              <p className={cn("text-sm leading-relaxed whitespace-pre-line text-muted-foreground", !expanded && "line-clamp-4")}>
                {description}
              </p>
              {description.length > 320 && (
                <Button variant="link" size="sm" className="self-start px-0" onClick={() => setExpanded((e) => !e)}>
                  {expanded ? "Show less" : "Read more"}
                </Button>
              )}
            </div>
          )}

          <Tabs defaultValue="episodes">
            <TabsList className="flex-wrap">
              <TabsTrigger value="episodes">Episodes</TabsTrigger>
              {anime.characters.length > 0 && <TabsTrigger value="characters">Characters</TabsTrigger>}
              {guide.length > 1 && <TabsTrigger value="watch-order">Watch order</TabsTrigger>}
            </TabsList>
            <TabsContent value="episodes" className="pt-4">
              <EpisodeGrid
                loading={episodes.isPending}
                error={episodes.isError ? errorMessage(episodes.error) : null}
                episodes={episodes.data?.episodes ?? []}
                dubCount={episodes.data?.dubCount ?? null}
                fallbackArt={anime.banner}
                blurEpisodeImages={appearance.blurEpisodeImages}
                progress={watchedByEpisode}
                now={lockNow}
                onPlay={play}
                onRetry={() => void episodes.refetch()}
                anime={anime}
                startDate={anime.startDate}
                audio={audio}
                downloads={downloads}
              />
            </TabsContent>
            <TabsContent value="characters" className="pt-4">
              <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
                {anime.characters.map((c) => (
                  <div key={c.name} className="surface flex items-center gap-3 rounded-lg p-2">
                    {c.image && <img src={c.image} alt="" className="size-14 rounded-md object-cover" />}
                    <div className="flex min-w-0 flex-col">
                      <span className="truncate text-sm font-medium" title={c.name}>{c.name}</span>
                      <span className="truncate text-xs text-muted-foreground">
                        {[c.role.charAt(0) + c.role.slice(1).toLowerCase(), c.voiceActor].filter(Boolean).join(" · ")}
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            </TabsContent>
            {guide.length > 1 && (
              <TabsContent value="watch-order" className="pt-4">
                <WatchOrderList entries={guide} />
              </TabsContent>
            )}
          </Tabs>
        </div>

        <aside className="flex flex-col gap-4 text-sm">
          {trailer && <TrailerCard trailer={trailer} onOpen={() => setTrailerOpen(true)} />}
          <Facts anime={anime} />
        </aside>
      </div>

      {suggestions.length > 0 && (
        <div className="pt-8">
          <AnimeRow title="More like this" subtitle="Recommended, not part of the series">
            {suggestions.map((r) => (
              <AnimeCard key={r.id} anime={r} />
            ))}
          </AnimeRow>
        </div>
      )}

      {trailer && <TrailerDialog trailer={trailer} title={anime.title} open={trailerOpen} onOpenChange={setTrailerOpen} />}

      {episodes.data && (
        <DownloadDialog
          open={downloadOpen}
          onOpenChange={setDownloadOpen}
          anime={anime}
          episodes={openEpisodes}
          dubCount={episodes.data.dubCount}
          progress={merged}
          resume={resume.episode}
        />
      )}

      <Dialog open={confirmRemove} onOpenChange={setConfirmRemove}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Remove from your list?</DialogTitle>
            <DialogDescription>
              {anime.title} comes off this library and off your account, with its progress and score. The account keeps no
              undo.
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmRemove(false)}>
              Cancel
            </Button>
            <Button
              variant="destructive"
              onClick={() => {
                setConfirmRemove(false)
                setLibrary.mutate({ anime, status: null }, { onError: (e) => toastFailure(e) })
              }}
            >
              Remove
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  )
}

/** Relations that are the same story told on, told beside, or told again (Rust
 * `franchise::FRANCHISE_RELATIONS`): AniList's OTHER points at anything at all, and a guide that
 * lists it sends someone looking for the franchise's film off into different anime. */
const FRANCHISE_RELATIONS = new Set([
  "PREQUEL",
  "SEQUEL",
  "PARENT",
  "SIDE_STORY",
  "SUMMARY",
  "ALTERNATIVE",
  "SPIN_OFF",
  "COMPILATION",
  "CONTAINS",
])

/** Formats a watch guide lists (Rust `franchise::FRANCHISE_FORMATS`): not reading formats, and not
 * MUSIC, or the guide drowns in opening videos. */
const FRANCHISE_FORMATS = new Set(["TV", "TV_SHORT", "MOVIE", "SPECIAL", "OVA", "ONA"])

/**
 * The franchise as one numbered watch guide. The engine's guide when it has landed and names more
 * than this title; until then this page's own relations stand in — the same relations and formats
 * the engine follows, numbered by year, so the tab never starts empty and never shows more than
 * the guide will. The stand-in carries no chronology (the relations list says nothing about
 * order), so both its numbers are the same.
 */
function watchGuide(anime: AnimeDetail, order: WatchOrderEntry[] | undefined): WatchOrderEntry[] {
  if (order && order.length > 1) return order
  const entries: Omit<WatchOrderEntry, "position" | "chronological">[] = anime.relations
    .filter(
      (r) =>
        r.anime.id !== anime.id &&
        FRANCHISE_RELATIONS.has(r.kind) &&
        r.anime.format != null &&
        FRANCHISE_FORMATS.has(r.anime.format),
    )
    .map((r) => ({
      anime: r.anime,
      shelf: r.kind === "PREQUEL" || r.kind === "SEQUEL" ? "seasons" : shelfOf(r.anime.format),
      seasonNumber: null,
      relation: r.kind === "PREQUEL" || r.kind === "SEQUEL" ? null : prettyRelation(r.kind),
      isCurrent: false,
    }))
  entries.push({ anime, shelf: shelfOf(anime.format), seasonNumber: null, relation: null, isCurrent: true })
  return entries
    .sort((a, b) => (a.anime.year ?? Infinity) - (b.anime.year ?? Infinity) || a.anime.id - b.anime.id)
    .map((e, i) => ({ ...e, position: i + 1, chronological: i + 1 }))
}

/**
 * The guide as the page's card grid, every entry in its place, the title itself marked where it
 * sits. Release order (the order the titles came out in) is the default; when AniList's relations
 * put the story elsewhere — a prequel that aired years after what it precedes — the story's own
 * order is offered beside it.
 */
function WatchOrderList({ entries }: { entries: WatchOrderEntry[] }) {
  const [story, setStory] = useState(false)
  const ordered = useMemo(
    () => [...entries].sort((a, b) => (story ? a.chronological - b.chronological : a.position - b.position)),
    [entries, story],
  )
  return (
    <div className="flex flex-col gap-4">
      {entries.some((e) => e.chronological !== e.position) && (
        <div className="flex flex-wrap items-center gap-2">
          <Button size="sm" variant={!story ? "secondary" : "ghost"} onClick={() => setStory(false)}>
            Release order
          </Button>
          <Button size="sm" variant={story ? "secondary" : "ghost"} onClick={() => setStory(true)}>
            Chronological
          </Button>
          <span className="text-xs text-muted-foreground">
            {story ? "The story's own timeline: prequels first, wherever they aired." : "The order the titles came out in."}
          </span>
        </div>
      )}
      <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
        {ordered.map((e, i) => (
          <div key={e.anime.id} className="relative">
            <AnimeCard anime={e.anime} className="w-full" caption={e.isCurrent ? "You're here" : watchOrderMeta(e)} />
            <span className="surface-chip pointer-events-none absolute top-1.5 left-1.5 z-10 rounded-md px-1.5 py-0.5 text-xs font-medium tabular-nums">
              {i + 1}
            </span>
          </div>
        ))}
      </div>
    </div>
  )
}

function shelfOf(format: string | null): WatchOrderEntry["shelf"] {
  return format === "MOVIE" ? "movies" : format === "OVA" ? "ovas" : format === "SPECIAL" ? "specials" : format === "TV" || format === "TV_SHORT" || format === "ONA" ? "seasons" : "other"
}

const prettyRelation = (kind: string) => kind.charAt(0) + kind.slice(1).toLowerCase().replace(/_/g, " ")

/** "Season 2 · 2023 · 12 eps", "Movie · Side story · 2024", as Android's metaLine. */
function watchOrderMeta(e: WatchOrderEntry): string {
  const kind = e.seasonNumber ? `Season ${e.seasonNumber}` : formatLabel(e.anime.format)
  const episodes = e.anime.episodes && e.anime.episodes > 1 && e.anime.format !== "MOVIE" ? `${e.anime.episodes} eps` : null
  return [kind, e.relation, e.anime.year, episodes].filter(Boolean).join(" · ")
}

/**
 * The episode to offer first: resume an unfinished one, else the one after the last finished.
 * `watchedElsewhere` is the account's count of finished episodes (the phone, the TV, the site): when
 * it is past this PC's own place, the offer moves on to the episode after it, as Android's
 * `historyAfterRemoteProgress` does. This PC's place wins while it is level or ahead.
 */
function nextToWatch(episodes: Episode[], progress: Progress[], watchedElsewhere: number): { episode: number; label: string } {
  const after = (n: number) => episodes.find((e) => e.number > n)?.number ?? (episodes.length === 0 ? n + 1 : null)
  const latest = [...progress].sort((a, b) => b.updatedAt - a.updatedAt)[0]
  const finished = latest && latest.duration > 0 && latest.position >= latest.duration * 0.9
  const here = !latest ? null : finished ? after(latest.episode) : latest.episode
  if (watchedElsewhere > 0 && (!latest || (here != null && watchedElsewhere >= here))) {
    const next = after(watchedElsewhere)
    // A clock this PC kept for that very episode still holds.
    if (next != null) return { episode: next, label: `${byEpisode(progress, next) ? "Resume" : "Watch"} ${episodeLabel(next)}` }
    return { episode: episodes[0]?.number ?? 1, label: "Watch again" }
  }
  if (!latest) return { episode: episodes[0]?.number ?? 1, label: "Watch now" }
  if (!finished) return { episode: latest.episode, label: `Resume ${episodeLabel(latest.episode)}` }
  if (here != null) return { episode: here, label: `Watch ${episodeLabel(here)}` }
  return { episode: episodes[0]?.number ?? 1, label: "Watch again" }
}

const byEpisode = (progress: Progress[], episode: number) =>
  progress.some((p) => p.episode === episode && p.position > 0 && !(p.duration > 0 && p.position >= p.duration * 0.9))

/**
 * This PC's progress with the account's count folded in: every episode up to the count is
 * finished (that is all AniList and MAL keep), so the grid ticks what was watched elsewhere.
 */
function withAccount(progress: Progress[], animeId: number, watchedElsewhere: number): Progress[] {
  if (watchedElsewhere < 1) return progress
  const done = progress.map((p) => (p.episode <= watchedElsewhere ? { ...p, position: Math.max(p.position, p.duration, 1), duration: Math.max(p.duration, 1) } : p))
  const known = new Set(progress.map((p) => p.episode))
  for (let n = 1; n <= watchedElsewhere; n++) {
    if (!known.has(n)) done.push({ animeId, episode: n, position: 1, duration: 1, updatedAt: 0 })
  }
  return done
}

/**
 * The picture for episodes without their own, as Android decides it: the banner, but only when
 * some episode in the list has art or the list is a single entry. Twelve copies of one banner
 * read as a list that failed to load; blank frames honestly say there is no art yet.
 */
function artFallback(episodes: Episode[], banner: string | null): string | null {
  if (!banner) return null
  return episodes.length <= 1 || episodes.some((e) => e.thumbnail) ? banner : null
}

function EpisodeGrid({
  loading,
  error,
  episodes,
  dubCount,
  fallbackArt,
  blurEpisodeImages,
  progress,
  now,
  onPlay,
  onRetry,
  anime,
  startDate,
  audio,
  downloads,
}: {
  loading: boolean
  error: string | null
  episodes: Episode[]
  dubCount: number | null
  /** The title's banner, for episodes without art of their own (see `artFallback`). */
  fallbackArt: string | null
  /** Settings → Appearance: blur the art of episodes not yet watched. */
  blurEpisodeImages: boolean
  progress: Map<number, Progress>
  /** The time locks are judged by (`useLockClock`). */
  now: number
  onPlay: (n: number) => void
  onRetry: () => void
  anime: AnimeCardData
  /** The premiere date (`YYYY-MM-DD`), when the schedule has no episode clock yet. */
  startDate: string | null
  /** The audio the page plays in: the one a row's download control fetches. */
  audio: Audio
  downloads: DownloadRecord[] | undefined
}) {
  // How the list is read, remembered across titles: oldest or newest first, and big cards or a
  // compact grid of numbers (the default for long series, where cards are a long scroll).
  const [newestFirst, setNewestFirst] = useState(() => localStorage.getItem("episodes.order") === "newest")
  const [storedLayout, setLayout] = useState(() => localStorage.getItem("episodes.layout"))
  const compact = storedLayout ? storedLayout === "compact" : episodes.length > 100
  const size = compact ? COMPACT_RANGE : RANGE
  const ordered = newestFirst ? [...episodes].reverse() : episodes
  const ranges = Math.ceil(ordered.length / size)
  const [rangeState, setRange] = useState(0)
  const range = Math.min(rangeState, Math.max(0, ranges - 1))
  if (loading) {
    return (
      <div className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-3">
        {Array.from({ length: 8 }, (_, i) => (
          <Skeleton key={i} className="aspect-video rounded-lg" />
        ))}
      </div>
    )
  }
  // A title still to come out has no streams anywhere: the lookup failing (DSRC-01) is expected,
  // so it gets the premiere, not the error.
  const upcoming = episodes.length === 0 && notYetOut(anime, now)
  if (error && !upcoming) {
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyTitle>No episodes available</EmptyTitle>
          <EmptyDescription>{error}</EmptyDescription>
        </EmptyHeader>
        <EmptyContent>
          <Button variant="outline" onClick={onRetry}>
            <RefreshCwIcon data-icon="inline-start" />
            Try again
          </Button>
        </EmptyContent>
      </Empty>
    )
  }
  if (episodes.length === 0) {
    // Not an error: this title simply has not aired (yet). Count down to the premiere
    // instead of claiming no streams are known, and offer no misleading retry.
    const movie = anime.format === "MOVIE"
    const premiere = anime.nextAiring && anime.nextAiring.airingAt > now ? anime.nextAiring : null
    const title = premiere
      ? movie
        ? "This movie isn't out yet"
        : `Episode ${premiere.episode} hasn't aired yet`
      : upcoming
        ? movie
          ? "This movie isn't out yet"
          : "This title hasn't aired yet"
        : "No episodes available"
    const later = movie ? "It will play here once it's out." : "Episodes will appear here once it premieres."
    return (
      <Empty className="border">
        <EmptyHeader>
          <EmptyTitle>{title}</EmptyTitle>
          <EmptyDescription className="flex flex-col items-center gap-1">
            {premiere ? (
              <>
                <AiringCountdown airingAt={premiere.airingAt} />
                <span>
                  {airsLabel(premiere.airingAt)}. {later}
                </span>
              </>
            ) : upcoming ? (
              <span>
                {startDate ? `${movie ? "Out" : "Premieres"} ${fuzzyDateLabel(startDate)}.` : "No release date yet."} {later}
              </span>
            ) : (
              <span>The catalog hasn't listed episodes for this title yet. Try again later.</span>
            )}
          </EmptyDescription>
        </EmptyHeader>
        {!premiere && !upcoming && (
          <EmptyContent>
            <Button variant="outline" onClick={onRetry}>
              <RefreshCwIcon data-icon="inline-start" />
              Try again
            </Button>
          </EmptyContent>
        )}
      </Empty>
    )
  }
  const shown = ordered.slice(range * size, (range + 1) * size)
  const fallback = artFallback(episodes, fallbackArt)
  const label = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(1))
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-2">
        {ranges > 1 &&
          Array.from({ length: ranges }, (_, i) => {
            const part = ordered.slice(i * size, (i + 1) * size)
            return (
              <Button key={i} size="sm" variant={i === range ? "secondary" : "ghost"} onClick={() => setRange(i)}>
                {label(part[0].number)}–{label(part[part.length - 1].number)}
              </Button>
            )
          })}
        <div className="ml-auto flex items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => {
              localStorage.setItem("episodes.order", newestFirst ? "oldest" : "newest")
              setNewestFirst(!newestFirst)
              setRange(0)
            }}
          >
            {newestFirst ? <ArrowDownWideNarrowIcon data-icon="inline-start" /> : <ArrowUpNarrowWideIcon data-icon="inline-start" />}
            {newestFirst ? "Newest first" : "Oldest first"}
          </Button>
          <Tooltip>
            <TooltipTrigger
              render={
                <Button
                  variant="outline"
                  size="icon-sm"
                  aria-label={compact ? "Show as cards" : "Show as a compact grid"}
                  onClick={() => {
                    localStorage.setItem("episodes.layout", compact ? "cards" : "compact")
                    setLayout(compact ? "cards" : "compact")
                    setRange(0)
                  }}
                />
              }
            >
              {compact ? <Grid3x3Icon /> : <LayoutGridIcon />}
            </TooltipTrigger>
            <TooltipContent>{compact ? "Compact grid · switch to cards" : "Cards · switch to a compact grid"}</TooltipContent>
          </Tooltip>
        </div>
      </div>
      {compact ? (
        <CompactEpisodes
          episodes={shown}
          progress={progress}
          now={now}
          onPlay={onPlay}
          saved={(n) => episodeDownload(downloads, anime.id, n, audio)}
        />
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-3">
          {shown.map((episode) => {
            const p = progress.get(episode.number)
            const fraction = p && p.duration ? p.position / p.duration : 0
            const download = episodeDownload(downloads, anime.id, episode.number, audio)
            if (episode.airsAt != null && episodeLock(episode, now)) {
              return <LockedEpisodeCard key={episode.number} episode={episode} airsAt={episode.airsAt} art={episode.thumbnail ?? fallback} />
            }
            return (
              <div key={episode.number} className="group relative">
                <button
                  type="button"
                  onClick={() => onPlay(episode.number)}
                  className="flex w-full flex-col gap-2 rounded-lg text-left outline-none"
                >
                  <div className="relative aspect-video overflow-hidden rounded-lg bg-muted ring-ring/60 transition group-hover:ring-2 group-has-focus-visible:ring-2">
                    {(episode.thumbnail ?? fallback) && (
                      <img
                        src={episode.thumbnail ?? fallback ?? undefined}
                        alt=""
                        loading="lazy"
                        className={cn(
                          "size-full object-cover transition group-hover:scale-105",
                          !episode.thumbnail && "opacity-50",
                          // "Blur episode images": an episode's art can give its story away until it's watched.
                          blurEpisodeImages && episode.thumbnail && !(p && p.duration && p.position / p.duration >= 0.9) && "scale-110 blur-lg",
                        )}
                      />
                    )}
                    <div className="absolute inset-0 flex items-center justify-center bg-background/40 opacity-0 transition group-hover:opacity-100">
                      <PlayIcon className="size-10" />
                    </div>
                    <div className="absolute top-2 left-2 flex gap-1">
                      <Badge variant="secondary" className="surface-chip">
                        {Number.isInteger(episode.number) ? episode.number : episode.number.toFixed(1)}
                      </Badge>
                      {episode.filler && <Badge variant="destructive">Filler</Badge>}
                      {dubCount != null && episode.number <= dubCount && (
                        <Badge variant="outline" className="surface-chip">
                          Dub
                        </Badge>
                      )}
                    </div>
                    {fraction > 0 && (
                      <div className="absolute inset-x-0 bottom-0 h-1 bg-background/60">
                        <div className="h-full bg-primary" style={{ width: `${Math.min(100, fraction * 100)}%` }} />
                      </div>
                    )}
                  </div>
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate text-sm font-medium">{episode.title ?? episodeLabel(episode.number)}</span>
                    {episode.airedOn && <span className="text-xs text-muted-foreground">{episode.airedOn}</span>}
                  </div>
                </button>
                <EpisodeDownloadButton
                  anime={anime}
                  episode={episode.number}
                  audio={audio}
                  row={download}
                  // Quiet until hovered, unless something is happening to the episode.
                  className={cn("absolute top-2 right-2", !download && "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100")}
                />
              </div>
            )
          })}
        </div>
      )}
    </div>
  )
}

/** Episodes as numbered tiles, for series with hundreds of them: watched ones dimmed with a tick,
 * the one in progress outlined with its bar, filler marked. Hover names the episode. */
function CompactEpisodes({
  episodes,
  progress,
  now,
  onPlay,
  saved,
}: {
  episodes: Episode[]
  progress: Map<number, Progress>
  /** The time locks are judged by (`useLockClock`). */
  now: number
  onPlay: (n: number) => void
  /** The episode's download, for a corner mark. */
  saved: (n: number) => DownloadRecord | undefined
}) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(3.75rem,1fr))] gap-1.5">
      {episodes.map((episode) => {
        const p = progress.get(episode.number)
        const fraction = p && p.duration ? p.position / p.duration : 0
        const watched = fraction >= 0.9
        const started = fraction > 0 && !watched
        const number = Number.isInteger(episode.number) ? episode.number : episode.number.toFixed(1)
        const download = saved(episode.number)
        if (episode.airsAt != null && episodeLock(episode, now)) {
          return (
            <Tooltip key={episode.number}>
              <TooltipTrigger
                render={
                  <div className="relative flex h-10 items-center justify-center gap-1 surface-control overflow-hidden rounded-md text-sm font-medium tabular-nums text-muted-foreground opacity-70" />
                }
              >
                <LockIcon className="size-3" />
                {number}
              </TooltipTrigger>
              <TooltipContent>
                {episodeLabel(episode.number)} · {airsLabel(episode.airsAt)}
              </TooltipContent>
            </Tooltip>
          )
        }
        return (
          <Tooltip key={episode.number}>
            <TooltipTrigger
              render={
                <button
                  type="button"
                  onClick={() => onPlay(episode.number)}
                  className={cn(
                    "relative flex h-10 items-center justify-center surface-control overflow-hidden rounded-md text-sm font-medium tabular-nums transition hover:bg-muted",
                    watched && "text-muted-foreground",
                    started && "border-primary",
                    episode.filler && "border-dashed",
                  )}
                />
              }
            >
              {watched && <CheckIcon className="mr-0.5 size-3" />}
              {number}
              {episode.filler && <span className="absolute top-1 right-1 size-1.5 rounded-full bg-destructive" />}
              {download?.status === "done" && <DownloadIcon className="absolute top-0.5 left-0.5 size-2.5 text-primary" />}
              {download && isActive(download) && <Ring value={download.status === "downloading" ? download.progress : null} className="absolute top-0.5 left-0.5 size-2.5" />}
              {started && (
                <span className="absolute inset-x-0 bottom-0 h-0.5 bg-muted">
                  <span className="block h-full bg-primary" style={{ width: `${fraction * 100}%` }} />
                </span>
              )}
            </TooltipTrigger>
            <TooltipContent>
              {episode.title ?? episodeLabel(episode.number)}
              {episode.filler ? " · Filler" : ""}
              {watched ? " · Watched" : ""}
              {download?.status === "done" ? " · Downloaded" : download && isActive(download) ? " · Downloading" : ""}
            </TooltipContent>
          </Tooltip>
        )
      })}
    </div>
  )
}

/** The season's next episode before it is out: its slot in the grid, locked, with a clock counting
 * to its broadcast and then to the servers carrying it (Android TV's counting card). It opens by
 * itself when the clock runs out (`useLockClock`). */
function LockedEpisodeCard({ episode, airsAt, art }: { episode: Episode; airsAt: number; art: string | null }) {
  return (
    <div className="flex flex-col gap-2" aria-label={`${episodeLabel(episode.number)}, ${airsLabel(airsAt)}`}>
      <div className="relative aspect-video overflow-hidden rounded-lg bg-muted">
        {art && <img src={art} alt="" loading="lazy" className="size-full object-cover" />}
        <EpisodeCountdown episode={episode} />
        <Badge variant="secondary" className="surface-chip absolute top-2 left-2">
          {Number.isInteger(episode.number) ? episode.number : episode.number.toFixed(1)}
        </Badge>
        <span className="surface-chip absolute top-2 right-2 flex size-6 items-center justify-center rounded-full text-white">
          <LockIcon className="size-3.5" />
        </span>
      </div>
      <div className="flex min-w-0 flex-col">
        <span className="truncate text-sm font-medium text-muted-foreground">{episode.title ?? episodeLabel(episode.number)}</span>
        <span className="text-xs text-muted-foreground">{airsLabel(airsAt)}</span>
      </div>
    </div>
  )
}

function Facts({ anime }: { anime: AnimeDetail }) {
  const rows: [string, string | null][] = [
    ["Studio", anime.studios.join(", ") || null],
    ["Source", anime.source ? anime.source.replace(/_/g, " ").toLowerCase() : null],
    notYetOut(anime)
      ? [anime.format === "MOVIE" ? "Release date" : "Premieres", anime.startDate ? fuzzyDateLabel(anime.startDate) : null]
      : ["Aired", [anime.startDate, anime.endDate].filter(Boolean).join(" → ") || null],
    ["Native title", anime.titleNative],
  ]
  return (
    <div className="surface flex flex-col gap-4 rounded-xl p-4">
      <dl className="flex flex-col gap-3">
        {rows
          .filter(([, v]) => v)
          .map(([k, v]) => (
            <div key={k} className="flex flex-col gap-0.5">
              <dt className="text-xs text-muted-foreground">{k}</dt>
              <dd className="first-letter:uppercase">{v}</dd>
            </div>
          ))}
      </dl>
      {anime.genres.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {anime.genres.map((g) => (
            <Badge key={g} variant="outline" render={<Link to="/search" search={{ genre: g }} />}>
              {g}
            </Badge>
          ))}
        </div>
      )}
      {anime.tags.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {anime.tags.map((t) => (
            <Badge key={t} variant="ghost">
              {t}
            </Badge>
          ))}
        </div>
      )}
    </div>
  )
}

function DetailSkeleton() {
  return (
    <div className="flex flex-col">
      <Skeleton className="h-72 w-full rounded-none" />
      <div className="-mt-40 flex gap-8 px-8">
        <Skeleton className="aspect-[2/3] w-52 rounded-xl" />
        <div className="flex flex-1 flex-col justify-end gap-3 pt-28">
          <Skeleton className="h-9 w-2/3" />
          <Skeleton className="h-5 w-1/3" />
          <Skeleton className="h-10 w-64" />
        </div>
      </div>
    </div>
  )
}

/** What the queue is doing for this title (Android's BulkDownloadStatus): batches take minutes,
 * and without this the only sign is episodes quietly turning into ticks. */
function BatchStatus({ rows }: { rows: DownloadRecord[] }) {
  const paused = useQueueState().data?.paused ?? false
  const active = rows.filter(isActive)
  if (active.length === 0) return null
  // The worker runs two at a time; together they show as one average.
  const running = active.filter((r) => r.status === "downloading")
  const progress = running.length > 0 ? running.reduce((sum, r) => sum + r.progress, 0) / running.length : null
  const finished = rows.filter((r) => r.status === "done" && r.updatedAt >= Math.min(...active.map((a) => a.createdAt))).length
  const total = finished + active.length
  return (
    <div className="flex items-center gap-3 text-xs text-muted-foreground">
      <Ring value={progress} className="size-3.5" />
      {running.length === 1 ? (
        <span>
          Downloading {finished + 1} of {total} · {episodeLabel(running[0].episode).toLowerCase()} · {Math.round(running[0].progress * 100)}%
          {running[0].eta != null && ` · ${formatEta(running[0].eta)}`}
        </span>
      ) : progress != null ? (
        <span>
          Downloading {finished + 1}–{finished + running.length} of {total} ·{" "}
          {running.map((r) => episodeLabel(r.episode).toLowerCase()).join(", ")} · {Math.round(progress * 100)}%
        </span>
      ) : (
        <span>
          {active.length} episode{active.length === 1 ? "" : "s"} {paused ? "waiting · the queue is paused" : "waiting their turn"}
        </span>
      )}
      <Button variant="link" size="xs" className="px-0" nativeButton={false} render={<Link to="/downloads" />}>
        View
      </Button>
      <Button
        variant="link"
        size="xs"
        className="px-0 text-muted-foreground"
        onClick={() => void Promise.all(active.map((r) => api.downloadCancel(r.id))).catch((e) => toastFailure(e))}
      >
        Stop
      </Button>
    </div>
  )
}
