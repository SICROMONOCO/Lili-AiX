// The player's Sources panel, as Android's `PlayerSourcesSheet` in its side-panel frame: the sub/dub
// switch (Android's "Audio" segmented selector), then a flat row per server for that audio, the one
// playing first, each with Android's second line (`serverDescription`), then the embedded players.

import { ChevronRightIcon } from "lucide-react"

import { ChoiceRow, PanelFrame, SectionLabel, Segmented } from "@/components/player/player-panel"
import { languageName, serverLanguages, visibleLanguages } from "@/lib/server-languages"
import type { Audio, Server } from "@/lib/types"

/** How a server carries the episode, as a person would say it (Android's `spokenLabel`). */
function spokenLabel(server: Server): string {
  if (server.audio === "dub") return "Dubbed"
  if (server.track === "ssub") return "Subtitles you can switch"
  if (server.track === "sub") return "Subtitles in the picture"
  return "Subtitled"
}

/** The second line of a server row: what a viewer chooses a server by. */
function serverDescription(server: Server, playing: boolean, preferredLanguages: string[]): string {
  const quality = server.streams.find((s) => s.height)?.height
  const languages = visibleLanguages(serverLanguages(server), preferredLanguages).map(languageName)
  return [playing ? "Playing now" : null, spokenLabel(server), quality ? `${quality}p` : null, languages.join(", ") || null]
    .filter(Boolean)
    .join(" · ")
}

export function SourcesPanel({
  current,
  native,
  embeds,
  preferredLanguages,
  onAudio,
  onServer,
  onEmbed,
  onClose,
}: {
  current: Server
  /** Servers mpv can play (any audio). */
  native: Server[]
  embeds: Server[]
  /** Settings → Servers → Server languages: the languages a row names; empty names all. */
  preferredLanguages: string[]
  onAudio: (audio: Audio) => void
  onServer: (serverId: string) => void
  onEmbed: (server: Server) => void
  onClose: () => void
}) {
  const hasDub = native.some((s) => s.audio === "dub")
  const hasSub = native.some((s) => s.audio !== "dub")
  // Playing first, as in Android's pickers, so the panel opens on what is on screen.
  const servers = native.filter((s) => s.audio === current.audio).sort((a, b) => Number(b.id === current.id) - Number(a.id === current.id))
  // The same player listed twice says nothing; keep the first. This audio's players before the other's.
  const players = embeds
    .filter((s, i) => embeds.findIndex((o) => o.embed === s.embed) === i)
    .sort((a, b) => Number(b.audio === current.audio) - Number(a.audio === current.audio))
  const choose = (run: () => void) => {
    run()
    onClose()
  }

  return (
    <PanelFrame title="Sources" onClose={onClose}>
      {hasSub && hasDub && (
        <>
          <SectionLabel>Audio</SectionLabel>
          <Segmented
            options={[
              { value: "sub", label: "Sub" },
              { value: "dub", label: "Dub" },
            ]}
            selected={current.audio === "dub" ? "dub" : "sub"}
            onSelect={(audio) => audio !== current.audio && choose(() => onAudio(audio))}
          />
        </>
      )}

      <SectionLabel>Server</SectionLabel>
      {servers.map((s) => (
        <ChoiceRow
          key={s.id}
          title={`${s.label} · ${s.name}`}
          subtitle={serverDescription(s, s.id === current.id, preferredLanguages)}
          selected={s.id === current.id}
          onSelect={() => s.id !== current.id && choose(() => onServer(s.id))}
        />
      ))}

      {players.length > 0 && (
        <>
          <SectionLabel>Embedded players</SectionLabel>
          {players.map((s) => (
            <ChoiceRow
              key={s.id}
              title={`${s.label} · ${s.name}`}
              subtitle={`${s.audio === "dub" ? "Dubbed" : "Subtitled"} · The site's own player`}
              onSelect={() => choose(() => onEmbed(s))}
              indicator={<ChevronRightIcon className="size-4 text-white/50" />}
            />
          ))}
        </>
      )}
    </PanelFrame>
  )
}
