import { getCurrentWindow } from "@tauri-apps/api/window"
import type { SeekMarker } from "@/components/player/seek-bar"
import type { StoryboardSource } from "@/components/player/seek-preview"
import {
  ArrowLeftIcon,
  CaptionsIcon,
  FastForwardIcon,
  LoaderCircleIcon,
  Maximize2Icon,
  PauseIcon,
  PlayIcon,
  RewindIcon,
  SettingsIcon,
  SkipBackIcon,
  SkipForwardIcon,
} from "lucide-react"
import { useCallback, useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import { SettingsPanel, SubtitlesPanel } from "@/components/player/options-panels"
import { ControlButton, PlayerControls } from "@/components/player/player-controls"
import { CHIP, PauseBilling, PlayerClock, SeriesName } from "@/components/player/player-overlays"
import { useMiniPlayer } from "@/components/player/use-mini-player"
import { Button } from "@/components/ui/button"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { useAppearance, useSettings } from "@/lib/api"
import { player, type PlayerState } from "@/lib/player"
import { accentVars } from "@/lib/theme"
import { cn } from "@/lib/utils"

const IDLE_HIDE_MS = 2500
/** Between the seek bar and an action floating above it; the corner inset while the bar is down. */
const ACTION_GAP = 12
const ACTION_INSET = 24
/** Space held this long speeds the picture up instead of pausing on release, as on YouTube. */
const HOLD_SPEED_MS = 300
const HOLD_SPEED = 2

/** Where an overlay stands, for the things that ride with the controls (skip, up next). */
export type OverlayLayout = {
  controlsVisible: boolean
  /** Bottom offset for an action in the bottom-right corner: just clear of the seek bar while it's up. */
  actionBottom: number
}

/** A side panel the controls offer as a pill (episodes, sources); one is open at a time. */
export type PlayerPanel = {
  id: string
  label: string
  icon?: React.ReactNode
  /** The panel's content; `close` shuts it, as Esc does. */
  render: (close: () => void) => React.ReactNode
}

type Props = {
  state: PlayerState
  /** The series (drawn as its logo when there is one). */
  title?: string
  logo?: string | null
  /** The episode line under it. */
  subtitle?: string
  onBack?: () => void
  /** Side panels, each opened by its pill after `extra`. */
  panels?: PlayerPanel[]
  /** Floating things over the video (skip, the up-next card, errors). */
  overlay?: React.ReactNode | ((layout: OverlayLayout) => React.ReactNode)
  /** Pills ahead of Subtitles and Settings. */
  extra?: React.ReactNode
  /** Icons under the clock, top right. */
  topRight?: React.ReactNode
  /** Previous and next episode, around the center cluster; omitted where there are none. */
  onPrevious?: () => void
  onNext?: () => void
  /** Episodes exist but there's no previous/next: the cluster shows the buttons disabled. */
  episodic?: boolean
  /** What a long pause bills besides the titles. */
  pauseDetails?: { provider?: string; description?: string }
  markers?: SeekMarker[]
  /** The server's storyboard, for the seek bar's preview pictures. */
  storyboard?: StoryboardSource | null
  /** Forces the controls visible (e.g. while an error card needs them). */
  pinned?: boolean
  /** Extra keyboard shortcuts for this page. Return true when handled. */
  onKey?: (e: KeyboardEvent) => boolean
  /** Shown while nothing plays (instead of the bare black). */
  idle?: React.ReactNode
  busy?: boolean
  /** Opens the OpenSubtitles search, from the Subtitles panel. */
  onSearchSubtitles?: () => void
}

/**
 * The transparent layer over mpv, laid out as Android's player: the title card (the series' logo)
 * top left, the clock top right, the transport cluster in the middle, the timeline and labelled
 * controls at the bottom. Controls auto-hide, paused or not; a pause that outlives them gets the
 * episode's billing. Also: keyboard shortcuts, fullscreen, side panels over the video (dimmed,
 * not shrunk), and subtitles lifted clear of the bar.
 */
export function PlayerSurface({
  state,
  title,
  logo,
  subtitle,
  onBack,
  panels = [],
  overlay,
  extra,
  topRight,
  onPrevious,
  onNext,
  episodic,
  pauseDetails,
  markers,
  storyboard,
  pinned: forcePinned,
  onKey,
  idle,
  busy,
  onSearchSubtitles,
}: Props) {
  const settings = useSettings().data
  const [fullscreen, setFullscreen] = useState(false)
  const miniPlayer = useMiniPlayer()
  const [openPanel, setOpenPanel] = useState<string | null>(null)
  const [controlsVisible, setControlsVisible] = useState(true)
  const [seekAnchor, setSeekAnchor] = useState(0)
  const hideTimer = useRef<number | undefined>(undefined)
  const controlsRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (state.error) toast.error(state.error)
  }, [state.error])

  // The page's panels, then the two every player has, as on Android's control row.
  const allPanels: PlayerPanel[] = [
    ...panels,
    {
      id: "subtitles",
      label: "Subtitles",
      icon: <CaptionsIcon />,
      render: (close) => <SubtitlesPanel state={state} onSearch={onSearchSubtitles} onClose={close} />,
    },
    { id: "settings", label: "Settings", icon: <SettingsIcon />, render: (close) => <SettingsPanel state={state} onClose={close} /> },
  ]
  const panel = allPanels.find((p) => p.id === openPanel) ?? null
  const showPanel = panel != null
  const closePanel = useCallback(() => setOpenPanel(null), [])
  const showPanelRef = useRef(showPanel)
  useEffect(() => {
    showPanelRef.current = showPanel
  }, [showPanel])

  const toggleFullscreen = useCallback(async () => {
    const window = getCurrentWindow()
    const next = !(await window.isFullscreen())
    await window.setFullscreen(next)
    setFullscreen(next)
  }, [])
  // Never strand the user in fullscreen after leaving the player.
  useEffect(() => () => void getCurrentWindow().setFullscreen(false), [])

  const poke = useCallback(() => {
    setControlsVisible(true)
    window.clearTimeout(hideTimer.current)
    hideTimer.current = window.setTimeout(() => setControlsVisible(false), IDLE_HIDE_MS)
  }, [])
  useEffect(() => () => window.clearTimeout(hideTimer.current), [])

  const loading = state.loading || state.buffering || !!busy
  // A pause no longer pins the controls: they go as they do while playing, which is what lets a
  // long pause show the billing (Android's controls behave the same).
  const pinned = forcePinned || !state.active || state.loading || !!busy || showPanel
  const showControls = pinned || controlsVisible
  // A genuine pause, not a stall and not the held last frame.
  const paused = state.active && state.paused && !state.buffering && !state.loading && !state.ended

  // Lift text subtitles above the control bar while it is on screen, from the gap Settings →
  // Subtitles keeps (mpv's 720-line units, as player.rs `caption_margin`). Captions held at the top
  // stay where they are.
  const captionMargin = settings?.captions.edgeMargin ?? 3
  const captionsOnTop = settings?.captions.position === "top"
  useEffect(() => {
    if (!state.active) return
    const bar = controlsRef.current?.offsetHeight ?? 0
    const lift = showControls && !captionsOnTop ? Math.round((bar * 720) / window.innerHeight) : 0
    void player.control({ type: "setSubtitleMargin", margin: Math.round(captionMargin * 7.2) + lift })
  }, [showControls, state.active, fullscreen, captionMargin, captionsOnTop])

  // Keyboard.
  const stateRef = useRef(state)
  const onKeyRef = useRef(onKey)
  useEffect(() => {
    stateRef.current = state
    onKeyRef.current = onKey
  }, [state, onKey])

  // Space while playing: a tap pauses on release, a hold plays at 2× until it's let go.
  const [boosted, setBoosted] = useState(false)
  const spaceHeld = useRef(false)
  const spaceTimer = useRef<number | undefined>(undefined)
  const speedBeforeHold = useRef<number | null>(null)
  const releaseSpace = useCallback(
    (tap: boolean) => {
      if (!spaceHeld.current) return
      spaceHeld.current = false
      window.clearTimeout(spaceTimer.current)
      if (speedBeforeHold.current != null) {
        void player.control({ type: "setSpeed", speed: speedBeforeHold.current })
        speedBeforeHold.current = null
        setBoosted(false)
      } else if (tap) {
        poke()
        void player.control({ type: "togglePause" })
      }
    },
    [poke],
  )

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)) return
      if (e.ctrlKey || e.metaKey || e.altKey) return
      if (onKeyRef.current?.(e)) {
        e.preventDefault()
        poke()
        return
      }
      const s = stateRef.current
      // Esc shuts the panel before it leaves fullscreen, as Back does on Android.
      if (e.key === "Escape" && showPanelRef.current) {
        e.preventDefault()
        setOpenPanel(null)
        return
      }
      if (e.key === "Escape" && document.fullscreenElement == null) {
        void getCurrentWindow()
          .isFullscreen()
          .then((fs) => {
            if (fs) {
              void getCurrentWindow().setFullscreen(false)
              setFullscreen(false)
            }
          })
        return
      }
      if (!s.active) return
      if (e.key === " ") {
        e.preventDefault()
        if (e.repeat || spaceHeld.current) return
        // Paused, Space plays at once; there is nothing to speed up.
        if (s.paused || s.ended) {
          poke()
          void player.control({ type: "togglePause" })
          return
        }
        spaceHeld.current = true
        spaceTimer.current = window.setTimeout(() => {
          speedBeforeHold.current = stateRef.current.speed
          setBoosted(true)
          void player.control({ type: "setSpeed", speed: HOLD_SPEED })
        }, HOLD_SPEED_MS)
        return
      }
      const actions: Record<string, () => unknown> = {
        k:() => player.control({ type: "togglePause" }),
        ArrowLeft: () => player.control({ type: "seek", seconds: -5, relative: true }),
        ArrowRight: () => player.control({ type: "seek", seconds: 5, relative: true }),
        j: () => player.control({ type: "seek", seconds: -10, relative: true }),
        l: () => player.control({ type: "seek", seconds: 10, relative: true }),
        ArrowUp: () => player.control({ type: "setVolume", volume: Math.min(150, s.volume + 5) }),
        ArrowDown: () => player.control({ type: "setVolume", volume: Math.max(0, s.volume - 5) }),
        m: () => player.control({ type: "setMute", muted: !s.muted }),
        f: () => toggleFullscreen(),
        z: () => player.control({ type: "setSubDelay", seconds: s.subDelay - 0.1 }),
        x: () => player.control({ type: "setSubDelay", seconds: s.subDelay + 0.1 }),
        s: () => player.control({ type: "screenshot" }).then(() => toast.success("Screenshot saved to Pictures\\Anilili")),
        i: () => miniPlayer.toggle(),
        "[": () => player.control({ type: "setSpeed", speed: Math.max(0.25, s.speed - 0.25) }),
        "]": () => player.control({ type: "setSpeed", speed: Math.min(4, s.speed + 0.25) }),
      }
      const action = actions[e.key]
      if (!action) return
      e.preventDefault()
      poke()
      void action()
    }
    window.addEventListener("keydown", handler)
    return () => window.removeEventListener("keydown", handler)
  }, [toggleFullscreen, poke, miniPlayer])
  useEffect(() => {
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key !== " " || !spaceHeld.current) return
      e.preventDefault()
      releaseSpace(true)
    }
    // Focus lost mid-hold (Alt+Tab): the keyup never comes, so put the speed back now.
    const onBlur = () => releaseSpace(false)
    window.addEventListener("keyup", onKeyUp)
    window.addEventListener("blur", onBlur)
    return () => {
      window.removeEventListener("keyup", onKeyUp)
      window.removeEventListener("blur", onBlur)
      releaseSpace(false)
    }
  }, [releaseSpace])

  // "Accent in the player": off, the controls stay white over any picture, as on Android.
  const playerAccent = useAppearance().playerAccent

  if (miniPlayer.mini) {
    return <MiniOverlay state={state} onExit={() => void miniPlayer.exit()} busy={loading} />
  }

  const layout: OverlayLayout = {
    controlsVisible: showControls && state.active,
    actionBottom: showControls && state.active ? seekAnchor + ACTION_GAP : ACTION_INSET,
  }
  const fade = (visible: boolean) => cn("transition-opacity duration-200", visible ? "opacity-100" : "pointer-events-none opacity-0")
  const seriesTitle = title ?? ""

  return (
    <div
      className={cn("relative flex h-svh w-full overflow-hidden select-none", !showControls && "cursor-none")}
      style={playerAccent ? undefined : (accentVars(null) as React.CSSProperties)}
      onMouseMove={poke}
    >
      <div
        className="relative flex flex-1 items-center justify-center"
        onClick={(e) => {
          if (e.target === e.currentTarget && state.active) void player.control({ type: "togglePause" })
        }}
        onDoubleClick={(e) => {
          if (e.target === e.currentTarget) void toggleFullscreen()
        }}
      >
        {!state.active && idle}

        {/* Under every control layer: the billing sits over the picture, never over a button. */}
        {paused && settings?.pauseDetailsOverlay !== false && pauseDetails && (
          <PauseBilling
            hidden={showControls}
            seriesTitle={seriesTitle}
            logo={logo}
            episodeTitle={subtitle}
            provider={pauseDetails.provider}
            description={pauseDetails.description}
          />
        )}

        {/* Top: the scrim, the back arrow and the title card. */}
        <div className={cn("pointer-events-none absolute inset-x-0 top-0 h-36 bg-linear-to-b from-black/62 to-transparent", fade(showControls))} />
        <div className={cn("absolute top-0 left-0 flex max-w-[62%] items-start gap-1 p-3", fade(showControls))}>
          {onBack && (
            <Button variant="ghost" size="icon-lg" aria-label="Back" className="text-white hover:bg-white/12 hover:text-white" onClick={onBack}>
              <ArrowLeftIcon />
            </Button>
          )}
          <div className={cn("flex min-w-0 flex-col gap-0.5", onBack ? "pt-2" : "px-1 pt-1")}>
            {seriesTitle && (
              <SeriesName
                title={seriesTitle}
                logo={logo}
                className="mb-0.5 max-h-11 max-w-64"
                fallback={<span className="truncate font-heading text-lg leading-tight font-bold text-white text-shadow-lg">{seriesTitle}</span>}
              />
            )}
            {subtitle && <span className="truncate text-sm text-white/85 text-shadow-md">{subtitle}</span>}
          </div>
        </div>

        {/* Top right: the clock (kept up through a pause that outlived the controls), then icons. */}
        <div className="absolute top-0 right-0 flex flex-col items-end gap-1 p-3 pt-4">
          {settings?.playerClock !== false && state.active && (
            <PlayerClock position={state.timePos} duration={state.duration} speed={state.speed} className={fade(showControls || paused)} />
          )}
          {topRight && <div className={cn("flex items-center", fade(showControls))}>{topRight}</div>}
        </div>

        {/* Top center, while Space is held: the picture is running fast. */}
        {boosted && (
          <div className="pointer-events-none absolute top-4 left-1/2 -translate-x-1/2 animate-in duration-150 fade-in">
            <div className={cn(CHIP, "flex items-center gap-1.5 px-3 py-1 text-sm")}>
              {HOLD_SPEED}×
              <FastForwardIcon className="size-4 fill-current" />
            </div>
          </div>
        )}

        {/* The center cluster: previous, back 10s, play/pause, forward 10s, next. */}
        {state.active && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            {showControls ? (
              <div className="pointer-events-auto flex items-center gap-6 animate-in duration-150 fade-in">
                {(episodic || onPrevious || onNext) && (
                  <ControlButton label="Previous episode (P)" disabled={!onPrevious} onClick={() => onPrevious?.()}>
                    <SkipBackIcon />
                  </ControlButton>
                )}
                <ControlButton label="Back 10s (J)" onClick={() => player.control({ type: "seek", seconds: -10, relative: true })}>
                  <RewindIcon />
                </ControlButton>
                <PlayPause paused={state.paused || state.ended} loading={loading} />
                <ControlButton label="Forward 10s (L)" onClick={() => player.control({ type: "seek", seconds: 10, relative: true })}>
                  <FastForwardIcon />
                </ControlButton>
                {(episodic || onPrevious || onNext) && (
                  <ControlButton label="Next episode (N)" disabled={!onNext} onClick={() => onNext?.()}>
                    <SkipForwardIcon />
                  </ControlButton>
                )}
              </div>
            ) : (
              loading && <LoaderCircleIcon className="size-12 animate-spin text-white/80 drop-shadow" />
            )}
          </div>
        )}
        {!state.active && loading && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <LoaderCircleIcon className="size-12 animate-spin text-white/80 drop-shadow" />
          </div>
        )}

        {typeof overlay === "function" ? overlay(layout) : overlay}

        {state.active && (
          <div ref={controlsRef} className={cn("absolute inset-x-0 bottom-0", fade(showControls))}>
            <PlayerControls
              state={state}
              fullscreen={fullscreen}
              onToggleFullscreen={() => void toggleFullscreen()}
              onMiniPlayer={() => void miniPlayer.enter()}
              onSeekAnchor={setSeekAnchor}
              extra={
                <>
                  {extra}
                  {allPanels.map((p) => (
                    <PanelPill
                      key={p.id}
                      label={p.label}
                      icon={p.icon}
                      open={p.id === openPanel}
                      onClick={() => setOpenPanel((open) => (open === p.id ? null : p.id))}
                    />
                  ))}
                </>
              }
              markers={markers}
              storyboard={storyboard}
            />
          </div>
        )}
      </div>

      {showPanel && (
        // Android's `PlayerSidePanel`: over the picture rather than beside it, which stays full-size
        // under a dimming scrim that closes the panel like any sheet's.
        <div className="absolute inset-0 z-20 bg-black/55 animate-in duration-200 fade-in" onClick={(e) => e.target === e.currentTarget && closePanel()}>
          {/* Near-black, so its edge reads against the video, rounded on the side facing the picture. */}
          <aside className="absolute inset-y-0 right-0 flex w-[42%] max-w-105 min-w-75 flex-col rounded-l-2xl bg-[#0c0c0c] text-white animate-in duration-200 slide-in-from-right-8">
            {panel.render(closePanel)}
          </aside>
        </div>
      )}
    </div>
  )
}

/** Play/pause keeps the accent: it and the seek bar are the only things that carry it. */
function PlayPause({ paused, loading }: { paused: boolean; loading: boolean }) {
  const label = paused ? "Play (Space)" : "Pause (Space)"
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            aria-label={label}
            onClick={() => void player.control({ type: "togglePause" })}
            className="flex size-18 items-center justify-center rounded-full border-2 border-primary bg-primary/24 text-white shadow-lg backdrop-blur-sm transition-colors outline-none hover:bg-primary/36 focus-visible:ring-3 focus-visible:ring-primary/50 [&_svg]:size-8"
          />
        }
      >
        {loading ? <LoaderCircleIcon className="animate-spin" /> : paused ? <PlayIcon className="fill-current" /> : <PauseIcon className="fill-current" />}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}

function PanelPill({ label, icon, open, onClick }: { label: string; icon?: React.ReactNode; open: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      aria-pressed={open}
      onClick={onClick}
      className={cn(
        "flex h-9 items-center gap-1.75 rounded-full px-3.5 text-sm font-medium whitespace-nowrap text-white outline-none transition-colors focus-visible:ring-3 focus-visible:ring-white/40 [&_svg]:size-4.5",
        open ? "bg-white/20 hover:bg-white/26" : "bg-black/42 hover:bg-white/15",
      )}
    >
      {icon}
      {label}
    </button>
  )
}

/** The mini player: drag anywhere to move it; hover for play/pause and the way back. */
function MiniOverlay({ state, onExit, busy }: { state: PlayerState; onExit: () => void; busy: boolean }) {
  const progress = state.duration > 0 ? (state.timePos / state.duration) * 100 : 0
  return (
    <div
      className="group relative h-svh w-full select-none"
      onMouseDown={(e) => {
        if ((e.target as HTMLElement).closest("button")) return
        void getCurrentWindow().startDragging()
      }}
      onDoubleClick={onExit}
    >
      {busy && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
          <LoaderCircleIcon className="size-8 animate-spin text-foreground/80" />
        </div>
      )}
      <div className="absolute inset-0 flex items-center justify-center gap-2 bg-black/40 opacity-0 transition-opacity group-hover:opacity-100">
        <Button variant="secondary" size="icon-lg" aria-label={state.paused ? "Play" : "Pause"} onClick={() => void player.control({ type: "togglePause" })}>
          {state.paused ? <PlayIcon /> : <PauseIcon />}
        </Button>
        <Button variant="secondary" size="icon-lg" aria-label="Back to full window" onClick={onExit}>
          <Maximize2Icon />
        </Button>
      </div>
      <div className="absolute inset-x-0 bottom-0 h-1 bg-background/40">
        <div className="h-full bg-primary" style={{ width: `${progress}%` }} />
      </div>
    </div>
  )
}
