// Settings → Appearance: Android's Appearance section (SettingsScreen.kt), theme and layout, as far
// as it means something on a computer. What each value looks like is src/lib/theme.ts.

import { useQuery } from "@tanstack/react-query"
import { ArrowDownIcon, ArrowUpIcon, CheckIcon, ImageIcon, RotateCcwIcon, SearchIcon } from "lucide-react"
import { useDeferredValue, useRef, useState } from "react"

import { Button } from "@/components/ui/button"
import { SettingsGroup } from "@/components/settings/section"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel, FieldSeparator } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { api, useAppearance, useSetAppearance } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import {
  ACCENTS,
  DEFAULT_APPEARANCE,
  GLASS_PARTS,
  HOME_ROWS,
  type HomeLayout,
  POSTER_ELEMENTS,
  PRESETS,
  decodeHomeLayout,
  encodeHomeLayout,
  presetMatches,
} from "@/lib/theme"
import type { AnimeCard } from "@/lib/types"
import { cn } from "@/lib/utils"

const STYLES = [
  { value: "vivid", label: "Vivid", description: "The exact colour you picked, surfaces lightly tinted" },
  { value: "tonal", label: "Tonal", description: "A softer, pastel accent" },
  { value: "neutral", label: "Neutral", description: "Mostly grey, with the accent used sparingly" },
  { value: "vibrant", label: "Vibrant", description: "Saturated colour on surfaces as well as controls" },
]

const BACKDROP_SOURCES = [
  { value: "none", label: "None" },
  { value: "anime", label: "From an anime" },
  { value: "image", label: "Your own image" },
]

export function AppearanceSettings() {
  const a = useAppearance()
  const set = useSetAppearance()
  const [picking, setPicking] = useState(false)
  const file = useRef<HTMLInputElement>(null)

  const pickImage = async (picked: File | undefined) => {
    if (!picked) return
    try {
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader()
        reader.onload = () => resolve(String(reader.result))
        reader.onerror = () => reject(reader.error)
        reader.readAsDataURL(picked)
      })
      await api.backdropSave(dataUrl)
      // A new marker each time, so the picture is read again.
      set({ backdropSource: "image", backdropImage: `${picked.name}#${Date.now()}` })
    } catch (e) {
      toastFailure(e)
    }
  }

  const resetTheme = () =>
    set({
      background: DEFAULT_APPEARANCE.background,
      accent: DEFAULT_APPEARANCE.accent,
      style: DEFAULT_APPEARANCE.style,
      contrast: DEFAULT_APPEARANCE.contrast,
      playerAccent: DEFAULT_APPEARANCE.playerAccent,
      showColourPages: DEFAULT_APPEARANCE.showColourPages,
      backdropSource: "none",
      backdropDim: DEFAULT_APPEARANCE.backdropDim,
      backdropBlur: DEFAULT_APPEARANCE.backdropBlur,
    })

  return (
    <>
      <SettingsGroup title="Theme">
        <FieldGroup>
          <Field>
            <FieldLabel>Theme presets</FieldLabel>
            <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-4">
              {PRESETS.map((p) => {
                const seed = ACCENTS.find((x) => x.value === p.accent)?.seed
                const active = presetMatches(p, a)
                return (
                  <button
                    key={p.label}
                    type="button"
                    onClick={() => set({ background: p.background, accent: p.accent, style: p.style, contrast: p.contrast ?? "standard" })}
                    className={cn(
                      "surface-control flex items-center gap-2 rounded-lg px-3 py-2 text-left text-sm transition hover:bg-muted",
                      active && "border-primary bg-muted",
                    )}
                  >
                    <span
                      className="size-5 shrink-0 rounded-full border"
                      style={{ background: `linear-gradient(135deg, ${seed ?? "#e5e5e5"} 50%, ${p.background === "dark" ? "#2a2a2a" : "#000"} 50%)` }}
                    />
                    <span className="flex-1 truncate">{p.label}</span>
                    {active && <CheckIcon className="size-4" />}
                  </button>
                )
              })}
            </div>
          </Field>
          <FieldSeparator />
          <Row label="Background" description="AMOLED black switches those pixels off; Dark is softer in a lit room.">
            <ToggleGroup variant="outline" value={[a.background]} onValueChange={(v) => v[0] && set({ background: String(v[0]) })}>
              <ToggleGroupItem value="amoled">AMOLED black</ToggleGroupItem>
              <ToggleGroupItem value="dark">Dark</ToggleGroupItem>
            </ToggleGroup>
          </Row>
          <FieldSeparator />
          <Field>
            <FieldLabel>Accent colour</FieldLabel>
            <div className="flex flex-wrap items-center gap-2">
              {ACCENTS.filter((x) => x.value !== "custom").map((x) => (
                <Swatch key={x.value} label={x.label} colour={x.seed ?? "#e5e5e5"} active={a.accent === x.value} onClick={() => set({ accent: x.value })} />
              ))}
              <label
                className={cn(
                  "relative flex h-9 cursor-pointer items-center gap-2 rounded-full border px-3 text-sm transition hover:bg-muted",
                  a.accent === "custom" && "ring-2 ring-foreground ring-offset-2 ring-offset-background",
                )}
              >
                <span className="size-5 rounded-full border" style={{ backgroundColor: a.customSeed }} />
                Custom
                <input
                  type="color"
                  value={a.customSeed}
                  onChange={(e) => set({ accent: "custom", customSeed: e.target.value })}
                  className="absolute inset-0 cursor-pointer opacity-0"
                  aria-label="Custom accent colour"
                />
              </label>
            </div>
          </Field>
          <FieldSeparator />
          <Row label="Colour style" description={a.accent === "monochrome" ? "Monochrome has no hue to spread." : STYLES.find((x) => x.value === a.style)?.description ?? ""}>
            <Select items={STYLES} value={a.style} disabled={a.accent === "monochrome"} onValueChange={(v) => set({ style: String(v) })}>
              <SelectTrigger className="w-36">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  {STYLES.map((x) => (
                    <SelectItem key={x.value} value={x.value}>
                      {x.label}
                    </SelectItem>
                  ))}
                </SelectGroup>
              </SelectContent>
            </Select>
          </Row>
          <FieldSeparator />
          <Row label="Contrast" description="Brighter secondary text and firmer outlines, for glare or low vision.">
            <ToggleGroup variant="outline" value={[a.contrast]} onValueChange={(v) => v[0] && set({ contrast: String(v[0]) })}>
              <ToggleGroupItem value="standard">Standard</ToggleGroupItem>
              <ToggleGroupItem value="medium">Medium</ToggleGroupItem>
              <ToggleGroupItem value="high">High</ToggleGroupItem>
            </ToggleGroup>
          </Row>
          <FieldSeparator />
          <SwitchRow
            label="Colour anime pages by the show"
            description="A title's page takes its accent from the artwork."
            checked={a.showColourPages}
            onChange={(showColourPages) => set({ showColourPages })}
          />
          <FieldSeparator />
          <SwitchRow
            label="Accent in the player"
            description="Off, the player's controls stay white over any picture."
            checked={a.playerAccent}
            onChange={(playerAccent) => set({ playerAccent })}
          />
          <FieldSeparator />
          <div>
            <Button variant="ghost" onClick={resetTheme}>
              <RotateCcwIcon data-icon="inline-start" />
              Reset theme
            </Button>
          </div>
        </FieldGroup>
      </SettingsGroup>
      <SettingsGroup>
        <FieldGroup>
          <Row
            label="Background image"
            description={
              a.backdropSource === "anime" && a.backdropAnimeTitle
                ? `From ${a.backdropAnimeTitle}`
                : a.backdropSource === "image"
                  ? "Your own picture, kept on this computer"
                  : "A picture behind the pages"
            }
          >
            <div className="flex items-center gap-2">
              {a.backdropSource === "anime" && (
                <Button variant="outline" onClick={() => setPicking(true)}>
                  Change…
                </Button>
              )}
              {a.backdropSource === "image" && (
                <Button variant="outline" onClick={() => file.current?.click()}>
                  Change picture…
                </Button>
              )}
              <Select
                items={BACKDROP_SOURCES}
                value={a.backdropSource}
                onValueChange={(v) => {
                  const source = String(v)
                  if (source === "anime" && !a.backdropAnimeUrl) setPicking(true)
                  else if (source === "image" && !a.backdropImage) file.current?.click()
                  else set({ backdropSource: source })
                }}
              >
                <SelectTrigger className="w-40">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {BACKDROP_SOURCES.map((x) => (
                      <SelectItem key={x.value} value={x.value}>
                        {x.label}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
              <input
                ref={file}
                type="file"
                accept="image/*"
                className="hidden"
                onChange={(e) => {
                  void pickImage(e.target.files?.[0])
                  e.target.value = ""
                }}
              />
            </div>
          </Row>
          {a.backdropSource !== "none" && (
            <>
              <FieldSeparator />
              <Row label="Darken background" description="Posters and text sit on top of it.">
                <ToggleGroup variant="outline" value={[a.backdropDim]} onValueChange={(v) => v[0] && set({ backdropDim: String(v[0]) })}>
                  <ToggleGroupItem value="light">Light</ToggleGroupItem>
                  <ToggleGroupItem value="medium">Medium</ToggleGroupItem>
                  <ToggleGroupItem value="strong">Strong</ToggleGroupItem>
                </ToggleGroup>
              </Row>
              <FieldSeparator />
              <SwitchRow label="Blur background" description="Softens the picture so the pages stay calm." checked={a.backdropBlur} onChange={(backdropBlur) => set({ backdropBlur })} />
            </>
          )}
        </FieldGroup>
      </SettingsGroup>
      <LiquidGlass />
      {picking && (
        <AnimeBackdropPicker
          onClose={() => setPicking(false)}
          onPick={(url, title) => {
            set({ backdropSource: "anime", backdropAnimeUrl: url, backdropAnimeTitle: title })
            setPicking(false)
          }}
        />
      )}
    </>
  )
}

export function LayoutSettings() {
  const a = useAppearance()
  const set = useSetAppearance()
  const [arranging, setArranging] = useState(false)
  const hidden = new Set(a.hiddenPosterElements)
  const shownCount = POSTER_ELEMENTS.filter((e) => !hidden.has(e.value)).length
  const summary =
    shownCount === POSTER_ELEMENTS.length
      ? `All ${shownCount}`
      : shownCount === 0
        ? "Artwork only"
        : POSTER_ELEMENTS.length - shownCount === 1
          ? `${POSTER_ELEMENTS.find((e) => hidden.has(e.value))?.label} off`
          : `${shownCount} of ${POSTER_ELEMENTS.length}`

  const toggleElement = (value: string, on: boolean) =>
    set({ hiddenPosterElements: on ? a.hiddenPosterElements.filter((v) => v !== value) : [...a.hiddenPosterElements, value] })

  return (
    <>
      <SettingsGroup title="Layout">
        <FieldGroup>
          <Row label="Interface size" description="Text and controls across the app.">
            <ToggleGroup variant="outline" value={[a.uiScale]} onValueChange={(v) => v[0] && set({ uiScale: String(v[0]) })}>
              <ToggleGroupItem value="small">Small</ToggleGroupItem>
              <ToggleGroupItem value="medium">Medium</ToggleGroupItem>
              <ToggleGroupItem value="large">Large</ToggleGroupItem>
            </ToggleGroup>
          </Row>
          <FieldSeparator />
          <Row label="Time format" description="Air times on the schedule.">
            <ToggleGroup variant="outline" value={[a.clockFormat]} onValueChange={(v) => v[0] && set({ clockFormat: String(v[0]) })}>
              <ToggleGroupItem value="system">System</ToggleGroupItem>
              <ToggleGroupItem value="12">12-hour</ToggleGroupItem>
              <ToggleGroupItem value="24">24-hour</ToggleGroupItem>
            </ToggleGroup>
          </Row>
          <FieldSeparator />
          <Row label="Arrange Home" description="The order of Home's rows, and which are shown.">
            <Button variant="outline" onClick={() => setArranging(true)}>
              Arrange…
            </Button>
          </Row>
        </FieldGroup>
      </SettingsGroup>
      <SettingsGroup title="Cards and images">
        <FieldGroup>
          <Row label="Card details" description="Which badges and captions posters draw around their artwork.">
            <Popover>
              <PopoverTrigger render={<Button variant="outline" className="w-36 justify-between" />}>{summary}</PopoverTrigger>
              <PopoverContent align="end" className="w-80">
                <FieldGroup className="gap-3">
                  {POSTER_ELEMENTS.map((e) => (
                    <Field key={e.value} orientation="horizontal">
                      <FieldContent>
                        <FieldLabel>{e.label}</FieldLabel>
                        <FieldDescription>{e.description}</FieldDescription>
                      </FieldContent>
                      <Switch checked={!hidden.has(e.value)} onCheckedChange={(on) => toggleElement(e.value, on)} aria-label={e.label} />
                    </Field>
                  ))}
                </FieldGroup>
              </PopoverContent>
            </Popover>
          </Row>
          <FieldSeparator />
          <SwitchRow
            label="Blur episode images"
            description="Hide possible spoilers in the art of episodes you haven't watched."
            checked={a.blurEpisodeImages}
            onChange={(blurEpisodeImages) => set({ blurEpisodeImages })}
          />
          <FieldSeparator />
          <SwitchRow
            label="Series logos"
            description="A title's own wordmark instead of its name, where there is one."
            checked={a.showSeriesLogos}
            onChange={(showSeriesLogos) => set({ showSeriesLogos })}
          />
        </FieldGroup>
      </SettingsGroup>
      {arranging && <ArrangeHome stored={a.homeLayout} onClose={() => setArranging(false)} onSave={(homeLayout) => set({ homeLayout })} />}
    </>
  )
}

/** Android's Liquid glass group: one switch over the lot, in the heading (on only while every part
 * is on, so one part off reads as off, and turning it on restores all), then each part. Off, a part's panels and
 * bars are a solid fill. On macOS the window's own Liquid Glass is under them (src-tauri glass.rs). */
function LiquidGlass() {
  const a = useAppearance()
  const set = useSetAppearance()
  const states = GLASS_PARTS.map((part) => a[part.key])
  const on = states.filter(Boolean).length
  const native = document.documentElement.dataset.nativeGlass === "on"
  return (
    <SettingsGroup
      title="Liquid glass"
      description={
        on === states.length
          ? native
            ? "The window's own glass shows through every part below"
            : "Every part below blurs what is behind it"
          : on === 0
            ? "Off everywhere: solid panels. Turn on for glass again"
            : `${on} of ${states.length} parts on; turn on for all`
      }
      action={
        <Switch
          aria-label="All glass effects"
          className="mr-1.5 scale-125"
          checked={on === states.length}
          onCheckedChange={(all) =>
            // Android's refresh spinner has no desktop part; it follows the master so backups agree.
            set({ ...Object.fromEntries(GLASS_PARTS.map((part) => [part.key, all])), glassRefresh: all })
          }
        />
      }
    >
      <FieldGroup>
        {GLASS_PARTS.map((part) => (
          <SwitchRow key={part.key} label={part.label} description={part.description} checked={a[part.key]} onChange={(v) => set({ [part.key]: v })} />
        ))}
      </FieldGroup>
    </SettingsGroup>
  )
}

function Row({ label, description, children }: { label: string; description: string; children: React.ReactNode }) {
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel>{label}</FieldLabel>
        <FieldDescription>{description}</FieldDescription>
      </FieldContent>
      {children}
    </Field>
  )
}

function SwitchRow({ label, description, checked, onChange }: { label: string; description: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <Row label={label} description={description}>
      <Switch checked={checked} onCheckedChange={onChange} aria-label={label} />
    </Row>
  )
}

function Swatch({ label, colour, active, onClick }: { label: string; colour: string; active: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={active}
      onClick={onClick}
      className={cn("size-9 rounded-full border-2 border-background transition hover:scale-110", active && "ring-2 ring-foreground")}
      style={{ backgroundColor: colour }}
    />
  )
}

/** "From an anime": search a title, then take its wide banner or its cover. */
function AnimeBackdropPicker({ onClose, onPick }: { onClose: () => void; onPick: (url: string, title: string) => void }) {
  const [text, setText] = useState("")
  const query = useDeferredValue(text.trim())
  const results = useQuery({
    queryKey: ["backdrop-search", query],
    queryFn: () => api.search({ query, page: 1 }),
    enabled: query.length > 1,
    staleTime: 5 * 60_000,
  })
  const [chosen, setChosen] = useState<AnimeCard | null>(null)
  const images = chosen ? [chosen.banner, chosen.cover].filter((u): u is string => !!u) : []

  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>{chosen ? chosen.title : "Background from an anime"}</DialogTitle>
          <DialogDescription>{chosen ? "Pick one of its pictures." : "Search for a title, then pick one of its pictures."}</DialogDescription>
        </DialogHeader>
        {chosen ? (
          <div className="grid grid-cols-2 gap-3">
            {images.map((url) => (
              <button key={url} type="button" onClick={() => onPick(url, chosen.title)} className="overflow-hidden rounded-lg border transition hover:ring-2 hover:ring-ring">
                <img src={url} alt="" className="aspect-video w-full object-cover" />
              </button>
            ))}
            {images.length === 0 && (
              <p className="flex items-center gap-2 text-sm text-muted-foreground">
                <ImageIcon className="size-4" />
                This title has no pictures to use.
              </p>
            )}
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <div className="relative">
              <SearchIcon className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input autoFocus value={text} onChange={(e) => setText(e.target.value)} placeholder="Search anime" className="pl-9" />
              {results.isFetching && <Spinner className="absolute top-1/2 right-3 -translate-y-1/2 text-muted-foreground" />}
            </div>
            <div className="grid max-h-96 grid-cols-4 gap-3 overflow-y-auto sm:grid-cols-5">
              {(results.data?.results ?? []).map((card) => (
                <button key={card.id} type="button" onClick={() => setChosen(card)} className="flex flex-col gap-1 text-left">
                  <img src={card.cover ?? undefined} alt="" className="aspect-[2/3] w-full rounded-md bg-muted object-cover" />
                  <span className="line-clamp-2 text-xs">{card.title}</span>
                </button>
              ))}
            </div>
          </div>
        )}
        <DialogFooter>
          {chosen && (
            <Button variant="ghost" onClick={() => setChosen(null)}>
              Back to search
            </Button>
          )}
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

/** Arrange Home: move rows up and down, and switch them off. Saved in Android's encoding. Opened
 * from Settings → Appearance and from the foot of Home. */
export function ArrangeHome({ stored, onClose, onSave }: { stored: string | null; onClose: () => void; onSave: (encoded: string | null) => void }) {
  const [layout, setLayout] = useState<HomeLayout>(() => decodeHomeLayout(stored))
  const move = (index: number, by: -1 | 1) => {
    const order = [...layout.order]
    const [row] = order.splice(index, 1)
    order.splice(index + by, 0, row)
    setLayout({ ...layout, order })
  }
  const toggle = (key: HomeLayout["order"][number], on: boolean) => {
    const hidden = new Set(layout.hidden)
    if (on) hidden.delete(key)
    else hidden.add(key)
    setLayout({ ...layout, hidden })
  }
  return (
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Arrange Home</DialogTitle>
          <DialogDescription>Rows with nothing in them are skipped either way.</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-1">
          {layout.order.map((key, index) => (
            <div key={key} className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-muted">
              <span className={cn("flex-1 text-sm", layout.hidden.has(key) && "text-muted-foreground line-through")}>
                {HOME_ROWS.find((r) => r.key === key)?.label}
              </span>
              <Button variant="ghost" size="icon-sm" aria-label="Move up" disabled={index === 0} onClick={() => move(index, -1)}>
                <ArrowUpIcon />
              </Button>
              <Button variant="ghost" size="icon-sm" aria-label="Move down" disabled={index === layout.order.length - 1} onClick={() => move(index, 1)}>
                <ArrowDownIcon />
              </Button>
              <Switch checked={!layout.hidden.has(key)} onCheckedChange={(on) => toggle(key, on)} aria-label="Show this row" />
            </div>
          ))}
        </div>
        <DialogFooter>
          <Button
            variant="ghost"
            onClick={() => {
              onSave(null)
              onClose()
            }}
          >
            Reset
          </Button>
          <Button
            onClick={() => {
              onSave(encodeHomeLayout(layout))
              onClose()
            }}
          >
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
