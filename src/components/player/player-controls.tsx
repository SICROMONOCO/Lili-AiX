import { MaximizeIcon, MinimizeIcon, PictureInPicture2Icon, Volume2Icon, VolumeXIcon } from "lucide-react"
import { useLayoutEffect, useRef, useState } from "react"

import { CHIP } from "@/components/player/player-overlays"
import { SeekBar, type SeekMarker } from "@/components/player/seek-bar"
import type { StoryboardSource } from "@/components/player/seek-preview"
import { Slider } from "@/components/ui/slider"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { formatTime, player, type PlayerState } from "@/lib/player"
import { cn } from "@/lib/utils"

type Props = {
  state: PlayerState
  fullscreen: boolean
  onToggleFullscreen: () => void
  onMiniPlayer: () => void
  /** Pills ahead of Mini player and Fullscreen (sources, episodes, subtitles, settings…). */
  extra?: React.ReactNode
  /** Skip windows drawn on the seek bar. */
  markers?: SeekMarker[]
  /** The server's storyboard: preview pictures over the seek bar. */
  storyboard?: StoryboardSource | null
  /** How far the top of the seek bar sits above the bottom edge, so an action can float just clear of it. */
  onSeekAnchor?: (px: number) => void
}

/**
 * The bottom bar, after Android's: the timeline, the elapsed and remaining times in chips, and a
 * row of controls that say what they do (labelled pills rather than bare glyphs).
 */
export function PlayerControls({ state, fullscreen, onToggleFullscreen, onMiniPlayer, extra, markers = [], storyboard, onSeekAnchor }: Props) {
  // While dragging, show the drag position instead of mpv's.
  const [scrub, setScrub] = useState<number | null>(null)
  const position = scrub ?? state.timePos
  const duration = Math.max(state.duration, 0)

  const bar = useRef<HTMLDivElement>(null)
  const seek = useRef<HTMLDivElement>(null)
  useLayoutEffect(() => {
    const el = bar.current
    if (!el || !onSeekAnchor) return
    const measure = () => {
      const top = seek.current?.getBoundingClientRect().top ?? 0
      onSeekAnchor(Math.max(0, el.getBoundingClientRect().bottom - top))
    }
    const observer = new ResizeObserver(measure)
    observer.observe(el)
    measure()
    return () => observer.disconnect()
  }, [onSeekAnchor])

  return (
    <div ref={bar} className="flex flex-col gap-2 bg-linear-to-b from-transparent via-black/68 via-32% to-black/94 px-6 pt-12 pb-3">
      <div ref={seek}>
        <SeekBar
          position={position}
          duration={duration}
          bufferedAhead={state.cacheTime}
          markers={markers}
          buffering={state.buffering}
          storyboard={storyboard}
          onScrub={setScrub}
          onSeek={(seconds) => {
            setScrub(null)
            void player.control({ type: "seek", seconds, relative: false })
          }}
        />
      </div>

      <div className="flex items-center justify-between">
        <span className={cn(CHIP, "font-mono tabular-nums")}>{formatTime(position)}</span>
        <span className={cn(CHIP, "font-mono text-white/68 tabular-nums")}>-{formatTime(Math.max(0, duration - position))}</span>
      </div>

      <div className="flex items-center gap-2">
        <Volume state={state} />
        <div className="ml-auto flex items-center gap-1.5">
          {extra}
          <PlayerPill label="Mini player" shortcut="I" icon={<PictureInPicture2Icon />} onClick={onMiniPlayer} iconOnly />
          <PlayerPill
            label={fullscreen ? "Exit" : "Fullscreen"}
            shortcut="F"
            icon={fullscreen ? <MinimizeIcon /> : <MaximizeIcon />}
            onClick={onToggleFullscreen}
          />
        </div>
      </div>
    </div>
  )
}

function Volume({ state }: { state: PlayerState }) {
  return (
    <div className="flex items-center gap-1 rounded-full bg-black/42 py-1 pr-3.5 pl-1">
      <button
        type="button"
        aria-label={state.muted ? "Unmute (M)" : "Mute (M)"}
        onClick={() => void player.control({ type: "setMute", muted: !state.muted })}
        className="flex size-8 items-center justify-center rounded-full text-white outline-none hover:bg-white/12 focus-visible:ring-3 focus-visible:ring-white/40 [&_svg]:size-4.5"
      >
        {state.muted || state.volume === 0 ? <VolumeXIcon /> : <Volume2Icon />}
      </button>
      <div className="w-24">
        <Slider
          aria-label="Volume"
          min={0}
          max={100}
          value={[state.muted ? 0 : Math.min(state.volume, 100)]}
          onValueChange={(value) => {
            const volume = Array.isArray(value) ? value[0] : value
            void player.control({ type: "setVolume", volume })
            if (state.muted) void player.control({ type: "setMute", muted: false })
          }}
        />
      </div>
    </div>
  )
}

type PillProps = {
  label: string
  icon: React.ReactNode
  shortcut?: string
  /** The glyph alone, the label in the tooltip. */
  iconOnly?: boolean
  active?: boolean
} & Omit<React.ComponentProps<"button">, "children">

/** Android's PlayerActionPill: a glyph and what it does, on a dark rounded fill. */
export function PlayerPill({ label, icon, shortcut, iconOnly, active, className, ...props }: PillProps) {
  const pill = (
    <button
      type="button"
      aria-label={label}
      {...props}
      className={cn(
        "flex h-9 items-center gap-1.75 rounded-full bg-black/42 text-sm font-medium whitespace-nowrap text-white outline-none transition-colors hover:bg-white/15 focus-visible:ring-3 focus-visible:ring-white/40 disabled:opacity-35 data-popup-open:bg-white/15 [&_svg]:size-4.5 [&_svg]:shrink-0",
        iconOnly ? "w-9 justify-center" : "px-3.5",
        active && "bg-white/20 hover:bg-white/26",
        className,
      )}
    >
      {icon}
      {!iconOnly && label}
    </button>
  )
  if (!iconOnly && !shortcut) return pill
  return (
    <Tooltip>
      <TooltipTrigger render={pill} />
      <TooltipContent>{shortcut ? `${label} (${shortcut})` : label}</TooltipContent>
    </Tooltip>
  )
}

/** A round icon control, for the center cluster and the top corners. */
export function ControlButton({
  label,
  onClick,
  children,
  disabled,
  className,
}: {
  label: string
  onClick: () => unknown
  children: React.ReactNode
  disabled?: boolean
  className?: string
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            aria-label={label}
            disabled={disabled}
            onClick={() => void onClick()}
            className={cn(
              "flex size-12 items-center justify-center rounded-full outline-none disabled:pointer-events-none disabled:opacity-35 focus-visible:[&>span]:ring-3 focus-visible:[&>span]:ring-white/40",
              className,
            )}
          />
        }
      >
        <span className="flex size-10 items-center justify-center rounded-full bg-black/42 text-white transition-colors hover:bg-white/15 [&_svg]:size-5">
          {children}
        </span>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}
