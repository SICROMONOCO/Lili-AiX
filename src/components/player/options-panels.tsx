// The player's Subtitles and Settings panels, as Android's in-player Captions and Settings sheets
// (`PlayerSettingsSheet.kt`: `CaptionSections` and `SheetSections`) in the shared side-panel frame.

import { GaugeIcon, Volume2Icon, VolumeXIcon } from "lucide-react"
import { useState } from "react"

import {
  ActionRow,
  ChoiceRow,
  Helper,
  PanelFrame,
  QuietAction,
  SectionLabel,
  Segmented,
  SliderRow,
  StepperRow,
  SwitchRow,
} from "@/components/player/player-panel"
import { useSaveSettings, useSettings } from "@/lib/api"
import { player, type PlayerState, type Track } from "@/lib/player"
import type { PlayerContentScale, Settings } from "@/lib/types"

const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2]
const SCALES: { value: PlayerContentScale; label: string }[] = [
  { value: "fit", label: "Fit" },
  { value: "crop", label: "Crop" },
  { value: "fill", label: "Fill" },
]
const SUB_SIZES = [0.75, 1, 1.25, 1.5]
/** The Z and X keys' step. */
const DELAY_STEP = 0.1

function trackLabel(track: Track): string {
  const parts = [track.title, track.lang?.toUpperCase(), track.codec].filter(Boolean)
  return parts.length > 0 ? parts.join(" · ") : `Track ${track.id}`
}

function useUpdateSettings() {
  const settings = useSettings().data
  const save = useSaveSettings()
  return [settings, (patch: Partial<Settings>) => settings && save.mutate({ ...settings, ...patch })] as const
}

/** Android's Captions sheet: the track, adding one, how they look, and their timing. */
export function SubtitlesPanel({ state, onSearch, onClose }: { state: PlayerState; onSearch?: () => void; onClose: () => void }) {
  const [settings, update] = useUpdateSettings()
  const [dead, setDead] = useState<string | null>(null)
  const subs = state.tracks.filter((t) => t.type === "sub")
  const selected = subs.find((t) => t.selected)
  const delay = state.subDelay
  const setDelay = (seconds: number) => void player.control({ type: "setSubDelay", seconds: Math.round(seconds * 10) / 10 })
  // Picking an external track fetches its file again: a link that was dead when the episode
  // opened (the track sits “selected” showing no cues — some servers' subtitle links answer
  // 403 one minute and work the next) is often alive on the next try. When it still refuses,
  // the panel says so instead of staying silently blank.
  const pick = (t: Track) => {
    setDead(null)
    if (!t.external || !t.url) {
      void player.control({ type: "selectTrack", kind: "sub", id: String(t.id) })
      return
    }
    void player.reopenSubtitle({ url: t.url, title: t.title, lang: t.lang }).then((alive) =>
      setDead(alive ? null : `${trackLabel(t)} didn't load — the server's link is down. Try again in a bit, another server, or OpenSubtitles.`),
    )
  }
  const off = () => {
    setDead(null)
    void player.control({ type: "selectTrack", kind: "sub", id: "no" })
  }

  return (
    <PanelFrame title="Subtitles" onClose={onClose}>
      <SectionLabel>Subtitle track</SectionLabel>
      <ChoiceRow title="Off" selected={!selected} onSelect={off} />
      {subs.map((t) => (
        <ChoiceRow key={t.id} title={trackLabel(t)} selected={t.id === selected?.id} onSelect={() => pick(t)} />
      ))}
      {dead && <Helper>{dead}</Helper>}

      {onSearch && (
        <>
          <SectionLabel>Add subtitles</SectionLabel>
          <ActionRow
            title="Search OpenSubtitles"
            description="Uses a free OpenSubtitles account"
            onClick={() => {
              onClose()
              onSearch()
            }}
          />
        </>
      )}

      {settings && (
        <>
          <SectionLabel>Size</SectionLabel>
          <Segmented
            options={SUB_SIZES.map((size) => ({ value: String(size), label: size === 1 ? "Normal" : `${Math.round(size * 100)}%` }))}
            selected={String(settings.subtitleScale)}
            onSelect={(v) => update({ subtitleScale: Number(v) })}
          />
        </>
      )}

      <SectionLabel>Subtitle delay</SectionLabel>
      {/* Applied while the episode plays, so each press can be judged against the picture. */}
      <StepperRow
        label="Shift"
        value={`${delay > 0 ? "+" : ""}${delay.toFixed(1)} s`}
        decrease={{ label: "−0.1s (Z)", onClick: () => setDelay(delay - DELAY_STEP) }}
        increase={{ label: "+0.1s (X)", onClick: () => setDelay(delay + DELAY_STEP) }}
      />
      <Helper>
        {delay === 0 ? "Subtitles play as the provider timed them." : delay > 0 ? "Subtitles are held back." : "Subtitles run ahead."}
      </Helper>
      {delay !== 0 && (
        <QuietAction className="mt-1.5" onClick={() => setDelay(0)}>
          Reset delay
        </QuietAction>
      )}
    </PanelFrame>
  )
}

/** Android's Settings sheet: volume, speed, the audio track, screen size, and the playback switches. */
export function SettingsPanel({ state, onClose }: { state: PlayerState; onClose: () => void }) {
  const [settings, update] = useUpdateSettings()
  const audio = state.tracks.filter((t) => t.type === "audio")
  const volume = state.muted ? 0 : Math.min(state.volume, 100)
  const speedIndex = Math.max(0, SPEEDS.indexOf(state.speed))

  return (
    <PanelFrame title="Settings" onClose={onClose}>
      <SectionLabel>Volume</SectionLabel>
      <SliderRow
        icon={volume === 0 ? <VolumeXIcon /> : <Volume2Icon />}
        label="Volume"
        value={volume}
        min={0}
        max={100}
        display={`${Math.round(volume)}%`}
        onChange={(v) => {
          void player.control({ type: "setVolume", volume: v })
          if (state.muted) void player.control({ type: "setMute", muted: false })
        }}
      />

      <SectionLabel>Playback speed</SectionLabel>
      <SliderRow
        icon={<GaugeIcon />}
        label="Playback speed"
        value={speedIndex}
        min={0}
        max={SPEEDS.length - 1}
        display={`${state.speed}×`}
        onChange={(i) => void player.control({ type: "setSpeed", speed: SPEEDS[i] })}
      />

      {audio.length > 1 && (
        <>
          {/* "Audio track" rather than "Audio": the tracks inside this stream, not the sub/dub switch. */}
          <SectionLabel>Audio track</SectionLabel>
          {audio.map((t) => (
            <ChoiceRow
              key={t.id}
              title={trackLabel(t)}
              selected={t.selected}
              onSelect={() => void player.control({ type: "selectTrack", kind: "audio", id: String(t.id) })}
            />
          ))}
        </>
      )}

      {settings && (
        <>
          <SectionLabel>Screen size</SectionLabel>
          <Segmented options={SCALES} selected={settings.playerContentScale} onSelect={(v) => update({ playerContentScale: v })} />

          <SectionLabel>Playback</SectionLabel>
          <SwitchRow title="Auto-play next episode" checked={settings.autoplayNext} onChange={(autoplayNext) => update({ autoplayNext })} />
          <SwitchRow title="Auto-skip intro and recap" checked={settings.autoSkipIntro} onChange={(autoSkipIntro) => update({ autoSkipIntro })} />
          <SwitchRow title="Auto-skip outro" checked={settings.autoSkipOutro} onChange={(autoSkipOutro) => update({ autoSkipOutro })} />
          <SwitchRow
            title="Pause details"
            description="The episode's billing over a long pause"
            checked={settings.pauseDetailsOverlay}
            onChange={(pauseDetailsOverlay) => update({ pauseDetailsOverlay })}
          />
          <SwitchRow title="Clock and finishing time" checked={settings.playerClock} onChange={(playerClock) => update({ playerClock })} />
        </>
      )}
    </PanelFrame>
  )
}
