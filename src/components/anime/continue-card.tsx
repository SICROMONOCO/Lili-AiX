// A Continue watching card, as Android's ContinueRail draws one: the artwork at 16:9 with a play
// mark (a click resumes the episode), Details and Remove in its corners, the progress along its
// foot, and the title and episode under it. Right-click adds the list moves (anime-actions.tsx).

import { useNavigate } from "@tanstack/react-router"
import { useQueryClient } from "@tanstack/react-query"
import { InfoIcon, PlayIcon, XIcon } from "lucide-react"
import { useState } from "react"

import { AnimeContextMenu } from "@/components/anime/anime-actions"
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
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import { api, keys, useMalUser, useViewer } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import { episodeLabel } from "@/lib/format"
import type { ContinueEntry } from "@/lib/types"

export function ContinueCard({ entry }: { entry: ContinueEntry }) {
  const { anime, progress } = entry
  const navigate = useNavigate()
  const client = useQueryClient()
  const [removing, setRemoving] = useState(false)
  const viewer = useViewer().data
  const malUser = useMalUser().data
  const account = viewer ? "AniList" : malUser ? "MyAnimeList" : null
  const fraction = progress.duration > 0 ? Math.min(1, progress.position / progress.duration) : 0
  const resume = () =>
    void navigate({ to: "/watch/$id/$episode", params: { id: String(anime.id), episode: String(progress.episode) } })
  const details = () => void navigate({ to: "/anime/$id", params: { id: String(anime.id) } })
  // Only this row: progress and the account's list stay as they are (Android's removeHistory).
  const remove = () =>
    api
      .continueHide(anime.id)
      .then(() => client.invalidateQueries({ queryKey: keys.continue }))
      .catch(toastFailure)

  return (
    <>
      <AnimeContextMenu
        anime={anime}
        actions={[
          { label: `Resume ${episodeLabel(progress.episode)}`, icon: <PlayIcon />, onSelect: resume },
          { label: "Remove from Continue watching", icon: <XIcon />, onSelect: () => setRemoving(true), destructive: true },
        ]}
      >
        <div className="group flex w-64 shrink-0 flex-col gap-1.5">
          <div className="relative aspect-video overflow-hidden rounded-lg bg-muted" style={{ backgroundColor: anime.color ?? undefined }}>
            <button
              type="button"
              aria-label={`Resume ${anime.title}`}
              onClick={resume}
              className="absolute inset-0 outline-none focus-visible:ring-3 focus-visible:ring-ring/60"
            >
              {(anime.banner ?? anime.cover) && (
                <img
                  src={anime.banner ?? anime.cover ?? undefined}
                  alt=""
                  loading="lazy"
                  className="size-full object-cover transition duration-300 group-hover:scale-105"
                />
              )}
              <span className="absolute inset-0 flex items-center justify-center bg-black/30 transition-colors group-hover:bg-black/45">
                <PlayIcon className="size-9 fill-white text-white drop-shadow" />
              </span>
            </button>
            {/* What anyone actually does to a card here, on the card (Android's corner actions). */}
            <CornerAction label={`Details for ${anime.title}`} className="top-1.5 left-1.5" onClick={details}>
              <InfoIcon />
            </CornerAction>
            <CornerAction label="Remove from Continue watching" className="top-1.5 right-1.5" onClick={() => setRemoving(true)}>
              <XIcon />
            </CornerAction>
            <div className="pointer-events-none absolute inset-x-0 bottom-0 h-1 bg-white/25">
              <div className="h-full bg-primary" style={{ width: `${Math.max(3, fraction * 100)}%` }} />
            </div>
          </div>
          <span className="truncate text-sm font-medium" title={anime.title}>{anime.title}</span>
          <span className="-mt-1 text-xs text-muted-foreground">{episodeLabel(progress.episode)}</span>
        </div>
      </AnimeContextMenu>
      {/* Asked first: the ✕ sits over the artwork that resumes, and a slightly wide click took the
          title off the row with no way back. */}
      <AlertDialog open={removing} onOpenChange={setRemoving}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Remove {anime.title}?</AlertDialogTitle>
            <AlertDialogDescription>
              It comes off this row only{account ? `. Your ${account} list stays as it is.` : "."}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => {
                setRemoving(false)
                void remove()
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

function CornerAction({ label, className, onClick, children }: { label: string; className: string; onClick: () => void; children: React.ReactNode }) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            aria-label={label}
            onClick={onClick}
            className={`surface-chip absolute flex size-7.5 items-center justify-center rounded-full text-white opacity-90 outline-none hover:opacity-100 focus-visible:ring-3 focus-visible:ring-ring/60 [&_svg]:size-4 ${className}`}
          />
        }
      >
        {children}
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  )
}
