// "Download episodes": Android's season download dialog (SeasonDownloadDialog.kt) for the title
// page. Audio, the first-choice server, "Hard subs only" (remembered), a quality ceiling (with a
// check of what that server really offers, said before the download rather than discovered after
// it), which episodes, and whether they fit on the disk. Episodes already saved or queued in the chosen audio are skipped.

import { useNavigate } from "@tanstack/react-router"
import { useQuery } from "@tanstack/react-query"
import { CheckIcon, FolderIcon, TriangleAlertIcon } from "lucide-react"
import { useEffect, useMemo, useRef, useState } from "react"
import { toast } from "sonner"

import { Alert, AlertDescription } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldContent, FieldDescription, FieldLabel } from "@/components/ui/field"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, useDownloads, useQueueState, useSaveSettings, useSettings } from "@/lib/api"
import {
  QUALITIES,
  estimatedEpisodeBytes,
  formatBytes,
  isActive,
  qualityHeight,
  spaceVerdict,
} from "@/lib/downloads"
import { toastFailure } from "@/lib/failure"
import { episodeLabel } from "@/lib/format"
import { MIRURO_PROVIDERS, providerLabel } from "@/lib/providers"
import type { AnimeDetail, Audio, DownloadQuality, Episode, Progress } from "@/lib/types"
import { cn } from "@/lib/utils"

const AUTO = "auto"
/** Long series are picked a block at a time, as the episode list pages them. */
const BLOCK = 100

export function DownloadDialog({
  open,
  onOpenChange,
  anime,
  episodes,
  dubCount,
  progress,
  resume,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  anime: AnimeDetail
  episodes: Episode[]
  dubCount: number | null
  progress: Progress[]
  /** The episode the page offers to play next: where a long series' preselection starts. */
  resume: number
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex max-h-[88vh] flex-col gap-0 p-0 sm:max-w-2xl">
        {/* Mounted per opening, so every visit starts from the current settings. */}
        {open && (
          <DownloadForm
            anime={anime}
            episodes={episodes}
            dubCount={dubCount}
            progress={progress}
            resume={resume}
            onDone={() => onOpenChange(false)}
          />
        )}
      </DialogContent>
    </Dialog>
  )
}

function DownloadForm({
  anime,
  episodes,
  dubCount,
  progress,
  resume,
  onDone,
}: {
  anime: AnimeDetail
  episodes: Episode[]
  dubCount: number | null
  progress: Progress[]
  resume: number
  onDone: () => void
}) {
  const navigate = useNavigate()
  const settings = useSettings().data
  const saveSettings = useSaveSettings()
  const downloads = useDownloads().data
  const queue = useQueueState().data
  const extras = useQuery({ queryKey: ["providers"], queryFn: api.providers }).data ?? []

  const [audio, setAudio] = useState<Audio>(() => (settings?.audio === "dub" && dubCount !== 0 ? "dub" : "sub"))
  const [provider, setProvider] = useState<string>(() => settings?.preferredProvider ?? AUTO)
  const [quality, setQuality] = useState<DownloadQuality>(() => settings?.downloadQuality ?? "best")
  const [matchQuality, setMatchQuality] = useState(false)
  const [hardSubsOnly, setHardSubsOnly] = useState(() => settings?.downloadHardSubs ?? false)
  // Dub has no subtitles to burn in.
  const hardSubs = hardSubsOnly && audio === "sub"
  const changeHardSubs = (on: boolean) => {
    setHardSubsOnly(on)
    if (settings) saveSettings.mutate({ ...settings, downloadHardSubs: on }, { onError: (e) => toastFailure(e) })
  }
  const [skipFillers, setSkipFillers] = useState(false)
  const [busy, setBusy] = useState(false)

  // Saved, or on its way, in the audio chosen: not offered again.
  const held = useMemo(
    () =>
      new Set(
        (downloads ?? [])
          .filter((d) => d.animeId === anime.id && d.audio === audio && (d.status === "done" || isActive(d)))
          .map((d) => d.episode),
      ),
    [downloads, anime.id, audio],
  )
  const watched = useMemo(
    () => new Set(progress.filter((p) => p.duration > 0 && p.position >= p.duration * 0.9).map((p) => p.episode)),
    [progress],
  )
  const noDub = (e: Episode) => audio === "dub" && dubCount != null && e.number > dubCount
  const available = (e: Episode) => !held.has(e.number) && !noDub(e) && !(skipFillers && e.filler)

  // Everything not yet saved when the list is short; a long series starts at the block holding the
  // episode to watch next, so "all" never quietly means a thousand episodes.
  const [selected, setSelected] = useState<Set<number>>(() => {
    const startAt = episodes.length > BLOCK ? Math.max(0, episodes.findIndex((e) => e.number >= resume)) : 0
    const block = episodes.length > BLOCK ? episodes.slice(startAt, startAt + BLOCK) : episodes
    return new Set(block.filter((e) => !held.has(e.number) && !noDub(e)).map((e) => e.number))
  })
  const chosen = episodes.filter((e) => selected.has(e.number) && available(e))
  const selectable = episodes.filter(available)
  const fillerCount = episodes.filter((e) => e.filler).length
  const heldCount = episodes.filter((e) => held.has(e.number)).length

  const maxHeight = qualityHeight(quality)
  const perEpisode = estimatedEpisodeBytes(maxHeight)
  const needed = perEpisode * chosen.length
  const verdict = spaceVerdict(needed, queue?.freeBytes ?? null, queue?.headroomBytes ?? 0)

  const providers = [
    { label: "Auto", value: AUTO },
    // A soft-sub-only server as first choice would be passed over on every episode.
    ...MIRURO_PROVIDERS.filter((p) => !(hardSubs && p.softSubOnly)),
    ...extras.map((p) => ({ label: p.name, value: p.id })),
  ]
  const serverName = provider === AUTO ? "Auto" : providerLabel(provider, extras)

  const confirm = () => {
    setBusy(true)
    void api
      .download(
        anime,
        chosen.map((e) => e.number),
        audio,
        { provider: provider === AUTO ? null : provider, maxHeight, matchQuality: maxHeight != null && matchQuality, hardSubs },
      )
      .then((added) => {
        onDone()
        toast.success(added > 0 ? `Queued ${added} episode${added === 1 ? "" : "s"}` : "Already downloaded or queued", {
          description: "Fetched one at a time; you can keep using the app.",
          action: { label: "View", onClick: () => void navigate({ to: "/downloads" }) },
        })
      })
      .catch((e) => toastFailure(e))
      .finally(() => setBusy(false))
  }

  return (
    <>
      <DialogHeader className="flex-row items-center gap-4 border-b p-4">
        {anime.cover && <img src={anime.cover} alt="" className="aspect-[2/3] w-14 shrink-0 rounded-md object-cover" />}
        <div className="flex min-w-0 flex-col gap-1">
          <span className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Download episodes</span>
          <DialogTitle className="truncate">{anime.title}</DialogTitle>
          <DialogDescription>
            {episodes.length} episode{episodes.length === 1 ? "" : "s"}
            {heldCount > 0 && ` · ${heldCount} already saved or queued`}
          </DialogDescription>
        </div>
      </DialogHeader>

      <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto p-4">
        <Section title="Audio">
          <ToggleGroup variant="outline" value={[audio]} onValueChange={(v) => v[0] && setAudio(v[0] as Audio)}>
            <ToggleGroupItem value="sub">Sub</ToggleGroupItem>
            <ToggleGroupItem value="dub" disabled={dubCount === 0}>
              Dub{dubCount != null && dubCount > 0 && dubCount < episodes.length ? ` (1–${dubCount})` : ""}
            </ToggleGroupItem>
          </ToggleGroup>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>First-choice server</FieldLabel>
              <FieldDescription>Tried first for each episode. If it fails, another may be used for the same audio.</FieldDescription>
            </FieldContent>
            <Select items={providers} value={provider} onValueChange={(v) => setProvider((v as string | null) ?? AUTO)}>
              <SelectTrigger className="w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {providers.map((p) => (
                    <SelectItem key={p.value} value={p.value}>
                      {p.label}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Field>
          {audio === "sub" && (
            <Field orientation="horizontal">
              <FieldContent>
                <FieldLabel htmlFor="hard-subs">Hard subs only</FieldLabel>
                <FieldDescription>
                  Subtitles burned into the picture, so they show in any player. Soft-sub servers are skipped, and an episode no
                  hard-sub server has fails.
                </FieldDescription>
              </FieldContent>
              <Switch id="hard-subs" checked={hardSubsOnly} onCheckedChange={changeHardSubs} />
            </Field>
          )}
        </Section>

        <Section title="Quality" description="The best rendition at or below the resolution chosen.">
          <ToggleGroup variant="outline" value={[quality]} onValueChange={(v) => v[0] && setQuality(v[0] as DownloadQuality)}>
            {QUALITIES.map((q) => (
              <ToggleGroupItem key={q.value} value={q.value}>
                {q.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
          {maxHeight != null && chosen[0] && (
            <QualityNote
              animeId={anime.id}
              episode={chosen[0].number}
              audio={audio}
              provider={provider === AUTO ? null : provider}
              maxHeight={maxHeight}
              hardSubs={hardSubs}
              quality={QUALITIES.find((q) => q.value === quality)?.label ?? quality}
              serverName={serverName}
              onUseProvider={setProvider}
            />
          )}
          {maxHeight != null && (
            <Field orientation="horizontal">
              <FieldContent>
                <FieldLabel htmlFor="match-quality">Switch servers per episode</FieldLabel>
                <FieldDescription>
                  When an episode's server has nothing at {maxHeight}p or below, try others for that episode. Slower, and the
                  subtitle style may differ.
                </FieldDescription>
              </FieldContent>
              <Switch id="match-quality" checked={matchQuality} onCheckedChange={setMatchQuality} />
            </Field>
          )}
        </Section>

        <Section title="Episodes">
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel htmlFor="skip-fillers">Skip filler episodes</FieldLabel>
              <FieldDescription>{fillerCount === 0 ? "None of these are marked as filler." : `${fillerCount} marked as filler.`}</FieldDescription>
            </FieldContent>
            <Switch id="skip-fillers" checked={skipFillers} disabled={fillerCount === 0} onCheckedChange={setSkipFillers} />
          </Field>
          <EpisodePicker
            episodes={episodes}
            selected={selected}
            onChange={setSelected}
            available={available}
            held={held}
            watched={watched}
            noDub={noDub}
            resume={resume}
          />
          <p className="text-xs text-muted-foreground">
            {chosen.length} of {selectable.length} selected · fetched one at a time; you can keep using the app while they run.
          </p>
        </Section>

        <Section title="Storage">
          <StorageGauge needed={needed} count={chosen.length} free={queue?.freeBytes ?? null} verdict={verdict} />
          {verdict === "wontFit" && (
            <Alert variant="destructive">
              <TriangleAlertIcon />
              <AlertDescription>Not enough room. Choose fewer episodes or a lower quality, or free some space.</AlertDescription>
            </Alert>
          )}
          {verdict === "tight" && (
            <Alert>
              <TriangleAlertIcon />
              <AlertDescription>The disk is nearly full: the downloads may stop part-way. The queue pauses itself before it runs out.</AlertDescription>
            </Alert>
          )}
          {queue && (
            <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <FolderIcon className="size-3.5 shrink-0" />
              <span className="truncate">Saved as files in {queue.folder}</span>
            </p>
          )}
        </Section>
      </div>

      <DialogFooter className="m-0 border-t p-4">
        <Button variant="outline" onClick={onDone}>
          Cancel
        </Button>
        <Button disabled={chosen.length === 0 || verdict === "wontFit" || busy} onClick={confirm}>
          {busy && <Spinner data-icon="inline-start" />}
          {verdict === "wontFit"
            ? "Not enough space"
            : chosen.length === 0
              ? "Choose episodes"
              : verdict === "tight"
                ? "Download anyway"
                : `Download ${chosen.length} episode${chosen.length === 1 ? "" : "s"}`}
        </Button>
      </DialogFooter>
    </>
  )
}

function Section({ title, description, children }: { title: string; description?: string; children: React.ReactNode }) {
  return (
    <section className="flex flex-col gap-3">
      <div className="flex flex-col gap-0.5">
        <h3 className="text-xs font-semibold tracking-wide text-muted-foreground uppercase">{title}</h3>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      {children}
    </section>
  )
}

/** A value that settles: stepping through the quality tabs would otherwise check a server per tab. */
function useSettled<T>(value: T, ms: number): T {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms)
    return () => clearTimeout(timer)
  }, [value, ms])
  return settled
}

/** What the first-choice server really offers for the first selected episode (Android's
 * `BatchQualityCheck`): most servers publish one rendition, and a ceiling can do nothing with one. */
function QualityNote({
  animeId,
  episode,
  audio,
  provider,
  maxHeight,
  hardSubs,
  quality,
  serverName,
  onUseProvider,
}: {
  animeId: number
  episode: number
  audio: Audio
  provider: string | null
  maxHeight: number
  hardSubs: boolean
  quality: string
  serverName: string
  onUseProvider: (provider: string) => void
}) {
  const request = useSettled(JSON.stringify({ animeId, episode, audio, provider, maxHeight, hardSubs }), 600)
  const check = useQuery({
    queryKey: ["download-check", request],
    queryFn: () => api.downloadCheck(JSON.parse(request)),
    staleTime: 5 * 60_000,
    retry: false,
  })
  const label = (n: number) => (Number.isInteger(n) ? n : n.toFixed(1))
  if (check.isPending || request !== JSON.stringify({ animeId, episode, audio, provider, maxHeight, hardSubs })) {
    return (
      <p className="flex items-center gap-2 text-xs text-muted-foreground">
        <Spinner className="size-3.5" /> Checking what {serverName === "Auto" ? "the server" : serverName} offers…
      </p>
    )
  }
  const result = check.data
  if (!result || result.fits == null) return null
  const name = result.server
  if (result.fits) {
    return (
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <CheckIcon className="size-3.5 text-primary" />
        {name} will download at {result.delivered}p (checked on episode {label(episode)}).
      </p>
    )
  }
  const ladder = result.offered.map((h) => `${h}p`).join(" / ")
  return (
    <div className="flex flex-col gap-2">
      <Alert>
        <TriangleAlertIcon />
        <AlertDescription>
          {result.offered.length === 1
            ? `${name} only has ${ladder} for episode ${label(episode)}, so it would download at ${ladder}.`
            : `${name} has ${ladder} for episode ${label(episode)}: no ${quality}, so it would download at ${result.delivered}p.`}
        </AlertDescription>
      </Alert>
      {result.alternative ? (
        <div className="flex items-center justify-between gap-3 rounded-lg border px-3 py-2 text-sm">
          <span>
            Use <span className="font-medium">{result.alternative.server}</span> instead
            <span className="text-muted-foreground"> · has {result.alternative.height}p for episode {label(episode)}</span>
          </span>
          <Button size="sm" variant="secondary" onClick={() => onUseProvider(result.alternative!.provider)}>
            Switch
          </Button>
        </div>
      ) : (
        <p className="text-xs text-muted-foreground">No other server checked has {quality} for this episode.</p>
      )}
    </div>
  )
}

/** Episodes as tiles to tick. Click toggles one; Shift+click ticks or clears the run since the
 * last one clicked. Saved ones show a tick and can't be picked again. */
function EpisodePicker({
  episodes,
  selected,
  onChange,
  available,
  held,
  watched,
  noDub,
  resume,
}: {
  episodes: Episode[]
  selected: Set<number>
  onChange: (next: Set<number>) => void
  available: (e: Episode) => boolean
  held: Set<number>
  watched: Set<number>
  noDub: (e: Episode) => boolean
  resume: number
}) {
  const last = useRef<number | null>(null)
  const blocks = Math.ceil(episodes.length / BLOCK)
  const [block, setBlock] = useState(() => (blocks > 1 ? Math.max(0, Math.floor(episodes.findIndex((e) => e.number >= resume) / BLOCK)) : 0))
  const shown = blocks > 1 ? episodes.slice(block * BLOCK, (block + 1) * BLOCK) : episodes
  const label = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(1))

  const set = (list: Episode[], on: boolean) => {
    const next = new Set(selected)
    for (const e of list) {
      if (!available(e)) continue
      if (on) next.add(e.number)
      else next.delete(e.number)
    }
    onChange(next)
  }
  const shownAvailable = shown.filter(available)
  const allShown = shownAvailable.length > 0 && shownAvailable.every((e) => selected.has(e.number))

  return (
    <div className="flex flex-col gap-2">
      {blocks > 1 && (
        <div className="flex flex-wrap gap-1">
          {Array.from({ length: blocks }, (_, i) => {
            const part = episodes.slice(i * BLOCK, (i + 1) * BLOCK)
            const picked = part.filter((e) => selected.has(e.number) && available(e)).length
            return (
              <Button key={i} size="xs" variant={i === block ? "secondary" : "ghost"} onClick={() => setBlock(i)}>
                {label(part[0].number)}–{label(part[part.length - 1].number)}
                {picked > 0 && <Badge className="ml-1 h-4 px-1 text-[10px]">{picked}</Badge>}
              </Button>
            )
          })}
        </div>
      )}
      <div className="flex flex-wrap gap-1">
        <Button size="xs" variant="outline" onClick={() => set(shown, !allShown)}>
          {allShown ? "Clear these" : `Select ${blocks > 1 ? "these" : "all"} ${shownAvailable.length}`}
        </Button>
        <Button size="xs" variant="outline" onClick={() => set(shown.filter((e) => !watched.has(e.number)), true)}>
          Unwatched
        </Button>
        <Button size="xs" variant="outline" onClick={() => set(shown.filter((e) => e.number >= resume), true)}>
          From {episodeLabel(resume).toLowerCase()}
        </Button>
        {selected.size > 0 && (
          <Button size="xs" variant="ghost" onClick={() => onChange(new Set())}>
            Clear all
          </Button>
        )}
      </div>
      <div className="grid grid-cols-[repeat(auto-fill,minmax(3.25rem,1fr))] gap-1">
        {shown.map((e, index) => {
          const saved = held.has(e.number)
          const can = available(e)
          const on = can && selected.has(e.number)
          const why = saved ? "Saved or queued" : noDub(e) ? "No dub yet" : !can ? "Filler, skipped" : null
          return (
            <Tooltip key={e.number}>
              <TooltipTrigger
                render={
                  <button
                    type="button"
                    aria-pressed={on}
                    disabled={!can}
                    onClick={(event) => {
                      const at = block * BLOCK + index
                      if (event.shiftKey && last.current != null) {
                        const [from, to] = [Math.min(last.current, at), Math.max(last.current, at)]
                        set(episodes.slice(from, to + 1), !on)
                      } else {
                        set([e], !on)
                      }
                      last.current = at
                    }}
                    className={cn(
                      "relative flex h-9 items-center justify-center rounded-md border text-xs font-medium tabular-nums transition outline-none select-none focus-visible:ring-2 focus-visible:ring-ring",
                      on ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
                      !can && "cursor-not-allowed opacity-45",
                      e.filler && "border-dashed",
                    )}
                  />
                }
              >
                {saved && <CheckIcon className="mr-0.5 size-3" />}
                {label(e.number)}
              </TooltipTrigger>
              <TooltipContent>
                {e.title ?? episodeLabel(e.number)}
                {e.filler ? " · Filler" : ""}
                {watched.has(e.number) ? " · Watched" : ""}
                {why ? ` · ${why}` : ""}
              </TooltipContent>
            </Tooltip>
          )
        })}
      </div>
    </div>
  )
}

/** The size of the selection against the room on the download disk (Android's storage strip). */
function StorageGauge({
  needed,
  count,
  free,
  verdict,
}: {
  needed: number
  count: number
  free: number | null
  verdict: "fits" | "tight" | "wontFit"
}) {
  const fraction = free && free > 0 ? Math.min(1, needed / free) : 0
  return (
    <div className="flex flex-col gap-2 rounded-lg border p-3">
      <div className="flex items-baseline justify-between gap-3 text-sm">
        <span>
          <span className="font-semibold">≈ {formatBytes(needed)}</span>
          <span className="text-muted-foreground"> · {count === 1 ? "1 episode" : `${count} episodes`}</span>
        </span>
        <span className="text-xs text-muted-foreground">
          {free == null
            ? "Free space unreadable"
            : verdict === "wontFit"
              ? `${formatBytes(needed - free)} short`
              : `${formatBytes(free)} free · ${formatBytes(free - needed)} after`}
        </span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-muted">
        <div
          className={cn("h-full rounded-full transition-all", verdict === "wontFit" ? "bg-destructive" : "bg-primary")}
          style={{ width: `${Math.max(fraction > 0 ? 2 : 0, fraction * 100)}%` }}
        />
      </div>
    </div>
  )
}
