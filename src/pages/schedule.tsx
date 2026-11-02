// The schedule: a strip of days around today with each day's count, the chosen day as a timeline
// grouped by air time (a "now" line splitting what aired from what's to come), and beside it a rail
// with the next episode, what airs from the library this week, a month calendar and filters.

import { Link, useNavigate } from "@tanstack/react-router"
import { BellIcon, BellRingIcon, BookmarkIcon, ChevronLeftIcon, ChevronRightIcon, PlayIcon, StarIcon, XIcon } from "lucide-react"
import { useQueryClient } from "@tanstack/react-query"
import { Fragment, useEffect, useMemo, useState } from "react"

import { AnimeContextMenu } from "@/components/anime/anime-actions"
import { ErrorNotice } from "@/components/layout/error-notice"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, keys, useAppearance, useDubTitles, useLibrary, useReminders, useSchedule } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import { useDragScroll } from "@/lib/use-drag-scroll"
import { countdown, formatLabel, timeAgo } from "@/lib/format"
import { clockTime } from "@/lib/theme"
import type { AiringEntry, LibraryEntry } from "@/lib/types"
import { cn } from "@/lib/utils"

/** The strip's first day relative to today, and how many it shows: as on Android. */
const STRIP_FIRST = -2
const STRIP_LENGTH = 9
const MINE_KEY = "schedule.mine"
const DUB_KEY = "schedule.dub"
const FORMATS_KEY = "schedule.formats"
/** The order format chips are offered in. */
const FORMAT_ORDER = ["TV", "TV_SHORT", "ONA", "OVA", "SPECIAL", "MOVIE", "MUSIC"]

type Keep = (entry: AiringEntry) => boolean

export function SchedulePage() {
  const todayTime = startOfDay(new Date()).getTime()
  const today = useMemo(() => new Date(todayTime), [todayTime])
  const [selected, setSelected] = useState(today)
  const [month, setMonth] = useState(() => monthOf(today))
  // "My list": only the titles in the library (every list, as the Library page holds them).
  const [mine, setMine] = useState(() => localStorage.getItem(MINE_KEY) === "1")
  const [dubOnly, setDubOnly] = useState(() => localStorage.getItem(DUB_KEY) === "1")
  const [formats, setFormats] = useState<string[]>(() => storedList(FORMATS_KEY))
  const library = useLibrary()
  const dubbed = useDubTitles().data
  const now = useNow()

  const stripDays = useMemo(() => Array.from({ length: STRIP_LENGTH }, (_, i) => addDays(today, STRIP_FIRST + i)), [today])
  const gridDays = useMemo(() => monthGrid(month, FIRST_DAY), [month])
  const selectedTime = selected.getTime()
  const selectedGrid = useMemo(() => monthGrid(monthOf(new Date(selectedTime)), FIRST_DAY), [selectedTime])
  // The strip's nine days, the calendar's month and the chosen day's month: usually two requests,
  // as the media index publishes a month per file and React Query shares equal ranges.
  const strip = useSchedule(...span(stripDays))
  const calendar = useSchedule(...span(gridDays))
  const day = useSchedule(...span(selectedGrid))

  const libraryById = useMemo(() => new Map(library.data?.map((e) => [e.anime.id, e])), [library.data])
  const keep = useMemo<Keep>(() => {
    const allowed = new Set(formats)
    return (entry) =>
      (!mine || libraryById.has(entry.anime.id)) &&
      (allowed.size === 0 || allowed.has(entry.anime.format ?? "")) &&
      (!dubOnly || (dubbed?.has(entry.anime.id) ?? false))
  }, [mine, formats, dubOnly, dubbed, libraryById])
  const stripByDay = useMemo(() => byDay(strip.data, keep), [strip.data, keep])
  const calendarByDay = useMemo(() => byDay(calendar.data, keep), [calendar.data, keep])
  const dayEntries = useMemo(() => byDay(day.data, keep).get(selectedTime) ?? [], [day.data, keep, selectedTime])
  // The week ahead, for the rail: what's next, and what's next from the library.
  const ahead = useMemo(() => (strip.data ?? []).filter((e) => e.airingAt > now && keep(e)).sort((a, b) => a.airingAt - b.airingAt), [strip.data, keep, now])
  const listAhead = useMemo(() => ahead.filter((e) => libraryById.has(e.anime.id)), [ahead, libraryById])
  const offered = useMemo(() => {
    const present = new Set([...(strip.data ?? []), ...(day.data ?? [])].map((e) => e.anime.format ?? ""))
    return FORMAT_ORDER.filter((f) => present.has(f) || formats.includes(f))
  }, [strip.data, day.data, formats])

  const toggleMine = () => {
    localStorage.setItem(MINE_KEY, mine ? "0" : "1")
    setMine(!mine)
  }
  const toggleDub = () => {
    localStorage.setItem(DUB_KEY, dubOnly ? "0" : "1")
    setDubOnly(!dubOnly)
  }
  const changeFormats = (next: string[]) => {
    localStorage.setItem(FORMATS_KEY, JSON.stringify(next))
    setFormats(next)
  }
  const select = (date: Date) => {
    setSelected(date)
    setMonth(monthOf(date))
  }

  return (
    <div className="flex flex-col gap-5 p-8">
      <div className="flex items-center gap-3">
        <h1 className="font-heading text-2xl font-bold">Schedule</h1>
        <button
          type="button"
          aria-pressed={mine}
          onClick={toggleMine}
          className={cn(
            "ml-auto flex h-8 items-center gap-1.5 rounded-lg border px-3 text-sm transition",
            mine ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
          )}
        >
          <BookmarkIcon className="size-4" />
          My list
        </button>
      </div>

      <DayStrip days={stripDays} today={today} selected={selected} byDay={stripByDay} library={libraryById} onSelect={select} />

      <div className="grid grid-cols-1 items-start gap-8 xl:grid-cols-[minmax(0,1fr)_22rem]">
        <DayList
          date={selected}
          isToday={selectedTime === today.getTime()}
          entries={dayEntries}
          schedule={day}
          mine={mine}
          filtered={formats.length > 0 || dubOnly}
          waiting={mine && library.isPending}
          library={libraryById}
          now={now}
        />
        <aside className="flex flex-col gap-4 xl:sticky xl:top-4">
          {ahead[0] && <UpNext entry={ahead[0]} now={now} />}
          {libraryById.size > 0 && !mine && <FromYourList entries={listAhead} library={libraryById} today={today} />}
          <MonthCalendar
            month={month}
            days={gridDays}
            today={today}
            selected={selected}
            counts={calendarByDay}
            loading={calendar.isFetching}
            onStep={(delta) => setMonth(new Date(month.getFullYear(), month.getMonth() + delta, 1))}
            onSelect={select}
          />
          <Filters offered={offered} formats={formats} dubOnly={dubOnly} onFormats={changeFormats} onDub={toggleDub} />
        </aside>
      </div>
    </div>
  )
}

function DayStrip({
  days,
  today,
  selected,
  byDay,
  library,
  onSelect,
}: {
  days: Date[]
  today: Date
  selected: Date
  byDay: Map<number, AiringEntry[]>
  library: Map<number, LibraryEntry>
  onSelect: (d: Date) => void
}) {
  const strip = useDragScroll<HTMLDivElement>()
  return (
    <div ref={strip} className="flex gap-2 overflow-x-auto pb-1">
      {days.map((date) => {
        const offset = Math.round((date.getTime() - today.getTime()) / 86_400_000)
        const active = date.getTime() === selected.getTime()
        const entries = byDay.get(date.getTime()) ?? []
        const fromList = entries.some((e) => library.has(e.anime.id))
        const label =
          offset === 0
            ? "Today"
            : offset === 1
              ? "Tomorrow"
              : offset === -1
                ? "Yesterday"
                : date.toLocaleDateString(undefined, { weekday: "short" })
        return (
          <button
            key={date.getTime()}
            type="button"
            onClick={() => onSelect(date)}
            aria-pressed={active}
            className={cn(
              "flex h-20 min-w-24 flex-1 shrink-0 flex-col items-center justify-center gap-0.5 rounded-xl border transition",
              active
                ? "border-primary bg-primary text-primary-foreground"
                : offset === 0
                  ? "surface-control border-primary hover:bg-muted"
                  : "surface-control hover:bg-muted",
            )}
          >
            <span className="text-sm font-semibold">{label}</span>
            <span className={cn("text-xs", active ? "text-primary-foreground/70" : "text-muted-foreground")}>
              {date.toLocaleDateString(undefined, { month: "short", day: "numeric" })}
            </span>
            <span className={cn("flex items-center gap-1 text-xs", active ? "text-primary-foreground/80" : "text-muted-foreground")}>
              {fromList && (
                <span
                  className={cn("size-1.5 rounded-full", active ? "bg-primary-foreground" : "bg-primary")}
                  title="Something from your list airs"
                />
              )}
              {entries.length > 0 ? `${entries.length} ep${entries.length === 1 ? "" : "s"}` : "—"}
            </span>
          </button>
        )
      })}
    </div>
  )
}

function DayList({
  date,
  isToday,
  entries,
  schedule,
  mine,
  filtered,
  waiting,
  library,
  now,
}: {
  date: Date
  isToday: boolean
  entries: AiringEntry[]
  schedule: ReturnType<typeof useSchedule>
  mine: boolean
  /** A format or dub filter is on. */
  filtered: boolean
  /** The library is still loading, so "My list" can't filter yet. */
  waiting: boolean
  library: Map<number, LibraryEntry>
  now: number
}) {
  const slots = useMemo(() => {
    const groups: { airingAt: number; entries: AiringEntry[] }[] = []
    for (const entry of entries) {
      const last = groups[groups.length - 1]
      if (last && last.airingAt === entry.airingAt) last.entries.push(entry)
      else groups.push({ airingAt: entry.airingAt, entries: [entry] })
    }
    return groups
  }, [entries])
  const aired = entries.filter((e) => e.airingAt <= now).length
  // Today, the "now" line goes before the first slot still to come (or after the last one).
  const nowAt = isToday ? slots.findIndex((s) => s.airingAt > now) : -1
  const nowLast = isToday && slots.length > 0 && nowAt === -1

  return (
    <div className="flex min-w-0 flex-col gap-1">
      <h2 className="text-sm font-medium text-muted-foreground">
        {date.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}
        {!schedule.isPending && !waiting && entries.length > 0 && (
          <>
            {` · ${entries.length} episode${entries.length === 1 ? "" : "s"}`}
            {aired > 0 && aired < entries.length && ` · ${aired} aired`}
          </>
        )}
      </h2>
      {schedule.isError ? (
        <ErrorNotice error={schedule.error} onRetry={() => void schedule.refetch()} />
      ) : schedule.isPending || waiting ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(22rem,1fr))] gap-3 pt-2 pl-9">
          {Array.from({ length: 6 }, (_, i) => (
            <Skeleton key={i} className="h-30 rounded-xl" />
          ))}
        </div>
      ) : slots.length === 0 ? (
        <Empty className="border">
          <EmptyHeader>
            <EmptyTitle>Nothing scheduled</EmptyTitle>
            <EmptyDescription>
              {mine ? "Nothing on your list airs on this day." : filtered ? "Nothing airs on this day with these filters." : "No episodes air on this day."}
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="flex flex-col">
          {slots.map((slot, i) => (
            <Fragment key={slot.airingAt}>
              {i === nowAt && <NowLine now={now} />}
              <TimeSlot airingAt={slot.airingAt} entries={slot.entries} library={library} now={now} />
            </Fragment>
          ))}
          {nowLast && <NowLine now={now} />}
        </div>
      )}
    </div>
  )
}

/**
 * Brings the line into view when it first appears (opening the page, or picking today again): a
 * frame later, after the router has put a newly opened page back at its top.
 */
const scrollToNow = (el: HTMLElement | null) => {
  if (el) requestAnimationFrame(() => el.scrollIntoView({ block: "center" }))
}

function NowLine({ now }: { now: number }) {
  const { clockFormat } = useAppearance()
  return (
    <div ref={scrollToNow} className="relative flex items-center gap-3 py-3 pl-9">
      <span className="absolute top-0 bottom-0 left-2 w-px bg-border" aria-hidden />
      <span className="absolute left-2 size-3 -translate-x-1/2 rounded-full bg-primary ring-4 ring-primary/25" aria-hidden />
      <span className="text-xs font-semibold tracking-wide text-primary">NOW · {clockTime(now, clockFormat)}</span>
      <span className="h-px flex-1 bg-primary/60" aria-hidden />
    </div>
  )
}

/** One air time: a dot on the timeline (lit while still to come), the time, and its episodes. */
function TimeSlot({
  airingAt,
  entries,
  library,
  now,
}: {
  airingAt: number
  entries: AiringEntry[]
  library: Map<number, LibraryEntry>
  now: number
}) {
  const { clockFormat } = useAppearance()
  const upcoming = airingAt > now
  return (
    <div className="relative pb-3 pl-9">
      <span className="absolute top-0 bottom-0 left-2 w-px bg-border" aria-hidden />
      <span
        className={cn(
          "absolute top-4 left-2 size-2.5 -translate-x-1/2 rounded-full ring-4 ring-background",
          upcoming ? "bg-primary" : "bg-muted-foreground",
        )}
        aria-hidden
      />
      <p className="flex items-baseline gap-2 pt-2 pb-2">
        <span className="font-heading font-bold">{clockTime(airingAt, clockFormat)}</span>
        <span className={cn("text-xs", upcoming ? "text-primary" : "text-muted-foreground")}>
          {upcoming ? countdown(airingAt, now) : timeAgo(airingAt, now)}
        </span>
      </p>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(22rem,1fr))] gap-3">
        {entries.map((entry) => (
          <ScheduleCard key={`${entry.anime.id}-${entry.episode}`} entry={entry} listed={library.get(entry.anime.id)} now={now} />
        ))}
      </div>
    </div>
  )
}

function ScheduleCard({ entry, listed, now }: { entry: AiringEntry; listed?: LibraryEntry; now: number }) {
  const navigate = useNavigate()
  const { anime, episode } = entry
  const upcoming = entry.airingAt > now
  const dubbed = useDubTitles().data?.has(anime.id) ?? false
  const format = formatLabel(anime.format)
  const watched = listed?.progress ?? null
  // Behind on a title in the list: the button picks up where the account left off.
  const resumeAt = watched != null && watched < episode - 1 ? watched + 1 : episode
  const art = anime.banner ?? anime.cover
  return (
    <AnimeContextMenu anime={anime}>
      <div className="surface group relative flex overflow-hidden rounded-xl transition hover:bg-muted">
        {/* The banner (else the poster, blurred) fades in from the right, behind the text. */}
        {art && (
          <img
            src={art}
            alt=""
            loading="lazy"
            decoding="async"
            aria-hidden
            className={cn(
              "pointer-events-none absolute inset-y-0 right-0 h-full w-3/4 object-cover opacity-25 transition group-hover:opacity-40 [mask-image:linear-gradient(to_right,transparent,black_70%)]",
              !anime.banner && "scale-110 blur-xl",
            )}
          />
        )}
        <span className="absolute inset-y-0 left-0 w-1" style={{ backgroundColor: anime.color ?? "var(--primary)" }} aria-hidden />
        <Link
          to="/anime/$id"
          params={{ id: String(anime.id) }}
          className="relative flex min-w-0 flex-1 items-center gap-3 rounded-xl py-2.5 pr-1 pl-3.5 outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          {anime.cover ? (
            <img src={anime.cover} alt="" loading="lazy" className="h-24 w-16 shrink-0 rounded-lg object-cover shadow-md" />
          ) : (
            <div className="h-24 w-16 shrink-0 rounded-lg bg-muted" />
          )}
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <span className="line-clamp-2 leading-snug font-medium" title={anime.title}>
              {anime.title}
            </span>
            <span className="flex flex-wrap items-center gap-1.5 text-sm text-muted-foreground">
              Episode {episode}
              {anime.episodes ? ` / ${anime.episodes}` : ""}
            </span>
            <span className="flex items-center gap-1.5 truncate text-xs text-muted-foreground">
              {format}
              {anime.score != null && (
                <span className="flex items-center gap-0.5">
                  {format && "·"} <StarIcon className="size-3 fill-current" /> {anime.score}%
                </span>
              )}
              {anime.genres.length > 0 && <span className="truncate">· {anime.genres.slice(0, 2).join(", ")}</span>}
            </span>
            {(dubbed || listed) && (
              <span className="flex gap-1.5">
                {dubbed && <Badge variant="outline">Dub</Badge>}
                {listed && (
                  <Badge variant="secondary">
                    <BookmarkIcon />
                    {watched ? `Watched ${watched}` : "In your list"}
                  </Badge>
                )}
              </span>
            )}
          </div>
        </Link>
        {/* The action in the top corner, a premiere or finale in the bottom one. */}
        <div className="relative flex shrink-0 flex-col items-end justify-between gap-2 py-2.5 pr-2.5">
          {upcoming ? (
            <div className="flex items-center gap-1">
              <span className="text-sm font-semibold text-primary">{countdown(entry.airingAt, now)}</span>
              <ReminderBell entry={entry} />
            </div>
          ) : (
            <PlayMark
              label={resumeAt === episode ? `Watch episode ${episode}` : `Watch from episode ${resumeAt}, the next you haven't seen`}
              onClick={() => void navigate({ to: "/watch/$id/$episode", params: { id: String(anime.id), episode: String(resumeAt) } })}
            />
          )}
          {episode === 1 ? (
            <Badge>Premiere</Badge>
          ) : anime.episodes != null && episode === anime.episodes ? (
            <Badge>Finale</Badge>
          ) : null}
        </div>
      </div>
    </AnimeContextMenu>
  )
}

/** Android TV's Continue watching play mark: the arrow on a disc of the accent. */
function PlayMark({ label, onClick }: { label: string; onClick: () => void }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            aria-label={label}
            onClick={onClick}
            className="flex size-10 items-center justify-center rounded-full bg-primary text-primary-foreground shadow-md transition outline-none hover:scale-110 focus-visible:ring-2 focus-visible:ring-ring"
          />
        }
      >
        <PlayIcon className="size-5 translate-x-px fill-current" />
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

/** "Remind me": a notification ten minutes before the episode airs (Android's schedule bell). */
function ReminderBell({ entry }: { entry: AiringEntry }) {
  const client = useQueryClient()
  const set = useReminders().data?.includes(`${entry.anime.id}:${entry.episode}:${entry.airingAt}`) ?? false
  const label = set ? "Reminder set · click to cancel" : "Remind me 10 minutes before"
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <Button
            variant="ghost"
            size="icon"
            aria-label={label}
            aria-pressed={set}
            className="shrink-0"
            onClick={() =>
              api
                .reminderToggle(entry)
                .then(() => client.invalidateQueries({ queryKey: keys.reminders }))
                .catch((e) => toastFailure(e))
            }
          />
        }
      >
        {set ? <BellRingIcon className="text-primary" /> : <BellIcon className="text-muted-foreground" />}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

// ---- the rail ---------------------------------------------------------------------------------

/** The next episode to air (filters applied), as a banner with its countdown. */
function UpNext({ entry, now }: { entry: AiringEntry; now: number }) {
  const { clockFormat } = useAppearance()
  const { anime } = entry
  return (
    <section className="surface overflow-hidden rounded-2xl">
      <Link to="/anime/$id" params={{ id: String(anime.id) }} className="group block outline-none focus-visible:ring-2 focus-visible:ring-ring">
        <div className="relative h-28 overflow-hidden" style={{ backgroundColor: anime.color ?? undefined }}>
          {(anime.banner ?? anime.cover) && (
            <img
              src={anime.banner ?? anime.cover ?? undefined}
              alt=""
              className={cn("size-full object-cover transition duration-300 group-hover:scale-105", !anime.banner && "scale-110 blur-md")}
            />
          )}
          <div className="absolute inset-0 bg-gradient-to-t from-black/85 via-black/30 to-transparent" />
          <span className="absolute top-2.5 left-3 rounded-md bg-primary px-1.5 py-0.5 text-[0.7rem] font-semibold tracking-wide text-primary-foreground">
            UP NEXT
          </span>
          <div className="absolute inset-x-3 bottom-2.5 flex min-w-0 flex-col text-white">
            <span className="truncate font-medium" title={anime.title}>
              {anime.title}
            </span>
            <span className="text-xs text-white/75">Episode {entry.episode}</span>
          </div>
        </div>
      </Link>
      <div className="flex items-center gap-3 p-3">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="font-heading text-2xl font-bold text-primary">{countdown(entry.airingAt, now)}</span>
          <span className="text-xs text-muted-foreground">
            {dayName(entry.airingAt)} · {clockTime(entry.airingAt, clockFormat)}
          </span>
        </div>
        <ReminderBell entry={entry} />
      </div>
    </section>
  )
}

/** What airs from the library over the week ahead. */
function FromYourList({ entries, library, today }: { entries: AiringEntry[]; library: Map<number, LibraryEntry>; today: Date }) {
  const { clockFormat } = useAppearance()
  return (
    <section className="surface flex flex-col gap-1 rounded-2xl p-3">
      <h2 className="flex items-center gap-1.5 px-1 pb-1 text-sm font-semibold">
        <BookmarkIcon className="size-4 text-primary" />
        From your list this week
      </h2>
      {entries.length === 0 ? (
        <p className="px-1 pb-1 text-sm text-muted-foreground">Nothing from your list airs this week.</p>
      ) : (
        entries.slice(0, 6).map((entry) => {
          const watched = library.get(entry.anime.id)?.progress
          const behind = watched != null && watched < entry.episode - 1
          return (
            <Link
              key={`${entry.anime.id}-${entry.episode}`}
              to="/anime/$id"
              params={{ id: String(entry.anime.id) }}
              className="flex items-center gap-2.5 rounded-lg p-1 outline-none hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
            >
              {entry.anime.cover ? (
                <img src={entry.anime.cover} alt="" loading="lazy" className="h-12 w-8 shrink-0 rounded object-cover" />
              ) : (
                <div className="h-12 w-8 shrink-0 rounded bg-muted" />
              )}
              <div className="flex min-w-0 flex-1 flex-col">
                <span className="truncate text-sm font-medium" title={entry.anime.title}>
                  {entry.anime.title}
                </span>
                <span className="truncate text-xs text-muted-foreground">
                  Ep {entry.episode}
                  {behind && ` · you're on ${watched + 1}`}
                </span>
              </div>
              <span className="shrink-0 text-right text-xs text-muted-foreground">
                <span className="block font-medium text-foreground">{dayName(entry.airingAt, today)}</span>
                {clockTime(entry.airingAt, clockFormat)}
              </span>
            </Link>
          )
        })
      )}
      {entries.length > 6 && <p className="px-1 pt-1 text-xs text-muted-foreground">and {entries.length - 6} more</p>}
    </section>
  )
}

function MonthCalendar({
  month,
  days,
  today,
  selected,
  counts,
  loading,
  onStep,
  onSelect,
}: {
  month: Date
  days: Date[]
  today: Date
  selected: Date
  counts: Map<number, AiringEntry[]>
  loading: boolean
  onStep: (delta: number) => void
  onSelect: (d: Date) => void
}) {
  const weekdays = Array.from({ length: 7 }, (_, i) =>
    // 2023-01-01 was a Sunday: day `n` of the week is the 1st plus `n`.
    new Date(2023, 0, 1 + ((FIRST_DAY + i) % 7)).toLocaleDateString(undefined, { weekday: "narrow" }),
  )
  return (
    <section className="surface flex flex-col gap-1 rounded-2xl p-2">
      <div className="flex items-center">
        <Button variant="ghost" size="icon-sm" aria-label="Previous month" onClick={() => onStep(-1)}>
          <ChevronLeftIcon />
        </Button>
        <div className="flex flex-1 items-center justify-center gap-2">
          <span className="text-sm font-semibold">{month.toLocaleDateString(undefined, { month: "long", year: "numeric" })}</span>
          {loading && <Spinner className="size-3.5 text-muted-foreground" />}
        </div>
        <Button variant="ghost" size="icon-sm" aria-label="Next month" onClick={() => onStep(1)}>
          <ChevronRightIcon />
        </Button>
      </div>
      <div className="grid grid-cols-7">
        {weekdays.map((d, i) => (
          <span key={i} className="py-0.5 text-center text-xs text-muted-foreground">
            {d}
          </span>
        ))}
      </div>
      <div className="grid grid-cols-7 gap-0.5">
        {days.map((date) => {
          const inMonth = date.getMonth() === month.getMonth()
          const isSelected = date.getTime() === selected.getTime()
          const isToday = date.getTime() === today.getTime()
          const episodes = counts.get(date.getTime())?.length ?? 0
          return (
            <button
              key={date.getTime()}
              type="button"
              onClick={() => onSelect(date)}
              aria-pressed={isSelected}
              aria-label={`${date.toLocaleDateString(undefined, { dateStyle: "long" })}, ${episodes} episodes`}
              className={cn(
                "flex h-10 flex-col items-center justify-center rounded-lg transition",
                isSelected ? "bg-primary text-primary-foreground" : isToday ? "ring-1 ring-primary hover:bg-muted" : "hover:bg-muted",
                !inMonth && !isSelected && "opacity-40",
              )}
            >
              <span className={cn("text-sm leading-none", (isToday || isSelected) && "font-bold", isToday && !isSelected && "text-primary")}>
                {date.getDate()}
              </span>
              {/* An empty day leaves the space blank rather than printing a nought, so the grid
                  reads as the shape of the month. */}
              <span className={cn("h-3 text-[0.65rem] leading-3", isSelected ? "text-primary-foreground/80" : "text-muted-foreground")}>
                {episodes > 0 ? episodes : ""}
              </span>
            </button>
          )
        })}
      </div>
    </section>
  )
}

function Filters({
  offered,
  formats,
  dubOnly,
  onFormats,
  onDub,
}: {
  offered: string[]
  formats: string[]
  dubOnly: boolean
  onFormats: (next: string[]) => void
  onDub: () => void
}) {
  const active = formats.length > 0 || dubOnly
  return (
    <section className="surface flex flex-col gap-2.5 rounded-2xl p-3">
      <div className="flex items-center">
        <h2 className="flex-1 px-1 text-sm font-semibold">Filters</h2>
        {active && (
          <Button
            variant="ghost"
            size="xs"
            onClick={() => {
              onFormats([])
              if (dubOnly) onDub()
            }}
          >
            <XIcon data-icon="inline-start" />
            Clear
          </Button>
        )}
      </div>
      <div className="flex flex-wrap gap-1.5">
        {offered.map((f) => {
          const on = formats.includes(f)
          return (
            <Chip key={f} on={on} onClick={() => onFormats(on ? formats.filter((x) => x !== f) : [...formats, f])}>
              {formatLabel(f)}
            </Chip>
          )
        })}
        <Chip on={dubOnly} onClick={onDub}>
          Dub available
        </Chip>
      </div>
    </section>
  )
}

function Chip({ on, onClick, children }: { on: boolean; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={cn(
        "h-7 rounded-full border px-3 text-xs transition",
        on ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
      )}
    >
      {children}
    </button>
  )
}

// ---- dates ------------------------------------------------------------------------------------

function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate())
}

function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n)
}

const monthOf = (d: Date) => new Date(d.getFullYear(), d.getMonth(), 1)

const unix = (d: Date) => Math.floor(d.getTime() / 1000)

/** The request range covering a run of days: the first's midnight to the midnight after the last. */
const span = (days: Date[]): [number, number] => [unix(days[0]), unix(addDays(days[days.length - 1], 1))]

/** "Today", "Tomorrow", else the short weekday. */
function dayName(unixSeconds: number, today = startOfDay(new Date())): string {
  const offset = Math.round((startOfDay(new Date(unixSeconds * 1000)).getTime() - today.getTime()) / 86_400_000)
  if (offset === 0) return "Today"
  if (offset === 1) return "Tomorrow"
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, { weekday: "short" })
}

/** Entries kept by `keep`, by local midnight, each day in air order. */
function byDay(entries: AiringEntry[] | undefined, keep: Keep): Map<number, AiringEntry[]> {
  const map = new Map<number, AiringEntry[]>()
  for (const entry of entries ?? []) {
    if (!keep(entry)) continue
    const day = startOfDay(new Date(entry.airingAt * 1000)).getTime()
    const list = map.get(day)
    if (list) list.push(entry)
    else map.set(day, [entry])
  }
  for (const list of map.values()) list.sort((a, b) => a.airingAt - b.airingAt)
  return map
}

function storedList(key: string): string[] {
  try {
    const value: unknown = JSON.parse(localStorage.getItem(key) ?? "[]")
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : []
  } catch {
    return []
  }
}

/** The viewer's first day of the week, 0 = Sunday: the grid has to line up with the week they read. */
function firstDayOfWeek(): number {
  try {
    const locale = new Intl.Locale(navigator.language) as Intl.Locale & {
      getWeekInfo?: () => { firstDay: number }
      weekInfo?: { firstDay: number }
    }
    const info = locale.getWeekInfo?.() ?? locale.weekInfo
    // Intl counts Monday as 1 and Sunday as 7.
    if (info) return info.firstDay % 7
  } catch {
    // An unknown locale: Monday, as most of the world counts.
  }
  return 1
}

/** The month's days plus the neighbours that fill its first and last weeks: five rows or six. */
function monthGrid(month: Date, firstDay: number): Date[] {
  const first = new Date(month.getFullYear(), month.getMonth(), 1)
  const lead = (first.getDay() - firstDay + 7) % 7
  const length = new Date(month.getFullYear(), month.getMonth() + 1, 0).getDate()
  const cells = Math.ceil((lead + length) / 7) * 7
  return Array.from({ length: cells }, (_, i) => addDays(first, i - lead))
}

const FIRST_DAY = firstDayOfWeek()

/** Unix seconds, ticking every minute so "upcoming" and countdowns stay current. */
function useNow(): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Math.floor(Date.now() / 1000)), 60_000)
    return () => window.clearInterval(timer)
  }, [])
  return now
}
