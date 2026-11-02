// Typed bridge to the Rust player (src-tauri/src/player.rs).
//
// Hand-written until tauri-specta supports Tauri 3; keep these in step with
// `LoadRequest`, `Control` and `anilili_mpv::Event`.

import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { useEffect, useReducer } from "react"

export type LoadRequest = {
  url: string
  headers?: Record<string, string>
  start?: number
  subtitles?: { url: string; title?: string; lang?: string }[]
  title?: string
  subtitleLanguages?: string[]
}

export type Control =
  | { type: "togglePause" }
  | { type: "setPause"; paused: boolean }
  | { type: "seek"; seconds: number; relative: boolean }
  | { type: "setVolume"; volume: number }
  | { type: "setMute"; muted: boolean }
  | { type: "setSpeed"; speed: number }
  | { type: "setSubDelay"; seconds: number }
  | { type: "selectTrack"; kind: "sub" | "audio" | "video"; id: string }
  | { type: "addSubtitle"; url: string }
  | { type: "stop" }
  | { type: "setMargins"; left: number; right: number; top: number; bottom: number }
  | { type: "setSubtitleMargin"; margin: number }
  | { type: "screenshot" }

type PropertyValue = null | boolean | number | string

export type PlayerEvent =
  | { type: "startFile" | "fileLoaded" | "playbackRestart" | "seek" | "videoReconfig" | "audioReconfig" | "shutdown" }
  | {
      type: "endFile"
      reason: "eof" | "stop" | "quit" | "error" | "redirect" | "unknown"
      /** On failure, the catalogue message with its code: "Playback stopped: … (DPLAY-02/h403)". */
      error: string | null
      /** mpv's error number on failure. */
      code: number | null
    }
  | { type: "propertyChange"; name: string; value: PropertyValue }

export type Track = {
  id: number
  type: "video" | "audio" | "sub"
  title?: string
  lang?: string
  codec?: string
  selected: boolean
  external?: boolean
  /** The file an external track plays from; picking it again fetches it anew. */
  url?: string
  default?: boolean
}

export type PlayerState = {
  /** Something is loaded (or loading); false at idle. */
  active: boolean
  loading: boolean
  title: string
  timePos: number
  duration: number
  paused: boolean
  buffering: boolean
  bufferPercent: number
  /** Seconds of media buffered ahead, as reported by the demuxer. */
  cacheTime: number
  ended: boolean
  error: string | null
  volume: number
  muted: boolean
  speed: number
  subDelay: number
  tracks: Track[]
  hwdec: string
  videoWidth: number
  videoHeight: number
}

const initialState: PlayerState = {
  active: false,
  loading: false,
  title: "",
  timePos: 0,
  duration: 0,
  paused: false,
  buffering: false,
  bufferPercent: 100,
  cacheTime: 0,
  ended: false,
  error: null,
  volume: 100,
  muted: false,
  speed: 1,
  subDelay: 0,
  tracks: [],
  hwdec: "",
  videoWidth: 0,
  videoHeight: 0,
}

const num = (v: PropertyValue, fallback = 0) => (typeof v === "number" ? v : fallback)
const flag = (v: PropertyValue) => v === true
const str = (v: PropertyValue) => (typeof v === "string" ? v : "")

function parseTracks(json: string): Track[] {
  try {
    const raw = JSON.parse(json) as Array<Record<string, unknown>>
    return raw.map((t) => ({
      id: Number(t.id),
      type: t.type as Track["type"],
      title: t.title as string | undefined,
      lang: t.lang as string | undefined,
      codec: t.codec as string | undefined,
      selected: t.selected === true,
      external: t.external === true,
      url: typeof t["external-filename"] === "string" ? (t["external-filename"] as string) : undefined,
      default: t.default === true,
    }))
  } catch {
    return []
  }
}

function reduce(state: PlayerState, event: PlayerEvent): PlayerState {
  switch (event.type) {
    case "startFile":
      return { ...state, active: true, loading: true, ended: false, error: null, timePos: 0, duration: 0 }
    case "fileLoaded":
      return { ...state, loading: false }
    case "playbackRestart":
      // mpv's proof that frames are flowing: the file is loaded and the cache is not
      // empty. It also heals a lost `fileLoaded`/`paused-for-cache`, whose absence
      // would otherwise spin the play button over an episode that plays fine.
      return { ...state, loading: false, buffering: false }
    case "endFile":
      if (event.reason === "error") return { ...state, loading: false, error: event.error ?? "Playback failed" }
      if (event.reason === "stop") return { ...initialState, volume: state.volume, muted: state.muted, speed: state.speed }
      return state
    case "propertyChange": {
      const { name, value } = event
      switch (name) {
        case "time-pos": return { ...state, timePos: num(value, state.timePos) }
        case "duration": return { ...state, duration: num(value) }
        case "pause": return { ...state, paused: flag(value) }
        case "paused-for-cache": return { ...state, buffering: flag(value) }
        case "cache-buffering-state": return { ...state, bufferPercent: num(value, 100) }
        case "demuxer-cache-time": return { ...state, cacheTime: num(value) }
        case "eof-reached": return { ...state, ended: flag(value) }
        case "volume": return { ...state, volume: num(value, state.volume) }
        case "mute": return { ...state, muted: flag(value) }
        case "speed": return { ...state, speed: num(value, 1) }
        case "sub-delay": return { ...state, subDelay: num(value) }
        case "media-title": return { ...state, title: str(value) }
        case "track-list": return { ...state, tracks: parseTracks(str(value) || "[]") }
        case "hwdec-current": return { ...state, hwdec: str(value) }
        case "video-params/w": return { ...state, videoWidth: num(value) }
        case "video-params/h": return { ...state, videoHeight: num(value) }
        default: return state
      }
    }
    default:
      return state
  }
}

export function usePlayer(): PlayerState {
  const [state, dispatch] = useReducer(reduce, initialState)
  useEffect(() => {
    // Subscribe first, then catch up: anything that happened before mount
    // (e.g. a stream started from Rust) arrives as a replay of the same events.
    // Events heard while the snapshot is on its way back are held and applied
    // after it: the snapshot is older than they are, and letting it replay over
    // them used to strand `loading`/`buffering` on — the spinner spun over an
    // episode that was playing fine, until the app was restarted.
    let caught = false
    const held: PlayerEvent[] = []
    const unlisten = listen<PlayerEvent>("player", (e) => {
      if (caught) dispatch(e.payload)
      else held.push(e.payload)
    })
    const settle = () => {
      if (caught) return
      caught = true
      held.forEach(dispatch)
      held.length = 0
    }
    void unlisten
      .then(() => invoke<PlayerEvent[]>("player_snapshot"))
      .then(
        (events) => events.forEach(dispatch),
        () => {},
      )
      .then(settle)
    return () => {
      settle()
      void unlisten.then((fn) => fn())
    }
  }, [])
  return state
}

export const player = {
  load: (request: LoadRequest) => invoke<void>("player_load", { request }),
  control: (control: Control) => invoke<void>("player_control", { control }),
  /** Re-picks an external subtitle track, fetching its file anew; `false` says it wouldn't load. */
  reopenSubtitle: (track: { url: string; title?: string; lang?: string }) => invoke<boolean>("subtitle_reopen", track),
}

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) seconds = 0
  const s = Math.floor(seconds % 60)
  const m = Math.floor((seconds / 60) % 60)
  const h = Math.floor(seconds / 3600)
  const mm = h > 0 ? String(m).padStart(2, "0") : String(m)
  return `${h > 0 ? `${h}:` : ""}${mm}:${String(s).padStart(2, "0")}`
}

let pendingStop: number | undefined

/**
 * Stops playback when the page leaves, but not when it is merely remounted: StrictMode's dev
 * double-mount, or moving between player pages, cancels the stop by mounting again first.
 */
export function useStopOnLeave() {
  useEffect(() => {
    window.clearTimeout(pendingStop)
    return () => {
      pendingStop = window.setTimeout(() => void player.control({ type: "stop" }), 150)
    }
  }, [])
}
