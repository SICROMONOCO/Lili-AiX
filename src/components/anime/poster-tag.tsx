// The one shape every label over poster artwork takes, as Android's PosterTag: a near-black plate,
// because it sits on arbitrary artwork and only a colour of its own keeps white type legible on a
// bright frame, with a thin accent outline so it reads as the app's rather than the artwork's.
// Signals rather than facts (18+, the next episode) keep the geometry with a fill of their own; the
// score drops the outline, since its star already carries the accent.

import type { LucideIcon } from "lucide-react"

import { cn } from "@/lib/utils"

type Tone = "plain" | "accent" | "danger"

const TONES: Record<Tone, string> = {
  plain: "bg-black/78 text-white",
  accent: "bg-primary text-primary-foreground",
  danger: "bg-destructive text-white",
}

export function PosterTag({
  children,
  label,
  icon: Icon,
  tone = "plain",
  outlined = tone === "plain",
  className,
}: {
  children: React.ReactNode
  /** What the tag means, for screen readers and the hover tooltip ("English dub available"). */
  label?: string
  icon?: LucideIcon
  tone?: Tone
  outlined?: boolean
  className?: string
}) {
  return (
    <span
      role={label ? "img" : undefined}
      aria-label={label}
      title={label}
      className={cn(
        "inline-flex items-center gap-0.5 rounded-[5px] px-[5px] py-0.5 text-[11px] leading-4 font-bold whitespace-nowrap",
        TONES[tone],
        outlined && "ring-1 ring-primary/60 ring-inset",
        className,
      )}
    >
      {Icon && <Icon aria-hidden className="size-[11px] fill-current text-primary" />}
      {children}
    </span>
  )
}
