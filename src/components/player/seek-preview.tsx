// The seek bar's preview pictures, from the playing server's storyboard (Rust `storyboard`): a
// WebVTT whose cues name a tile of a sprite sheet. The cues are fetched once per stream; a sheet
// the first time the pointer lands on one of its tiles. Both go through the app, which sends the
// server's headers: the CDNs refuse the webview's own requests.

import { useQuery } from "@tanstack/react-query"
import { useEffect, useMemo } from "react"

import { api } from "@/lib/api"
import type { StoryboardFrame } from "@/lib/types"

export type StoryboardSource = { url: string; headers: Record<string, string> }

/** Shown this wide; the height follows the tile. */
export const PREVIEW_WIDTH = 192

/** The storyboard's cues; none while loading, when it failed, or when there is none. */
export function useStoryboard(source: StoryboardSource | null | undefined): StoryboardFrame[] {
  const frames = useQuery({
    queryKey: ["storyboard", source?.url],
    queryFn: () => api.storyboard(source!.url, source!.headers),
    enabled: !!source,
    staleTime: Infinity,
    retry: 1,
    meta: { background: true },
  })
  return frames.data ?? []
}

/** The cue covering `seconds` (else the last one before it), by bisection: cues come in order. */
export function frameAt(frames: StoryboardFrame[], seconds: number): StoryboardFrame | null {
  let lo = 0
  let hi = frames.length - 1
  let found: StoryboardFrame | null = null
  while (lo <= hi) {
    const mid = (lo + hi) >> 1
    if (frames[mid].start <= seconds) {
      found = frames[mid]
      lo = mid + 1
    } else {
      hi = mid - 1
    }
  }
  return found
}

/** One frame: its tile cut out of the sheet, scaled to the preview's width. Black until the sheet arrives. */
export function SeekPreview({ frame, headers }: { frame: StoryboardFrame; headers: Record<string, string> }) {
  // The sheet stays in the query cache as a Blob, with its size (a tile is cut out as a CSS
  // background, which needs it); its object URL lives as long as this picture.
  const sheet = useQuery({
    queryKey: ["storyboard-sheet", frame.image],
    queryFn: async () => {
      const blob = new Blob([await api.storyboardImage(frame.image, headers)])
      const bitmap = await createImageBitmap(blob)
      const size = { width: bitmap.width, height: bitmap.height }
      bitmap.close()
      return { blob, ...size }
    },
    staleTime: Infinity,
    gcTime: 10 * 60_000,
    retry: 1,
    meta: { background: true },
  })
  const src = useMemo(() => (sheet.data ? URL.createObjectURL(sheet.data.blob) : undefined), [sheet.data])
  useEffect(() => () => void (src && URL.revokeObjectURL(src)), [src])

  // Without a tile, the whole picture is the frame.
  const rect = frame.rect ?? (sheet.data ? { x: 0, y: 0, w: sheet.data.width, h: sheet.data.height } : null)
  const scale = rect ? PREVIEW_WIDTH / rect.w : 1
  return (
    <div
      className="overflow-hidden rounded-md bg-black bg-no-repeat shadow-lg ring-1 ring-white/15"
      style={{
        width: PREVIEW_WIDTH,
        height: rect ? Math.round(rect.h * scale) : (PREVIEW_WIDTH * 9) / 16,
        ...(src && rect && sheet.data
          ? {
              backgroundImage: `url(${src})`,
              backgroundSize: `${sheet.data.width * scale}px ${sheet.data.height * scale}px`,
              backgroundPosition: `${-rect.x * scale}px ${-rect.y * scale}px`,
            }
          : {}),
      }}
    />
  )
}
