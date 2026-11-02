import { ChevronLeftIcon, ChevronRightIcon, RotateCwIcon } from "lucide-react"
import { useEffect, useRef } from "react"

import { useDragScroll } from "@/lib/use-drag-scroll"

import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"

/** Lazy loading at a row's end: `load` asks for the next page when the end comes into view. */
export type RowMore = {
  /** The row still has a page to ask for (AniList said so on the last one). */
  hasMore: boolean
  loading: boolean
  failed: boolean
  load: () => void
}

type Props = {
  title: string
  /** A quieter line beside the title (a hint about the row). */
  subtitle?: string
  action?: React.ReactNode
  loading?: boolean
  /** Fetching more at the row's end; without it the row is finite and scrolls out. */
  more?: RowMore
  children: React.ReactNode
}

/** A titled, horizontally scrolling shelf with arrow buttons. */
export function AnimeRow({ title, subtitle, action, loading, more, children }: Props) {
  const scroller = useDragScroll<HTMLDivElement>()
  const scroll = (direction: 1 | -1) => {
    const el = scroller.current
    if (el) el.scrollBy({ left: direction * el.clientWidth * 0.85, behavior: "smooth" })
  }
  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center gap-2 px-8">
        <h2 className="font-heading text-lg font-semibold">{title}</h2>
        {subtitle && <span className="text-xs text-muted-foreground">{subtitle}</span>}
        <div className="ml-auto flex items-center gap-1">
          {action}
          <Button variant="ghost" size="icon-sm" aria-label="Scroll left" onClick={() => scroll(-1)}>
            <ChevronLeftIcon />
          </Button>
          <Button variant="ghost" size="icon-sm" aria-label="Scroll right" onClick={() => scroll(1)}>
            <ChevronRightIcon />
          </Button>
        </div>
      </div>
      <div ref={scroller} className="no-scrollbar flex gap-4 overflow-x-auto scroll-smooth px-8 pb-2">
        {loading ? Array.from({ length: 8 }, (_, i) => <CardSkeleton key={i} />) : children}
        {!loading && more && <RowEnd more={more} scroller={scroller} />}
      </div>
    </section>
  )
}

/** How far before the row's end the next page is asked for: about three cards. */
const PRELOAD_PX = 600

/** The row's lazy tail: a marker watched for coming into view, a skeleton while a page loads, and a
 * retry when one failed. The watcher is rebuilt whenever loading settles, so a short page that
 * still leaves the end on screen chains straight into the next. */
function RowEnd({ more, scroller }: { more: RowMore; scroller: React.RefObject<HTMLDivElement | null> }) {
  const end = useRef<HTMLDivElement>(null)
  const { hasMore, loading, failed, load } = more
  useEffect(() => {
    const el = end.current
    const root = scroller.current
    if (!el || !root || !hasMore || loading || failed) return
    const watch = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) load()
      },
      { root, rootMargin: `0px ${PRELOAD_PX}px 0px 0px` },
    )
    watch.observe(el)
    return () => watch.disconnect()
  }, [hasMore, loading, failed, load, scroller])
  if (loading) return <CardSkeleton />
  if (failed)
    return (
      <Button variant="ghost" size="sm" className="self-center" onClick={() => load()}>
        <RotateCwIcon data-icon="inline-start" />
        Try again
      </Button>
    )
  return hasMore ? <div ref={end} className="h-1 w-px shrink-0" aria-hidden /> : null
}

export function CardSkeleton() {
  return (
    <div className="flex w-40 shrink-0 flex-col gap-2">
      <Skeleton className="aspect-[2/3] w-full rounded-lg" />
      <Skeleton className="h-4 w-3/4" />
      <Skeleton className="h-3 w-1/2" />
    </div>
  )
}
