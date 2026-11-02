// The account's page, as Android's account sheet lays it out: a header, then Overview, Stats and
// Activity. Everything below the header is worked out from the list the app already loads
// (src/lib/insights.ts, the port of Android's `accountInsights`).

import { Link } from "@tanstack/react-router"
import { useQueryClient } from "@tanstack/react-query"
import { ChevronRightIcon, ExternalLinkIcon, LogOutIcon, PlayIcon, RefreshCwIcon } from "lucide-react"
import { useMemo, useState } from "react"
import { toast } from "sonner"

import anilistLogo from "@/assets/anilist.svg"
import malLogo from "@/assets/mal.svg"
import { ErrorNotice } from "@/components/layout/error-notice"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, keys, useAccountProfile, useAnilistList, useMalUser, useViewer } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import { timeAgo } from "@/lib/format"
import { type Bar, type Insights, accountInsights, activityDays, watchTime } from "@/lib/insights"
import type { AccountProfile, ListEntry } from "@/lib/types"
import { useDragScroll } from "@/lib/use-drag-scroll"
import { cn } from "@/lib/utils"

/** The single hue bars are drawn in: one series, so no legend and no second colour (Android's). */
const BAR = "#3987E5"
const SERVICE = { anilist: "AniList", mal: "MyAnimeList" } as const

export function ProfilePage({ initialTab }: { initialTab?: string }) {
  const viewer = useViewer()
  const mal = useMalUser()
  const signedIn = !!viewer.data || !!mal.data
  if (viewer.isPending || mal.isPending) {
    return (
      <div className="flex flex-col gap-6 p-8">
        <Skeleton className="h-48 rounded-2xl" />
        <Skeleton className="h-32 rounded-2xl" />
      </div>
    )
  }
  return signedIn ? <Signedin initialTab={initialTab} /> : <SignIn />
}

// ---- signed out ---------------------------------------------------------------------------------

function SignIn() {
  return (
    <div className="flex flex-col gap-6 p-8">
      <h1 className="font-heading text-2xl font-bold">Profile</h1>
      <div className="surface flex max-w-xl flex-col gap-4 rounded-2xl p-6">
        <div className="flex flex-col gap-1">
          <h2 className="font-heading text-xl font-bold">Your anime, in one place</h2>
          <p className="text-sm text-muted-foreground">
            Connect AniList or MyAnimeList to see every list, your stats and your activity in Anilili.
          </p>
        </div>
        <SignInRow
          logo={anilistLogo}
          title="Continue with AniList"
          subtitle="Sync with anilist.co"
          emphasized
          onClick={() => api.login().catch(toastFailure)}
        />
        <SignInRow
          logo={malLogo}
          title="Continue with MyAnimeList"
          subtitle="Sync with myanimelist.net"
          onClick={() => api.malLogin().catch(toastFailure)}
        />
      </div>
    </div>
  )
}

function SignInRow({ logo, title, subtitle, emphasized, onClick }: { logo: string; title: string; subtitle: string; emphasized?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "surface-control flex items-center gap-4 rounded-xl px-4 py-3 text-left transition hover:bg-muted",
        emphasized && "bg-foreground/10",
      )}
    >
      <img src={logo} alt="" className="size-10 rounded-lg" />
      <span className="flex flex-1 flex-col">
        <span className="font-semibold">{title}</span>
        <span className="text-sm text-muted-foreground">{subtitle}</span>
      </span>
      <ChevronRightIcon className="text-muted-foreground" />
    </button>
  )
}

// ---- signed in ----------------------------------------------------------------------------------

function Signedin({ initialTab }: { initialTab?: string }) {
  const account = useAccountProfile(true)
  const list = useAnilistList(true)
  const [tab, setTab] = useState(initialTab ?? "overview")
  const entries = useMemo(() => list.data ?? [], [list.data])
  const insights = useMemo(() => (account.data ? accountInsights(account.data, entries) : null), [account.data, entries])

  if (account.isError || list.isError) {
    const error = account.error ?? list.error
    return (
      <div className="p-8">
        <ErrorNotice error={error} onRetry={() => void Promise.all([account.refetch(), list.refetch()])} />
      </div>
    )
  }
  if (!account.data || !insights || list.isPending) {
    return (
      <div className="flex flex-col gap-6 p-8">
        <Skeleton className="h-56 rounded-2xl" />
        <div className="grid grid-cols-3 gap-4">
          <Skeleton className="h-24 rounded-2xl" />
          <Skeleton className="h-24 rounded-2xl" />
          <Skeleton className="h-24 rounded-2xl" />
        </div>
      </div>
    )
  }
  const profile = account.data
  return (
    <div className="flex flex-col gap-6 p-8">
      <Header profile={profile} fallbackBanner={insights.recent.find((e) => e.anime.banner)?.anime.banner ?? null} />
      <Tabs value={tab} onValueChange={(v) => setTab(String(v))} className="gap-6">
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="stats">Stats</TabsTrigger>
          <TabsTrigger value="activity">Activity</TabsTrigger>
        </TabsList>
        <TabsContent value="overview">
          <Overview insights={insights} onAllActivity={() => setTab("activity")} />
        </TabsContent>
        <TabsContent value="stats">
          <Stats insights={insights} />
        </TabsContent>
        <TabsContent value="activity">
          <Activity profile={profile} entries={entries} insights={insights} />
        </TabsContent>
      </Tabs>
    </div>
  )
}

/** Banner, a large picture overlapping it, the name and the service. An account with no banner
 * (every MyAnimeList one) borrows the art of what it changed last, so the space still says
 * something about the viewer. */
function Header({ profile, fallbackBanner }: { profile: AccountProfile; fallbackBanner: string | null }) {
  const client = useQueryClient()
  const [syncing, setSyncing] = useState(false)
  const banner = profile.banner ?? fallbackBanner
  const joined = profile.joined
    ? new Date(profile.joined * 1000).toLocaleDateString(undefined, { month: "long", year: "numeric" })
    : null

  const sync = async () => {
    setSyncing(true)
    try {
      await api.accountRefresh()
      await Promise.all([client.invalidateQueries({ queryKey: keys.account }), client.invalidateQueries({ queryKey: keys.anilistList })])
      toast.success(`Synced with ${SERVICE[profile.service]}`)
    } catch (e) {
      toastFailure(e)
    } finally {
      setSyncing(false)
    }
  }
  const logOut = () => void (profile.service === "anilist" ? api.logout() : api.malLogout()).catch(toastFailure)

  return (
    <div className="flex flex-col">
      <div className="relative h-44 overflow-hidden rounded-2xl bg-muted">
        {banner && <img src={banner} alt="" className="size-full object-cover" />}
        <div className="absolute inset-0 bg-linear-to-b from-black/15 to-black/70" />
      </div>
      <div className="flex flex-wrap items-end gap-4 px-6">
        <Avatar className="-mt-14 size-28 rounded-2xl border-4 border-background">
          <AvatarImage src={profile.avatar ?? undefined} alt="" className="rounded-xl object-cover" />
          <AvatarFallback className="rounded-xl text-3xl">{profile.name.slice(0, 1)}</AvatarFallback>
        </Avatar>
        <div className="flex min-w-0 flex-1 flex-col pt-3">
          <h1 className="truncate font-heading text-2xl font-bold">{profile.name}</h1>
          <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
            <img src={profile.service === "anilist" ? anilistLogo : malLogo} alt="" className="size-4 rounded-sm" />
            {SERVICE[profile.service]}
            {joined && ` · Joined ${joined}`}
          </p>
        </div>
        <div className="flex gap-2 pt-3">
          <Button variant="outline" onClick={() => api.openAccountPage(profile.url).catch(toastFailure)}>
            <ExternalLinkIcon data-icon="inline-start" />
            View profile
          </Button>
          <Tooltip>
            <TooltipTrigger render={<Button variant="outline" size="icon" aria-label="Sync now" loading={syncing} onClick={() => void sync()} />}>
              <RefreshCwIcon />
            </TooltipTrigger>
            <TooltipContent>Pull your lists from {SERVICE[profile.service]} again</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger render={<Button variant="outline" size="icon" aria-label="Log out" onClick={logOut} />}>
              <LogOutIcon />
            </TooltipTrigger>
            <TooltipContent>Sign this computer out of {SERVICE[profile.service]}</TooltipContent>
          </Tooltip>
        </div>
      </div>
    </div>
  )
}

// ---- overview -----------------------------------------------------------------------------------

function Overview({ insights, onAllActivity }: { insights: Insights; onAllActivity: () => void }) {
  const watching = insights.recent.filter((e) => e.status === "CURRENT" || e.status === "REPEATING").slice(0, 10)
  return (
    <div className="flex flex-col gap-8">
      <div className="surface grid grid-cols-3 divide-x rounded-2xl py-5">
        <Headline value={insights.titles.toLocaleString()} label="Anime" />
        <Headline value={watchTime(insights.minutes)} label="Watch time" />
        <Headline value={insights.meanScore == null ? "—" : insights.meanScore.toFixed(1)} label={insights.meanScore == null ? "No scores yet" : "Mean score"} />
      </div>

      <section className="flex flex-col gap-3">
        <SectionHeading title="Your anime" action={<Link to="/library" className="text-sm text-muted-foreground hover:text-foreground">View all</Link>} />
        <StackedStatusBar insights={insights} />
        <div className="flex flex-wrap gap-x-6 gap-y-2">
          {insights.statusCounts.map(({ slice, count }) => (
            <span key={slice.key} className="flex items-center gap-2 text-sm">
              <span className="size-2.5 rounded-full" style={{ backgroundColor: slice.color }} />
              {slice.label}
              <span className="font-semibold tabular-nums">{count}</span>
            </span>
          ))}
        </div>
      </section>

      {watching.length > 0 && (
        <section className="flex flex-col gap-3">
          <SectionHeading title="Continue watching" />
          <ContinueRow entries={watching} />
        </section>
      )}

      <section className="flex flex-col gap-3">
        <SectionHeading
          title="Recent activity"
          action={
            <button type="button" className="text-sm text-muted-foreground hover:text-foreground" onClick={onAllActivity}>
              View all
            </button>
          }
        />
        {insights.recent.length === 0 ? (
          <p className="text-sm text-muted-foreground">Nothing on your list has changed yet.</p>
        ) : (
          <div className="flex max-w-3xl flex-col gap-1">
            {insights.recent.slice(0, 6).map((entry) => (
              <ActivityRow key={entry.anime.id} entry={entry} />
            ))}
          </div>
        )}
      </section>
    </div>
  )
}

function Headline({ value, label }: { value: string; label: string }) {
  return (
    <div className="flex flex-col items-center gap-1 px-4">
      <span className="font-heading text-2xl font-bold tabular-nums">{value}</span>
      <span className="text-sm text-muted-foreground">{label}</span>
    </div>
  )
}

function SectionHeading({ title, action }: { title: string; action?: React.ReactNode }) {
  return (
    <div className="flex items-baseline justify-between gap-4">
      <h2 className="font-heading text-lg font-semibold">{title}</h2>
      {action}
    </div>
  )
}

function StackedStatusBar({ insights }: { insights: Insights }) {
  const total = insights.statusCounts.reduce((sum, s) => sum + s.count, 0)
  return (
    <div className="flex h-3 gap-0.5 overflow-hidden rounded-full bg-muted">
      {total > 0 &&
        insights.statusCounts
          .filter((s) => s.count > 0)
          .map(({ slice, count }) => (
            <Tooltip key={slice.key}>
              <TooltipTrigger render={<span className="h-full" style={{ width: `${(count * 100) / total}%`, backgroundColor: slice.color }} />} />
              <TooltipContent>
                {slice.label}: {count} ({Math.round((count * 100) / total)}%)
              </TooltipContent>
            </Tooltip>
          ))}
    </div>
  )
}

function ContinueRow({ entries }: { entries: ListEntry[] }) {
  const row = useDragScroll<HTMLDivElement>()
  return (
    <div ref={row} className="flex gap-4 overflow-x-auto pb-2">
      {entries.map((entry) => (
        <ContinueCard key={entry.anime.id} entry={entry} />
      ))}
    </div>
  )
}

/** A title in progress: on to the next episode, or to its page once there's nothing left. */
function ContinueCard({ entry }: { entry: ListEntry }) {
  const next = entry.progress + 1
  const more = entry.anime.episodes == null || next <= entry.anime.episodes
  const label =
    entry.progress === 0
      ? "Not started"
      : entry.anime.episodes
        ? `Episode ${entry.progress} of ${entry.anime.episodes}`
        : `Episode ${entry.progress}`
  const body = (
    <>
      <div className="relative aspect-[2/3] w-full overflow-hidden rounded-xl bg-muted">
        {entry.anime.cover && <img src={entry.anime.cover} alt="" loading="lazy" className="size-full object-cover transition group-hover:scale-105" />}
        {more && (
          <span className="absolute right-2 bottom-2 flex size-9 items-center justify-center rounded-full bg-black/70 opacity-0 transition group-hover:opacity-100">
            <PlayIcon className="size-4 fill-current" />
          </span>
        )}
        {entry.anime.episodes ? (
          <span className="absolute inset-x-0 bottom-0 h-1 bg-black/50">
            <span className="block h-full bg-primary" style={{ width: `${Math.min(100, (entry.progress * 100) / entry.anime.episodes)}%` }} />
          </span>
        ) : null}
      </div>
      <span className="line-clamp-2 text-sm font-medium" title={entry.anime.title}>{entry.anime.title}</span>
      <span className="text-xs text-muted-foreground">{label}</span>
    </>
  )
  const className = "group flex w-36 shrink-0 flex-col gap-1.5"
  return more ? (
    <Link to="/watch/$id/$episode" params={{ id: String(entry.anime.id), episode: String(next) }} className={className}>
      {body}
    </Link>
  ) : (
    <Link to="/anime/$id" params={{ id: String(entry.anime.id) }} className={className}>
      {body}
    </Link>
  )
}

function activityText(entry: ListEntry): string {
  switch (entry.status) {
    case "CURRENT":
      return entry.progress > 0 ? `Watched episode ${entry.progress}` : "Started watching"
    case "REPEATING":
      return `Rewatching, episode ${entry.progress}`
    case "COMPLETED":
      return entry.score > 0 ? `Completed · scored ${formatScore(entry.score)}` : "Completed"
    case "PLANNING":
      return "Plans to watch"
    case "PAUSED":
      return `Paused at episode ${entry.progress}`
    case "DROPPED":
      return `Dropped at episode ${entry.progress}`
    default:
      return entry.status
  }
}

const formatScore = (score: number) => (Number.isInteger(score) ? String(score) : score.toFixed(1))

function ActivityRow({ entry }: { entry: ListEntry }) {
  return (
    <Link to="/anime/$id" params={{ id: String(entry.anime.id) }} className="flex items-center gap-3 rounded-lg p-2 transition hover:bg-muted">
      {entry.anime.cover ? (
        <img src={entry.anime.cover} alt="" loading="lazy" className="h-14 w-10 shrink-0 rounded-md object-cover" />
      ) : (
        <div className="h-14 w-10 shrink-0 rounded-md bg-muted" />
      )}
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="truncate font-medium" title={entry.anime.title}>{entry.anime.title}</span>
        <span className="text-sm text-muted-foreground">{activityText(entry)}</span>
      </span>
      <span className="shrink-0 text-xs text-muted-foreground">{timeAgo(entry.updatedAt)}</span>
    </Link>
  )
}

// ---- stats --------------------------------------------------------------------------------------

function Stats({ insights }: { insights: Insights }) {
  const wholeDay = insights.minutes >= 24 * 60
  const onlyPlanning = insights.statusCounts.every((s) => s.slice.key === "planning" || s.count === 0)
  return (
    <div className="flex flex-col gap-8">
      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <Tile
          value={wholeDay ? (insights.minutes / 1440).toLocaleString(undefined, { maximumFractionDigits: 1 }) : watchTime(insights.minutes)}
          label={wholeDay ? "Days watched" : "Watch time"}
        />
        <Tile value={insights.episodes.toLocaleString()} label="Episodes" />
        <Tile value={insights.completionRate == null ? "—" : `${insights.completionRate}%`} label="Completion rate" />
        <Tile value={insights.episodesToGo.toLocaleString()} label="Episodes to go" />
      </div>
      <div className="grid grid-cols-1 gap-6 xl:grid-cols-2">
        <StatusBreakdown insights={insights} />
        {insights.topGenres.length > 0 && (
          <HorizontalBars
            title="Top genres"
            subtitle={onlyPlanning ? "Across the titles you plan to watch" : "Across the titles you have started"}
            items={insights.topGenres}
          />
        )}
        {insights.formats.length > 0 && <HorizontalBars title="Formats" subtitle="What your list is made of" items={insights.formats} />}
        {insights.studios.length > 0 && <HorizontalBars title="Top studios" subtitle="Who made what you have started" items={insights.studios} />}
        {insights.releaseYears.length > 0 && <ColumnChart title="Release years" subtitle="When the titles on your list came out" items={insights.releaseYears} />}
        {insights.scoreSpread.length > 0 ? (
          <ColumnChart title="Your scores" subtitle="How you rate what you have scored" items={insights.scoreSpread} />
        ) : (
          <Panel title="Your scores" subtitle="Score a few titles and your spread shows up here" />
        )}
      </div>
    </div>
  )
}

function Tile({ value, label }: { value: string; label: string }) {
  return (
    <div className="surface flex flex-col gap-1 rounded-2xl p-4">
      <span className="font-heading text-2xl font-bold tabular-nums">{value}</span>
      <span className="text-sm text-muted-foreground">{label}</span>
    </div>
  )
}

function Panel({ title, subtitle, children }: { title: string; subtitle: string; children?: React.ReactNode }) {
  return (
    <section className="surface flex flex-col gap-4 rounded-2xl p-5">
      <div className="flex flex-col">
        <h3 className="font-heading font-semibold">{title}</h3>
        <p className="text-sm text-muted-foreground">{subtitle}</p>
      </div>
      {children}
    </section>
  )
}

function StatusBreakdown({ insights }: { insights: Insights }) {
  const total = insights.statusCounts.reduce((sum, s) => sum + s.count, 0)
  return (
    <Panel title="Your list" subtitle={`${total.toLocaleString()} titles by status`}>
      <div className="flex flex-col gap-3">
        {insights.statusCounts.map(({ slice, count }) => (
          <div key={slice.key} className="flex flex-col gap-1">
            <div className="flex justify-between text-sm">
              <span>{slice.label}</span>
              <span className="text-muted-foreground tabular-nums">
                {count} · {total ? Math.round((count * 100) / total) : 0}%
              </span>
            </div>
            <div className="h-2 overflow-hidden rounded-full bg-muted">
              <div className="h-full rounded-full" style={{ width: `${total ? (count * 100) / total : 0}%`, backgroundColor: slice.color }} />
            </div>
          </div>
        ))}
      </div>
    </Panel>
  )
}

function HorizontalBars({ title, subtitle, items }: { title: string; subtitle: string; items: Bar[] }) {
  const max = Math.max(...items.map((i) => i.value), 1)
  return (
    <Panel title={title} subtitle={subtitle}>
      <div className="flex flex-col gap-2.5">
        {items.map((item) => (
          <div key={item.label} className="grid grid-cols-[8rem_1fr_2.5rem] items-center gap-3 text-sm">
            <span className="truncate">{item.label}</span>
            <div className="h-2 overflow-hidden rounded-full bg-muted">
              <div className="h-full rounded-full" style={{ width: `${(item.value * 100) / max}%`, backgroundColor: BAR }} />
            </div>
            <span className="text-right text-muted-foreground tabular-nums">{item.value}</span>
          </div>
        ))}
      </div>
    </Panel>
  )
}

function ColumnChart({ title, subtitle, items }: { title: string; subtitle: string; items: Bar[] }) {
  const max = Math.max(...items.map((i) => i.value), 1)
  return (
    <Panel title={title} subtitle={subtitle}>
      <div className="flex h-40 items-end gap-1.5">
        {items.map((item) => (
          <Tooltip key={item.label}>
            <TooltipTrigger
              render={
                <div className="flex h-full flex-1 flex-col items-center justify-end gap-1">
                  <span className="text-xs text-muted-foreground tabular-nums">{item.value || ""}</span>
                  <div className="w-full rounded-t-md" style={{ height: `${(item.value * 100) / max}%`, minHeight: item.value ? 3 : 0, backgroundColor: BAR }} />
                </div>
              }
            />
            <TooltipContent>
              {item.label}: {item.value}
            </TooltipContent>
          </Tooltip>
        ))}
      </div>
      <div className="flex gap-1.5">
        {items.map((item) => (
          <span key={item.label} className="flex-1 truncate text-center text-xs text-muted-foreground">
            {item.label}
          </span>
        ))}
      </div>
    </Panel>
  )
}

// ---- activity -----------------------------------------------------------------------------------

const WEEKS = 26
const DAY = 86_400

function Activity({ profile, entries, insights }: { profile: AccountProfile; entries: ListEntry[]; insights: Insights }) {
  const { days, fromAccount } = useMemo(() => activityDays(profile, entries), [profile, entries])
  // Columns are weeks (Monday first), the last ending today.
  const [today] = useState(() => Math.floor(Date.now() / 1000 / DAY) * DAY)
  const weekday = (new Date(today * 1000).getUTCDay() + 6) % 7
  const start = today - (weekday + (WEEKS - 1) * 7) * DAY
  const cells = Array.from({ length: WEEKS * 7 }, (_, i) => start + i * DAY)
  const values = cells.filter((d) => d <= today).map((d) => days.get(d) ?? 0)
  const active = values.filter((v) => v > 0).sort((a, b) => a - b)
  // Four levels by the account's own spread, so a busy and a quiet account both read.
  const cut = (q: number) => active[Math.min(active.length - 1, Math.floor(q * active.length))] ?? 1
  const level = (v: number) => (v <= 0 ? 0 : v <= cut(0.25) ? 1 : v <= cut(0.5) ? 2 : v <= cut(0.75) ? 3 : 4)
  let streak = 0
  for (let d = (days.get(today) ?? 0) > 0 ? today : today - DAY; (days.get(d) ?? 0) > 0; d -= DAY) streak++

  return (
    <div className="flex flex-col gap-8">
      <Panel
        title="Your activity"
        subtitle={fromAccount ? `From your ${SERVICE[profile.service]} activity, the last six months` : "When titles on your list last changed, the last six months"}
      >
        <div className="flex gap-2 overflow-x-auto overflow-y-hidden pb-1">
          <div className="grid grid-rows-7 gap-1.5 pr-1 text-xs text-muted-foreground">
            {["Mon", "", "Wed", "", "Fri", "", "Sun"].map((d, i) => (
              <span key={i} className="flex h-5 items-center">
                {d}
              </span>
            ))}
          </div>
          <div className="grid grid-flow-col grid-rows-7 gap-1.5">
            {cells.map((day) => {
              const v = days.get(day) ?? 0
              const future = day > today
              return (
                <Tooltip key={day}>
                  <TooltipTrigger
                    render={
                      <span
                        className={cn("size-5 rounded-[4px]", future ? "bg-transparent" : level(v) === 0 && "bg-muted")}
                        style={!future && v > 0 ? { backgroundColor: BAR, opacity: 0.25 + level(v) * 0.1875 } : undefined}
                      />
                    }
                  />
                  {!future && (
                    <TooltipContent>
                      {new Date(day * 1000).toLocaleDateString(undefined, { timeZone: "UTC", dateStyle: "medium" })}:{" "}
                      {v === 0 ? "nothing" : `${v} ${v === 1 ? "change" : "changes"}`}
                    </TooltipContent>
                  )}
                </Tooltip>
              )
            })}
          </div>
        </div>
        <div className="flex items-center justify-between gap-4 text-sm">
          <span className="text-muted-foreground">
            {streak === 0 ? "No streak yet" : `${streak}-day streak`} · {active.length} active {active.length === 1 ? "day" : "days"}
          </span>
          <span className="flex items-center gap-1 text-xs text-muted-foreground">
            Less
            {[0, 1, 2, 3, 4].map((l) => (
              <span key={l} className={cn("size-3 rounded-[3px]", l === 0 && "bg-muted")} style={l > 0 ? { backgroundColor: BAR, opacity: 0.25 + l * 0.1875 } : undefined} />
            ))}
            More
          </span>
        </div>
      </Panel>
      <section className="flex flex-col gap-3">
        <SectionHeading title="Recent activity" />
        <div className="flex max-w-3xl flex-col gap-1">
          {insights.recent.slice(0, 60).map((entry) => (
            <ActivityRow key={entry.anime.id} entry={entry} />
          ))}
        </div>
      </section>
    </div>
  )
}
