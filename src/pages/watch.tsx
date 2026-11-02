import { useNavigate, useRouter } from "@tanstack/react-router"
import { LinkIcon, ListVideoIcon, RefreshCwIcon, ServerIcon } from "lucide-react"
import { listen } from "@tauri-apps/api/event"
import { useQuery } from "@tanstack/react-query"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { toast } from "sonner"

import { ControlButton } from "@/components/player/player-controls"
import { AutoSkipCountdown, SkipAction, UpNextCard, useSkipPopup } from "@/components/player/player-overlays"
import { EmbedPlayer } from "@/components/player/embed-player"
import { EpisodeDrawer } from "@/components/player/episode-drawer"
import { EpisodeCountdownText } from "@/components/anime/episode-countdown"
import { SourcesPanel } from "@/components/player/sources-panel"
import { SubtitleSearch } from "@/components/player/subtitle-search"
import { PlayerSurface } from "@/components/player/player-surface"
import type { SeekMarker } from "@/components/player/seek-bar"
import { Button } from "@/components/ui/button"
import { Card, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { api, useAppearance, useDetail, useEpisodes, useProgress, useSettings } from "@/lib/api"
import { airsLabel, episodeLock } from "@/lib/episodes"
import { episodeLabel, plainDescription } from "@/lib/format"
import { player, usePlayer, useStopOnLeave, type PlayerEvent } from "@/lib/player"
import { activeSkipWindow, nextEpisodeDue, skipTarget } from "@/lib/player-policy"
import type { Audio, Episode, Server, WatchInfo } from "@/lib/types"
import { errorMessage } from "@/lib/types"
import { copyLink, watchLink } from "@/lib/links"
import { useLockClock } from "@/lib/use-lock-clock"

/** `start`: seconds to begin at instead of the saved position (from an `anilili://` link). */
export function WatchPage({ id, episode, start }: { id: number; episode: number; start?: number }) {
  const navigate = useNavigate()
  const router = useRouter()
  const state = usePlayer()
  const detail = useDetail(id)
  const episodes = useEpisodes(id)
  const progress = useProgress(id)
  const settings = useSettings().data
  const appearance = useAppearance()

  // What the user (or failover) asked for, tagged with its episode so a choice made for one
  // episode never carries into the next.
  type Choice = { episode: number; audio: Audio | null; serverId?: string; start?: number; refresh?: boolean; nonce: number }
  const [stored, setChoice] = useState<Choice | null>(null)
  const choice: Choice = stored?.episode === episode ? stored : { episode, audio: null, start, nonce: 0 }
  const audio = choice.audio
  const tried = useRef(new Set<string>())
  const [failedFor, setFailedFor] = useState<{ key: string; message: string } | null>(null)
  // An iframe-only server the user chose, tagged with its episode like the other choices.
  const [searchingSubtitles, setSearchingSubtitles] = useState(false)
  const [embedFor, setEmbedFor] = useState<{ episode: number; server: Server } | null>(null)
  const embed = embedFor?.episode === episode ? embedFor.server : null

  const title = detail.data?.title
  const titleRef = useRef(title)
  useEffect(() => {
    titleRef.current = title
  }, [title])

  const list = episodes.data?.episodes ?? []
  const index = list.findIndex((e) => e.number === episode)
  const current: Episode | undefined = list[index]
  // Read when the episode loads (like the title), so a list arriving later doesn't reload it.
  const episodeTitleRef = useRef(current?.title ?? null)
  useEffect(() => {
    episodeTitleRef.current = current?.title ?? null
  }, [current])
  // The next episode before it is out (locked, counting down on the title page) is not one to go
  // on to; nor is this one, opened by a link, a play that can work yet.
  const lockNow = useLockClock(list)
  const following = index >= 0 ? list[index + 1] : undefined
  const next = following && !episodeLock(following, lockNow) ? following : undefined
  const locked = current?.airsAt != null && episodeLock(current, lockNow) ? current : undefined
  const previous = index > 0 ? list[index - 1] : undefined

  // Resolving and loading is one query per choice: React Query owns loading/error state and
  // dedupes the double mount of StrictMode, so mpv is told to load once.
  const choiceKey = JSON.stringify([id, choice])
  const watch = useQuery({
    queryKey: ["watch", choiceKey],
    queryFn: async () => {
      if (!choice.serverId) tried.current = new Set()
      const result = await api.watch({
        animeId: id,
        episode,
        serverId: choice.serverId ?? null,
        audio: choice.audio,
        start: choice.start ?? null,
        refresh: choice.refresh ?? false,
        title: titleRef.current ? `${titleRef.current} · ${episodeLabel(episode)}` : episodeLabel(episode),
        episodeTitle: episodeTitleRef.current,
      })
      tried.current.add(result.server.id)
      return result
    },
    retry: false,
    staleTime: Infinity,
    gcTime: 0,
    refetchOnMount: false,
  })
  const info: WatchInfo | null = watch.data ?? null
  const busy = watch.isFetching
  const loadError = watch.error
    ? errorMessage(watch.error)
    : failedFor?.key === choiceKey
      ? failedFor.message
      : null

  const choose = useCallback(
    (patch: Partial<Omit<Choice, "episode" | "nonce">>) =>
      setChoice((prev) => ({
        ...(prev?.episode === episode ? prev : { episode, audio: null, nonce: 0 }),
        serverId: undefined,
        start: undefined,
        refresh: undefined,
        ...patch,
        episode,
        nonce: (prev?.nonce ?? 0) + 1,
      })),
    [episode],
  )
  const setAudio = (value: Audio) => {
    tried.current = new Set()
    choose({ audio: value })
  }

  useStopOnLeave()

  // Failover, driven by mpv's own events: when a server fails mid-stream, move to the next one
  // with the same audio, keeping the position. When every server has failed, say so.
  const latest = useRef({ info, timePos: 0, choiceKey })
  useEffect(() => {
    latest.current = { info, timePos: state.timePos, choiceKey }
  }, [info, state.timePos, choiceKey])
  useEffect(() => {
    const unlisten = listen<PlayerEvent>("player", ({ payload }) => {
      if (payload.type !== "endFile" || payload.reason !== "error") return
      const { info: active, timePos, choiceKey: key } = latest.current
      if (!active) return
      const candidates = active.servers.filter((s) => s.streams.length > 0 && s.audio === active.server.audio)
      const nextServer = candidates.find((s) => !tried.current.has(s.id))
      if (nextServer) {
        toast.warning(`${active.server.label} failed, trying ${nextServer.label}…`)
        choose({ audio: active.server.audio, serverId: nextServer.id, start: timePos > 5 ? timePos : undefined })
      } else {
        setFailedFor({ key, message: `Every server failed for this episode. Last error: ${payload.error ?? "unknown"}` })
      }
    })
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [choose])

  // Skip the recap, opening and ending: the server's timings, else the media index's mirror of
  // AniSkip, else AniSkip itself once the stream's length is known (it picks the right encode).
  const known = info?.skip ?? current?.skip ?? null
  const length = Math.round(state.duration)
  const aniskip = useQuery({
    queryKey: ["aniskip", id, episode, length],
    queryFn: () => api.skipTimes(id, episode, length),
    enabled: !known && !!info && length > 60,
    staleTime: Infinity,
    retry: false,
  })
  const skip = known ?? aniskip.data ?? null
  const markers = useMemo(
    () =>
      (["recap", "intro", "outro"] as const)
        .map((kind) => (skip?.[kind] ? { kind, start: skip[kind]!.start, end: skip[kind]!.end } : null))
        .filter((m): m is SeekMarker => !!m),
    [skip],
  )
  // Android's rules (src/lib/player-policy.ts): the window the playhead is in, an auto-skip that
  // counts down on the button first, and the next episode offered only in the file's last
  // half-minute. Both are answered once per stream: a skipped window or a refused card stays so.
  const streamKey = `${episode}|${info?.server.id ?? ""}`
  const [autoHandled, setAutoHandled] = useState<{ stream: string; keys: string[] }>({ stream: "", keys: [] })
  const [nextHandledFor, setNextHandledFor] = useState<string | null>(null)
  // Between choosing an episode and mpv starting it, the clock is still the last one's.
  const settled = !!info && !busy && !state.loading && !loadError
  const activeWindow = settled ? activeSkipWindow(markers, state.timePos, state.duration) : null
  const windowKey = activeWindow ? `${activeWindow.kind}-${activeWindow.start}-${activeWindow.end}` : ""
  const autoSkip = activeWindow?.kind === "outro" ? settings?.autoSkipOutro : settings?.autoSkipIntro
  // Preview pictures over the seek bar, from the playing server's storyboard (not the last
  // episode's while the next one loads).
  const thumbnails = settled ? info.server.thumbnails : null
  const storyboard = useMemo(() => (thumbnails && info ? { url: thumbnails, headers: info.server.headers } : null), [thumbnails, info])
  const pendingAuto =
    activeWindow && autoSkip && !(autoHandled.stream === streamKey && autoHandled.keys.includes(windowKey)) ? activeWindow : null
  const playing = state.active && !state.paused && !state.buffering && !state.loading
  const skipWindow = (window: SeekMarker, auto: boolean) => {
    if (auto) setAutoHandled((h) => ({ stream: streamKey, keys: [...(h.stream === streamKey ? h.keys : []), windowKey] }))
    void player.control({ type: "seek", seconds: skipTarget(window, state.duration), relative: false })
  }
  const outro = markers.find((m) => m.kind === "outro")
  const offerNext = !!(
    settled &&
    next &&
    settings?.autoplayNext &&
    nextHandledFor !== streamKey &&
    nextEpisodeDue(state.timePos, state.duration, outro)
  )

  // Autoplay the next episode after a short countdown.
  const goTo = useCallback(
    (n: number) => void navigate({ to: "/watch/$id/$episode", params: { id: String(id), episode: String(n) }, replace: true }),
    [navigate, id],
  )

  // Hardware media keys / the Windows media overlay.
  const nav = useRef({ next, previous, goTo })
  useEffect(() => {
    nav.current = { next, previous, goTo }
  }, [next, previous, goTo])
  useEffect(() => {
    const unlisten = listen<string>("media-key", ({ payload }) => {
      const { next: n, previous: p, goTo: go } = nav.current
      if (payload === "next" && n) go(n.number)
      if (payload === "previous" && p) go(p.number)
    })
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [])

  // Android's end of an episode (`WatchScreen`'s `onEnded`): with auto-play on, a file that plays
  // out, or is skipped to its very end, goes straight on to the next episode. The card is only the
  // warning; with nothing behind it, a card that never came left the last frame on screen. Only for
  // a stream seen playing, so one opened at its end (a link's `?t=`) is shown rather than replaced.
  const ending = useRef({ streamKey, settled, next, autoplay: false, goTo })
  const playedStream = useRef<string | null>(null)
  useEffect(() => {
    ending.current = { streamKey, settled, next, autoplay: settings?.autoplayNext === true, goTo }
    if (settled && playing) playedStream.current = streamKey
  }, [streamKey, settled, next, settings?.autoplayNext, goTo, playing])
  useEffect(() => {
    const unlisten = listen<PlayerEvent>("player", ({ payload }) => {
      const ended =
        (payload.type === "propertyChange" && payload.name === "eof-reached" && payload.value === true) ||
        (payload.type === "endFile" && payload.reason === "eof")
      const { streamKey: stream, settled: ready, next: n, autoplay, goTo: go } = ending.current
      if (!ended || !autoplay || !n || !ready || playedStream.current !== stream) return
      playedStream.current = null
      go(n.number)
    })
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [])

  const back = () => {
    if (window.history.length > 1) router.history.back()
    else void navigate({ to: "/anime/$id", params: { id: String(id) } })
  }

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "n" && next) {
      goTo(next.number)
      return true
    }
    if (e.key === "p" && previous) {
      goTo(previous.number)
      return true
    }
    if (e.key === "Enter" && activeWindow) {
      skipWindow(activeWindow, activeWindow === pendingAuto)
      return true
    }
    return false
  }

  // When resolving failed there is no server list from `watch`; fetch it for the embed options.
  const fallbackSources = useQuery({
    queryKey: ["sources", id, episode],
    queryFn: () => api.sources(id, episode),
    enabled: !info && !!loadError,
    staleTime: 5 * 60_000,
  })
  const embeds = (info?.servers ?? fallbackSources.data?.servers ?? []).filter((s) => s.streams.length === 0 && s.embed)
  const openEmbed = (server: Server) => {
    void player.control({ type: "stop" })
    setEmbedFor({ episode, server })
  }

  const progressByEpisode = useMemo(() => new Map((progress.data ?? []).map((p) => [p.episode, p])), [progress.data])
  const native = info?.servers.filter((s) => s.streams.length > 0) ?? []

  if (embed) {
    return (
      <EmbedPlayer
        server={embed}
        title={`${title ?? ""} · ${episodeLabel(episode)}`}
        playing={{ animeId: id, episode, title: title ?? null, episodeTitle: current?.title ?? null }}
        onBack={back}
        onClose={() => {
          setEmbedFor(null)
          choose({ audio })
        }}
      />
    )
  }

  return (
    <>
      <PlayerSurface
        state={state}
        title={title ?? "Loading…"}
        onSearchSubtitles={() => setSearchingSubtitles(true)}
        logo={detail.data?.logo}
        subtitle={episodeLine(episode, current)}
        onBack={back}
        busy={busy}
        pinned={!!loadError}
        onKey={onKey}
        markers={markers}
        storyboard={storyboard}
        episodic={list.length > 1}
        onPrevious={previous ? () => goTo(previous.number) : undefined}
        onNext={next ? () => goTo(next.number) : undefined}
        pauseDetails={{
          provider: info?.server.label,
          description: plainDescription(current?.synopsis ?? detail.data?.description ?? null, 400),
        }}
        idle={
          detail.data?.banner && !loadError ? (
            <img src={detail.data.banner} alt="" className="absolute inset-0 size-full object-cover opacity-20 blur-sm" />
          ) : null
        }
        topRight={
          <ControlButton label="Copy a link to this moment" onClick={() => copyLink(watchLink(id, episode, state.timePos))}>
            <LinkIcon />
          </ControlButton>
        }
        overlay={(layout) => (
          <>
            {/* Above the controls, drawn after it: its scrim's padding reaches up over these buttons. */}
            <div className="absolute right-6 z-10 flex flex-col items-end gap-3" style={{ bottom: layout.actionBottom }}>
              {offerNext && next ? (
                <UpNextCard
                  key={streamKey}
                  label={episodeLine(next.number, next)}
                  playing={playing || state.ended}
                  onPlay={() => {
                    setNextHandledFor(streamKey)
                    goTo(next.number)
                  }}
                  onStay={() => setNextHandledFor(streamKey)}
                />
              ) : null}
              <SkipSlot
                pendingAuto={pendingAuto}
                windowKey={windowKey}
                activeWindow={activeWindow}
                controlsVisible={layout.controlsVisible}
                offerNext={offerNext}
                playing={playing}
                onSkip={skipWindow}
                fixed={
                  settings?.showSkipButton && settled && state.duration > 0 && state.timePos <= state.duration
                    ? {
                        seconds: settings.skipButtonSeconds,
                        onSkip: () =>
                          void player.control({
                            type: "seek",
                            seconds: Math.min(state.duration, state.timePos + settings.skipButtonSeconds),
                            relative: false,
                          }),
                      }
                    : null
                }
              />
            </div>
            {loadError && (
              // Its own dimmed layer: over the transport cluster, which can't do anything here.
              <div className="absolute inset-0 flex items-center justify-center bg-black/60 p-6 animate-in fade-in">
                <Card className="w-full max-w-lg shadow-2xl">
                  {locked?.airsAt != null ? (
                    <CardHeader>
                      <CardTitle>{episodeLabel(episode)} isn't out yet</CardTitle>
                      <CardDescription className="flex flex-col gap-0.5">
                        <EpisodeCountdownText episode={locked} />
                        <span>{airsLabel(locked.airsAt)}, this computer's time.</span>
                      </CardDescription>
                    </CardHeader>
                  ) : (
                    <CardHeader>
                      <CardTitle>Can't play {episodeLabel(episode)}</CardTitle>
                      <CardDescription>{loadError}</CardDescription>
                    </CardHeader>
                  )}
                  <CardFooter className="flex-wrap gap-2">
                    {!locked && (
                      <Button
                        onClick={() => {
                          tried.current = new Set()
                          choose({ audio, refresh: true })
                        }}
                      >
                        <RefreshCwIcon data-icon="inline-start" />
                        Try again
                      </Button>
                    )}
                    {!locked && audio !== "dub" && (
                      <Button variant="outline" onClick={() => setAudio("dub")}>
                        Try the dub
                      </Button>
                    )}
                    {!locked && embeds.length > 0 && (
                      <Button variant="outline" onClick={() => openEmbed(embeds.find((e) => e.audio === (audio ?? "sub")) ?? embeds[0])}>
                        Embedded player
                      </Button>
                    )}
                    <Button variant="ghost" className="ml-auto" onClick={back}>
                      Back
                    </Button>
                  </CardFooter>
                </Card>
              </div>
            )}
          </>
        )}
        panels={[
          ...(info
            ? [
                {
                  id: "sources",
                  label: info.server.label,
                  icon: <ServerIcon />,
                  render: (close: () => void) => (
                    <SourcesPanel
                      current={info.server}
                      native={native}
                      embeds={embeds}
                      preferredLanguages={settings?.preferredSourceLanguages ?? []}
                      onAudio={setAudio}
                      onServer={(serverId) => choose({ audio: info.server.audio, serverId, start: state.timePos })}
                      onEmbed={openEmbed}
                      onClose={close}
                    />
                  ),
                },
              ]
            : []),
          {
            id: "episodes",
            label: "Episodes",
            icon: <ListVideoIcon />,
            render: (close) => (
              <EpisodeDrawer
                episodes={list}
                current={episode}
                progress={progressByEpisode}
                banner={detail.data?.banner}
                blurEpisodeImages={appearance.blurEpisodeImages}
                onSelect={(n) => n !== episode && goTo(n)}
                onClose={close}
              />
            ),
          },
        ]}
      />
      {/* Mounted only while open, so it starts from the title as it is then. */}
      {searchingSubtitles && <SubtitleSearch open onOpenChange={setSearchingSubtitles} title={title ?? ""} episode={episode} />}
    </>
  )
}

/** "Episode 5: The Title". */
function episodeLine(n: number, episode?: Episode): string {
  return `${episodeLabel(n)}${episode?.title ? `: ${episode.title}` : ""}`
}

const SEGMENT: Record<SeekMarker["kind"], string> = { recap: "Recap", intro: "Intro", outro: "Outro" }

/**
 * The one action in the bottom-right corner, in Android's order: a pending auto-skip (counting down,
 * shown with or without the controls), the recap and the opening (popping up by themselves on entry,
 * then with the controls only: a window can start at 0:00 and last ninety seconds; see
 * SKIP_POPUP_SECONDS), nothing while the up-next card is up, the credits,
 * and last the fixed-length skip for shows the timings miss (with the controls only).
 */
function SkipSlot({
  pendingAuto,
  windowKey,
  activeWindow,
  controlsVisible,
  offerNext,
  playing,
  onSkip,
  fixed,
}: {
  pendingAuto: SeekMarker | null
  windowKey: string
  activeWindow: SeekMarker | null
  controlsVisible: boolean
  offerNext: boolean
  playing: boolean
  onSkip: (window: SeekMarker, auto: boolean) => void
  fixed: { seconds: number; onSkip: () => void } | null
}) {
  const timed = activeWindow && activeWindow.kind !== "outro" ? activeWindow : null
  const popup = useSkipPopup(timed ? windowKey : "", playing)
  if (pendingAuto) {
    return <AutoSkipCountdown key={windowKey} segment={SEGMENT[pendingAuto.kind]} playing={playing} onSkip={() => onSkip(pendingAuto, true)} />
  }
  if (timed && (controlsVisible || popup)) {
    return <SkipAction label={`Skip ${SEGMENT[timed.kind]}`} onClick={() => onSkip(timed, false)} />
  }
  if (offerNext) return null
  if (activeWindow?.kind === "outro") {
    return <SkipAction label="Skip Outro" onClick={() => onSkip(activeWindow, false)} />
  }
  if (fixed && controlsVisible) return <SkipAction label={`+${fixed.seconds}s`} onClick={fixed.onSkip} />
  return null
}
