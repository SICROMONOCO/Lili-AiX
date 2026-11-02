// A card's right-click menu: Android's long-press list menu (AnimeListActions.kt and, for Continue
// watching, ContinueWatchingActions.kt). Details, the list destinations with the current one
// marked, and removal; a row can add its own actions (resume, take off Continue watching).

import { useNavigate } from "@tanstack/react-router"
import {
  BookmarkIcon,
  CheckCircle2Icon,
  InfoIcon,
  LinkIcon,
  PauseCircleIcon,
  PlayIcon,
  Trash2Icon,
  XCircleIcon,
} from "lucide-react"
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
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import { useLibraryStatus, useMalUser, useSetLibrary, useViewer } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import { animeLink, copyLink } from "@/lib/links"
import type { AnimeCard, LibraryStatus } from "@/lib/types"

/** Android's list destinations (`listDestinations`), as the desktop library names them. */
export const LIST_DESTINATIONS: { value: LibraryStatus; label: string; hint: string; icon: React.ReactNode }[] = [
  { value: "watching", label: "Watching", hint: "Working through it now", icon: <PlayIcon /> },
  { value: "planning", label: "Plan to watch", hint: "Saved for later", icon: <BookmarkIcon /> },
  { value: "paused", label: "Paused", hint: "On hold for now", icon: <PauseCircleIcon /> },
  { value: "completed", label: "Completed", hint: "Finished every episode", icon: <CheckCircle2Icon /> },
  { value: "dropped", label: "Dropped", hint: "Given up on", icon: <XCircleIcon /> },
]

export type CardAction = { label: string; icon: React.ReactNode; onSelect: () => void; destructive?: boolean }

/**
 * Wraps a card so a right-click opens its menu. `actions` come first (the row's own: Resume, Remove
 * from Continue watching), then Details, the lists and the link.
 */
export function AnimeContextMenu({ anime, actions, children }: { anime: AnimeCard; actions?: CardAction[]; children: React.ReactNode }) {
  const [confirmRemove, setConfirmRemove] = useState(false)
  const setLibrary = useSetLibrary()
  return (
    <>
      <ContextMenu>
        {/* `contents`: the card keeps its own box; the right-click still bubbles up to here, and goes
            no further, so the app's own menu stays shut. */}
        <ContextMenuTrigger className="contents" onContextMenu={(e) => e.stopPropagation()}>
          {children}
        </ContextMenuTrigger>
        <ContextMenuContent className="w-60">
          <Menu anime={anime} actions={actions} onRemove={() => setConfirmRemove(true)} />
        </ContextMenuContent>
      </ContextMenu>
      <AlertDialog open={confirmRemove} onOpenChange={setConfirmRemove}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove from your list?</AlertDialogTitle>
            <AlertDialogDescription>
              {anime.title} comes off this library and off your account, with its progress and score. The account keeps no undo.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                setConfirmRemove(false)
                setLibrary.mutate({ anime, status: null }, { onError: (e) => toastFailure(e) })
              }}
            >
              Remove
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  )
}

/** Mounted only while the menu is open, so a grid of cards doesn't ask for every title's status. */
function Menu({ anime, actions, onRemove }: { anime: AnimeCard; actions?: CardAction[]; onRemove: () => void }) {
  const navigate = useNavigate()
  const status = useLibraryStatus(anime.id).data ?? null
  const setLibrary = useSetLibrary()
  const viewer = useViewer().data
  const malUser = useMalUser().data
  const account = viewer ? "AniList" : malUser ? "MyAnimeList" : null
  const move = (value: LibraryStatus) => {
    if (value === status) return
    setLibrary.mutate({ anime, status: value }, { onError: (e) => toastFailure(e) })
  }
  return (
    <>
      <ContextMenuGroup>
        <ContextMenuLabel className="truncate">{anime.title}</ContextMenuLabel>
        {actions?.map((a) => (
          <ContextMenuItem key={a.label} variant={a.destructive ? "destructive" : "default"} onClick={a.onSelect}>
            {a.icon}
            {a.label}
          </ContextMenuItem>
        ))}
        <ContextMenuItem onClick={() => void navigate({ to: "/anime/$id", params: { id: String(anime.id) } })}>
          <InfoIcon />
          Details
        </ContextMenuItem>
      </ContextMenuGroup>
      <ContextMenuSeparator />
      <ContextMenuGroup>
        <ContextMenuLabel>{account ? `Move on ${account}` : "Your library"}</ContextMenuLabel>
        <ContextMenuRadioGroup value={status ?? ""} onValueChange={(v) => move(v as LibraryStatus)}>
          {LIST_DESTINATIONS.map((d) => (
            <ContextMenuRadioItem key={d.value} value={d.value} closeOnClick>
              {d.icon}
              {d.label}
            </ContextMenuRadioItem>
          ))}
        </ContextMenuRadioGroup>
        {status && (
          <ContextMenuItem
            variant="destructive"
            onClick={() => (account ? onRemove() : setLibrary.mutate({ anime, status: null }, { onError: (e) => toastFailure(e) }))}
          >
            <Trash2Icon />
            Remove from list
          </ContextMenuItem>
        )}
      </ContextMenuGroup>
      <ContextMenuSeparator />
      <ContextMenuItem onClick={() => copyLink(animeLink(anime.id))}>
        <LinkIcon />
        Copy link
      </ContextMenuItem>
    </>
  )
}
