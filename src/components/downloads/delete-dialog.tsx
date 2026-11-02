// The downloads deletion confirms: "Delete these episodes?" for a picked set, and "Remove all of
// this anime's downloads?" for the whole title. The files go from this computer; progress and the
// list entry stay.

import { useState } from "react"

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { api } from "@/lib/api"
import { formatBytes } from "@/lib/downloads"
import { toastFailure } from "@/lib/failure"
import { episodeLabel } from "@/lib/format"
import type { DownloadRecord } from "@/lib/types"
import { episodeRanges } from "@/components/downloads/shared"

export function DeleteDialog({
  episodes,
  title,
  onClose,
  onDeleted,
}: {
  episodes: DownloadRecord[]
  title: string
  onClose: () => void
  onDeleted?: () => void
}) {
  const [busy, setBusy] = useState(false)
  const bytes = episodes.reduce((sum, e) => sum + e.bytes, 0)
  const which =
    episodes.length === 1 ? episodeLabel(episodes[0].episode) : `${episodes.length} episodes (${episodeRanges(episodes.map((e) => e.episode))})`
  const remove = async () => {
    setBusy(true)
    try {
      for (const episode of episodes) await api.downloadRemove(episode.id, true)
      onDeleted?.()
      onClose()
    } catch (e) {
      toastFailure(e)
    } finally {
      setBusy(false)
    }
  }
  return (
    <AlertDialog open onOpenChange={(open) => !open && !busy && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            Delete {which} of {title}?
          </AlertDialogTitle>
          <AlertDialogDescription>
            {formatBytes(bytes)} comes off this computer. Where you stopped watching, and the title in your lists, stay.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={busy}>Keep</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={busy} onClick={() => void remove()}>
            Delete
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

/** Everything one anime has in downloads, gone in one go: saved files deleted, what is still
 * fetching stopped (`download_remove` cancels a running one itself), failed records forgotten. */
export function RemoveAllDialog({
  title,
  saved,
  pending,
  failed,
  onClose,
}: {
  title: string
  /** Finished downloads, whose files leave this computer. */
  saved: DownloadRecord[]
  /** Downloading or waiting, which are stopped. */
  pending: DownloadRecord[]
  /** Failed or stopped, which are only forgotten. */
  failed: DownloadRecord[]
  onClose: () => void
}) {
  const [busy, setBusy] = useState(false)
  const bytes = saved.reduce((sum, e) => sum + e.bytes, 0)
  const happening = [
    saved.length > 0 &&
      `${saved.length} saved ${saved.length === 1 ? "episode" : "episodes"} (${formatBytes(bytes)}) ${saved.length === 1 ? "comes" : "come"} off this computer`,
    pending.length > 0 && `${pending.length} on their way ${pending.length === 1 ? "is" : "are"} stopped`,
    failed.length > 0 && `${failed.length} failed ${failed.length === 1 ? "is" : "are"} forgotten`,
  ]
    .filter(Boolean)
    .join(", ")
  const remove = async () => {
    setBusy(true)
    try {
      for (const episode of saved) await api.downloadRemove(episode.id, true)
      for (const row of [...pending, ...failed]) await api.downloadRemove(row.id, false)
      onClose()
    } catch (e) {
      toastFailure(e)
    } finally {
      setBusy(false)
    }
  }
  return (
    <AlertDialog open onOpenChange={(open) => !open && !busy && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Remove all downloads of {title}?</AlertDialogTitle>
          <AlertDialogDescription>
            {happening}. Where you stopped watching, and the title in your lists, stay.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={busy}>Keep</AlertDialogCancel>
          <AlertDialogAction variant="destructive" disabled={busy} onClick={() => void remove()}>
            Remove all
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}
