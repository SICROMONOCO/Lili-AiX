import { QueryCache, QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { RouterProvider } from "@tanstack/react-router"
import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "./index.css"
import { Toaster } from "@/components/ui/sonner"
import { TooltipProvider } from "@/components/ui/tooltip"
import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

import { openUpdateFromNotification } from "@/components/layout/update-dialog"
import { toastFailure } from "@/lib/failure"
import { router } from "@/router"

const queryClient = new QueryClient({
  queryCache: import.meta.env.DEV
    ? new QueryCache({
        // Dev: query errors reach the app log, where the webview console doesn't.
        onError: (error, query) => void invoke("dev_log", { message: `query ${query.queryHash}: ${error}` }).catch(() => {}),
      })
    : undefined,
  defaultOptions: {
    queries: {
      // The Rust side caches and falls back to stale data itself; one retry covers a blip.
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
})

// Dark only (`class="dark"` on <html>): the UI floats over video.
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delay={400}>
        <RouterProvider router={router} />
        <Toaster position="top-center" />
      </TooltipProvider>
    </QueryClientProvider>
  </StrictMode>
)

// The route an `anilili://` link launched the app with (or, in debug builds, ANILILI_ROUTE), and
// links opened while it runs. Routes can carry a query (`/watch/1/2?t=60`), so they go to history.
void invoke<string | null>("initial_route")
  .then((route) => {
    if (route === "/update") void openUpdateFromNotification(queryClient)
    else if (route) router.history.push(route)
  })
  .catch(toastFailure)
// `/update` is the update notification's click: the update dialog, not a page (updates.rs).
void listen<string>("open-route", (e) =>
  e.payload === "/update" ? void openUpdateFromNotification(queryClient) : router.history.push(e.payload),
)
// Linux (WebKitGTK) draws the glass without its blur (src/index.css).
if (/Linux/.test(navigator.userAgent)) document.documentElement.dataset.platform = "linux"
// macOS puts its own glass under the window (src-tauri/src/glass.rs); the CSS lets it through.
void invoke<boolean>("native_glass").then((native) => {
  if (native) document.documentElement.dataset.nativeGlass = "on"
})
if (import.meta.env.DEV) {
  // `scroll:600` scrolls the page instead, for captures of what's below the fold; `row-scroll:0:end`
  // scrolls the page's nth row, top first (`end` or pixels) — Home's lazy loading check; `dice`
  // opens the dice; `hover-seek:0.4` holds the pointer 40% along the player's seek bar (its preview).
  const rowScroll = (index: number, to: string) => {
    const el = [...document.querySelectorAll<HTMLElement>("main .no-scrollbar")][index]
    el?.scrollTo({ left: to === "end" ? el.scrollWidth : el.scrollLeft + Number(to) })
  }
  const hoverSeek = (fraction: number) => {
    const control = document.querySelector<HTMLElement>(".group\\/seek > *")
    const box = control?.getBoundingClientRect()
    if (!control || !box) return
    const at = { bubbles: true, clientX: box.left + box.width * fraction, clientY: box.top + box.height / 2 }
    control.dispatchEvent(new MouseEvent("mousemove", at)) // keeps the controls up
    control.dispatchEvent(new PointerEvent("pointermove", at))
  }
  void listen<string>("dev-navigate", (e) =>
    e.payload === "dice"
      ? window.dispatchEvent(new Event("anilili:dice"))
      : e.payload.startsWith("hover-seek:")
        ? hoverSeek(Number(e.payload.slice(11)))
        : e.payload.startsWith("scroll:")
          ? document.querySelector("main")?.scrollTo({ top: Number(e.payload.slice(7)) })
          : e.payload.startsWith("row-scroll:")
          ? (([index, to]) => rowScroll(Number(index), to))(e.payload.slice(11).split(":"))
          : e.payload.startsWith("click:")
            ? [...document.querySelectorAll<HTMLElement>("button, a, [role=button], [role=menuitem], [role=menuitemradio], [role=switch], [role=checkbox], [role=tab]")]
                .find((el) => el.textContent?.trim() === e.payload.slice(6) || el.getAttribute("aria-label") === e.payload.slice(6))
                ?.click()
            : router.history.push(e.payload),
  )
  // After each navigation: how long until no skeleton is left on the page, and how smooth the
  // frames were meanwhile (gaps over 50 ms are janks), into the app log (`dev_log`).
  router.subscribe("onResolved", ({ toLocation }) => {
    const start = performance.now()
    let last = start
    let settled: number | null = null
    let frames = 0
    let janks = 0
    let worst = 0
    const tick = (now: number) => {
      const gap = now - last
      last = now
      frames++
      if (gap > 50) janks++
      worst = Math.max(worst, gap)
      if (settled === null && !document.querySelector('main [data-slot="skeleton"]')) settled = now - start
      if (now - start < 6000) requestAnimationFrame(tick)
      else
        void invoke("dev_log", {
          message: `page ${toLocation.pathname}: settled ${settled === null ? "never" : Math.round(settled) + " ms"}, ${frames} frames in 6 s, ${janks} over 50 ms, worst ${Math.round(worst)} ms`,
        })
    }
    requestAnimationFrame(tick)
  })
  void listen<{ cmd: string; args?: Record<string, unknown> }>("dev-invoke", (e) =>
    invoke(e.payload.cmd, e.payload.args)
      .then((r) => console.info("dev-invoke", e.payload.cmd, r))
      .catch((err) => console.error("dev-invoke", e.payload.cmd, err)),
  )
}
