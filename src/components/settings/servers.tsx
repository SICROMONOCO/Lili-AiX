// Settings → Servers: Android's "Server priority" and "Enabled servers" as one list. Every server,
// Miruro's and the extensions', numbered in the order they are tried; drag a row (or use its arrows)
// to move it, and switch one off to never use it. Unlisted servers follow in the catalogue's order,
// so an untouched list is Automatic.

import { useQuery } from "@tanstack/react-query"
import { ArrowDownIcon, ArrowUpIcon, GripVerticalIcon, RotateCcwIcon } from "lucide-react"
import { useRef, useState } from "react"
import { toast } from "sonner"

import { SettingsGroup } from "@/components/settings/section"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Spinner } from "@/components/ui/spinner"
import { Switch } from "@/components/ui/switch"
import { api } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import { MIRURO_PROVIDERS } from "@/lib/providers"
import { SOURCE_LANGUAGES } from "@/lib/server-languages"
import type { Settings } from "@/lib/types"
import { cn } from "@/lib/utils"

type Update = (patch: Partial<Settings>) => void
type Row = { key: string; name: string; source: string; version?: string }

export function ServersSettings({ s, update }: { s: Settings; update: Update }) {
  const providers = useQuery({ queryKey: ["providers"], queryFn: api.providers })
  const [checking, setChecking] = useState(false)

  const known: Row[] = [
    ...MIRURO_PROVIDERS.map((p) => ({ key: p.value, name: p.label, source: "Miruro" })),
    ...(providers.data ?? []).map((p) => ({ key: p.id, name: p.name, source: "Extension", version: p.version })),
  ]
  // The saved order first (a server no longer installed drops out), then the rest as they come.
  const order = [
    ...s.serverPriority.flatMap((key) => known.filter((r) => r.key === key)),
    ...known.filter((r) => !s.serverPriority.includes(r.key)),
  ]
  const off = new Set(s.disabledProviders)
  const enabledCount = order.filter((r) => !off.has(r.key)).length
  const custom = s.serverPriority.length > 0 || s.disabledProviders.length > 0

  const reorder = (keys: string[]) => update({ serverPriority: keys, preferredProvider: keys.find((k) => !off.has(k)) ?? null })
  const toggle = (key: string, on: boolean) => {
    const disabled = on ? s.disabledProviders.filter((k) => k !== key) : [...s.disabledProviders, key]
    const keys = order.map((r) => r.key)
    update({
      disabledProviders: disabled,
      preferredProvider: s.serverPriority.length ? (keys.find((k) => !disabled.includes(k)) ?? null) : s.preferredProvider,
    })
  }

  return (
    <>
      <SettingsGroup
        title="Server order"
        description="Tried from the top when an episode opens. Drag to reorder; switch a server off to never use it."
        action={
          custom && (
            <Button variant="ghost" size="sm" onClick={() => update({ serverPriority: [], disabledProviders: [], preferredProvider: null })}>
              <RotateCcwIcon data-icon="inline-start" />
              Automatic
            </Button>
          )
        }
      >
        {providers.isPending && (
          <span className="flex items-center gap-2 text-sm text-muted-foreground">
            <Spinner /> Loading extensions…
          </span>
        )}
        <SortableList
          rows={order}
          onMove={(keys) => reorder(keys)}
          render={(row, index, { grip, arrows }) => {
            const enabled = !off.has(row.key)
            // Numbered among the servers that are on: the order they are actually tried in.
            const rank = enabled ? order.slice(0, index + 1).filter((r) => !off.has(r.key)).length : null
            return (
              <>
                {grip}
                <span
                  className={cn(
                    "flex size-7 shrink-0 items-center justify-center rounded-full text-xs font-bold tabular-nums",
                    rank === 1 ? "bg-primary text-primary-foreground" : rank ? "bg-muted" : "bg-transparent text-muted-foreground",
                  )}
                >
                  {rank ?? "–"}
                </span>
                <span className={cn("flex min-w-0 flex-1 flex-col", !enabled && "opacity-50")}>
                  <span className="truncate text-sm font-medium">{row.name}</span>
                  <span className="truncate text-xs text-muted-foreground">
                    {row.source === "Miruro" ? `Miruro · ${row.key}` : `Extension · v${row.version}`}
                  </span>
                </span>
                {rank === 1 && <Badge variant="secondary">Tried first</Badge>}
                {arrows}
                <Switch
                  checked={enabled}
                  // The last server on stays on: with none, nothing could play.
                  disabled={enabled && enabledCount === 1}
                  onCheckedChange={(on) => toggle(row.key, on)}
                  aria-label={`Use ${row.name}`}
                />
              </>
            )
          }}
        />
        <div className="flex flex-wrap items-center gap-3">
          <Button
            variant="outline"
            loading={checking}
            onClick={() => {
              setChecking(true)
              void api
                .providersUpdate()
                .then((ids) => {
                  toast.success(ids.length ? `Updated: ${ids.join(", ")}` : "Extensions are up to date")
                  return providers.refetch()
                })
                .catch((e) => toastFailure(e))
                .finally(() => setChecking(false))
            }}
          >
            {checking ? "Checking…" : "Check for extension updates"}
          </Button>
          <span className="text-xs text-muted-foreground">Audio comes first: a dub request still goes to a server with the dub.</span>
        </div>
      </SettingsGroup>
      <ServerLanguages selected={s.preferredSourceLanguages} onChange={(preferredSourceLanguages) => update({ preferredSourceLanguages })} />
      <SecureDns value={s.dohProvider} onChange={(dohProvider) => update({ dohProvider })} />
    </>
  )
}

/** Android's DNS over HTTPS choices (AppDns.kt), under its stored values; anilili_core::dns has the addresses. */
const DOH_CHOICES = [
  { value: "auto", label: "Automatic", description: "Your network's DNS, switching to secure DNS where it blocks a source" },
  { value: "none", label: "Off", description: "Whatever your network hands out" },
  { value: "cloudflare", label: "Cloudflare", description: "cloudflare-dns.com · no filtering" },
  { value: "google", label: "Google", description: "dns.google · no filtering" },
  { value: "quad9", label: "Quad9", description: "dns.quad9.net · blocks hosts it rates malicious" },
  { value: "cloudflare_security", label: "Cloudflare Security", description: "security.cloudflare-dns.com · blocks hosts it rates malicious" },
  { value: "adguard", label: "AdGuard", description: "dns.adguard-dns.com · blocks ads and trackers" },
  { value: "dns_sb", label: "DNS.SB", description: "doh.dns.sb · no filtering" },
  { value: "canadian_shield", label: "Canadian Shield", description: "private.canadianshield.cira.ca · no filtering" },
]

/**
 * Android's "DNS over HTTPS": some providers refuse to hand out a source's address while leaving the
 * source up, which looks like every server failing at once. A secure resolver gets the address.
 */
function SecureDns({ value, onChange }: { value: string; onChange: (value: string) => void }) {
  const engaged = useQuery({ queryKey: ["dns-engaged"], queryFn: api.dnsEngaged, refetchInterval: 30_000 }).data
  const current = DOH_CHOICES.find((c) => c.value === value) ?? DOH_CHOICES[0]
  return (
    <SettingsGroup
      title="DNS over HTTPS"
      description="For when your internet provider blocks a source by refusing its address: sources, downloads and the built-in player then get it from a secure resolver instead."
    >
      <div className="flex flex-col gap-1.5" role="radiogroup" aria-label="DNS over HTTPS">
        {DOH_CHOICES.map((choice) => {
          const on = choice.value === current.value
          return (
            <button
              key={choice.value}
              type="button"
              role="radio"
              aria-checked={on}
              onClick={() => onChange(choice.value)}
              className={cn(
                "flex items-center gap-3 rounded-xl border px-3 py-2 text-left transition",
                on ? "border-primary bg-primary/10" : "surface-control hover:bg-muted",
              )}
            >
              <span className={cn("flex size-4 shrink-0 items-center justify-center rounded-full border-2", on ? "border-primary" : "border-muted-foreground/50")}>
                {on && <span className="size-2 rounded-full bg-primary" />}
              </span>
              <span className="flex min-w-0 flex-1 flex-col">
                <span className="text-sm font-medium">{choice.label}</span>
                <span className="truncate text-xs text-muted-foreground">{choice.description}</span>
              </span>
            </button>
          )
        })}
      </div>
      {value === "auto" && engaged && (
        <p className="text-xs text-muted-foreground">Your network blocked a source this session, so lookups go to a secure resolver first.</p>
      )}
      <p className="text-xs text-muted-foreground">Embedded players load in the web view, which uses the system's own DNS settings.</p>
    </SettingsGroup>
  )
}

/** Android's "Preferred server languages": which language tags and filters the player's server menu shows. */
function ServerLanguages({ selected, onChange }: { selected: string[]; onChange: (languages: string[]) => void }) {
  const toggle = (value: string) =>
    // Kept in the list's order, as Android stores them.
    onChange(SOURCE_LANGUAGES.map((l) => l.value).filter((v) => (v === value ? !selected.includes(v) : selected.includes(v))))
  return (
    <SettingsGroup
      title="Server languages"
      description={
        selected.length === 0
          ? "The player's server menu tags every language a server has. Pick yours to show only those, and filter by them."
          : `The server menu tags and filters by ${selected.length === 1 ? "this language" : `these ${selected.length} languages`} only.`
      }
      action={
        selected.length > 0 && (
          <Button variant="ghost" size="sm" onClick={() => onChange([])}>
            <RotateCcwIcon data-icon="inline-start" />
            All languages
          </Button>
        )
      }
    >
      <div className="flex flex-wrap gap-1.5">
        {SOURCE_LANGUAGES.map((l) => {
          const on = selected.includes(l.value)
          return (
            <button
              key={l.value}
              type="button"
              aria-pressed={on}
              onClick={() => toggle(l.value)}
              className={cn(
                "flex h-8 items-center gap-1.5 rounded-full border px-3 text-sm transition",
                on ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
              )}
            >
              <span className={cn("text-[0.65rem] font-bold", on ? "text-primary-foreground/70" : "text-muted-foreground")}>{l.tag}</span>
              {l.label}
            </button>
          )
        })}
      </div>
    </SettingsGroup>
  )
}

/**
 * A list reordered by dragging a row's grip: the row follows the pointer and a line marks where it
 * will land; letting go moves it. Pointer events rather than HTML drag and drop, which the window's
 * own file-drop handling takes over. The arrows do the same a step at a time, for the keyboard.
 */
function SortableList({
  rows,
  onMove,
  render,
}: {
  rows: Row[]
  onMove: (keys: string[]) => void
  render: (row: Row, index: number, parts: { grip: React.ReactNode; arrows: React.ReactNode }) => React.ReactNode
}) {
  const list = useRef<HTMLDivElement>(null)
  const [drag, setDrag] = useState<{ from: number; startY: number; dy: number; tops: number[]; heights: number[] } | null>(null)

  // Where the dragged row's middle now sits among the others' middles.
  const target = (() => {
    if (!drag) return null
    const middle = drag.tops[drag.from] + drag.heights[drag.from] / 2 + drag.dy
    let at = 0
    drag.tops.forEach((top, i) => {
      if (i !== drag.from && top + drag.heights[i] / 2 < middle) at++
    })
    return at
  })()

  const move = (from: number, to: number) => {
    if (from === to) return
    const keys = rows.map((r) => r.key)
    const [key] = keys.splice(from, 1)
    keys.splice(to, 0, key)
    onMove(keys)
  }

  const start = (index: number, e: React.PointerEvent) => {
    const children = [...(list.current?.children ?? [])] as HTMLElement[]
    e.currentTarget.setPointerCapture(e.pointerId)
    setDrag({ from: index, startY: e.clientY, dy: 0, tops: children.map((c) => c.offsetTop), heights: children.map((c) => c.offsetHeight) })
  }

  // The landing line: above the row now at `target` (counting without the dragged one), else after the last.
  const others = drag ? drag.tops.filter((_, i) => i !== drag.from) : []
  const lineAt = drag && target != null ? (target < others.length ? others[target] - 3 : drag.tops[drag.tops.length - 1] + drag.heights[drag.heights.length - 1] + 3) : null

  return (
    <div ref={list} className="relative flex flex-col gap-1.5" role="list">
      {rows.map((row, index) => {
        const dragging = drag?.from === index
        return (
          <div
            key={row.key}
            role="listitem"
            className={cn(
              "surface-control group flex items-center gap-3 rounded-xl border py-1.5 pr-3 pl-1.5 transition-shadow",
              dragging && "relative z-10 shadow-xl ring-2 ring-primary/60",
            )}
            style={dragging ? { transform: `translateY(${drag.dy}px)`, transition: "none" } : undefined}
          >
            {render(row, index, {
              grip: (
                <span
                  className="flex h-9 w-6 shrink-0 cursor-grab touch-none items-center justify-center text-muted-foreground active:cursor-grabbing"
                  aria-hidden
                  onPointerDown={(e) => start(index, e)}
                  onPointerMove={(e) => drag && setDrag({ ...drag, dy: e.clientY - drag.startY })}
                  onPointerUp={() => {
                    if (drag && target != null) move(drag.from, target)
                    setDrag(null)
                  }}
                  onPointerCancel={() => setDrag(null)}
                >
                  <GripVerticalIcon className="size-4" />
                </span>
              ),
              // Shown on hover or focus, so the list stays calm.
              arrows: (
                <span className="flex shrink-0 gap-0.5 opacity-0 transition group-focus-within:opacity-100 group-hover:opacity-100">
                  <Button variant="ghost" size="icon-sm" aria-label={`Move ${row.name} up`} disabled={index === 0} onClick={() => move(index, index - 1)}>
                    <ArrowUpIcon />
                  </Button>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Move ${row.name} down`}
                    disabled={index === rows.length - 1}
                    onClick={() => move(index, index + 1)}
                  >
                    <ArrowDownIcon />
                  </Button>
                </span>
              ),
            })}
          </div>
        )
      })}
      {lineAt != null && target !== drag?.from && (
        <span className="pointer-events-none absolute inset-x-2 h-0.5 rounded-full bg-primary" style={{ top: lineAt }} aria-hidden />
      )}
    </div>
  )
}
