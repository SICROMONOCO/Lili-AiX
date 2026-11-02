import { useNavigate } from "@tanstack/react-router"
import { toast } from "sonner"

import { OpenStream } from "@/components/player/open-stream"
import { PlayerSurface } from "@/components/player/player-surface"
import { usePlayer, useStopOnLeave } from "@/lib/player"

/** Plays anything: a dropped local file or a pasted stream URL. */
export function PlayerPage() {
  const navigate = useNavigate()
  const state = usePlayer()
  useStopOnLeave()
  return (
    <PlayerSurface
      state={state}
      title={state.title || "Player"}
      onBack={() => void navigate({ to: "/" })}
      idle={<OpenStream onError={(m) => toast.error(m)} />}
    />
  )
}
