// The player's timeline, drawn in layers as Android's PlayerSeekBar: the track, what is buffered
// ahead, the skip windows (recap, opening, ending) in the marker colour, and what has played in
// the accent. Hovering says where a click would land, and which window it falls in, under the
// frame there when the server has a storyboard (`seek-preview.tsx`).
//
// The layers live inside the slider's own track. Drawn as siblings under it, as before, the track
// painted over them and neither the buffer nor the skip windows could be seen.

import { Slider as SliderPrimitive } from "@base-ui/react/slider"
import { useState } from "react"

import { frameAt, PREVIEW_WIDTH, SeekPreview, type StoryboardSource, useStoryboard } from "@/components/player/seek-preview"
import { formatTime } from "@/lib/player"
import { cn } from "@/lib/utils"

export type SeekMarker = { kind: "intro" | "outro" | "recap"; start: number; end: number }

const LABEL: Record<SeekMarker["kind"], string> = { intro: "Opening", outro: "Ending", recap: "Recap" }

export function SeekBar({
  position,
  duration,
  /** Seconds buffered ahead of `position`. */
  bufferedAhead,
  markers,
  buffering,
  storyboard,
  onScrub,
  onSeek,
}: {
  position: number
  duration: number
  bufferedAhead: number
  markers: SeekMarker[]
  /** Playback is stalled waiting for data: the buffered band pulses. */
  buffering: boolean
  /** The server's storyboard, for preview pictures. */
  storyboard?: StoryboardSource | null
  onScrub: (seconds: number) => void
  onSeek: (seconds: number) => void
}) {
  const [hover, setHover] = useState<{ x: number; width: number; seconds: number } | null>(null)
  const frames = useStoryboard(storyboard)
  const pct = (seconds: number) => `${Math.min(100, Math.max(0, (seconds / duration) * 100))}%`
  const buffered = duration > 0 ? Math.min(duration, position + Math.max(0, bufferedAhead)) : 0
  const hovered = hover ? markers.find((m) => hover.seconds >= m.start && hover.seconds < m.end) : undefined
  const frame = hover && storyboard ? frameAt(frames, hover.seconds) : null
  // A picture is wider than the time alone: keep it inside the bar at either end.
  const half = PREVIEW_WIDTH / 2
  const left = hover && frame ? Math.min(Math.max(hover.x, half), Math.max(hover.width - half, half)) : hover?.x

  return (
    <div className="relative">
      {hover && duration > 0 && (
        <div className="pointer-events-none absolute bottom-full mb-2 flex -translate-x-1/2 flex-col items-center gap-1.5" style={{ left }}>
          {frame && storyboard && <SeekPreview frame={frame} headers={storyboard.headers} />}
          <div className="rounded-md bg-black/80 px-2 py-1 text-xs font-medium whitespace-nowrap text-white tabular-nums">
            {formatTime(hover.seconds)}
            {hovered && <span className="text-(--player-marker)"> · {LABEL[hovered.kind]}</span>}
          </div>
        </div>
      )}
      <SliderPrimitive.Root
        aria-label="Seek"
        min={0}
        max={duration || 1}
        step={0.1}
        value={[Math.min(position, duration || 1)]}
        disabled={duration <= 0}
        thumbAlignment="edge"
        onValueChange={(value) => onScrub(Array.isArray(value) ? value[0] : value)}
        onValueCommitted={(value) => onSeek(Array.isArray(value) ? value[0] : value)}
        className="group/seek w-full"
      >
        <SliderPrimitive.Control
          className="relative flex h-4 w-full cursor-pointer touch-none items-center select-none data-disabled:cursor-default"
          onPointerMove={(e) => {
            const box = e.currentTarget.getBoundingClientRect()
            const x = Math.min(box.width, Math.max(0, e.clientX - box.left))
            setHover({ x, width: box.width, seconds: duration > 0 ? (x / box.width) * duration : 0 })
          }}
          onPointerLeave={() => setHover(null)}
        >
          <SliderPrimitive.Track className="relative h-1 w-full overflow-hidden rounded-full bg-white/20 transition-[height] group-hover/seek:h-1.5">
            {duration > 0 && (
              <>
                <div
                  className={cn("absolute inset-y-0 left-0 bg-(--player-buffered)", buffering && "animate-pulse")}
                  style={{ width: pct(buffered) }}
                />
                <SliderPrimitive.Indicator className="bg-primary" />
                {markers.map((m) => (
                  <div
                    key={`${m.kind}-${m.start}`}
                    className="absolute inset-y-0 bg-(--player-marker)/85"
                    style={{ left: pct(m.start), width: `calc(${pct(m.end)} - ${pct(m.start)})` }}
                  />
                ))}
              </>
            )}
          </SliderPrimitive.Track>
          <SliderPrimitive.Thumb className="block size-3.5 rounded-full bg-primary shadow ring-primary/40 transition-[box-shadow,transform] outline-none group-hover/seek:scale-110 focus-visible:ring-3" />
        </SliderPrimitive.Control>
      </SliderPrimitive.Root>
    </div>
  )
}
