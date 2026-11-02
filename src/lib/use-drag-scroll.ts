// Click-and-drag to scroll a horizontal row with the mouse, as on a touch screen. Scrollbars are
// hidden app-wide, so without this a mouse could only move a row with its arrows or Shift+wheel.
// Touch and pen keep the browser's own panning; trackpads keep their two-finger swipe.

import { useEffect, useRef } from "react"

/** A drag starts after this many pixels, so an ordinary click on a card still opens it. */
const THRESHOLD = 6

export function useDragScroll<T extends HTMLElement>() {
  const ref = useRef<T>(null)
  useEffect(() => {
    const el = ref.current
    if (!el) return
    let down = false
    let dragged = false
    let startX = 0
    let startLeft = 0

    const onDown = (e: PointerEvent) => {
      if (e.pointerType !== "mouse" || e.button !== 0) return
      down = true
      dragged = false
      startX = e.clientX
      startLeft = el.scrollLeft
    }
    const onMove = (e: PointerEvent) => {
      if (!down) return
      const dx = e.clientX - startX
      if (!dragged && Math.abs(dx) > THRESHOLD) {
        dragged = true
        // Smooth scrolling would trail the pointer.
        el.style.scrollBehavior = "auto"
        el.style.cursor = "grabbing"
        document.body.style.userSelect = "none"
      }
      if (dragged) el.scrollLeft = startLeft - dx
    }
    const onUp = () => {
      if (!down) return
      down = false
      if (dragged) {
        el.style.scrollBehavior = ""
        el.style.cursor = ""
        document.body.style.userSelect = ""
      }
    }
    // The click that ends a drag must not open the card the drag started on.
    const onClick = (e: MouseEvent) => {
      if (dragged) {
        e.preventDefault()
        e.stopPropagation()
        dragged = false
      }
    }
    // Images and links would otherwise start the browser's own drag-and-drop.
    const onDragStart = (e: DragEvent) => e.preventDefault()

    el.addEventListener("pointerdown", onDown)
    window.addEventListener("pointermove", onMove)
    window.addEventListener("pointerup", onUp)
    window.addEventListener("pointercancel", onUp)
    el.addEventListener("click", onClick, true)
    el.addEventListener("dragstart", onDragStart)
    return () => {
      el.removeEventListener("pointerdown", onDown)
      window.removeEventListener("pointermove", onMove)
      window.removeEventListener("pointerup", onUp)
      window.removeEventListener("pointercancel", onUp)
      el.removeEventListener("click", onClick, true)
      el.removeEventListener("dragstart", onDragStart)
    }
  }, [])
  return ref
}
