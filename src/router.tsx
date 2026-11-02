import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { getCurrentWebview } from "@tauri-apps/api/webview"
import { Outlet, createRootRoute, createRoute, createRouter, useNavigate } from "@tanstack/react-router"
import { useEffect } from "react"
import { toast } from "sonner"

import { AppShell } from "@/components/layout/app-shell"
import { NotFound, RouteError } from "@/components/layout/route-states"
import { ShortcutsDialog } from "@/components/layout/shortcuts-dialog"
import { useAppearance } from "@/lib/api"
import { player } from "@/lib/player"
import { applyAppearance } from "@/lib/theme"
import { AnimePage } from "@/pages/anime"
import { DownloadsPage } from "@/pages/downloads"
import { DownloadsQueuePage } from "@/pages/downloads-queue"
import { DownloadSeriesPage } from "@/pages/downloads-series"
import { AppContextMenu } from "@/components/layout/app-context-menu"
import { HomePage } from "@/pages/home"
import { LibraryPage } from "@/pages/library"
import { NotificationsPage } from "@/pages/notifications"
import { OfflinePage } from "@/pages/offline"
import { PlayerPage } from "@/pages/player"
import { SchedulePage } from "@/pages/schedule"
import { SearchPage } from "@/pages/search"
import { ProfilePage } from "@/pages/profile"
import { SettingsPage } from "@/pages/settings"
import { WatchPage } from "@/pages/watch"
import { EmbedPlayer } from "@/components/player/embed-player"
import { toastFailure } from "@/lib/failure"

const VIDEO_EXT = /\.(mkv|mp4|webm|avi|mov|m4v|ts|m3u8|mpd|flv|wmv)$/i
const SUB_EXT = /\.(ass|ssa|srt|vtt|sub)$/i

/** Drag and drop anywhere: videos open in the player, subtitle files attach to what plays. */
function Root() {
  const navigate = useNavigate()
  useEffect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return
      for (const path of event.payload.paths) {
        if (SUB_EXT.test(path)) {
          void player
            .control({ type: "addSubtitle", url: path })
            .then(() => toast.success("Subtitle added"))
            .catch((e) => toastFailure(e))
        } else if (VIDEO_EXT.test(path)) {
          void navigate({ to: "/player" }).then(() => player.load({ url: path }).catch((e) => toastFailure(e)))
        }
      }
    })
    const openFile = (path: string) =>
      void navigate({ to: "/player" }).then(() => player.load({ url: path }).catch((e) => toastFailure(e)))
    const opened = listen<string>("open-file", (e) => openFile(e.payload))
    void invoke<string | null>("launch_file").then((path) => {
      if (path) openFile(path)
    })
    return () => {
      void unlisten.then((fn) => fn())
      void opened.then((fn) => fn())
    }
  }, [navigate])
  return (
    <>
      <AppearanceApplier />
      <AppContextMenu>
        <Outlet />
      </AppContextMenu>
      <ShortcutsDialog />
    </>
  )
}

/** Keeps the document's colours and interface size in step with the appearance settings. */
function AppearanceApplier() {
  const appearance = useAppearance()
  useEffect(() => applyAppearance(appearance), [appearance])
  return null
}

const rootRoute = createRootRoute({ component: Root })

const shellRoute = createRoute({ getParentRoute: () => rootRoute, id: "shell", component: AppShell })

const homeRoute = createRoute({ getParentRoute: () => shellRoute, path: "/", component: HomePage })

const searchRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/search",
  validateSearch: (search: Record<string, unknown>): { genre?: string; q?: string } => ({
    genre: typeof search.genre === "string" ? search.genre : undefined,
    // TanStack parses `?q=86` as a number.
    q: typeof search.q === "string" || typeof search.q === "number" ? String(search.q) : undefined,
  }),
  component: function Search() {
    const { genre, q } = searchRoute.useSearch()
    return <SearchPage key={`${genre ?? ""}|${q ?? ""}`} initialGenre={genre} initialQuery={q} />
  },
})

const animeRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/anime/$id",
  component: function Anime() {
    const { id } = animeRoute.useParams()
    return <AnimePage key={id} id={Number(id)} />
  },
})

const scheduleRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/schedule",
  component: SchedulePage,
})
const libraryRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/library",
  validateSearch: (search: Record<string, unknown>): { view?: "library" | "history" } => ({
    view: search.view === "history" || search.view === "library" ? search.view : undefined,
  }),
  component: function Library() {
    const { view } = libraryRoute.useSearch()
    return <LibraryPage key={view ?? ""} initialView={view} />
  },
})
const settingsRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/settings",
  validateSearch: (search: Record<string, unknown>): { section?: string } => ({
    section: typeof search.section === "string" ? search.section : undefined,
  }),
  component: function Settings() {
    const { section } = settingsRoute.useSearch()
    return <SettingsPage key={section ?? ""} focus={section} />
  },
})
const profileRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/profile",
  validateSearch: (search: Record<string, unknown>): { tab?: "overview" | "stats" | "activity" } => ({
    tab: search.tab === "stats" || search.tab === "activity" || search.tab === "overview" ? search.tab : undefined,
  }),
  component: function Profile() {
    const { tab } = profileRoute.useSearch()
    return <ProfilePage key={tab ?? ""} initialTab={tab} />
  },
})
const downloadsRoute = createRoute({ getParentRoute: () => shellRoute, path: "/downloads", component: DownloadsPage })
const downloadsQueueRoute = createRoute({ getParentRoute: () => shellRoute, path: "/downloads/queue", component: DownloadsQueuePage })
const downloadSeriesRoute = createRoute({
  getParentRoute: () => shellRoute,
  path: "/downloads/$animeId",
  component: function DownloadSeries() {
    const { animeId } = downloadSeriesRoute.useParams()
    return <DownloadSeriesPage key={animeId} animeId={Number(animeId)} />
  },
})
const notificationsRoute = createRoute({ getParentRoute: () => shellRoute, path: "/notifications", component: NotificationsPage })

const watchRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/watch/$id/$episode",
  // `t`: where to start, in seconds (an `anilili://watch/…?t=` link).
  validateSearch: (search: Record<string, unknown>): { t?: number } => {
    const t = Number(search.t)
    return { t: Number.isFinite(t) && t > 0 ? t : undefined }
  },
  component: function Watch() {
    const { id, episode } = watchRoute.useParams()
    const { t } = watchRoute.useSearch()
    return <WatchPage key={`${id}|${t ?? ""}`} id={Number(id)} episode={Number(episode)} start={t} />
  },
})

const playerRoute = createRoute({ getParentRoute: () => rootRoute, path: "/player", component: PlayerPage })

const offlineRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/offline/$id",
  component: function Offline() {
    const { id } = offlineRoute.useParams()
    return <OfflinePage key={id} id={Number(id)} />
  },
})

// Development aid: open any embed URL in the sandboxed embed player.
const devEmbedRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/dev/embed",
  validateSearch: (search: Record<string, unknown>): { url: string } => ({ url: String(search.url ?? "") }),
  component: function DevEmbed() {
    const { url } = devEmbedRoute.useSearch()
    const server = { id: "dev", provider: "dev", label: "dev", name: "embed", track: "sub", audio: "sub" as const, headers: {}, streams: [], embed: url, subtitles: [], thumbnails: null }
    return <EmbedPlayer server={server} title="Embed test" onBack={() => history.back()} onClose={() => history.back()} />
  },
})

const routeTree = rootRoute.addChildren([
  shellRoute.addChildren([homeRoute, searchRoute, animeRoute, scheduleRoute, libraryRoute, downloadsRoute, downloadsQueueRoute, downloadSeriesRoute, notificationsRoute, settingsRoute, profileRoute]),
  watchRoute,
  playerRoute,
  offlineRoute,
  ...(import.meta.env.DEV ? [devEmbedRoute] : []),
])

export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
  scrollRestoration: true,
  // The pages scroll in <main>, not the window: a new page opens at its top, and Back returns to
  // where the page was. Left out, the router carried the last page's offset over to the next.
  scrollToTopSelectors: ['[data-slot="app-main"]'],
  defaultErrorComponent: RouteError,
  defaultNotFoundComponent: NotFound,
})

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}
