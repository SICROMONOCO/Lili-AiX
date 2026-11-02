// Updating, as Android does it: a notification that a version is out; a click opens this dialog,
// which downloads it with progress and then asks before installing. Closing the dialog doesn't stop
// a download, and a toast says when it's ready.

import type { QueryClient } from "@tanstack/react-query"
import { listen } from "@tauri-apps/api/event"
import { useSyncExternalStore } from "react"
import { toast } from "sonner"

import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Progress } from "@/components/ui/progress"
import { api } from "@/lib/api"
import { formatBytes } from "@/lib/downloads"
import { type Update, type UpdateStatus, errorMessage } from "@/lib/types"

type Phase = "idle" | "downloading" | "ready" | "installing" | "failed"
type State = { open: boolean; update: Update | null; phase: Phase; done: number; total: number; error: string }

let state: State = { open: false, update: null, phase: "idle", done: 0, total: 0, error: "" }
const listeners = new Set<() => void>()
const set = (patch: Partial<State>) => {
  state = { ...state, ...patch }
  for (const listener of listeners) listener()
}
const subscribe = (listener: () => void) => {
  listeners.add(listener)
  return () => void listeners.delete(listener)
}

/** The in-app "a version is out" toast's id: the dialog replaces it, so the two never show together. */
export const UPDATE_TOAST = "update-available"

/** Opens the dialog for `update`; `download` starts fetching it straight away. */
export function openUpdate(update: Update, { download = false } = {}) {
  toast.dismiss(UPDATE_TOAST)
  set({ open: true })
  if (state.update?.version !== update.version) set({ update, phase: "idle", done: 0, total: update.sizeBytes, error: "" })
  if (download && (state.phase === "idle" || state.phase === "failed")) startDownload()
}

/** A click on the update notification: the update the launch check found (or a fresh check). */
export async function openUpdateFromNotification(client: QueryClient) {
  const status =
    client.getQueryData<UpdateStatus>(["update"]) ?? (await client.fetchQuery({ queryKey: ["update"], queryFn: api.updateCheck }).catch(() => null))
  if (status?.update) openUpdate(status.update, { download: true })
}

function startDownload() {
  const update = state.update
  if (!update) return
  set({ phase: "downloading", done: 0, total: update.sizeBytes, error: "" })
  const progress = listen<[number, number]>("update-progress", (e) => set({ done: e.payload[0], total: e.payload[1] }))
  api
    .updateDownload(update)
    .then(() => {
      set({ phase: "ready" })
      if (!state.open) {
        toast(`Anilili ${update.version} is ready to install`, { action: { label: "Install", onClick: () => openUpdate(update) } })
      }
    })
    .catch((e) => set({ phase: "failed", error: errorMessage(e) }))
    .finally(() => void progress.then((stop) => stop()))
}

function install() {
  set({ phase: "installing" })
  api.updateApply().catch((e) => set({ phase: "failed", error: errorMessage(e) }))
}

export function UpdateDialog() {
  const { open, update, phase, done, total, error } = useSyncExternalStore(subscribe, () => state)
  if (!update) return null
  const percent = total > 0 ? Math.min(100, Math.round((done / total) * 100)) : 0
  return (
    <Dialog open={open} onOpenChange={(next) => set({ open: next })}>
      <DialogContent className="max-h-[calc(100dvh-2rem)] overflow-y-auto sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>{phase === "ready" || phase === "installing" ? `Install Anilili ${update.version}?` : `Anilili ${update.version} is available`}</DialogTitle>
          <DialogDescription>What's new in this update</DialogDescription>
        </DialogHeader>

        <div
          role="region"
          aria-label="Release notes"
          tabIndex={0}
          className="max-h-[min(40dvh,20rem)] overflow-y-auto overscroll-contain rounded-sm pr-3 text-sm leading-relaxed text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
        >
          <ReleaseNotes changelog={update.changelog} />
        </div>

        {phase === "downloading" && (
          <div className="flex flex-col gap-2">
            <Progress value={percent} />
            <p className="text-sm text-muted-foreground tabular-nums">
              Downloading… {formatBytes(done)} of {formatBytes(total)} ({percent}%)
            </p>
          </div>
        )}
        {(phase === "ready" || phase === "installing") && (
          <p className="text-sm text-muted-foreground">Downloaded and checked. Anilili closes to install it and opens again when it's done.</p>
        )}
        {phase === "failed" && <p className="text-sm text-destructive">The update couldn't be installed: {error}</p>}

        <DialogFooter>
          {phase === "downloading" ? (
            <Button variant="outline" onClick={() => set({ open: false })}>
              Hide
            </Button>
          ) : (
            <Button variant="outline" disabled={phase === "installing"} onClick={() => set({ open: false })}>
              Later
            </Button>
          )}
          {(phase === "idle" || phase === "failed") && <Button onClick={startDownload}>{phase === "failed" ? "Try again" : "Download and install"}</Button>}
          {(phase === "ready" || phase === "installing") && (
            <Button loading={phase === "installing"} onClick={install}>
              Install and restart
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

function ReleaseNotes({ changelog }: { changelog: string }) {
  const blocks: { bullets: boolean; lines: string[] }[] = []
  for (const line of changelog.split(/\r?\n/).map((line) => line.trim()).filter(Boolean)) {
    const bullet = /^[-*•]\s+/.test(line)
    const text = bullet ? line.replace(/^[-*•]\s+/, "") : line
    const previous = blocks.at(-1)
    if (bullet && previous?.bullets) previous.lines.push(text)
    else blocks.push({ bullets: bullet, lines: [text] })
  }
  if (!blocks.length) return <p>A new version of Anilili.</p>
  return (
    <div className="flex flex-col gap-3">
      {blocks.map((block, index) => block.bullets ? (
        <ul key={index} className="flex list-disc flex-col gap-2 pl-5">
          {block.lines.map((line, item) => <li key={item} className="break-words pl-1">{line}</li>)}
        </ul>
      ) : <p key={index} className="break-words">{block.lines[0]}</p>)}
    </div>
  )
}
