// Settings as the television lays them out (Android's TvSettingsTwoPane): the categories down a rail
// on the left, with search above them, and the open category's rows beside it. A narrow window
// shows one pane at a time, as a phone does: the rail, then the category with a way back.

import { getVersion } from "@tauri-apps/api/app"
import { listen } from "@tauri-apps/api/event"
import { ArrowLeftIcon, ChevronRightIcon, ExternalLinkIcon, HeartIcon, LogOutIcon, SearchIcon, XIcon } from "lucide-react"
import { useEffect, useRef, useState } from "react"
import { toast } from "sonner"

import mark from "@/assets/anilili-mark.svg"
import { DiscordGlyph } from "@/components/layout/app-context-menu"
import { ShortcutList } from "@/components/layout/shortcuts-dialog"
import { openUpdate } from "@/components/layout/update-dialog"
import { AppearanceSettings, LayoutSettings } from "@/components/settings/appearance"
import { Backup } from "@/components/settings/backup"
import { searchSettings } from "@/components/settings/search-index"
import { ServersSettings } from "@/components/settings/servers"
import { SubtitlesSettings } from "@/components/settings/subtitles"
import { SECTIONS, type SectionId, SettingsGroup, sectionFor } from "@/components/settings/section"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { Button } from "@/components/ui/button"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel, FieldSeparator } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Skeleton } from "@/components/ui/skeleton"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { useQueryClient } from "@tanstack/react-query"

import { api, keys, useMalUser, useQueueState, useSaveSettings, useSettings, useSimklAccount, useUpdateCheck, useViewer } from "@/lib/api"
import { QUALITIES, formatBytes } from "@/lib/downloads"
import type { DownloadQuality, Settings, SimklCode } from "@/lib/types"
import { errorMessage } from "@/lib/types"
import { saveDiagnostics, toastFailure } from "@/lib/failure"
import { cn } from "@/lib/utils"

/** Android's choices, under its stored values. */
const SKIP_LENGTHS = [30, 60, 85, 90, 120].map((n) => ({ value: String(n), label: `${n} seconds` }))
const BUFFER_SIZES = [
  { value: "auto", label: "Automatic", description: "about 2 minutes" },
  { value: "small", label: "Small", description: "about 15 seconds" },
  { value: "medium", label: "Medium", description: "about 40 seconds" },
  { value: "large", label: "Large", description: "about 90 seconds" },
  { value: "very_large", label: "Very large", description: "up to 4 minutes" },
]
const VIDEO_CACHE_SIZES = [
  { value: "auto", label: "Automatic" },
  { value: "off", label: "Off" },
  { value: "256", label: "256 MB" },
  { value: "512", label: "512 MB" },
  { value: "1024", label: "1 GB" },
  { value: "2048", label: "2 GB" },
]

/** The classes that mark a row a search result led to, for a moment. */
const SPOTLIGHT = ["outline-2", "outline-offset-8", "outline-primary"]

type Update = (patch: Partial<Settings>) => void

/** `focus` (from `?section=`) opens that category. */
export function SettingsPage({ focus }: { focus?: string }) {
  const settings = useSettings()
  const save = useSaveSettings()
  const [section, setSection] = useState<SectionId>(() => sectionFor(focus) ?? "player")
  // Narrow windows only: whether the open category is showing rather than the rail.
  const [opened, setOpened] = useState(() => sectionFor(focus) != null)
  const [query, setQuery] = useState("")
  const [spotlight, setSpotlight] = useState<{ title: string } | null>(null)
  const content = useRef<HTMLDivElement>(null)
  // The launch check's answer, if it ran: an update waiting badges App, as Android's rail does.
  const updateReady = !!useUpdateCheck(false).data?.update

  // A search result opens its category, then the row is brought into view and outlined briefly.
  useEffect(() => {
    if (!spotlight || !content.current) return
    const labels = content.current.querySelectorAll<HTMLElement>('[data-slot="field-label"], h3, [data-search-title]')
    // A `data-search-title` with a value names its row where the text says more ("Version 1.4.0").
    const label = [...labels].find((el) => (el.dataset.searchTitle || el.textContent?.trim()) === spotlight.title)
    const target = label?.closest<HTMLElement>('[data-slot="field"], [data-search-row], section') ?? label
    if (!target) return
    // A group keeps its own corners; a row has none, so it borrows some for the outline.
    const marks = target.tagName === "SECTION" ? SPOTLIGHT : [...SPOTLIGHT, "rounded-md"]
    target.scrollIntoView({ block: "center", behavior: "smooth" })
    target.classList.add(...marks)
    const timer = window.setTimeout(() => target.classList.remove(...marks), 2000)
    return () => {
      window.clearTimeout(timer)
      target.classList.remove(...marks)
    }
  }, [spotlight])

  if (!settings.data) return <Skeleton className="m-8 h-96" />
  const s = settings.data
  const update: Update = (patch) => save.mutate({ ...s, ...patch }, { onError: (e) => toastFailure(e) })

  const open = (id: SectionId, row?: string) => {
    setSection(id)
    setOpened(true)
    setQuery("")
    setSpotlight(row ? { title: row } : null)
    if (!row) document.querySelector('[data-slot="app-main"]')?.scrollTo({ top: 0 })
  }
  const current = SECTIONS.find((x) => x.id === section) ?? SECTIONS[0]
  const results = searchSettings(query, (id) => SECTIONS.find((x) => x.id === id)?.title ?? "")

  return (
    <div className="mx-auto grid w-full max-w-6xl grid-cols-1 items-start gap-8 p-8 lg:grid-cols-[17rem_minmax(0,1fr)]">
      <nav className={cn("flex flex-col gap-1.5 lg:sticky lg:top-8", opened && !query && "max-lg:hidden")} aria-label="Settings categories">
        <h1 className="px-1 pb-2 font-heading text-2xl font-bold">Settings</h1>
        <InputGroup className="mb-2">
          <InputGroupAddon>
            <SearchIcon />
          </InputGroupAddon>
          <InputGroupInput
            // Opened plainly (not at a category), typing goes straight to search.
            autoFocus={!focus}
            placeholder="Search settings"
            aria-label="Search settings"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") setQuery("")
              if (e.key === "Enter" && results[0]) open(results[0].section, results[0].title)
            }}
          />
          {query && (
            <InputGroupAddon align="inline-end">
              <InputGroupButton size="icon-xs" aria-label="Clear" onClick={() => setQuery("")}>
                <XIcon />
              </InputGroupButton>
            </InputGroupAddon>
          )}
        </InputGroup>
        <div className={cn("flex flex-col gap-1.5", query && "max-lg:hidden")}>
          {SECTIONS.map((x) => (
            <RailRow
              key={x.id}
              icon={x.icon}
              title={x.title}
              selected={x.id === section}
              badged={x.id === "app" && updateReady}
              onClick={() => open(x.id)}
            />
          ))}
        </div>
      </nav>

      <div ref={content} className={cn("flex min-w-0 flex-col gap-4", !opened && !query && "max-lg:hidden")}>
        {query ? (
          <SearchResults query={query} results={results} onOpen={open} />
        ) : (
          <>
            <header className="flex items-start gap-2 pt-1 pb-1">
              <Button variant="ghost" size="icon" className="-ml-2 lg:hidden" aria-label="All settings" onClick={() => setOpened(false)}>
                <ArrowLeftIcon />
              </Button>
              <div className="flex flex-col gap-0.5">
                <h2 className="font-heading text-2xl font-bold">{current.title}</h2>
                <p className="text-sm text-muted-foreground">{current.summary}</p>
              </div>
            </header>
            <SectionBody id={section} s={s} update={update} />
          </>
        )}
      </div>
    </div>
  )
}

/** One category in the rail: glyph, name, chevron, as the television's TvSettingsRailRow. */
function RailRow({
  icon: Icon,
  title,
  selected,
  badged,
  onClick,
}: {
  icon: (typeof SECTIONS)[number]["icon"]
  title: string
  selected: boolean
  badged: boolean
  onClick: () => void
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={selected ? "page" : undefined}
      className={cn(
        "flex items-center gap-3 rounded-xl border px-4 py-3 text-left text-sm transition outline-none focus-visible:ring-2 focus-visible:ring-ring",
        selected ? "border-primary/40 bg-primary/15 font-bold" : "surface-control font-semibold hover:bg-muted",
      )}
    >
      <span className="relative">
        <Icon className="size-5" />
        {badged && (
          <span className="absolute -top-1.5 -right-1.5 flex size-3.5 items-center justify-center rounded-full bg-primary text-[0.6rem] leading-none font-bold text-primary-foreground">
            1
          </span>
        )}
      </span>
      <span className="min-w-0 flex-1 truncate">{title}</span>
      <ChevronRightIcon className="size-4 text-muted-foreground" />
    </button>
  )
}

function SearchResults({
  query,
  results,
  onOpen,
}: {
  query: string
  results: ReturnType<typeof searchSettings>
  onOpen: (id: SectionId, row: string) => void
}) {
  return (
    <>
      <header className="flex flex-col gap-0.5 pt-1 pb-1">
        <h2 className="font-heading text-2xl font-bold">Search</h2>
        <p className="text-sm text-muted-foreground">
          {results.length === 0 ? `Nothing matches “${query.trim()}”` : `${results.length} setting${results.length === 1 ? "" : "s"} · Enter opens the first`}
        </p>
      </header>
      {results.length === 0 ? (
        <Empty className="border">
          <EmptyHeader>
            <EmptyTitle>No settings found</EmptyTitle>
            <EmptyDescription>Try another word: “dub”, “spoilers”, “glass”, “cache”.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        <div className="surface flex flex-col gap-0.5 rounded-2xl p-2">
          {results.map((r) => {
            const sec = SECTIONS.find((x) => x.id === r.section)
            const Icon = sec?.icon
            return (
              <button
                key={`${r.section}-${r.title}`}
                type="button"
                onClick={() => onOpen(r.section, r.title)}
                className="flex items-center gap-3 rounded-xl px-3 py-2.5 text-left transition outline-none hover:bg-muted focus-visible:bg-muted"
              >
                {Icon && <Icon className="size-4 shrink-0 text-muted-foreground" />}
                <span className="flex min-w-0 flex-1 flex-col">
                  <span className="truncate text-sm font-medium">{r.title}</span>
                  <span className="truncate text-xs text-muted-foreground">{sec?.title}</span>
                </span>
                <ChevronRightIcon className="size-4 shrink-0 text-muted-foreground" />
              </button>
            )
          })}
        </div>
      )}
    </>
  )
}

function SectionBody({ id, s, update }: { id: SectionId; s: Settings; update: Update }) {
  switch (id) {
    case "player":
      return <PlayerSection s={s} update={update} />
    case "subtitles":
      return <SubtitlesSettings s={s} update={update} />
    case "downloads":
      return <DownloadsSection s={s} update={update} />
    case "servers":
      return <ServersSettings s={s} update={update} />
    case "accounts":
      return <AccountsSection />
    case "notifications":
      return <NotificationsSection s={s} update={update} />
    case "content":
      return <ContentSection s={s} update={update} />
    case "appearance":
      return (
        <>
          <AppearanceSettings />
          <LayoutSettings />
        </>
      )
    case "data":
      return <DataSection s={s} update={update} />
    case "app":
      return (
        <AppSection
          checkOnLaunch={s.checkUpdatesOnLaunch}
          onCheckOnLaunch={(v) => update({ checkUpdatesOnLaunch: v })}
        />
      )
    case "about":
      return <AboutSection />
  }
}

// ---- categories -------------------------------------------------------------------------------

function PlayerSection({ s, update }: { s: Settings; update: Update }) {
  return (
    <>
      <SettingsGroup title="Playback">
        <FieldGroup>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>Audio</FieldLabel>
              <FieldDescription>Which version to start with. You can switch per episode.</FieldDescription>
            </FieldContent>
            <ToggleGroup variant="outline" value={[s.audio]} onValueChange={(v) => v[0] && update({ audio: v[0] as Settings["audio"] })}>
              <ToggleGroupItem value="sub">Subtitled</ToggleGroupItem>
              <ToggleGroupItem value="dub">Dubbed</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          <FieldSeparator />
          <SwitchField
            label="Resume where you left off"
            description="Start unfinished episodes from the saved position."
            checked={s.resume}
            onChange={(resume) => update({ resume })}
          />
          <SwitchField
            label="Autoplay next episode"
            description="Offer the next episode in the last half-minute, and play it after a short countdown."
            checked={s.autoplayNext}
            onChange={(autoplayNext) => update({ autoplayNext })}
          />
        </FieldGroup>
      </SettingsGroup>

      <SettingsGroup title="Skipping">
        <FieldGroup>
          <SwitchField
            label="Skip intros and recaps automatically"
            description="When the server, or AniSkip, knows the episode's opening and recap. The skip button counts down three seconds first."
            checked={s.autoSkipIntro}
            onChange={(autoSkipIntro) => update({ autoSkipIntro })}
          />
          <SwitchField
            label="Skip endings automatically"
            description="When the server, or AniSkip, knows the episode's ending."
            checked={s.autoSkipOutro}
            onChange={(autoSkipOutro) => update({ autoSkipOutro })}
          />
          <SwitchField
            label="Skip button"
            description="A one-press jump in the player when a show has no skip timings."
            checked={s.showSkipButton}
            onChange={(showSkipButton) => update({ showSkipButton })}
          />
          {s.showSkipButton && (
            <ChoiceField
              label="Skip button length"
              description="How far one press jumps. A TV anime opening is 85 seconds."
              value={String(s.skipButtonSeconds)}
              options={SKIP_LENGTHS}
              onChange={(v) => update({ skipButtonSeconds: Number(v) })}
            />
          )}
        </FieldGroup>
      </SettingsGroup>

      <SettingsGroup title="Picture">
        <FieldGroup>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>Screen size</FieldLabel>
              <FieldDescription>Fit shows the whole picture; Crop fills the window and cuts the edges; Fill stretches it.</FieldDescription>
            </FieldContent>
            <ToggleGroup
              variant="outline"
              value={[s.playerContentScale]}
              onValueChange={(v) => v[0] && update({ playerContentScale: v[0] as Settings["playerContentScale"] })}
            >
              <ToggleGroupItem value="fit">Fit</ToggleGroupItem>
              <ToggleGroupItem value="crop">Crop</ToggleGroupItem>
              <ToggleGroupItem value="fill">Fill</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          <FieldSeparator />
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>Upscaling (Anime4K)</FieldLabel>
              <FieldDescription>Sharper line art on big screens. Quality needs a capable GPU.</FieldDescription>
            </FieldContent>
            <ToggleGroup variant="outline" value={[s.upscaler]} onValueChange={(v) => v[0] && update({ upscaler: v[0] as Settings["upscaler"] })}>
              <ToggleGroupItem value="off">Off</ToggleGroupItem>
              <ToggleGroupItem value="fast">Fast</ToggleGroupItem>
              <ToggleGroupItem value="quality">Quality</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          <FieldSeparator />
          <SwitchField
            label="Pause details"
            description="After a long pause, the show's logo, the episode and its synopsis over the picture."
            checked={s.pauseDetailsOverlay}
            onChange={(pauseDetailsOverlay) => update({ pauseDetailsOverlay })}
          />
          <SwitchField
            label="Clock and finishing time"
            description="The time now and when the episode ends, in the player's top corner."
            checked={s.playerClock}
            onChange={(playerClock) => update({ playerClock })}
          />
        </FieldGroup>
      </SettingsGroup>

      <SettingsGroup title="Performance">
        <FieldGroup>
          <ChoiceField
            label="Buffer size"
            description="How far ahead the player downloads. Larger rides out an unsteady connection and waits longer after a stall before resuming, so one stall doesn't become a stutter. From the next episode."
            value={s.playbackBufferSize}
            options={BUFFER_SIZES}
            onChange={(v) => update({ playbackBufferSize: v as Settings["playbackBufferSize"] })}
          />
          <SwitchField
            label="Hardware decoding"
            description="Lower CPU use. Turn off if video shows artefacts or green frames."
            checked={s.hardwareDecoding}
            onChange={(hardwareDecoding) => update({ hardwareDecoding })}
          />
        </FieldGroup>
      </SettingsGroup>
    </>
  )
}

function DownloadsSection({ s, update }: { s: Settings; update: Update }) {
  return (
    <SettingsGroup>
      <FieldGroup>
        <DownloadFolder value={s.downloadDir} onChange={(downloadDir) => update({ downloadDir })} />
        <FieldSeparator />
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel>Quality</FieldLabel>
            <FieldDescription>Where the download dialog starts, and what an episode's own download button uses.</FieldDescription>
          </FieldContent>
          <ToggleGroup variant="outline" value={[s.downloadQuality]} onValueChange={(v) => v[0] && update({ downloadQuality: v[0] as DownloadQuality })}>
            {QUALITIES.map((q) => (
              <ToggleGroupItem key={q.value} value={q.value}>
                {q.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Field>
      </FieldGroup>
    </SettingsGroup>
  )
}

function AccountsSection() {
  const anilist = useViewer().data
  return (
    <>
      <SettingsGroup title="AniList" description="Sync progress and see your lists.">
        <Account />
      </SettingsGroup>
      <SettingsGroup
        title="MyAnimeList"
        description={`Sync progress and see your list.${anilist ? " While AniList is signed in, AniList is the one kept in sync." : ""}`}
      >
        <MalAccount />
      </SettingsGroup>
      <SettingsGroup
        title="Simkl"
        description="Also track what you watch on Simkl, beside AniList or MyAnimeList: finished episodes, library changes and statuses."
      >
        <SimklAccountRow />
      </SettingsGroup>
    </>
  )
}

function NotificationsSection({ s, update }: { s: Settings; update: Update }) {
  return (
    <SettingsGroup>
      <FieldGroup>
        <SwitchField
          label="New episode alerts"
          description="A Windows notification when an episode of a title in your library airs, and your AniList notifications as they arrive. Checked while the app is open."
          checked={s.releaseNotifications}
          onChange={(releaseNotifications) => update({ releaseNotifications })}
        />
        <SwitchField
          label="Finished downloads"
          description="One notification when a batch of downloads is done, saying what failed."
          checked={s.downloadNotifications}
          onChange={(downloadNotifications) => update({ downloadNotifications })}
        />
        <FieldSeparator />
        <Field orientation="horizontal">
          <FieldContent>
            <FieldLabel>Test</FieldLabel>
            <FieldDescription>Sends a sample. If nothing appears, Windows may have notifications off for Anilili, or Do not disturb is on.</FieldDescription>
          </FieldContent>
          <Button variant="outline" onClick={() => api.notificationTest().catch(toastFailure)}>
            Send a test
          </Button>
        </Field>
      </FieldGroup>
    </SettingsGroup>
  )
}

function ContentSection({ s, update }: { s: Settings; update: Update }) {
  return (
    <SettingsGroup>
      <FieldGroup>
        <SwitchField
          label="Show adult titles"
          description="Include 18+ titles on Home, in browse, search and the schedule."
          checked={s.showAdult}
          onChange={(showAdult) => update({ showAdult })}
        />
        <SwitchField
          label="Discord status"
          description="Show what you're watching on your Discord profile (when Discord is running)."
          checked={s.discordPresence}
          onChange={(discordPresence) => update({ discordPresence })}
        />
      </FieldGroup>
    </SettingsGroup>
  )
}

function DataSection({ s, update }: { s: Settings; update: Update }) {
  const client = useQueryClient()
  return (
    <>
      <SettingsGroup>
        <FieldGroup>
          <ChoiceField
            label="Video cache"
            description="Video already watched is kept on this computer's disk, so seeking back plays straight from it. Off keeps nothing; Automatic keeps a little in memory. From the next episode."
            value={s.videoCacheSize}
            options={VIDEO_CACHE_SIZES}
            onChange={(v) => update({ videoCacheSize: v as Settings["videoCacheSize"] })}
          />
          <FieldSeparator />
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>Clear cache</FieldLabel>
              <FieldDescription>Catalogue pages, episode lists and sources fetched before. They are fetched again as needed.</FieldDescription>
            </FieldContent>
            <Button
              variant="outline"
              onClick={() =>
                api
                  .clearCache()
                  .then(() => client.invalidateQueries())
                  .then(() => toast.success("Cache cleared"))
              }
            >
              Clear
            </Button>
          </Field>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>Clear watch history</FieldLabel>
              <FieldDescription>Where you stopped in every episode, and Continue watching. Your lists and the account stay.</FieldDescription>
            </FieldContent>
            <Button
              variant="outline"
              onClick={() =>
                api
                  .clearHistory(null)
                  .then(() => Promise.all([client.invalidateQueries({ queryKey: keys.continue }), client.invalidateQueries({ queryKey: ["progress"] })]))
                  .then(() => toast.success("Watch history cleared"))
              }
            >
              Clear
            </Button>
          </Field>
        </FieldGroup>
      </SettingsGroup>
      <Backup />
    </>
  )
}

/**
 * Android's App section: checking for updates (on launch, and now) and diagnostics. Android's
 * menu language has no counterpart here: the desktop is English only.
 */
function AppSection({ checkOnLaunch, onCheckOnLaunch }: { checkOnLaunch: boolean; onCheckOnLaunch: (v: boolean) => void }) {
  const check = useUpdateCheck(checkOnLaunch)
  const update = check.data?.update
  const version = useVersion()
  return (
    <>
      <SettingsGroup title="Updates">
        <FieldGroup>
          <SwitchField label="Check for updates on launch" description="Prompt when a new version is available" checked={checkOnLaunch} onChange={onCheckOnLaunch} />
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>{check.isFetching ? "Checking for updates…" : "Check for updates"}</FieldLabel>
              <FieldDescription>
                {check.isError
                  ? `Couldn't check: ${errorMessage(check.error)}`
                  : update
                    ? `Version ${update.version} is ready to download and install`
                    : check.data
                      ? "You're on the latest published version"
                      : `Installed v${version}`}
              </FieldDescription>
            </FieldContent>
            <span className="text-sm text-muted-foreground tabular-nums">v{version}</span>
            {update ? (
              <Button onClick={() => openUpdate(update)}>Update</Button>
            ) : (
              <Button variant="outline" disabled={check.isFetching} onClick={() => void check.refetch()}>
                Check now
              </Button>
            )}
          </Field>
        </FieldGroup>
      </SettingsGroup>
      <SettingsGroup title="Diagnostics">
        <FieldGroup>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>Send diagnostics</FieldLabel>
              <FieldDescription>
                Saves a redacted report (versions, settings, recent failures and the log) to Downloads, ready to send. Nothing is sent anywhere by
                itself.
              </FieldDescription>
            </FieldContent>
            <Button variant="outline" onClick={() => saveDiagnostics()}>
              Save report
            </Button>
          </Field>
          <Field orientation="horizontal">
            <FieldContent>
              <FieldLabel>Log folder</FieldLabel>
              <FieldDescription>The app's logs, if someone helping with a problem asks for them</FieldDescription>
            </FieldContent>
            <Button variant="outline" onClick={() => api.diagnosticsOpenLogs().catch(toastFailure)}>
              Open
            </Button>
          </Field>
        </FieldGroup>
      </SettingsGroup>
    </>
  )
}

/** Android's About: the Discord server and the sign-off, with the version and keyboard. */
function AboutSection() {
  const version = useVersion()
  return (
    <>
      <SettingsGroup>
        <div className="flex items-center gap-4">
          <img src={mark} alt="" className="size-14 shrink-0" />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="font-heading text-xl font-bold">Anilili Desktop</span>
            <span className="text-sm text-muted-foreground" data-search-title="Version">
              Version {version}
            </span>
          </div>
        </div>
        <div className="flex flex-col gap-2">
          <LinkRow
            icon={<DiscordGlyph className="size-[22px] shrink-0" />}
            title="Discord server"
            description="Questions, source problems and release news"
            onClick={() => api.openDiscord().catch((e) => toastFailure(e))}
          />
        </div>
        <p className="text-sm text-muted-foreground">
          Anilili is a one-person project: an anime app that finds an episode across dozens of sources, remembers where you stopped, and keeps your
          list in step with AniList or MyAnimeList. No adverts, and no account of its own.
        </p>
        <p className="flex items-center gap-2 text-sm font-bold">
          <HeartIcon className="size-4 text-primary" />
          Made with love by lili
        </p>
      </SettingsGroup>
      <SettingsGroup title="Keyboard shortcuts" description="Player shortcuts work while something is playing. Press ? anywhere for this list.">
        <ShortcutList className="sm:grid-cols-[auto_1fr_auto_1fr]" />
      </SettingsGroup>
    </>
  )
}

// ---- rows -------------------------------------------------------------------------------------

/** A row that opens a page in the browser (About's Discord server). */
function LinkRow({ icon, title, description, onClick }: { icon: React.ReactNode; title: string; description: string; onClick: () => void }) {
  return (
    <button
      type="button"
      data-search-row
      onClick={onClick}
      className="surface-control flex items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors outline-none hover:bg-muted focus-visible:bg-muted"
    >
      {icon}
      <span className="flex min-w-0 flex-1 flex-col">
        <span className="text-sm font-medium" data-search-title>
          {title}
        </span>
        <span className="text-sm text-muted-foreground">{description}</span>
      </span>
      <ExternalLinkIcon className="size-4 shrink-0 text-muted-foreground" />
    </button>
  )
}

/** A setting with a few named choices (Android's choice pickers). */
function ChoiceField({
  label,
  description,
  value,
  options,
  onChange,
}: {
  label: string
  description: string
  value: string
  options: { value: string; label: string; description?: string }[]
  onChange: (value: string) => void
}) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel>{label}</FieldLabel>
        <FieldDescription>{description}</FieldDescription>
      </FieldContent>
      <Select items={options} value={value} onValueChange={(v) => v != null && onChange(String(v))}>
        <SelectTrigger className="w-44 shrink-0">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {options.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.description ? `${o.label} · ${o.description}` : o.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}

function SwitchField({
  label,
  description,
  checked,
  onChange,
}: {
  label: string
  description: string
  checked: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel>{label}</FieldLabel>
        <FieldDescription>{description}</FieldDescription>
      </FieldContent>
      <Switch checked={checked} onCheckedChange={onChange} aria-label={label} />
    </Field>
  )
}

/** The download folder, typed in; empty means Videos\Anilili. Saved when the field is left. */
function DownloadFolder({ value, onChange }: { value: string | null; onChange: (dir: string | null) => void }) {
  const queue = useQueueState().data
  return (
    <Field>
      <FieldContent>
        <FieldLabel htmlFor="download-dir">Folder</FieldLabel>
        <FieldDescription>
          Each title gets a folder of its own here. Leave empty for Videos\Anilili.
          {queue?.freeBytes != null && ` ${formatBytes(queue.freeBytes)} free on this disk.`}
        </FieldDescription>
      </FieldContent>
      <div className="flex gap-2">
        <Input
          id="download-dir"
          key={value ?? ""}
          defaultValue={value ?? ""}
          placeholder={queue?.folder ?? "Videos\\Anilili"}
          onBlur={(e) => {
            const next = e.currentTarget.value.trim() || null
            if (next !== value) onChange(next)
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur()
          }}
        />
        <Button variant="outline" onClick={() => api.downloadsOpenFolder().catch(toastFailure)}>
          Open
        </Button>
      </div>
    </Field>
  )
}

function Account() {
  const viewer = useViewer()
  const settings = useSettings().data
  const save = useSaveSettings()
  return (
    <div className="flex flex-col gap-4">
      {viewer.data ? (
        <div className="flex items-center gap-3">
          <Avatar className="size-10">
            <AvatarImage src={viewer.data.avatar ?? undefined} alt="" />
            <AvatarFallback>{viewer.data.name.slice(0, 1)}</AvatarFallback>
          </Avatar>
          <div className="flex flex-col">
            <span className="font-medium">{viewer.data.name}</span>
            <span className="text-xs text-muted-foreground">Signed in</span>
          </div>
          <Button variant="outline" className="ml-auto" onClick={() => api.logout()}>
            <LogOutIcon data-icon="inline-start" />
            Sign out
          </Button>
        </div>
      ) : (
        <Button className="self-start" onClick={() => api.login().catch((e) => toastFailure(e))}>
          Sign in with AniList
        </Button>
      )}
      {viewer.data && settings && (
        <FieldGroup>
          <SwitchField
            label="Update AniList automatically"
            description="Set your progress when you finish an episode."
            checked={settings.syncAnilist}
            onChange={(syncAnilist) => save.mutate({ ...settings, syncAnilist })}
          />
        </FieldGroup>
      )}
    </div>
  )
}

function MalAccount() {
  const user = useMalUser()
  return user.data ? (
    <div className="flex items-center gap-3">
      <Avatar className="size-10">
        <AvatarImage src={user.data.picture ?? undefined} alt="" />
        <AvatarFallback>{user.data.name.slice(0, 1)}</AvatarFallback>
      </Avatar>
      <div className="flex flex-col">
        <span className="font-medium">{user.data.name}</span>
        <span className="text-xs text-muted-foreground">Signed in</span>
      </div>
      <Button variant="outline" className="ml-auto" onClick={() => api.malLogout()}>
        <LogOutIcon data-icon="inline-start" />
        Sign out
      </Button>
    </div>
  ) : (
    <Button variant="outline" className="self-start" onClick={() => api.malLogin().catch((e) => toastFailure(e))}>
      Sign in with MyAnimeList
    </Button>
  )
}

/**
 * Simkl, the tracker beside the list service. Connecting shows a code and opens simkl.com/pin with
 * it filled in; the app polls until it is approved (`simkl.rs`), and `simkl-changed` replaces the
 * code with the account.
 */
function SimklAccountRow() {
  const account = useSimklAccount()
  const [code, setCode] = useState<SimklCode | null>(null)
  useEffect(() => {
    const ended = listen("simkl-login-ended", () => setCode(null))
    const changed = listen("simkl-changed", () => setCode(null))
    return () => {
      void ended.then((fn) => fn())
      void changed.then((fn) => fn())
    }
  }, [])
  if (account.data) {
    return (
      <div className="flex items-center gap-3">
        <div className="flex flex-col">
          <span className="font-medium">{account.data.name ?? "Simkl"}</span>
          <span className="text-xs text-muted-foreground">Connected</span>
        </div>
        <Button variant="outline" className="ml-auto" onClick={() => api.simklLogout().catch(toastFailure)}>
          <LogOutIcon data-icon="inline-start" />
          Disconnect
        </Button>
      </div>
    )
  }
  if (code) {
    return (
      <div className="flex flex-col gap-3">
        <p className="text-sm text-muted-foreground">
          Approve this code on the page that opened in your browser, or at {code.verificationUri.replace(/^https?:\/\//, "")}:
        </p>
        <span className="font-mono text-2xl font-semibold tracking-[0.2em]">{code.userCode}</span>
        <div className="flex gap-2">
          <Button variant="outline" onClick={() => api.openAccountPage(code.verificationUriComplete).catch(toastFailure)}>
            <ExternalLinkIcon data-icon="inline-start" />
            Open the page again
          </Button>
          <Button
            variant="ghost"
            onClick={() => {
              setCode(null)
              return api.simklLoginCancel()
            }}
          >
            Cancel
          </Button>
        </div>
      </div>
    )
  }
  return (
    <Button variant="outline" className="self-start" onClick={() => api.simklLogin().then(setCode).catch(toastFailure)}>
      Connect Simkl
    </Button>
  )
}

/** The installed version, once Tauri has said. */
function useVersion(): string {
  const [version, setVersion] = useState("")
  useEffect(() => void getVersion().then(setVersion), [])
  return version
}
