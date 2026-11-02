import { Link, useNavigate } from "@tanstack/react-router"
import { GripVerticalIcon, InfoIcon, LayoutListIcon, PlayIcon } from "lucide-react"
import { useQueries } from "@tanstack/react-query"
import { useCallback, useEffect, useRef, useState } from "react"

import { AnimeCard } from "@/components/anime/anime-card"
import { AnimeRow } from "@/components/anime/anime-row"
import { ContinueCard } from "@/components/anime/continue-card"
import { ArrangeHome } from "@/components/settings/appearance"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { api, useAnilistList, useAppearance, useContinue, useHome, useHomeRowMore, useMalUser, useRecs, useSetAppearance, useViewer } from "@/lib/api"
import { type HomeRowKey, decodeHomeLayout, encodeHomeLayout, placeBeside } from "@/lib/theme"
import { formatLabel, seasonLabel } from "@/lib/format"
import type { AnimeCard as Card } from "@/lib/types"
import { cn } from "@/lib/utils"
import { ErrorNotice } from "@/components/layout/error-notice"

export function HomePage() {
  const home = useHome()
  const continueWatching = useContinue()
  const recs = useRecs()
  const { homeLayout } = useAppearance()
  const setAppearance = useSetAppearance()
  const viewer = useViewer().data
  const malUser = useMalUser().data
  const anilist = useAnilistList(!!viewer || !!malUser)
  const [arranging, setArranging] = useState(false)
  // Opening Home picks up what the phone or TV changed, as Android's Home does; the result arrives
  // as `library-changed`, which refreshes Continue watching and the account rows.
  useEffect(() => {
    void api.librarySync()
  }, [])
  const watching = (anilist.data ?? [])
    .filter((e) => e.status === "CURRENT" || e.status === "REPEATING")
    .sort((a, b) => b.updatedAt - a.updatedAt)

  if (home.isError && !home.data) {
    return (
      <div className="flex h-full items-center justify-center">
        <ErrorNotice error={home.error} onRetry={() => void home.refetch()} />
      </div>
    )
  }

  const data = home.data
  // The rows in the viewer's order (Arrange Home, or a drag on Home itself), hidden ones left out.
  const layout = decodeHomeLayout(homeLayout)
  const rows: Record<HomeRowKey, React.ReactNode> = {
    continue: continueWatching.data && continueWatching.data.length > 0 && (
      <AnimeRow title="Continue watching" subtitle="Right-click a title to move it on your list">
        {continueWatching.data.map((entry) => (
          <ContinueCard key={entry.anime.id} entry={entry} />
        ))}
      </AnimeRow>
    ),
    account: watching.length > 0 && (
      <AnimeRow title={viewer ? "Watching on AniList" : "Watching on MyAnimeList"}>
        {watching.map((e) => (
          <AnimeCard
            key={e.anime.id}
            anime={e.anime}
            caption={`${e.progress}${e.anime.episodes ? ` / ${e.anime.episodes}` : ""} watched`}
            progress={e.anime.episodes ? e.progress / e.anime.episodes : undefined}
          />
        ))}
      </AnimeRow>
    ),
    for_you: recs.data?.forYou && recs.data.forYou.length > 0 && (
      <AnimeRow title="For you">
        {recs.data.forYou.map((anime) => (
          <AnimeCard key={anime.id} anime={anime} />
        ))}
      </AnimeRow>
    ),
    because: recs.data?.because && (
      <AnimeRow title={`Because you watched ${recs.data.because.seed.title}`}>
        {recs.data.because.items.map((anime) => (
          <AnimeCard key={anime.id} anime={anime} />
        ))}
      </AnimeRow>
    ),
    newest: <Shelf title="New episodes" row="newest" items={data?.newest} loading={home.isPending} />,
    trending: <Shelf title="Trending now" row="trending" items={data?.trending} loading={home.isPending} />,
    popular: <Shelf title="This season" row="seasonal" items={data?.seasonal} loading={home.isPending} />,
    all_time: <Shelf title="All-time popular" row="popular" items={data?.popular} loading={home.isPending} />,
    top_rated: <Shelf title="Top rated" row="top_rated" items={data?.topRated} loading={home.isPending} />,
    movies: <Shelf title="Movies" row="movies" items={data?.movies} loading={home.isPending} />,
    upcoming: <Shelf title="Coming soon" row="upcoming" items={data?.upcoming} loading={home.isPending} />,
  }
  const featured = data?.trending.filter((a) => a.banner).slice(0, 6) ?? []
  const shown = layout.order.filter((key) => !layout.hidden.has(key) && rows[key])

  return (
    <div className="flex flex-col gap-10 pb-12">
      <Hero items={featured} loading={home.isPending} />

      <DraggableRows
        keys={shown}
        render={(key) => rows[key]}
        onMove={(row, anchor, after) => setAppearance({ homeLayout: encodeHomeLayout(placeBeside(layout, row, anchor, after)) })}
      />

      <div className="flex justify-center">
        <Button variant="outline" onClick={() => setArranging(true)}>
          <LayoutListIcon data-icon="inline-start" />
          Arrange Home
        </Button>
      </div>
      {arranging && <ArrangeHome stored={homeLayout} onClose={() => setArranging(false)} onSave={(homeLayout) => setAppearance({ homeLayout })} />}
    </div>
  )
}

/**
 * Home's rows, each held by the grip beside its title and dragged into place (Android's
 * HomeRowDrag): dropped over a row's upper half it goes above it, over the lower half below.
 * Saved once, on the drop; Esc cancels. Near the scroll area's top or bottom edge the page scrolls,
 * so a row can travel all of Home.
 *
 * Pointer events, not HTML drag and drop: on Windows the window's own file-drop handler (videos and
 * subtitles dropped on the app, see router.tsx) swallows the page's dragover and drop events.
 */
function DraggableRows({
  keys,
  render,
  onMove,
}: {
  keys: HomeRowKey[]
  render: (key: HomeRowKey) => React.ReactNode
  onMove: (row: HomeRowKey, anchor: HomeRowKey, after: boolean) => void
}) {
  type Target = { key: HomeRowKey; after: boolean }
  const [dragging, setDragging] = useState<HomeRowKey | null>(null)
  const [target, setTarget] = useState<Target | null>(null)
  const rowEls = useRef(new Map<HomeRowKey, HTMLDivElement>())
  const drag = useRef<{ key: HomeRowKey; y: number; target: Target | null; scroller: HTMLElement | null } | null>(null)
  const latestMove = useRef(onMove)
  useEffect(() => {
    latestMove.current = onMove
  })

  useEffect(() => {
    const d = drag.current
    if (!dragging || !d) return
    // The row under the pointer; in the gap between two rows the last target stands.
    const aim = () => {
      for (const [key, el] of rowEls.current) {
        const box = el.getBoundingClientRect()
        if (d.y < box.top || d.y > box.bottom) continue
        const after = d.y > box.top + box.height / 2
        if (d.target?.key !== key || d.target.after !== after) setTarget((d.target = { key, after }))
        return
      }
    }
    let frame = 0
    const scroll = () => {
      const s = d.scroller
      if (s) {
        const box = s.getBoundingClientRect()
        const edge = 72
        const over = d.y < box.top + edge ? d.y - (box.top + edge) : d.y > box.bottom - edge ? d.y - (box.bottom - edge) : 0
        if (over) {
          s.scrollTop += over / 3
          aim()
        }
      }
      frame = requestAnimationFrame(scroll)
    }
    frame = requestAnimationFrame(scroll)
    const finish = (commit: boolean) => {
      if (commit && d.target && d.target.key !== d.key) latestMove.current(d.key, d.target.key, d.target.after)
      drag.current = null
      setDragging(null)
      setTarget(null)
    }
    const move = (e: PointerEvent) => {
      d.y = e.clientY
      aim()
    }
    const up = () => finish(true)
    const cancel = () => finish(false)
    const key = (e: KeyboardEvent) => e.key === "Escape" && cancel()
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
    window.addEventListener("pointercancel", cancel)
    window.addEventListener("keydown", key)
    const { cursor, userSelect } = document.body.style
    document.body.style.cursor = "grabbing"
    document.body.style.userSelect = "none"
    return () => {
      cancelAnimationFrame(frame)
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
      window.removeEventListener("pointercancel", cancel)
      window.removeEventListener("keydown", key)
      document.body.style.cursor = cursor
      document.body.style.userSelect = userSelect
    }
  }, [dragging])

  return (
    <>
      {keys.map((key) => (
        <div
          key={key}
          ref={(el) => {
            if (el) rowEls.current.set(key, el)
            else rowEls.current.delete(key)
          }}
          className={cn("group/row relative transition-opacity", dragging === key && "opacity-40")}
        >
          {target?.key === key && dragging !== key && (
            <div className={cn("pointer-events-none absolute inset-x-8 z-10 h-0.5 rounded-full bg-primary", target.after ? "-bottom-5" : "-top-5")} />
          )}
          <button
            type="button"
            aria-label="Drag to move this row"
            title="Drag to move this row"
            onPointerDown={(e) => {
              if (e.button !== 0) return
              e.preventDefault()
              let scroller = e.currentTarget.parentElement
              while (scroller && !/(auto|scroll)/.test(getComputedStyle(scroller).overflowY)) scroller = scroller.parentElement
              drag.current = { key, y: e.clientY, target: null, scroller: scroller ?? (document.scrollingElement as HTMLElement | null) }
              setDragging(key)
            }}
            className={cn(
              "absolute top-0.5 left-2 z-10 flex h-7 w-5 cursor-grab touch-none items-center justify-center rounded text-muted-foreground opacity-0 transition-opacity group-hover/row:opacity-100 hover:bg-muted focus-visible:opacity-100 [&_svg]:size-4",
              dragging === key && "opacity-100",
            )}
          >
            <GripVerticalIcon />
          </button>
          {render(key)}
        </div>
      ))}
    </>
  )
}

/** One of Home's AniList shelves: its first page comes with `home`, and scrolling its end into
 * view asks for the next one (`row`, a `Home` field). A repeated title across pages (a "New
 * episodes" window moving with "now") is shown once. */
function Shelf({ title, row, items, loading }: { title: string; row: string; items?: Card[]; loading: boolean }) {
  // The pages sleep until the row's end is first reached; `load` wakes them, for these first
  // titles only: when a refetch brings new ones, the extras start over asleep.
  const firstKey = items?.map((anime) => anime.id).join(",") ?? ""
  const [startedFor, setStartedFor] = useState<string | null>(null)
  const started = startedFor === firstKey
  const more = useHomeRowMore(row, items, started)
  const { fetchNextPage } = more
  // Stable, so the row's end watcher is rebuilt when loading settles, not on every render.
  const load = useCallback(() => {
    if (started) void fetchNextPage()
    else setStartedFor(firstKey)
  }, [started, fetchNextPage, firstKey])
  if (!loading && (!items || items.length === 0)) return null
  const seen = new Set(items?.map((anime) => anime.id))
  const extra: Card[] = []
  for (const page of more.data?.pages ?? []) {
    for (const anime of page.cards) {
      if (!seen.has(anime.id)) {
        seen.add(anime.id)
        extra.push(anime)
      }
    }
  }
  return (
    <AnimeRow
      title={title}
      loading={loading}
      more={
        items && !loading
          ? {
              hasMore: more.data?.pages.at(-1)?.hasMore ?? true,
              loading: more.isFetching,
              failed: more.isError,
              load,
            }
          : undefined
      }
    >
      {items?.map((anime) => (
        <AnimeCard key={anime.id} anime={anime} />
      ))}
      {extra.map((anime) => (
        <AnimeCard key={anime.id} anime={anime} />
      ))}
    </AnimeRow>
  )
}

/** Rotating banner of trending titles. */
function Hero({ items, loading }: { items: Card[]; loading: boolean }) {
  const [index, setIndex] = useState(0)
  const navigate = useNavigate()
  const count = items.length
  // Restarts after every change, so a swipe isn't followed straight away by the timer's own turn.
  useEffect(() => {
    if (count < 2) return
    const timer = window.setInterval(() => setIndex((i) => (i + 1) % count), 8000)
    return () => window.clearInterval(timer)
  }, [count, index])
  const step = (by: number) => setIndex((i) => (i + by + count) % Math.max(count, 1))
  const { offset, bind } = useSwipe(step)
  const [slide, setSlide] = useState<{ width: number; height: number } | null>(null)
  // The series' logos (the media index's, as on the title page), fetched for every slide up front
  // so changing slide never swaps text for art mid-view. Settings → Appearance → Series logos.
  const showLogos = useAppearance().showSeriesLogos
  const logos = useQueries({
    queries: items.map((item) => ({
      queryKey: ["logo", item.id],
      queryFn: () => api.animeLogo(item.id),
      staleTime: 24 * 60 * 60_000,
      enabled: showLogos,
    })),
  })
  const observer = useRef<ResizeObserver | null>(null)
  // The slide's size decides where each picture goes (see `heroPictureGeometry`).
  const measure = (el: HTMLElement | null) => {
    observer.current?.disconnect()
    if (!el) return
    observer.current = new ResizeObserver(([entry]) => setSlide({ width: entry.contentRect.width, height: entry.contentRect.height }))
    observer.current.observe(el)
  }

  if (loading) return <Skeleton className="h-[26rem] w-full rounded-none" />
  const shown = index % Math.max(items.length, 1)
  const anime = items[shown]
  if (!anime) return null
  const logo = showLogos ? logos[shown]?.data : null

  return (
    <section
      ref={measure}
      className={cn("relative h-[26rem] w-full touch-pan-y overflow-hidden select-none", offset !== 0 && "cursor-grabbing")}
      {...bind}
    >
      {items.map((item, i) => item.banner && <HeroPicture key={item.id} url={item.banner} slide={slide} shown={i === index} />)}
      <div className="absolute inset-0 bg-linear-to-t from-background via-transparent to-transparent" />
      <div
        className="relative flex h-full max-w-2xl flex-col justify-end gap-4 p-8"
        style={{ transform: `translateX(${offset}px)`, transition: offset === 0 ? "transform 200ms ease-out" : "none" }}
      >
        <div className="flex flex-wrap gap-2">
          {[formatLabel(anime.format), seasonLabel(anime), anime.score ? `${anime.score}%` : null]
            .filter(Boolean)
            .map((label) => (
              <Badge key={label} variant="secondary">
                {label}
              </Badge>
            ))}
        </div>
        {logo ? (
          <h1>
            <img key={logo} src={logo} alt={anime.title} className="max-h-32 max-w-md object-contain object-left drop-shadow-lg" />
          </h1>
        ) : (
          <h1 className="font-heading text-4xl leading-tight font-bold text-balance">{anime.title}</h1>
        )}
        <p className="line-clamp-3 text-sm text-muted-foreground">{anime.genres.join(" · ")}</p>
        <div className="flex gap-2">
          <Button
            size="lg"
            onClick={() => void navigate({ to: "/watch/$id/$episode", params: { id: String(anime.id), episode: "1" } })}
          >
            <PlayIcon data-icon="inline-start" />
            Watch now
          </Button>
          <Button size="lg" variant="secondary" nativeButton={false} render={<Link to="/anime/$id" params={{ id: String(anime.id) }} />}>
            <InfoIcon data-icon="inline-start" />
            Details
          </Button>
        </div>
        <div className="flex gap-1.5">
          {items.map((item, i) => (
            <button
              key={item.id}
              type="button"
              aria-label={`Show ${item.title}`}
              onClick={() => setIndex(i)}
              className={cn("h-1 rounded-full transition-all", i === index ? "w-8 bg-primary" : "w-4 bg-muted-foreground/40")}
            />
          ))}
        </div>
      </div>
    </section>
  )
}

/** A slide changes past this many pixels; less snaps back. */
const SWIPE = 60

/** Drag the hero left or right (mouse, touch or pen) to change slide. Small movements stay clicks,
 * and a drag doesn't click the button it started on. */
function useSwipe(step: (by: number) => void) {
  const [offset, setOffset] = useState(0)
  const drag = useRef<{ x: number; moved: boolean } | null>(null)
  const swallowClick = useRef(false)
  useEffect(() => {
    const move = (e: PointerEvent) => {
      const d = drag.current
      if (!d) return
      const dx = e.clientX - d.x
      if (!d.moved && Math.abs(dx) > 6) d.moved = true
      if (d.moved) setOffset(dx * 0.4)
    }
    const up = (e: PointerEvent) => {
      const d = drag.current
      drag.current = null
      if (!d?.moved) return
      swallowClick.current = true
      const dx = e.clientX - d.x
      if (Math.abs(dx) > SWIPE) step(dx < 0 ? 1 : -1)
      setOffset(0)
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
    window.addEventListener("pointercancel", up)
    return () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
      window.removeEventListener("pointercancel", up)
    }
  }, [step])
  const bind = {
    onPointerDown: (e: React.PointerEvent) => {
      if (e.button !== 0) return
      drag.current = { x: e.clientX, moved: false }
      swallowClick.current = false
    },
    onClickCapture: (e: React.MouseEvent) => {
      if (swallowClick.current) {
        e.preventDefault()
        e.stopPropagation()
        swallowClick.current = false
      }
    },
    onDragStart: (e: React.DragEvent) => e.preventDefault(),
  }
  return { offset, bind }
}

/** Where the clear part of a slide begins, as a fraction of its width: the title and buttons take
 * everything before it, so a picture's subject belongs after it. */
const CLEAR_AREA_START = 0.35
/** The narrowest a picture is drawn, as a fraction of the slide: below this it is cropped instead. */
const MIN_PICTURE_WIDTH = 0.5
/** How far, as a fraction of the slide's width, a picture's left edge takes to fade in. */
const PICTURE_FADE_WIDTH = 0.1

/**
 * Android's `heroPictureGeometry` (TvHeroArtwork.kt), the fix for heroes that showed a soft,
 * over-zoomed slice of their art: the picture is sized to the slide's height, so it is cropped as
 * little as the slide allows, rather than stretched to cover it. A banner (AniList's are 1900x400)
 * is still wider than the slide at that height, so it covers it, centred on the clear area and never
 * short of the right edge; a 16:9 backdrop comes out about half the slide wide, shown whole against
 * the right. A picture squarer than MIN_PICTURE_WIDTH allows is drawn that wide and loses its bottom.
 */
function heroPictureGeometry(src: { width: number; height: number }, slide: { width: number; height: number }) {
  const scale = Math.max(slide.height / src.height, (slide.width * MIN_PICTURE_WIDTH) / src.width)
  const width = src.width * scale
  const clearStart = slide.width * CLEAR_AREA_START
  const centred = (slide.width * (1 + CLEAR_AREA_START)) / 2 - width / 2
  const left = Math.min(Math.max(centred, slide.width - width), Math.max(slide.width - width, clearStart))
  const fadeStart = Math.max(left, clearStart)
  return { width, height: src.height * scale, left, fadeStart, fadeEnd: fadeStart + slide.width * PICTURE_FADE_WIDTH }
}

/** One slide's picture, placed by heroPictureGeometry, its left edge faded out of its own pixels
 * (a mask, not a black gradient painted over it), so the text sits on the plain canvas, or on the
 * viewer's own background picture, without a seam. */
function HeroPicture({ url, slide, shown }: { url: string; slide: { width: number; height: number } | null; shown: boolean }) {
  const [natural, setNatural] = useState<{ width: number; height: number } | null>(null)
  const g = natural && slide ? heroPictureGeometry(natural, slide) : null
  return (
    <img
      src={url}
      alt=""
      draggable={false}
      onLoad={(e) => setNatural({ width: e.currentTarget.naturalWidth, height: e.currentTarget.naturalHeight })}
      className="absolute top-0 max-w-none transition-opacity duration-1000"
      style={
        g
          ? {
              left: g.left,
              width: g.width,
              height: g.height,
              opacity: shown ? 1 : 0,
              maskImage: `linear-gradient(to right, transparent ${g.fadeStart - g.left}px, black ${g.fadeEnd - g.left}px)`,
            }
          : // Until it has a size, laid out invisibly so it loads.
            { width: "100%", opacity: 0 }
      }
    />
  )
}
