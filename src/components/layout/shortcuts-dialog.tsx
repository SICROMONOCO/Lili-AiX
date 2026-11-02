import { useEffect, useState } from "react"

import { SHORTCUTS_EVENT } from "@/components/layout/app-context-menu"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Kbd, KbdGroup } from "@/components/ui/kbd"
import { cn } from "@/lib/utils"

/** The search shortcut's modifier as the platform's keyboard prints it (either works). */
const MOD = /Mac/i.test(navigator.platform) ? "⌘" : "Ctrl"

/** Each shortcut's keys: the alternatives, each the keys pressed together. */
const SHORTCUTS: { keys: string[][]; action: string }[] = [
  { keys: [["Space"], ["K"]], action: "Play or pause" },
  { keys: [["Hold Space"]], action: "Play at 2× while held" },
  { keys: [["←"], ["→"]], action: "Back or forward 5 seconds" },
  { keys: [["J"], ["L"]], action: "Back or forward 10 seconds" },
  { keys: [["↑"], ["↓"]], action: "Volume" },
  { keys: [["M"]], action: "Mute" },
  { keys: [["F"]], action: "Fullscreen" },
  { keys: [["I"]], action: "Mini player" },
  { keys: [["N"], ["P"]], action: "Next or previous episode" },
  { keys: [["Enter"]], action: "Skip the intro or ending" },
  { keys: [["Z"], ["X"]], action: "Subtitles earlier or later" },
  { keys: [["["], ["]"]], action: "Slower or faster" },
  { keys: [["S"]], action: "Screenshot" },
  { keys: [[MOD, "K"]], action: "Search" },
  { keys: [["?"]], action: "This list" },
]

/** `?` anywhere shows the keyboard shortcuts. */
export function ShortcutsDialog() {
  const [open, setOpen] = useState(false)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null
      if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return
      if (e.key === "?") setOpen((o) => !o)
    }
    // The app's right-click menu opens it too.
    const onOpen = () => setOpen(true)
    window.addEventListener("keydown", onKey)
    window.addEventListener(SHORTCUTS_EVENT, onOpen)
    return () => {
      window.removeEventListener("keydown", onKey)
      window.removeEventListener(SHORTCUTS_EVENT, onOpen)
    }
  }, [])
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Keyboard shortcuts</DialogTitle>
          <DialogDescription>Player shortcuts work while something is playing.</DialogDescription>
        </DialogHeader>
        <ShortcutList />
      </DialogContent>
    </Dialog>
  )
}

/** The keys and what they do, drawn as keycaps; also Settings → About. */
export function ShortcutList({ className }: { className?: string }) {
  return (
    <dl className={cn("grid grid-cols-[auto_1fr] items-center gap-x-6 gap-y-2.5 text-sm", className)}>
      {SHORTCUTS.map(({ keys, action }) => (
        <div key={action} className="contents">
          <dt className="flex items-center gap-1.5">
            {keys.map((chord, i) => (
              <span key={chord.join("+")} className="flex items-center gap-1.5">
                {i > 0 && <span className="text-xs text-muted-foreground">/</span>}
                <KbdGroup>
                  {chord.map((key) => (
                    <Kbd key={key} className="h-6 min-w-6 rounded-md border border-b-2 border-border bg-muted/60 px-1.5 text-foreground shadow-xs">
                      {key}
                    </Kbd>
                  ))}
                </KbdGroup>
              </span>
            ))}
          </dt>
          <dd className="text-muted-foreground">{action}</dd>
        </div>
      ))}
    </dl>
  )
}
