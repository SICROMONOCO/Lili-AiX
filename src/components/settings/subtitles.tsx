// Settings → Subtitles: Android's Subtitles section (SettingsScreen.kt) and its Caption appearance
// editor (CaptionAppearance.kt), laid out for a mouse: a live preview over a still from the catalogue,
// and every control beside it at once rather than one ladder at a time.

import { useQueryClient } from "@tanstack/react-query"
import { LogOutIcon, RotateCcwIcon } from "lucide-react"
import { toast } from "sonner"
import { useState } from "react"

import { SettingsGroup } from "@/components/settings/section"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Field, FieldContent, FieldDescription, FieldGroup, FieldLabel, FieldSeparator } from "@/components/ui/field"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Slider } from "@/components/ui/slider"
import { Switch } from "@/components/ui/switch"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { api, keys, useHome, useOpenSubtitles } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import {
  CAPTION_ACROSS,
  CAPTION_BACKGROUNDS,
  CAPTION_EDGES,
  CAPTION_FONTS,
  CAPTION_PRESETS,
  CAPTION_TEXT_COLORS,
  DEFAULT_CAPTIONS,
  captionAcross,
  captionAcrossStyle,
  captionStyle,
} from "@/lib/captions"
import type { Captions, Settings } from "@/lib/types"
import { cn } from "@/lib/utils"

/** The languages Android offers (backup.rs keeps the same list, as Android stores them). */
const LANGUAGES = [
  { label: "English", value: "en" },
  { label: "Japanese", value: "ja" },
  { label: "Spanish", value: "es" },
  { label: "Portuguese", value: "pt" },
  { label: "Arabic", value: "ar" },
  { label: "French", value: "fr" },
  { label: "German", value: "de" },
  { label: "Italian", value: "it" },
  { label: "Turkish", value: "tr" },
  { label: "Russian", value: "ru" },
  { label: "Polish", value: "pl" },
  { label: "Indonesian", value: "id" },
  { label: "Hindi", value: "hi" },
  { label: "Chinese", value: "zh" },
  { label: "Korean", value: "ko" },
]
const NONE = "none"

type Update = (patch: Partial<Settings>) => void

export function SubtitlesSettings({ s, update }: { s: Settings; update: Update }) {
  const [first, second] = s.subtitleLanguages
  return (
    <>
      <SettingsGroup>
        <FieldGroup>
          <Toggle
            label="Show subtitles by default"
            description="A subtitle track is on as an episode opens. Text burnt into the video can't be hidden."
            checked={s.subtitlesByDefault}
            onChange={(subtitlesByDefault) => update({ subtitlesByDefault })}
          />
          {s.subtitlesByDefault && (
            <Toggle
              label="Also on dubbed audio"
              description="Keep them on when the audio is dubbed. From the next episode."
              checked={s.subtitlesWithDub}
              onChange={(subtitlesWithDub) => update({ subtitlesWithDub })}
            />
          )}
          <FieldSeparator />
          <LanguageField
            label="Preferred language"
            description="Which track to take when a server carries several."
            value={first ?? "en"}
            onChange={(v) => update({ subtitleLanguages: [v, ...s.subtitleLanguages.slice(1).filter((l) => l !== v)] })}
          />
          <LanguageField
            label="Second choice"
            description="Used when the first isn't there."
            value={second ?? NONE}
            none
            exclude={first}
            onChange={(v) => update({ subtitleLanguages: v === NONE ? [first ?? "en"] : [first ?? "en", v] })}
          />
        </FieldGroup>
      </SettingsGroup>
      <OpenSubtitlesAccount />
      <CaptionAppearance s={s} update={update} />
    </>
  )
}

/**
 * Android's OpenSubtitles row: a free account lets the player add subtitles from OpenSubtitles.
 * The password goes to the system's credential store, for signing in again when the day's session
 * lapses (OpenSubtitles has no refresh token).
 */
function OpenSubtitlesAccount() {
  const client = useQueryClient()
  const account = useOpenSubtitles().data
  const [username, setUsername] = useState("")
  const [password, setPassword] = useState("")
  const refresh = () => client.invalidateQueries({ queryKey: keys.openSubtitles })
  const signIn = () =>
    api
      .opensubtitlesLogin(username, password)
      .then((name) => {
        setPassword("")
        toast.success(`Signed in to OpenSubtitles as ${name}`)
        return refresh()
      })
      .catch((e) => toastFailure(e))

  return (
    <SettingsGroup
      title="OpenSubtitles"
      description={
        !account
          ? undefined
          : !account.configured
            ? "Not available in this build of the app."
            : account.username
              ? "In the player: Subtitles → Search OpenSubtitles, in your preferred and second-choice languages."
              : "A free account lets the player find and add subtitles for any episode (Subtitles → Search OpenSubtitles)."
      }
    >
      {account?.configured &&
        (account.username ? (
          <div className="flex items-center gap-3">
            <span className="flex size-9 items-center justify-center rounded-full bg-muted text-sm font-semibold uppercase">{account.username.slice(0, 1)}</span>
            <div className="flex flex-col">
              <span className="font-medium">{account.username}</span>
              <span className="text-xs text-muted-foreground">Signed in</span>
            </div>
            <Button variant="outline" className="ml-auto" onClick={() => api.opensubtitlesLogout().then(refresh).catch((e) => toastFailure(e))}>
              <LogOutIcon data-icon="inline-start" />
              Sign out
            </Button>
          </div>
        ) : (
          <form
            className="flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault()
              return signIn()
            }}
          >
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              <Input aria-label="OpenSubtitles username" placeholder="Username" autoComplete="username" value={username} onChange={(e) => setUsername(e.target.value)} />
              <Input
                aria-label="OpenSubtitles password"
                placeholder="Password"
                type="password"
                autoComplete="current-password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
              />
            </div>
            <div className="flex flex-wrap items-center gap-3">
              <Button type="submit" disabled={!username.trim() || !password} onClick={(e) => (e.preventDefault(), signIn())}>
                Sign in
              </Button>
              <span className="text-xs text-muted-foreground">
                No account?{" "}
                <button type="button" className="underline underline-offset-4" onClick={() => api.openOpenSubtitlesSignup().catch((e) => toastFailure(e))}>
                  Create one free
                </button>
                . The password is kept in this computer's credential store, never in the app's files.
              </span>
            </div>
          </form>
        ))}
    </SettingsGroup>
  )
}

/**
 * The caption editor. A slider shows its value in the preview while it moves and saves once let go,
 * so a drag is one write rather than a hundred.
 */
function CaptionAppearance({ s, update }: { s: Settings; update: Update }) {
  const [live, setLive] = useState<{ captions: Partial<Captions>; scale?: number }>({ captions: {} })
  const c: Captions = { ...s.captions, ...live.captions }
  const scale = live.scale ?? s.subtitleScale
  const set = (patch: Partial<Captions>) => update({ captions: { ...s.captions, ...patch } })
  const preview = (patch: Partial<Captions>) => setLive({ ...live, captions: { ...live.captions, ...patch } })
  const commit = (patch: Partial<Captions>) => {
    set(patch)
    setLive({ captions: {} })
  }
  const custom = JSON.stringify(s.captions) !== JSON.stringify(DEFAULT_CAPTIONS) || s.subtitleScale !== 1
  const boxed = c.backgroundOpacity > 0

  return (
    <SettingsGroup
      title="Caption appearance"
      description="For plain subtitles. Styled (ASS) subtitles keep their own look."
      action={
        custom && (
          <Button variant="ghost" size="sm" onClick={() => update({ captions: DEFAULT_CAPTIONS, subtitleScale: 1 })}>
            <RotateCcwIcon data-icon="inline-start" />
            Reset
          </Button>
        )
      }
    >
      <CaptionPreview c={c} scale={scale} />

      <div className="flex flex-wrap gap-1.5">
        {CAPTION_PRESETS.map((p) => {
          const target = { ...DEFAULT_CAPTIONS, ...p.captions }
          // A preset is the look; where the caption sits stays the viewer's.
          const placed = { position: s.captions.position, edgeMargin: s.captions.edgeMargin, horizontalAnchor: s.captions.horizontalAnchor }
          const active = JSON.stringify({ ...target, ...placed }) === JSON.stringify(s.captions)
          return (
            <Chip key={p.label} on={active} onClick={() => set({ ...target, ...placed })}>
              {p.label}
            </Chip>
          )
        })}
      </div>

      <div className="grid grid-cols-1 gap-x-8 gap-y-5 md:grid-cols-2">
        <Control label="Font">
          <div className="grid grid-cols-3 gap-1.5">
            {CAPTION_FONTS.map((f) => (
              <Chip key={f.value} on={c.font === f.value} onClick={() => set({ font: f.value })} style={{ fontFamily: f.css }}>
                {f.label}
              </Chip>
            ))}
          </div>
        </Control>

        <Control label="Size" value={`${Math.round(scale * 100)}%`}>
          <div className="flex items-center gap-3">
            <Slider
              aria-label="Subtitle size"
              min={0.5}
              max={2}
              step={0.05}
              value={[scale]}
              onValueChange={(v) => setLive({ ...live, scale: Array.isArray(v) ? v[0] : v })}
              onValueCommitted={(v) => {
                update({ subtitleScale: Array.isArray(v) ? v[0] : v })
                setLive({ captions: live.captions })
              }}
            />
            <ToggleGroup variant="outline" size="sm" value={[c.bold ? "bold" : "regular"]} onValueChange={(v) => v[0] && set({ bold: v[0] === "bold" })}>
              <ToggleGroupItem value="regular" aria-label="Regular weight">
                Aa
              </ToggleGroupItem>
              <ToggleGroupItem value="bold" aria-label="Bold" className="font-bold">
                Aa
              </ToggleGroupItem>
            </ToggleGroup>
          </div>
        </Control>

        <Control label="Text colour" value={`${c.textOpacity}% opaque`}>
          <div className="flex items-center gap-2">
            {CAPTION_TEXT_COLORS.map((x) => (
              <Swatch key={x.value} label={x.label} hex={x.hex} on={c.textColor === x.value} onClick={() => set({ textColor: x.value })} />
            ))}
          </div>
          <Slider
            aria-label="Text opacity"
            min={0}
            max={100}
            step={5}
            value={[c.textOpacity]}
            onValueChange={(v) => preview({ textOpacity: Array.isArray(v) ? v[0] : v })}
            onValueCommitted={(v) => commit({ textOpacity: Array.isArray(v) ? v[0] : v })}
          />
        </Control>

        <Control label="Background" value={boxed ? `${c.backgroundOpacity}% opaque` : "None"}>
          <div className="flex items-center gap-2">
            {CAPTION_BACKGROUNDS.map((x) => (
              <Swatch
                key={x.value}
                label={x.label}
                hex={x.hex}
                on={boxed && c.backgroundColor === x.value}
                // Picking a colour with no box would show nothing: it brings the box up.
                onClick={() => set({ backgroundColor: x.value, backgroundOpacity: boxed ? c.backgroundOpacity : 60 })}
              />
            ))}
          </div>
          <Slider
            aria-label="Background opacity"
            min={0}
            max={100}
            step={5}
            value={[c.backgroundOpacity]}
            onValueChange={(v) => preview({ backgroundOpacity: Array.isArray(v) ? v[0] : v })}
            onValueCommitted={(v) => commit({ backgroundOpacity: Array.isArray(v) ? v[0] : v })}
          />
        </Control>

        <Control label="Edge" hint={boxed && c.edgeStyle === "shadow" ? "A box takes the shadow's place." : undefined}>
          <ToggleGroup variant="outline" value={[c.edgeStyle]} onValueChange={(v) => v[0] && set({ edgeStyle: v[0] as Captions["edgeStyle"] })}>
            {CAPTION_EDGES.map((x) => (
              <ToggleGroupItem key={x.value} value={x.value}>
                {x.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Control>

        <Control
          label="Position"
          value={`${c.edgeMargin}% from the ${c.position} · ${CAPTION_ACROSS.find((x) => x.value === captionAcross(c.horizontalAnchor))?.label.toLowerCase()}`}
        >
          <ToggleGroup variant="outline" value={[c.position]} onValueChange={(v) => v[0] && set({ position: v[0] as Captions["position"] })}>
            <ToggleGroupItem value="bottom">Bottom</ToggleGroupItem>
            <ToggleGroupItem value="top">Top</ToggleGroupItem>
          </ToggleGroup>
          <Slider
            aria-label="Distance from the edge"
            min={0}
            max={30}
            step={1}
            value={[c.edgeMargin]}
            onValueChange={(v) => preview({ edgeMargin: Array.isArray(v) ? v[0] : v })}
            onValueCommitted={(v) => commit({ edgeMargin: Array.isArray(v) ? v[0] : v })}
          />
          <ToggleGroup
            variant="outline"
            size="sm"
            aria-label="Across the picture"
            value={[String(captionAcross(c.horizontalAnchor))]}
            onValueChange={(v) => v[0] && set({ horizontalAnchor: Number(v[0]) })}
          >
            {CAPTION_ACROSS.map((x) => (
              <ToggleGroupItem key={x.value} value={String(x.value)} aria-label={x.label} title={x.label}>
                {x.short}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        </Control>
      </div>
    </SettingsGroup>
  )
}

/** A still from the catalogue with the caption over it, as the player will draw it. */
function CaptionPreview({ c, scale }: { c: Captions; scale: number }) {
  const home = useHome().data
  const still = home?.trending.find((a) => a.banner)?.banner ?? home?.popular.find((a) => a.banner)?.banner
  // The caption is sized and placed against the picture's height, as mpv's is.
  const [height, setHeight] = useState(0)
  const measure = (el: HTMLDivElement | null) => {
    if (!el) return
    const observer = new ResizeObserver(([entry]) => setHeight(entry.contentRect.height))
    observer.observe(el)
    return () => observer.disconnect()
  }
  const h = height || 300
  const style = captionStyle(c, scale, h)
  const across = captionAcrossStyle(c.horizontalAnchor)
  return (
    <div
      ref={measure}
      className="relative aspect-[21/9] w-full overflow-hidden rounded-xl bg-gradient-to-br from-slate-700 via-slate-800 to-slate-950 xl:sticky xl:top-4 xl:z-10"
      aria-label="Caption preview"
      role="img"
    >
      {still && <img src={still} alt="" className="absolute inset-0 size-full object-cover" />}
      <div
        className={cn("absolute inset-x-0 flex", c.position === "top" ? "top-0" : "bottom-0")}
        style={{
          justifyContent: across.justify,
          paddingInline: (across.inset * h) / 720,
          ...(c.position === "top" ? { paddingTop: (c.edgeMargin / 100) * h } : { paddingBottom: (c.edgeMargin / 100) * h }),
        }}
      >
        <div
          className="max-w-[90%]"
          style={{ ...style.box, textAlign: across.justify === "center" ? "center" : across.justify === "flex-start" ? "left" : "right" }}
        >
          <p style={style.text}>
            This is how subtitles will look,
            <br />
            over whatever is playing.
          </p>
        </div>
      </div>
    </div>
  )
}

function Control({
  label,
  value,
  hint,
  children,
}: {
  label: string
  value?: string
  hint?: string
  children: React.ReactNode
}) {
  return (
    <div className="flex flex-col gap-2.5">
      <div className="flex items-baseline gap-2">
        <span className="text-sm font-medium">{label}</span>
        {value && <span className="ml-auto text-xs text-muted-foreground tabular-nums">{value}</span>}
      </div>
      {children}
      {hint && <p className="text-xs text-muted-foreground">{hint}</p>}
    </div>
  )
}

function Chip({ on, onClick, style, children }: { on: boolean; onClick: () => void; style?: React.CSSProperties; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      style={style}
      className={cn(
        "h-8 truncate rounded-lg border px-3 text-sm transition",
        on ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
      )}
    >
      {children}
    </button>
  )
}

function Swatch({ label, hex, on, onClick }: { label: string; hex: string; on: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={on}
      onClick={onClick}
      className={cn("size-8 rounded-full border-2 border-background shadow-sm ring-1 ring-border transition hover:scale-110", on && "ring-2 ring-foreground")}
      style={{ backgroundColor: hex }}
    />
  )
}

function Toggle({ label, description, checked, onChange }: { label: string; description: string; checked: boolean; onChange: (v: boolean) => void }) {
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

function LanguageField({
  label,
  description,
  value,
  none,
  exclude,
  onChange,
}: {
  label: string
  description: string
  value: string
  /** Offer "None". */
  none?: boolean
  /** A language that can't be chosen here (the first choice, for the second). */
  exclude?: string
  onChange: (value: string) => void
}) {
  const items = [...(none ? [{ label: "None", value: NONE }] : []), ...LANGUAGES.filter((l) => l.value !== exclude)]
  return (
    <Field orientation="horizontal">
      <FieldContent>
        <FieldLabel>{label}</FieldLabel>
        <FieldDescription>{description}</FieldDescription>
      </FieldContent>
      <Select items={items} value={value} onValueChange={(v) => v != null && onChange(String(v))}>
        <SelectTrigger className="w-40">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {items.map((l) => (
              <SelectItem key={l.value} value={l.value}>
                {l.label}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )
}
