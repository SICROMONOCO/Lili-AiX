// The dice, as Android deals it (ui/random/RandomAnimeUi.kt): one card at a time, swiped right to
// open the title or left to skip it. Arrow keys do the same; a click on the card opens it.

import { useNavigate } from "@tanstack/react-router"
import { DicesIcon, PlayIcon, StarIcon, XIcon } from "lucide-react"
import { type PointerEvent, useState, useSyncExternalStore } from "react"

import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { toFailure } from "@/lib/failure"
import { dice } from "@/lib/dice"
import { formatLabel, plainDescription } from "@/lib/format"
import type { DicePick } from "@/lib/types"
import { cn } from "@/lib/utils"

/** How far a card travels before letting go of it throws it. */
const THRESHOLD = 110
const THROW_MS = 220

type Drag = { id: number | null; x: number; y: number; start: { x: number; y: number } | null; thrown: -1 | 0 | 1 }
const rest = (id: number | null): Drag => ({ id, x: 0, y: 0, start: null, thrown: 0 })

export function DiceDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const state = useSyncExternalStore(dice.subscribe, dice.snapshot)
  const navigate = useNavigate()
  const pick = state.pick
  // Tagged with the card it belongs to, so the card dealt next starts at rest however far the
  // last was thrown.
  const [stored, setDrag] = useState<Drag>(rest(null))
  const drag = stored.id === (pick?.id ?? null) ? stored : rest(pick?.id ?? null)
  const progress = Math.max(-1, Math.min(1, drag.x / THRESHOLD))

  const throwCard = (direction: -1 | 1) => {
    if (!pick || drag.thrown) return
    setDrag({ ...drag, thrown: direction })
    window.setTimeout(() => {
      if (direction === 1) {
        onOpenChange(false)
        void navigate({ to: "/anime/$id", params: { id: String(pick.id) } })
        dice.opened()
      } else {
        dice.skipped()
      }
    }, THROW_MS)
  }

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (!pick || drag.thrown) return
    e.currentTarget.setPointerCapture(e.pointerId)
    setDrag({ ...drag, start: { x: e.clientX - drag.x, y: e.clientY - drag.y } })
  }
  const onPointerMove = (e: PointerEvent<HTMLDivElement>) => {
    if (!drag.start) return
    setDrag({ ...drag, x: e.clientX - drag.start.x, y: (e.clientY - drag.start.y) * 0.3 })
  }
  const onPointerUp = () => {
    if (!drag.start) return
    const moved = Math.abs(drag.x) + Math.abs(drag.y)
    if (moved < 6) {
      setDrag(rest(drag.id))
      throwCard(1)
    } else if (Math.abs(drag.x) >= THRESHOLD) {
      throwCard(drag.x > 0 ? 1 : -1)
    } else {
      setDrag(rest(drag.id))
    }
  }

  const cardStyle = drag.thrown
    ? { transform: `translateX(${drag.thrown * 140}%) rotate(${drag.thrown * 24}deg)`, transition: `transform ${THROW_MS}ms ease-in`, opacity: 0.6 }
    : {
        transform: `translate(${drag.x}px, ${drag.y}px) rotate(${drag.x / 18}deg)`,
        transition: drag.start ? "none" : "transform 200ms ease-out",
      }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        showCloseButton={false}
        data-bare
        className="flex w-auto max-w-none flex-col items-center gap-4 bg-transparent p-0 ring-0 sm:max-w-none"
        onKeyDown={(e) => {
          if (e.key === "ArrowLeft") throwCard(-1)
          if (e.key === "ArrowRight" || e.key === "Enter") throwCard(1)
        }}
      >
        <div className="flex w-full items-center gap-2">
          <DicesIcon className="size-5" />
          <DialogTitle className="font-heading text-lg">Random pick</DialogTitle>
          <DialogDescription className="sr-only">Swipe right or press → to open the title, left or ← to skip it.</DialogDescription>
          <Button variant="ghost" size="icon" className="ml-auto" aria-label="Close" onClick={() => onOpenChange(false)}>
            <XIcon />
          </Button>
        </div>

        <div className="relative h-[min(34rem,70vh)] w-[min(22.5rem,calc(70vh*0.66))]">
          {state.upNext && <CardFace pick={state.upNext} className="scale-95 opacity-60" />}
          {pick ? (
            <div
              className="absolute inset-0 cursor-grab touch-none select-none active:cursor-grabbing"
              style={cardStyle}
              onPointerDown={onPointerDown}
              onPointerMove={onPointerMove}
              onPointerUp={onPointerUp}
              onPointerCancel={() => setDrag(rest(drag.id))}
            >
              <CardFace pick={pick} progress={progress} />
            </div>
          ) : (
            <div className="absolute inset-0 flex flex-col items-center justify-center gap-3 rounded-[18px] surface p-6 text-center">
              <DicesIcon className={cn("size-12", state.rolling && "animate-spin [animation-duration:1.2s]")} />
              {state.failure != null && !state.rolling && (
                <>
                  <p className="text-sm text-muted-foreground">{toFailure(state.failure).message}</p>
                  <Button variant="secondary" onClick={() => void dice.roll()}>
                    Try again
                  </Button>
                </>
              )}
            </div>
          )}
        </div>

        <div className="flex items-center gap-3">
          <Button variant="outline" size="lg" disabled={!pick || state.rolling} onClick={() => throwCard(-1)}>
            <XIcon data-icon="inline-start" />
            Skip
          </Button>
          <Button size="lg" disabled={!pick || state.rolling} onClick={() => throwCard(1)}>
            <PlayIcon data-icon="inline-start" />
            Watch
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">Drag the card, or press ← to skip and → to open</p>
      </DialogContent>
    </Dialog>
  )
}

/** Cover, title, the facts that decide whether to bother, genres and a taste of the synopsis. */
function CardFace({ pick, progress = 0, className }: { pick: DicePick; progress?: number; className?: string }) {
  const episodes = pick.episodes ?? (pick.nextAiring ? pick.nextAiring.episode - 1 : null)
  const facts = [formatLabel(pick.format), pick.year, episodes ? `${episodes} episode${episodes === 1 ? "" : "s"}` : null].filter(Boolean)
  const synopsis = plainDescription(pick.description, 240)
  return (
    <div
      className={cn("absolute inset-0 overflow-hidden rounded-[18px] border border-white/10 bg-muted shadow-2xl", className)}
      style={pick.color ? { boxShadow: `0 24px 80px -24px ${pick.color}` } : undefined}
    >
      {pick.cover && <img src={pick.cover} alt="" draggable={false} className="size-full object-cover" />}
      {progress !== 0 && (
        <>
          <div className="absolute inset-0" style={{ backgroundColor: progress > 0 ? "#22c55e" : "#ef4444", opacity: Math.abs(progress) * 0.28 }} />
          <Stamp text="WATCH" color="#22c55e" strength={Math.max(0, progress)} className="top-8 left-5 -rotate-[18deg]" />
          <Stamp text="SKIP" color="#ef4444" strength={Math.max(0, -progress)} className="top-8 right-5 rotate-[18deg]" />
        </>
      )}
      <div className="absolute inset-x-0 bottom-0 flex flex-col gap-2 bg-linear-to-t from-black via-black/85 to-transparent px-5 pt-16 pb-5 text-white">
        <h3 className="line-clamp-2 font-heading text-2xl leading-tight font-extrabold [text-shadow:0_2px_8px_rgb(0_0_0/0.6)]">{pick.title}</h3>
        <p className="flex flex-wrap items-center gap-x-2 text-sm text-white/80">
          {facts.join(" · ")}
          {pick.score != null && (
            <span className="flex items-center gap-1">
              {facts.length > 0 && "·"}
              <StarIcon className="size-3.5 fill-current" />
              {(pick.score / 10).toFixed(1)}
            </span>
          )}
        </p>
        {pick.genres.length > 0 && (
          <div className="flex flex-wrap gap-1.5">
            {pick.genres.slice(0, 3).map((g) => (
              <Badge key={g} variant="secondary" className="bg-white/15 text-white">
                {g}
              </Badge>
            ))}
          </div>
        )}
        {synopsis && <p className="line-clamp-3 text-sm text-white/75">{synopsis}</p>}
      </div>
    </div>
  )
}

function Stamp({ text, color, strength, className }: { text: string; color: string; strength: number; className: string }) {
  return (
    <span
      className={cn("absolute rounded-lg border-4 px-3 py-1 font-heading text-2xl font-black tracking-widest", className)}
      style={{ color, borderColor: color, opacity: strength }}
    >
      {text}
    </span>
  )
}
