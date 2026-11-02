// The parts every panel the player opens is built from, after Android's Midnight dialogs in
// `PlayerPanelTheme`'s palette: over a picture, selection is told by brightness (a sixth of the
// accent, which is white unless "Accent in the player" is on), not by a second colour.

import { ChevronRightIcon, XIcon } from "lucide-react"

import { Slider } from "@/components/ui/slider"
import { cn } from "@/lib/utils"

/**
 * Android's `PlayerSidePanel`: the "Now playing" header with the panel's name and a close button,
 * a rule, then a body that scrolls under it.
 */
export function PanelFrame({
  title,
  onClose,
  actions,
  children,
}: {
  title: string
  onClose: () => void
  /** Icon buttons before the close button. */
  actions?: React.ReactNode
  children: React.ReactNode
}) {
  return (
    <div className="flex h-full flex-col px-5 pt-4 pb-6">
      <div className="flex items-center gap-1 pb-3">
        <div className="flex min-w-0 flex-1 flex-col">
          <span className="text-[11px] font-extrabold tracking-[0.16em] text-white/64">NOW PLAYING</span>
          <h2 className="mt-0.75 font-heading text-base font-extrabold text-white">{title}</h2>
        </div>
        {actions}
        <PanelIconButton label={`Close ${title.toLowerCase()} (Esc)`} onClick={onClose}>
          <XIcon />
        </PanelIconButton>
      </div>
      <div className="h-px bg-white/10" />
      <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1">{children}</div>
    </div>
  )
}

export function PanelIconButton({ label, onClick, children }: { label: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      onClick={onClick}
      className="flex size-10 items-center justify-center rounded-full text-white/50 outline-none hover:bg-white/10 hover:text-white focus-visible:ring-2 focus-visible:ring-white/60 [&_svg]:size-4.5"
    >
      {children}
    </button>
  )
}

/** Android's `MidnightSectionLabel`: uppercase, tracked, dim. */
export function SectionLabel({ children }: { children: React.ReactNode }) {
  return <div className="pt-4 pb-2 text-[11px] font-extrabold tracking-[0.16em] text-white/45 uppercase">{children}</div>
}

/** Android's `MidnightHelper`: one muted line under a control. */
export function Helper({ children }: { children: React.ReactNode }) {
  return <p className="pt-1.75 text-[11px] text-white/50">{children}</p>
}

const ROW = "my-0.75 flex min-h-11.5 w-full items-center gap-2.5 rounded-xl px-4 py-2.25 text-left outline-none focus-visible:ring-2 focus-visible:ring-white/60"
const REST = "bg-white/4.5 text-white/80 hover:bg-white/10 hover:text-white"

function RowText({ title, subtitle, bold }: { title: string; subtitle?: string; bold?: boolean }) {
  return (
    <span className="flex min-w-0 flex-1 flex-col">
      <span className={cn("truncate text-sm", bold ? "font-bold" : "font-semibold")}>{title}</span>
      {subtitle && (
        <span title={subtitle} className="mt-0.5 truncate text-[11px] text-white/50">
          {subtitle}
        </span>
      )}
    </span>
  )
}

/**
 * Android's `MidnightChoiceRow`: title, a one-line subtitle, a radio on the right. Selected is told
 * three ways (bright bold text, a sixth of the accent, the filled radio); the pointer brightens.
 */
export function ChoiceRow({
  title,
  subtitle,
  selected = false,
  onSelect,
  indicator,
}: {
  title: string
  subtitle?: string
  selected?: boolean
  onSelect: () => void
  /** In place of the radio, for rows that act rather than choose. */
  indicator?: React.ReactNode
}) {
  return (
    <button
      type="button"
      role={indicator ? undefined : "radio"}
      aria-checked={indicator ? undefined : selected}
      onClick={onSelect}
      className={cn(ROW, selected ? "bg-primary/16 text-white" : REST)}
    >
      <RowText title={title} subtitle={subtitle} bold={selected} />
      {indicator ?? (
        <span
          aria-hidden
          className={cn("flex size-3.75 shrink-0 items-center justify-center rounded-full border", selected ? "border-current" : "border-current/42")}
        >
          {selected && <span className="size-2.25 rounded-full bg-current" />}
        </span>
      )}
    </button>
  )
}

/** Android's `MidnightSettingRow` with an action: title, a line of description, a chevron. */
export function ActionRow({ title, description, onClick }: { title: string; description?: string; onClick: () => void }) {
  return (
    <button type="button" onClick={onClick} className={cn(ROW, REST, "text-white")}>
      <RowText title={title} subtitle={description} />
      <ChevronRightIcon className="size-4.5 shrink-0 text-white/45" />
    </button>
  )
}

/** Android's `MidnightSettingSwitchRow`: the whole row toggles the switch. */
export function SwitchRow({
  title,
  description,
  checked,
  onChange,
}: {
  title: string
  description?: string
  checked: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <button type="button" role="switch" aria-checked={checked} onClick={() => onChange(!checked)} className={cn(ROW, REST, "text-white")}>
      <RowText title={title} subtitle={description} />
      <span aria-hidden className={cn("flex h-5 w-9 shrink-0 items-center rounded-full p-0.5 transition-colors", checked ? "bg-primary" : "bg-white/22")}>
        <span
          className={cn(
            "size-4 rounded-full transition-transform",
            checked ? "translate-x-4 bg-primary-foreground" : "bg-white/70",
          )}
        />
      </span>
    </button>
  )
}

/** Android's `MidnightUnderlineSelector`: a rounded track with the chosen option's pill inside it. */
export function Segmented<T extends string>({
  options,
  selected,
  onSelect,
}: {
  options: { value: T; label: string }[]
  selected: T
  onSelect: (value: T) => void
}) {
  return (
    <div role="radiogroup" className="flex gap-1 rounded-[14px] bg-white/4.5 p-1">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === selected}
          onClick={() => onSelect(o.value)}
          className={cn(
            "h-9.5 flex-1 truncate rounded-xl px-1 text-sm outline-none focus-visible:ring-2 focus-visible:ring-white/60",
            o.value === selected ? "bg-primary/16 font-bold text-white" : "font-semibold text-white/50 hover:text-white",
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  )
}

/**
 * Android's `PlayerSliderRow`: an icon slot, the slider, the value. White on a grey track with a
 * grey thumb, so the fill reads at a glance over the picture.
 */
export function SliderRow({
  icon,
  label,
  value,
  min,
  max,
  step = 1,
  display,
  onChange,
}: {
  icon: React.ReactNode
  label: string
  value: number
  min: number
  max: number
  step?: number
  display: string
  onChange: (value: number) => void
}) {
  return (
    <div className="flex items-center gap-2 rounded-xl px-2.5 py-1.5">
      <span className="flex size-6 shrink-0 items-center justify-center text-white/45 [&_svg]:size-5">{icon}</span>
      <Slider
        aria-label={label}
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={(v) => onChange(Array.isArray(v) ? v[0] : v)}
        className="mx-2 flex-1 [&_[data-slot=slider-range]]:bg-white [&_[data-slot=slider-thumb]]:border-0 [&_[data-slot=slider-thumb]]:bg-[#9e9e9e] [&_[data-slot=slider-track]]:bg-white/22"
      />
      <span className="w-12 shrink-0 text-right text-sm font-bold text-white tabular-nums">{display}</span>
    </div>
  )
}

/** Android's `MidnightStepperRow`: a value nudged by two quiet actions either side of it. */
export function StepperRow({
  label,
  value,
  decrease,
  increase,
}: {
  label: string
  value: string
  decrease: { label: string; onClick: () => void }
  increase: { label: string; onClick: () => void }
}) {
  return (
    <div className="flex min-h-11.5 items-center py-1.5 pr-2 pl-4">
      <span className="flex-1 text-sm font-semibold text-white/64">{label}</span>
      <QuietAction onClick={decrease.onClick}>{decrease.label}</QuietAction>
      <span className="px-2.5 text-sm font-extrabold text-white tabular-nums">{value}</span>
      <QuietAction onClick={increase.onClick}>{increase.label}</QuietAction>
    </div>
  )
}

/** Android's `MidnightQuietAction`: a text button that stays out of the way. */
export function QuietAction({ onClick, children, className }: { onClick: () => void; children: React.ReactNode; className?: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "min-h-9 rounded-md px-2.5 py-2 text-xs font-semibold text-white/50 outline-none hover:bg-white/10 hover:text-white focus-visible:ring-2 focus-visible:ring-white/60",
        className,
      )}
    >
      {children}
    </button>
  )
}
