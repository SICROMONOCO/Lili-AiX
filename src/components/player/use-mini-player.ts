import { LogicalSize, type PhysicalPosition, type PhysicalSize } from "@tauri-apps/api/dpi"
import { getCurrentWindow } from "@tauri-apps/api/window"
import { useCallback, useEffect, useMemo, useRef, useState } from "react"

const MINI = new LogicalSize(480, 270)
const MINI_MIN = new LogicalSize(320, 180)
const NORMAL_MIN = new LogicalSize(800, 480)

type Saved = { size: PhysicalSize; position: PhysicalPosition; maximized: boolean }

/**
 * Mini player: the window itself becomes a small, borderless, always-on-top video window, and
 * goes back exactly where it was (size, position, maximised) on exit or when the player closes.
 */
export function useMiniPlayer() {
  const [mini, setMini] = useState(false)
  const saved = useRef<Saved | null>(null)

  const enter = useCallback(async () => {
    const w = getCurrentWindow()
    if (await w.isFullscreen()) await w.setFullscreen(false)
    saved.current = { size: await w.innerSize(), position: await w.outerPosition(), maximized: await w.isMaximized() }
    if (saved.current.maximized) await w.unmaximize()
    await w.setDecorations(false)
    await w.setMinSize(MINI_MIN)
    await w.setSize(MINI)
    await w.setAlwaysOnTop(true)
    setMini(true)
  }, [])

  const exit = useCallback(async () => {
    const w = getCurrentWindow()
    const restore = saved.current
    saved.current = null
    await w.setAlwaysOnTop(false)
    await w.setDecorations(true)
    await w.setMinSize(NORMAL_MIN)
    if (restore) {
      await w.setSize(restore.size)
      await w.setPosition(restore.position)
      if (restore.maximized) await w.maximize()
    }
    setMini(false)
  }, [])

  // Never leave the app stuck as a tiny borderless window.
  useEffect(
    () => () => {
      if (saved.current) void exit()
    },
    [exit],
  )

  const toggle = useCallback(() => void (mini ? exit() : enter()), [mini, enter, exit])
  return useMemo(() => ({ mini, enter, exit, toggle }), [mini, enter, exit, toggle])
}
