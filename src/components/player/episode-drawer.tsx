import {
  CheckIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  CircleCheckIcon,
  LayoutGridIcon,
  ListIcon,
  LockIcon,
  XIcon,
} from "lucide-react"
import { useLayoutEffect, useMemo, useRef, useState } from "react"

import {
  airsLabel,
  blockIndexContaining,
  displayNumber,
  distinctTitle,
  episodeArtFallback,
  episodeBlocks,
  episodeLock,
  showsEpisodeGrid,
  watchFraction,
  type EpisodeLayout,
} from "@/lib/episodes"
import { useLockClock } from "@/lib/use-lock-clock"
import type { Episode, Progress } from "@/lib/types"
import { cn } from "@/lib/utils"

/** List or grid, once chosen with the header's toggle: kept for every series until switched back. */
const LAYOUT_KEY = "episodes.drawerLayout"
const GRID_COLUMNS = 5

// Android's panel palette (`PlayerPanelTheme.kt`): over a picture, selection is told by brightness,
// not by a second accent.
const ROW = "bg-[#17171a] border-transparent hover:bg-[#222227]"
const CURRENT = "bg-[#2b2b31] border-white/55"

function storedLayout(): EpisodeLayout | null {
  const value = localStorage.getItem(LAYOUT_KEY)
  return value === "list" || value === "grid" ? value : null
}

/**
 * The player's episode list, as Android's `InPlayerEpisodeDrawer`: the "Now playing" header with a
 * list/grid toggle, long series split into seasons of 100 (the one playing expanded), artwork rows
 * with the playing episode's equalizer, or a five-column grid of numbers once a season passes 25
 * episodes. Opens on the episode playing; choosing one closes it.
 */
export function EpisodeDrawer({
  episodes,
  current,
  progress,
  banner,
  blurEpisodeImages,
  onSelect,
  onClose,
}: {
  episodes: Episode[]
  /** The episode playing. */
  current: number
  progress: Map<number, Progress>
  /** The title's banner, for episodes without art of their own. */
  banner: string | null | undefined
  /** Settings → Appearance: blur the art of episodes not yet watched. */
  blurEpisodeImages: boolean
  onSelect: (episode: number) => void
  onClose: () => void
}) {
  const [layout, setLayout] = useState(storedLayout)
  const blocks = useMemo(() => episodeBlocks(episodes), [episodes])
  const art = useMemo(
    () => episodeArtFallback(episodes, banner),
    [episodes, banner]
  )
  // Null until a season header is clicked; -1 when every season is collapsed.
  const [chosenBlock, setChosenBlock] = useState<number | null>(null)
  const activeBlock = chosenBlock ?? blockIndexContaining(blocks, current)

  const shownCount = blocks[activeBlock]?.episodes.length ?? episodes.length
  const showingGrid = showsEpisodeGrid(shownCount, layout)

  // Open on the episode playing, at the top of the list. Once per opening and per episode, not per
  // season toggle: expanding another season must not yank the list back.
  const listRef = useRef<HTMLDivElement>(null)
  const currentRef = useRef<HTMLButtonElement>(null)
  useLayoutEffect(() => {
    const list = listRef.current
    const item = currentRef.current
    // The list is positioned, so it is the offset parent of every entry, season or not.
    if (list && item) list.scrollTop = item.offsetTop
  }, [current, blocks])

  const now = useLockClock(episodes)
  const select = (n: number) => {
    onSelect(n)
    onClose()
  }
  const entries = (list: Episode[]) => (
    <EpisodeEntries
      episodes={list}
      current={current}
      progress={progress}
      art={art}
      blurEpisodeImages={blurEpisodeImages}
      layout={layout}
      now={now}
      currentRef={currentRef}
      onSelect={select}
    />
  )

  return (
    <div className="flex h-full flex-col p-4">
      <div className="flex items-center gap-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="text-[11px] font-extrabold tracking-[0.16em] text-white/64">
            NOW PLAYING
          </span>
          <h2 className="mt-0.75 font-heading text-base font-extrabold text-white">
            Episodes
          </h2>
        </div>
        <IconButton
          label={
            showingGrid ? "Show episodes as a list" : "Show episodes as a grid"
          }
          onClick={() => {
            const next = showingGrid ? "list" : "grid"
            localStorage.setItem(LAYOUT_KEY, next)
            setLayout(next)
          }}
        >
          {showingGrid ? <ListIcon /> : <LayoutGridIcon />}
        </IconButton>
        <IconButton label="Close episodes (Esc)" onClick={onClose}>
          <XIcon />
        </IconButton>
      </div>

      <div
        ref={listRef}
        className="relative mt-3 flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto"
      >
        {blocks.length <= 1
          ? entries(episodes)
          : blocks.map((block, index) => {
              const expanded = index === activeBlock
              return (
                <div key={block.label} className="flex flex-col gap-2">
                  <button
                    type="button"
                    aria-expanded={expanded}
                    onClick={() => setChosenBlock(expanded ? -1 : index)}
                    className={cn(
                      "flex items-center justify-between gap-2 rounded-[10px] px-3.5 py-2.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-white/60 focus-visible:ring-inset",
                      expanded
                        ? "bg-white/10 hover:bg-white/14"
                        : "bg-white/7 hover:bg-white/11"
                    )}
                  >
                    <span className="flex flex-col">
                      <span className="font-bold text-white">
                        Season {index + 1} ({block.label})
                      </span>
                      <span className="text-xs text-white/65">
                        {block.episodes.length} episodes
                      </span>
                    </span>
                    {expanded ? (
                      <ChevronUpIcon className="size-5 text-white/85" />
                    ) : (
                      <ChevronDownIcon className="size-5 text-white/85" />
                    )}
                  </button>
                  {expanded && entries(block.episodes)}
                </div>
              )
            })}
      </div>
    </div>
  )
}

/** One season's episodes: artwork rows, or a grid of numbers once it gets long. */
function EpisodeEntries({
  episodes,
  current,
  progress,
  art,
  blurEpisodeImages,
  layout,
  now,
  currentRef,
  onSelect,
}: {
  episodes: Episode[]
  current: number
  progress: Map<number, Progress>
  art: string | null
  blurEpisodeImages: boolean
  layout: EpisodeLayout | null
  /** The time locks are judged by (`useLockClock`): the next episode waits, locked, until it is out. */
  now: number
  currentRef: React.RefObject<HTMLButtonElement | null>
  onSelect: (episode: number) => void
}) {
  if (showsEpisodeGrid(episodes.length, layout)) {
    return (
      <div
        className="grid gap-2"
        style={{
          gridTemplateColumns: `repeat(${GRID_COLUMNS}, minmax(0, 1fr))`,
        }}
      >
        {episodes.map((episode) => {
          const isCurrent = episode.number === current
          const fraction = watchFraction(progress.get(episode.number))
          const number = displayNumber(episode.number)
          const title = distinctTitle(episode)
          const locked = episode.airsAt != null && episodeLock(episode, now) ? airsLabel(episode.airsAt) : null
          return (
            <button
              key={episode.number}
              ref={isCurrent ? currentRef : undefined}
              type="button"
              title={locked ? `Episode ${number} · ${locked}` : title ? `${number}. ${title}` : `Episode ${number}`}
              aria-label={`Episode ${number}${locked ? `, ${locked}` : ""}${episode.filler ? ", filler" : ""}${stateLabel(fraction)}`}
              aria-current={isCurrent || undefined}
              disabled={!!locked}
              onClick={() => onSelect(episode.number)}
              className={cn(
                "relative flex aspect-[1.35] items-center justify-center gap-1 overflow-hidden rounded-[10px] border-[1.5px] text-base font-bold text-white transition outline-none focus-visible:ring-2 focus-visible:ring-white/60 focus-visible:ring-inset disabled:pointer-events-none disabled:text-white/45",
                isCurrent ? CURRENT : ROW
              )}
            >
              {locked && <LockIcon className="size-3.5" />}
              {number}
              {episode.filler && (
                <FillerBadge className="absolute top-1.25 right-1.25" />
              )}
              {fraction >= 1 && (
                <CircleCheckIcon className="absolute top-1.25 left-1.25 size-4 text-primary" />
              )}
              <WatchBar fraction={fraction} />
            </button>
          )
        })}
      </div>
    )
  }
  return episodes.map((episode) => {
    const isCurrent = episode.number === current
    const fraction = watchFraction(progress.get(episode.number))
    const number = displayNumber(episode.number)
    const picture = episode.thumbnail ?? art
    const locked = episode.airsAt != null && episodeLock(episode, now) ? airsLabel(episode.airsAt) : null
    return (
      <button
        key={episode.number}
        ref={isCurrent ? currentRef : undefined}
        type="button"
        aria-current={isCurrent || undefined}
        disabled={!!locked}
        onClick={() => onSelect(episode.number)}
        className={cn(
          "flex shrink-0 items-center gap-2.5 rounded-[10px] border-[1.5px] p-2 text-left transition outline-none focus-visible:ring-2 focus-visible:ring-white/60 focus-visible:ring-inset disabled:pointer-events-none disabled:opacity-60",
          isCurrent ? CURRENT : ROW
        )}
      >
        <span
          className={cn(
            "w-7 shrink-0 text-xs font-bold",
            isCurrent ? "text-white" : "text-white/70"
          )}
        >
          {number}
        </span>
        <span className="relative aspect-video w-20 shrink-0 overflow-hidden rounded-md bg-black/40">
          {picture && (
            <img
              src={picture}
              alt=""
              loading="lazy"
              className={cn(
                "size-full object-cover",
                !episode.thumbnail && "opacity-50",
                // An episode's art can give its story away until it's watched.
                blurEpisodeImages &&
                  episode.thumbnail &&
                  fraction < 1 &&
                  "scale-110 blur-md"
              )}
            />
          )}
          {isCurrent && (
            <span className="absolute inset-0 flex items-center justify-center bg-black/55">
              <Equalizer bars={4} barWidth={3.5} height={18} />
            </span>
          )}
          {locked && (
            <span className="absolute inset-0 flex items-center justify-center bg-black/65">
              <LockIcon className="size-4 text-white/85" />
            </span>
          )}
          <WatchBar fraction={fraction} />
        </span>
        <span className="flex min-w-0 flex-1 flex-col items-start gap-0.5">
          <span className="flex max-w-full items-center gap-1.5">
            <span
              className={cn(
                "truncate text-sm text-white",
                isCurrent ? "font-bold" : "font-medium"
              )}
            >
              {distinctTitle(episode) ?? `Episode ${number}`}
            </span>
            {isCurrent && <Equalizer bars={3} barWidth={2.5} height={12} />}
          </span>
          {episode.filler && <FillerBadge />}
          {locked ? (
            <span className="text-[11px] font-medium text-white/65">{locked}</span>
          ) : fraction >= 1 ? (
            <span className="flex items-center gap-1 text-[11px] font-medium text-primary">
              <CheckIcon className="size-3" />
              Watched
            </span>
          ) : fraction > 0.01 ? (
            <span className="text-[11px] font-medium text-white/65">
              {Math.floor(fraction * 100)}% watched
            </span>
          ) : null}
        </span>
      </button>
    )
  })
}

function stateLabel(fraction: number): string {
  if (fraction >= 1) return ", watched"
  if (fraction > 0.01) return `, ${Math.floor(fraction * 100)} percent watched`
  return ""
}

/** Android's thin progress line: nothing until there's progress, never thinner than a sliver. */
function WatchBar({ fraction }: { fraction: number }) {
  if (fraction <= 0.01) return null
  return (
    <span className="absolute inset-x-0 bottom-0 h-0.75 overflow-hidden rounded-[1.5px] bg-white/25">
      <span
        className="block h-full bg-primary"
        style={{ width: `${Math.max(4, fraction * 100)}%` }}
      />
    </span>
  )
}

/** A full word: a single letter or a dot was easy to miss. */
function FillerBadge({ className }: { className?: string }) {
  return (
    <span
      className={cn(
        "rounded bg-white/95 px-1 py-0.5 text-[8px] leading-[9px] font-black text-black",
        className
      )}
    >
      FILLER
    </span>
  )
}

// Android's table, not arithmetic: staggered so no two bars peak together.
const BARS = [
  { ms: 380, peak: 0.95 },
  { ms: 510, peak: 0.55 },
  { ms: 440, peak: 0.8 },
  { ms: 610, peak: 0.45 },
]

/** The bouncing bars on the episode playing. */
function Equalizer({
  bars,
  barWidth,
  height,
}: {
  bars: number
  barWidth: number
  height: number
}) {
  return (
    <span
      aria-hidden
      className="flex shrink-0 items-end gap-0.5"
      style={{ height }}
    >
      {BARS.slice(0, bars).map((bar, index) => (
        <span
          key={index}
          className="h-full equalizer-bar rounded-[1.5px] bg-primary"
          style={
            {
              width: barWidth,
              "--bar-ms": `${bar.ms}ms`,
              "--bar-peak": bar.peak,
            } as React.CSSProperties
          }
        />
      ))}
    </span>
  )
}

function IconButton({
  label,
  onClick,
  children,
}: {
  label: string
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="flex size-10 items-center justify-center rounded-full text-white/85 outline-none hover:bg-white/10 focus-visible:ring-2 focus-visible:ring-white/60 [&_svg]:size-5"
    >
      {children}
    </button>
  )
}
