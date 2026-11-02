import { useNavigate } from "@tanstack/react-router"
import { useQuery } from "@tanstack/react-query"

import { PlayerSurface } from "@/components/player/player-surface"
import { Button } from "@/components/ui/button"
import { Card, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card"
import { api, useDownloads } from "@/lib/api"
import { episodeLabel } from "@/lib/format"
import { usePlayer, useStopOnLeave } from "@/lib/player"
import { errorMessage } from "@/lib/types"

/**
 * Plays a finished download, with progress tracked like a stream. Previous and next move between
 * this title's other finished downloads in the same audio, as the stream player moves between
 * episodes.
 */
export function OfflinePage({ id }: { id: number }) {
  const navigate = useNavigate()
  const state = usePlayer()
  const record = useQuery({
    queryKey: ["offline", id],
    queryFn: () => api.downloadPlay(id),
    retry: false,
    staleTime: Infinity,
    gcTime: 0,
  })
  useStopOnLeave()
  const back = () => void navigate({ to: "/downloads" })

  const current = record.data
  const siblings = (useDownloads().data ?? [])
    .filter((d) => current && d.status === "done" && d.animeId === current.animeId && d.audio === current.audio)
    .sort((a, b) => a.episode - b.episode)
  const at = siblings.findIndex((d) => d.id === id)
  const previous = at > 0 ? siblings[at - 1] : undefined
  const next = at >= 0 ? siblings[at + 1] : undefined
  const go = (to: number) => void navigate({ to: "/offline/$id", params: { id: String(to) }, replace: true })

  return (
    <PlayerSurface
      state={state}
      title={current?.title ?? "Offline"}
      subtitle={current ? `${episodeLabel(current.episode)} · offline` : undefined}
      onBack={back}
      busy={record.isPending}
      episodic={siblings.length > 1}
      onPrevious={previous ? () => go(previous.id) : undefined}
      onNext={next ? () => go(next.id) : undefined}
      onKey={(e) => {
        if (e.key === "n" && next) go(next.id)
        else if (e.key === "p" && previous) go(previous.id)
        else return false
        return true
      }}
      overlay={
        record.isError && (
          <Card className="absolute top-1/2 left-1/2 w-96 -translate-1/2">
            <CardHeader>
              <CardTitle>Can't play this download</CardTitle>
              <CardDescription>{errorMessage(record.error)}</CardDescription>
            </CardHeader>
            <CardFooter>
              <Button onClick={back}>Back to downloads</Button>
            </CardFooter>
          </Card>
        )
      }
    />
  )
}
