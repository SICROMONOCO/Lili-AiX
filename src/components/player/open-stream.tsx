import { FilmIcon, PlayIcon } from "lucide-react"
import { useState } from "react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { player } from "@/lib/player"

// Public test streams covering each delivery format the spike must play.
const SAMPLES = [
  { label: "HLS · Big Buck Bunny", url: "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8" },
  { label: "HLS · Apple bipbop (subtitle + audio tracks)", url: "https://devstreaming-cdn.apple.com/videos/streaming/examples/img_bipbop_adv_example_fmp4/master.m3u8" },
  { label: "DASH · Big Buck Bunny", url: "https://dash.akamaized.net/akamai/bbb_30fps/bbb_30fps.mpd" },
  { label: "MP4 · Big Buck Bunny 1080p", url: "https://test-videos.co.uk/vids/bigbuckbunny/mp4/h264/1080/Big_Buck_Bunny_1080_10s_5MB.mp4" },
]

export function OpenStream({ onError }: { onError: (message: string) => void }) {
  const [url, setUrl] = useState("")
  const [referer, setReferer] = useState("")

  const open = (target: string) => {
    if (!target.trim()) return
    const headers: Record<string, string> = {}
    if (referer.trim()) headers.Referer = referer.trim()
    player.load({ url: target.trim(), headers }).catch((e) => onError(String(e)))
  }

  return (
    <div className="surface flex w-full max-w-lg flex-col gap-5 rounded-xl p-6 shadow-2xl">
      <div className="flex items-center gap-3">
        <FilmIcon className="text-muted-foreground" />
        <div className="flex flex-col">
          <h1 className="font-heading text-lg font-medium">Open a video</h1>
          <p className="text-sm text-muted-foreground">Play a stream URL or a file: drop a video or subtitle file anywhere on the window.</p>
        </div>
      </div>

      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          open(url)
        }}
      >
        <Input placeholder="Stream URL (HLS, DASH, MP4…)" value={url} onChange={(e) => setUrl(e.target.value)} />
        <div className="flex gap-2">
          <Input placeholder="Referer header (optional)" value={referer} onChange={(e) => setReferer(e.target.value)} />
          <Button type="submit">
            <PlayIcon data-icon="inline-start" />
            Play
          </Button>
        </div>
      </form>

      <div className="flex flex-col gap-1">
        <p className="text-xs font-medium text-muted-foreground uppercase">Samples</p>
        {SAMPLES.map((s) => (
          <Button key={s.url} variant="ghost" className="justify-start" onClick={() => open(s.url)}>
            {s.label}
          </Button>
        ))}
      </div>
    </div>
  )
}
