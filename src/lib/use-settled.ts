import { useEffect, useState } from "react"

/**
 * A search box's text once typing pauses. A search per keystroke sent AniList nine requests for
 * "One Piece", which runs into its rate limit, and the half-typed words find nothing there anyway.
 */
export function useSettled(value: string, ms = 350) {
  const [settled, setSettled] = useState(value)
  useEffect(() => {
    const timer = setTimeout(() => setSettled(value), ms)
    return () => clearTimeout(timer)
  }, [value, ms])
  return settled
}
