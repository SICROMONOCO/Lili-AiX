import { useInfiniteQuery, useQuery } from "@tanstack/react-query"
import { useLocation } from "@tanstack/react-router"
import { MinusIcon, PlusIcon, SearchIcon, SlidersHorizontalIcon, XIcon } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"

import { AnimeCard } from "@/components/anime/anime-card"
import { CardSkeleton } from "@/components/anime/anime-row"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Sheet, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle } from "@/components/ui/sheet"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { api, keys, useDubTitles, useSettings } from "@/lib/api"
import { GENRES } from "@/lib/format"
import type { SearchCorrection, SearchFilters, Tag } from "@/lib/types"
import { useSettled } from "@/lib/use-settled"
import { cn } from "@/lib/utils"
import { ErrorNotice } from "@/components/layout/error-notice"

type Option = { label: string; value: string | null }

const THIS_YEAR = new Date().getFullYear()
const YEARS: Option[] = [
  { label: "Any year", value: null },
  ...Array.from({ length: THIS_YEAR - 1969 + 1 }, (_, i) => String(THIS_YEAR + 1 - i)).map((y) => ({ label: y, value: y })),
]
// Android's choices (SearchViewModel), under AniList's values.
const SEASONS: Option[] = [
  { label: "Any season", value: null },
  { label: "Winter", value: "WINTER" },
  { label: "Spring", value: "SPRING" },
  { label: "Summer", value: "SUMMER" },
  { label: "Fall", value: "FALL" },
]
const FORMATS: Option[] = [
  { label: "Any format", value: null },
  { label: "TV", value: "TV" },
  { label: "Movie", value: "MOVIE" },
  { label: "TV short", value: "TV_SHORT" },
  { label: "OVA", value: "OVA" },
  { label: "ONA", value: "ONA" },
  { label: "Special", value: "SPECIAL" },
  { label: "Music", value: "MUSIC" },
]
const STATUSES: Option[] = [
  { label: "Any status", value: null },
  { label: "Ongoing", value: "RELEASING" },
  { label: "Completed", value: "FINISHED" },
  { label: "Upcoming", value: "NOT_YET_RELEASED" },
  { label: "On hiatus", value: "HIATUS" },
  { label: "Cancelled", value: "CANCELLED" },
]
const SORTS: Option[] = [
  { label: "Best match", value: null },
  { label: "Trending", value: "TRENDING_DESC" },
  { label: "Most popular", value: "POPULARITY_DESC" },
  { label: "Highest rated", value: "SCORE_DESC" },
  { label: "Most favorited", value: "FAVOURITES_DESC" },
  { label: "Newest", value: "START_DATE_DESC" },
  { label: "Oldest", value: "START_DATE" },
  { label: "Most episodes", value: "EPISODES_DESC" },
  { label: "A–Z", value: "TITLE_ROMAJI" },
]
const COUNTRIES = [
  { label: "Japan", value: "JP" },
  { label: "China", value: "CN" },
  { label: "South Korea", value: "KR" },
]
const SOURCES = [
  { label: "Original", value: "ORIGINAL" },
  { label: "Manga", value: "MANGA" },
  { label: "Light novel", value: "LIGHT_NOVEL" },
  { label: "Novel", value: "NOVEL" },
  { label: "Visual novel", value: "VISUAL_NOVEL" },
  { label: "Game", value: "VIDEO_GAME" },
  { label: "Other", value: "OTHER" },
]
const EPISODES = [
  { label: "1–12", from: 1, to: 12 },
  { label: "13–26", from: 13, to: 26 },
  { label: "27–51", from: 27, to: 51 },
  { label: "52+", from: 52, to: null },
]
const RATINGS = [60, 70, 80, 90]
/** Five decades back from this one, then everything older (Android's `decadeChoices`). */
const DECADES = (() => {
  const current = Math.floor(THIS_YEAR / 10) * 10
  const decades = Array.from({ length: 5 }, (_, i) => ({ label: `${current - i * 10}s`, from: current - i * 10, to: current - i * 10 + 9 }))
  return [...decades, { label: "Earlier", from: null as number | null, to: current - 41 }]
})()
const WATCHED = [
  { value: null, label: "Show everything", description: "Watched titles stay in the results" },
  { value: "completed", label: "Hide completed", description: "Titles you finished" },
  { value: "started", label: "Hide anything I've watched", description: "Also in progress, paused and dropped titles" },
] as const

/** Everything but the query and the page: what the panel and the pickers set. */
type Filters = Omit<SearchFilters, "query" | "page">

const EMPTY: Filters = { genres: [], excludedGenres: [], tags: [], excludedTags: [], dubbedOnly: false }

/** Android's badge count: each decision once, however many bounds it sets. */
function activeCount(f: Filters): number {
  return (
    (f.genres?.length ?? 0) +
    (f.excludedGenres?.length ?? 0) +
    (f.tags?.length ?? 0) +
    (f.excludedTags?.length ?? 0) +
    [f.year, f.season, f.format, f.status, f.country, f.source, f.minimumScore, f.hideWatched].filter((v) => v != null).length +
    [f.yearFrom ?? f.yearTo, f.episodesMin ?? f.episodesMax].filter((v) => v != null).length +
    (f.dubbedOnly ? 1 : 0)
  )
}

type Visit = { text: string; filters: Filters; exactFor: string | null }

/**
 * What each Browse page had searched, by its history entry: Back from a title returns to the
 * search and filters it left, not a fresh page. A new visit (the sidebar, a genre link) starts clean.
 */
const visits = new Map<string, Visit>()

export function SearchPage({ initialGenre, initialQuery }: { initialGenre?: string; initialQuery?: string }) {
  const entry = useLocation({ select: (l) => l.state.__TSR_key ?? l.state.key })
  const [saved] = useState(() => (entry ? visits.get(entry) : undefined))
  const [text, setText] = useState(saved?.text ?? initialQuery ?? "")
  const query = useSettled(text.trim())
  const [filters, setFilters] = useState<Filters>(() => saved?.filters ?? { ...EMPTY, genres: initialGenre ? [initialGenre] : [] })
  const [panel, setPanel] = useState(false)
  // The term "Search instead for" asked to be taken as typed; it lapses once the box changes.
  const [exactFor, setExactFor] = useState<string | null>(saved?.exactFor ?? null)
  useEffect(() => {
    if (entry) visits.set(entry, { text, filters, exactFor })
  }, [entry, text, filters, exactFor])
  const exact = exactFor !== null && exactFor === query
  const set = (patch: Partial<Filters>) => setFilters((f) => ({ ...f, ...patch }))

  const request: SearchFilters = { ...filters, query: query || null }
  const results = useInfiniteQuery({
    queryKey: [...keys.search(request), "infinite", exact],
    queryFn: ({ pageParam }) => api.search({ ...request, query: pageParam.query, page: pageParam.page }, pageParam.exact),
    initialPageParam: { page: 1, query: request.query ?? null, exact },
    // A corrected first page holds the correction's results, so the rest are the correction's too.
    getNextPageParam: (last, pages) => {
      if (!last.hasNext) return undefined
      const first = pages[0]?.correction
      return { page: pages.length + 1, query: first?.applied ? first.suggested : (request.query ?? null), exact: true }
    },
    staleTime: 5 * 60_000,
  })
  const correction = results.data?.pages[0]?.correction ?? null

  // Load the next page as the end of the grid scrolls into view.
  const sentinel = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const el = sentinel.current
    if (!el) return
    const observer = new IntersectionObserver((entries) => {
      if (entries[0]?.isIntersecting && results.hasNextPage && !results.isFetchingNextPage) void results.fetchNextPage()
    }, { rootMargin: "600px" })
    observer.observe(el)
    return () => observer.disconnect()
  }, [results])

  const items = results.data?.pages.flatMap((p) => p.results) ?? []
  const count = activeCount(filters)
  const pick = (key: keyof Filters) => (value: string | null) =>
    set(key === "year" ? { year: value ? Number(value) : null, yearFrom: null, yearTo: null } : { [key]: value })

  return (
    <div className="flex flex-col gap-6 px-8 pb-8">
      {/* Stays at the top while the results scroll under it, as Android's Browse header does. */}
      <div className="surface-bar sticky top-0 z-20 -mx-8 flex flex-col gap-4 border-b px-8 pt-8 pb-4">
        <h1 className="font-heading text-2xl font-bold">Browse</h1>
        <InputGroup className="max-w-xl">
          <InputGroupAddon>
            <SearchIcon />
          </InputGroupAddon>
          <InputGroupInput autoFocus placeholder="Search by title…" value={text} onChange={(e) => setText(e.target.value)} />
          {text && (
            <InputGroupAddon align="inline-end">
              <InputGroupButton size="icon-xs" aria-label="Clear" onClick={() => setText("")}>
                <XIcon />
              </InputGroupButton>
            </InputGroupAddon>
          )}
        </InputGroup>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant={count > 0 ? "default" : "outline"} size="sm" onClick={() => setPanel(true)}>
            <SlidersHorizontalIcon data-icon="inline-start" />
            Filters
            {count > 0 && (
              <Badge variant="secondary" className="ml-0.5 h-4.5 min-w-4.5 px-1">
                {count}
              </Badge>
            )}
          </Button>
          <FilterSelect options={SORTS} value={filters.sort ?? null} onChange={pick("sort")} />
          <FilterSelect options={YEARS} value={filters.year ? String(filters.year) : null} onChange={pick("year")} />
          <FilterSelect options={SEASONS} value={filters.season ?? null} onChange={pick("season")} />
          <FilterSelect options={FORMATS} value={filters.format ?? null} onChange={pick("format")} />
          <FilterSelect options={STATUSES} value={filters.status ?? null} onChange={pick("status")} />
          {(count > 0 || filters.sort) && (
            <Button variant="ghost" size="sm" onClick={() => setFilters(EMPTY)}>
              Clear filters
            </Button>
          )}
        </div>
        <ActiveChips filters={filters} set={set} />
      </div>

      {correction && (
        <SpellingNotice correction={correction} onSearchInstead={() => setExactFor(query)} onAccept={() => setText(correction.suggested)} />
      )}
      {results.isError && items.length === 0 ? (
        <ErrorNotice error={results.error} onRetry={() => void results.refetch()} />
      ) : results.isPending ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
          {Array.from({ length: 18 }, (_, i) => (
            <CardSkeleton key={i} />
          ))}
        </div>
      ) : items.length === 0 ? (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>Nothing found</EmptyTitle>
            <EmptyDescription>
              {count > 0 ? "Fewer filters may find something." : "AniList matches whole words: try the full word, or the romaji title."}
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-x-4 gap-y-6">
          {items.map((anime) => (
            <AnimeCard key={anime.id} anime={anime} className="w-full" />
          ))}
        </div>
      )}
      <div ref={sentinel} className="flex justify-center py-4">
        {results.isFetchingNextPage && <Spinner />}
      </div>

      <FiltersPanel open={panel} onOpenChange={setPanel} filters={filters} set={set} onClear={() => setFilters(EMPTY)} />
    </div>
  )
}

/**
 * The line a web search draws over corrected results (Android's `SpellingCorrectionNotice`):
 * "Showing results for naruto / Search instead for naurto" when the page holds the correction,
 * "Did you mean naruto?" when it holds the typed term's own few results.
 */
function SpellingNotice({ correction, onSearchInstead, onAccept }: { correction: SearchCorrection; onSearchInstead: () => void; onAccept: () => void }) {
  if (correction.applied) {
    return (
      <div className="flex flex-col gap-0.5">
        <p className="font-medium">
          Showing results for <span className="font-semibold italic">{correction.suggested}</span>
        </p>
        <p className="text-sm text-muted-foreground">
          Search instead for{" "}
          <Button variant="link" className="h-auto p-0 text-sm" onClick={onSearchInstead}>
            {correction.typed}
          </Button>
        </p>
      </div>
    )
  }
  return (
    <p className="text-sm text-muted-foreground">
      Did you mean{" "}
      <Button variant="link" className="h-auto p-0 text-sm font-semibold italic" onClick={onAccept}>
        {correction.suggested}
      </Button>
      ?
    </p>
  )
}

/** The genres and tags in play, each removable, under the pickers. */
function ActiveChips({ filters, set }: { filters: Filters; set: (patch: Partial<Filters>) => void }) {
  const chips = [
    ...(filters.genres ?? []).map((g) => ({ key: `g+${g}`, label: g, exclude: false, remove: () => set({ genres: filters.genres!.filter((x) => x !== g) }) })),
    ...(filters.excludedGenres ?? []).map((g) => ({
      key: `g-${g}`,
      label: g,
      exclude: true,
      remove: () => set({ excludedGenres: filters.excludedGenres!.filter((x) => x !== g) }),
    })),
    ...(filters.tags ?? []).map((t) => ({ key: `t+${t}`, label: t, exclude: false, remove: () => set({ tags: filters.tags!.filter((x) => x !== t) }) })),
    ...(filters.excludedTags ?? []).map((t) => ({
      key: `t-${t}`,
      label: t,
      exclude: true,
      remove: () => set({ excludedTags: filters.excludedTags!.filter((x) => x !== t) }),
    })),
  ]
  if (chips.length === 0) return null
  return (
    <div className="flex flex-wrap gap-1.5">
      {chips.map((c) => (
        <Badge key={c.key} variant={c.exclude ? "destructive" : "secondary"} className="gap-1 pr-1">
          {c.exclude ? "Not " : ""}
          {c.label}
          <button type="button" aria-label={`Remove ${c.label}`} onClick={c.remove} className="rounded-full p-0.5 hover:bg-foreground/10">
            <XIcon className="size-3" />
          </button>
        </Badge>
      ))}
    </div>
  )
}

/**
 * Android's filter sheet: genres and tags (each wanted, unwanted or neither), years by decade,
 * country, source, length, minimum score, dubbed only and hide watched.
 */
function FiltersPanel({
  open,
  onOpenChange,
  filters,
  set,
  onClear,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  filters: Filters
  set: (patch: Partial<Filters>) => void
  onClear: () => void
}) {
  const dubCount = useDubTitles().data?.size ?? 0
  const showAdult = useSettings().data?.showAdult ?? false
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="gap-0 sm:max-w-lg">
        <SheetHeader className="border-b">
          <SheetTitle>Filters</SheetTitle>
          <SheetDescription>Genres and tags: click once to want them, again to leave them out.</SheetDescription>
        </SheetHeader>
        <ScrollArea className="min-h-0 flex-1">
          <div className="flex flex-col gap-6 p-4">
            <Section title="Genres">
              <div className="flex flex-wrap gap-1.5">
                {GENRES.filter((g) => showAdult || g !== "Hentai").map((g) => (
                  <TriChip
                    key={g}
                    label={g}
                    state={filters.genres?.includes(g) ? "in" : filters.excludedGenres?.includes(g) ? "out" : "off"}
                    onChange={(next) => set(cycle(filters.genres ?? [], filters.excludedGenres ?? [], g, next, "genres", "excludedGenres"))}
                  />
                ))}
              </div>
            </Section>
            <TagsSection filters={filters} set={set} />
            <Section title="Years">
              <ChoiceChips
                options={DECADES.map((d) => ({ label: d.label, value: `${d.from}-${d.to}` }))}
                value={filters.yearFrom != null || filters.yearTo != null ? `${filters.yearFrom ?? null}-${filters.yearTo ?? null}` : null}
                onChange={(v) => {
                  const d = DECADES.find((x) => `${x.from}-${x.to}` === v)
                  set({ yearFrom: d?.from ?? null, yearTo: d?.to ?? null, year: d ? null : filters.year })
                }}
              />
            </Section>
            <Section title="Country">
              <ChoiceChips options={COUNTRIES} value={filters.country ?? null} onChange={(country) => set({ country })} />
            </Section>
            <Section title="Source">
              <ChoiceChips options={SOURCES} value={filters.source ?? null} onChange={(source) => set({ source })} />
            </Section>
            <Section title="Episodes">
              <ChoiceChips
                options={EPISODES.map((e) => ({ label: e.label, value: `${e.from}-${e.to}` }))}
                value={filters.episodesMin != null || filters.episodesMax != null ? `${filters.episodesMin ?? null}-${filters.episodesMax ?? null}` : null}
                onChange={(v) => {
                  const e = EPISODES.find((x) => `${x.from}-${x.to}` === v)
                  set({ episodesMin: e?.from ?? null, episodesMax: e?.to ?? null })
                }}
              />
            </Section>
            <Section title="Minimum score">
              <ChoiceChips
                options={RATINGS.map((r) => ({ label: `${r}%+`, value: String(r) }))}
                value={filters.minimumScore ? String(filters.minimumScore) : null}
                onChange={(v) => set({ minimumScore: v ? Number(v) : null })}
              />
            </Section>
            <Section title="Audio">
              <label className="flex items-center justify-between gap-4">
                <span className="flex flex-col">
                  <span className="text-sm font-medium">Dubbed only</span>
                  <span className="text-xs text-muted-foreground">
                    Titles an English dub has been found for{dubCount ? ` (${dubCount.toLocaleString()} known)` : ""}
                  </span>
                </span>
                <Switch checked={!!filters.dubbedOnly} onCheckedChange={(dubbedOnly) => set({ dubbedOnly })} />
              </label>
            </Section>
            <Section title="Watched">
              <div className="flex flex-col gap-1">
                {WATCHED.map((w) => (
                  <button
                    key={w.label}
                    type="button"
                    onClick={() => set({ hideWatched: w.value })}
                    className={cn(
                      "surface-control flex flex-col items-start rounded-lg px-3 py-2 text-left transition hover:bg-muted",
                      (filters.hideWatched ?? null) === w.value && "ring-2 ring-primary",
                    )}
                  >
                    <span className="text-sm font-medium">{w.label}</span>
                    <span className="text-xs text-muted-foreground">{w.description}</span>
                  </button>
                ))}
              </div>
            </Section>
          </div>
        </ScrollArea>
        <SheetFooter className="flex-row justify-between border-t">
          <Button variant="ghost" onClick={onClear}>
            Clear all
          </Button>
          <Button onClick={() => onOpenChange(false)}>Show results</Button>
        </SheetFooter>
      </SheetContent>
    </Sheet>
  )
}

/** AniList's tags by category, narrowed by a search box (there are hundreds). */
function TagsSection({ filters, set }: { filters: Filters; set: (patch: Partial<Filters>) => void }) {
  const tags = useQuery({ queryKey: ["anime-tags"], queryFn: api.animeTags, staleTime: 24 * 60 * 60_000 })
  const [find, setFind] = useState("")
  const groups = useMemo(() => {
    const needle = find.trim().toLowerCase()
    const byCategory = new Map<string, Tag[]>()
    for (const tag of tags.data ?? []) {
      if (needle && !tag.name.toLowerCase().includes(needle)) continue
      const category = tag.category ?? "Other"
      byCategory.set(category, [...(byCategory.get(category) ?? []), tag])
    }
    return [...byCategory.entries()].sort(([a], [b]) => a.localeCompare(b))
  }, [tags.data, find])
  return (
    <Section title="Tags">
      <InputGroup>
        <InputGroupAddon>
          <SearchIcon />
        </InputGroupAddon>
        <InputGroupInput placeholder="Find a tag…" value={find} onChange={(e) => setFind(e.target.value)} />
      </InputGroup>
      {tags.isPending ? (
        <Spinner />
      ) : (
        <div className="flex max-h-80 flex-col gap-3 overflow-y-auto pr-1">
          {groups.map(([category, list]) => (
            <div key={category} className="flex flex-col gap-1.5">
              <span className="text-xs font-medium text-muted-foreground">{category}</span>
              <div className="flex flex-wrap gap-1.5">
                {list.map((t) => (
                  <TriChip
                    key={t.name}
                    label={t.name}
                    title={t.description ?? undefined}
                    state={filters.tags?.includes(t.name) ? "in" : filters.excludedTags?.includes(t.name) ? "out" : "off"}
                    onChange={(next) => set(cycle(filters.tags ?? [], filters.excludedTags ?? [], t.name, next, "tags", "excludedTags"))}
                  />
                ))}
              </div>
            </div>
          ))}
          {groups.length === 0 && <span className="text-sm text-muted-foreground">No tag matches.</span>}
        </div>
      )}
    </Section>
  )
}

type ChipState = "off" | "in" | "out"

/** Moves `name` between a wanted and an unwanted list. */
function cycle(wanted: string[], unwanted: string[], name: string, next: ChipState, inKey: keyof Filters, outKey: keyof Filters): Partial<Filters> {
  const without = (list: string[]) => list.filter((x) => x !== name)
  return {
    [inKey]: next === "in" ? [...without(wanted), name] : without(wanted),
    [outKey]: next === "out" ? [...without(unwanted), name] : without(unwanted),
  }
}

/** A chip that goes off → wanted → unwanted → off. */
function TriChip({ label, title, state, onChange }: { label: string; title?: string; state: ChipState; onChange: (next: ChipState) => void }) {
  const next: ChipState = state === "off" ? "in" : state === "in" ? "out" : "off"
  return (
    <button
      type="button"
      title={title}
      aria-pressed={state !== "off"}
      onClick={() => onChange(next)}
      className={cn(
        "flex h-7 items-center gap-1 rounded-full px-2.5 text-xs font-medium transition [&_svg]:size-3",
        state === "off" && "surface-control hover:bg-muted",
        state === "in" && "bg-primary text-primary-foreground",
        state === "out" && "bg-destructive/15 text-destructive line-through",
      )}
    >
      {state === "in" && <PlusIcon />}
      {state === "out" && <MinusIcon />}
      {label}
    </button>
  )
}

/** One choice of a few, or none: clicking the chosen one clears it. */
function ChoiceChips({ options, value, onChange }: { options: { label: string; value: string }[]; value: string | null; onChange: (value: string | null) => void }) {
  return (
    <div className="flex flex-wrap gap-1.5">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          aria-pressed={value === o.value}
          onClick={() => onChange(value === o.value ? null : o.value)}
          className={cn(
            "h-7 rounded-full px-2.5 text-xs font-medium transition",
            value === o.value ? "bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="font-heading text-sm font-medium">{title}</h3>
      {children}
    </section>
  )
}

function FilterSelect({
  options,
  value,
  onChange,
}: {
  options: Option[]
  value: string | null
  onChange: (value: string | null) => void
}) {
  return (
    <Select items={options} value={value} onValueChange={(v) => onChange((v as string | null) ?? null)}>
      <SelectTrigger size="sm" className="min-w-32">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectGroup>
          {options.map((o) => (
            <SelectItem key={o.value ?? "any"} value={o.value}>
              {o.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  )
}
