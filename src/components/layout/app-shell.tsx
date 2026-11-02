import { Link, Outlet, useNavigate, useRouterState } from "@tanstack/react-router"
import {
  BellIcon,
  CalendarDaysIcon,
  CompassIcon,
  DicesIcon,
  DownloadIcon,
  HomeIcon,
  LibraryIcon,
  SearchIcon,
  SettingsIcon,
  UserIcon,
} from "lucide-react"
import { useIsFetching, useIsMutating, useQuery } from "@tanstack/react-query"
import { useEffect, useState } from "react"

import { DiceDialog } from "@/components/anime/dice-dialog"
import { CommandPalette } from "@/components/layout/command-palette"
import { UPDATE_TOAST, UpdateDialog, openUpdate } from "@/components/layout/update-dialog"
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar"
import { Kbd } from "@/components/ui/kbd"
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip"
import {
  api,
  useAppearance,
  useDownloads,
  useMalUser,
  useNotificationsUnread,
  useProgressRefresh,
  useSettings,
  useUpdateCheck,
  useViewer,
} from "@/lib/api"
import { isActive } from "@/lib/downloads"
import { glassPartFor } from "@/lib/theme"
import { dice } from "@/lib/dice"
import { useFailureEvents } from "@/lib/failure"
import mark from "@/assets/anilili-mark.svg"
import { cn } from "@/lib/utils"
import { toast } from "sonner"

const NAV = [
  { to: "/", label: "Home", icon: HomeIcon },
  { to: "/search", label: "Browse", icon: CompassIcon },
  { to: "/schedule", label: "Schedule", icon: CalendarDaysIcon },
  { to: "/library", label: "Library", icon: LibraryIcon },
  { to: "/downloads", label: "Downloads", icon: DownloadIcon },
] as const

/** Everything except the player: a slim icon sidebar and an opaque, scrolling page. */
export function AppShell() {
  const pathname = useRouterState({ select: (s) => s.location.pathname })
  const navigate = useNavigate()
  const viewer = useViewer().data
  const malUser = useMalUser().data
  const [paletteOpen, setPaletteOpen] = useState(false)
  const [diceOpen, setDiceOpen] = useState(false)
  const openDice = () => {
    setDiceOpen(true)
    dice.ensurePick()
  }
  // `anilili:dice` (the dev hook's `dice` command, or anything else) opens it too.
  useEffect(() => {
    const onDice = () => {
      setDiceOpen(true)
      dice.ensurePick()
    }
    window.addEventListener("anilili:dice", onDice)
    return () => window.removeEventListener("anilili:dice", onDice)
  }, [])
  useProgressRefresh()
  useFailureEvents()
  // The sidebar's counts: episodes waiting to download, and unread notifications.
  const queued = useDownloads().data?.filter(isActive).length ?? 0
  const unread = useNotificationsUnread().data ?? 0
  const badges: Record<string, number> = { "/downloads": queued }
  // Liquid glass (Settings → Appearance): the sidebar's own switch, the page's by its part, and
  // whether the window's native glass is under them (macOS; see src-tauri/src/glass.rs).
  const appearance = useAppearance()
  const windowGlass =
    appearance.glassNavigation || appearance.glassHome || appearance.glassDetails || appearance.glassBrowse || appearance.glassPages
  // A new version: a system notification, or a toast when the app is what's in front. Either
  // opens the update dialog, which downloads it and asks before installing.
  const checkOnLaunch = useSettings().data?.checkUpdatesOnLaunch ?? false
  const update = useUpdateCheck(checkOnLaunch).data?.update
  useEffect(() => {
    if (!update || !checkOnLaunch) return
    if (document.hasFocus()) {
      toast(`Anilili ${update.version} is available`, {
        id: UPDATE_TOAST,
        duration: 15_000,
        action: { label: "Update", onClick: () => openUpdate(update, { download: true }) },
      })
    } else {
      void api.updateNotify(update).catch(() => {})
    }
  }, [update, checkOnLaunch])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault()
        setPaletteOpen((open) => !open)
      }
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  }, [])

  const isCurrent = (to: string) => (to === "/" ? pathname === "/" : pathname.startsWith(to))

  return (
    <div
      data-slot="app-shell"
      data-window-glass={windowGlass ? "on" : "off"}
      className="relative flex h-svh w-full overflow-hidden bg-background text-foreground"
    >
      <Backdrop />
      <nav
        data-slot="app-nav"
        data-glass={appearance.glassNavigation ? "on" : "off"}
        className="relative z-10 flex w-16 shrink-0 flex-col items-center gap-1 border-r py-4"
      >
        <Link to="/" className="mb-4 flex size-10 items-center justify-center" aria-label="Anilili">
          {/* The mark as a mask, so it takes the theme's colour with no backdrop of its own. */}
          <span
            aria-hidden
            className="block size-9 bg-sidebar-foreground"
            style={{ maskImage: `url(${mark})`, maskSize: "contain", maskRepeat: "no-repeat", maskPosition: "center" }}
          />
        </Link>
        <NavButton label="Search (Ctrl+K)" onClick={() => setPaletteOpen(true)}>
          <SearchIcon />
        </NavButton>
        {NAV.map(({ to, label, icon: Icon }) => (
          <NavButton
            key={to}
            label={badges[to] ? `${label} · ${badges[to]} in the queue` : label}
            badge={badges[to]}
            active={isCurrent(to)}
            onClick={() => void navigate({ to })}
          >
            <Icon />
          </NavButton>
        ))}
        <NavButton label="Random pick" onClick={openDice}>
          <DicesIcon />
        </NavButton>
        <div className="mt-auto flex flex-col items-center gap-1">
          <NavButton
            label={unread ? `Notifications · ${unread} unread` : "Notifications"}
            badge={unread}
            active={isCurrent("/notifications")}
            onClick={() => void navigate({ to: "/notifications" })}
          >
            <BellIcon />
          </NavButton>
          <NavButton label="Profile" active={isCurrent("/profile")} onClick={() => void navigate({ to: "/profile" })}>
            {viewer || malUser ? (
              <Avatar className="size-7">
                <AvatarImage src={(viewer?.avatar ?? malUser?.picture) ?? undefined} alt="" />
                <AvatarFallback>{(viewer?.name ?? malUser?.name ?? "?").slice(0, 1)}</AvatarFallback>
              </Avatar>
            ) : (
              <UserIcon />
            )}
          </NavButton>
          <NavButton label="Settings" active={isCurrent("/settings")} onClick={() => void navigate({ to: "/settings" })}>
            <SettingsIcon />
          </NavButton>
        </div>
      </nav>
      <ActivityBar />
      <main
        data-slot="app-main"
        data-glass={appearance[glassPartFor(pathname)] ? "on" : "off"}
        className="relative z-10 min-w-0 flex-1 overflow-y-auto"
      >
        <Outlet />
      </main>
      <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
      <DiceDialog open={diceOpen} onOpenChange={setDiceOpen} />
      <UpdateDialog />
    </div>
  )
}

function NavButton({
  label,
  active,
  badge,
  onClick,
  children,
}: {
  label: string
  active?: boolean
  /** A count in the corner, when above zero. */
  badge?: number
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <Tooltip>
      <TooltipTrigger
        render={
          <button
            type="button"
            aria-label={label}
            onClick={onClick}
            className={cn(
              "relative flex size-10 items-center justify-center rounded-lg text-sidebar-foreground/70 transition outline-none hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-ring [&_svg:not([class*='size-'])]:size-5",
              active && "bg-sidebar-accent text-sidebar-accent-foreground",
            )}
          />
        }
      >
        {children}
        {!!badge && badge > 0 && (
          <span className="absolute top-0.5 right-0.5 flex h-4 min-w-4 items-center justify-center rounded-full bg-primary px-1 text-[10px] leading-none font-semibold text-primary-foreground tabular-nums">
            {badge > 99 ? "99+" : badge}
          </span>
        )}
      </TooltipTrigger>
      <TooltipContent side="right">
        {label.includes("Ctrl+K") ? (
          <>
            Search <Kbd>Ctrl K</Kbd>
          </>
        ) : (
          label
        )}
      </TooltipContent>
    </Tooltip>
  )
}

/** A thin sweep along the top of the page while anything loads for the first time or a change is
 * being saved: pages show skeletons for their own content, but a row, a tab or a save elsewhere on
 * the page would otherwise arrive with no sign it was coming. Queries that poll quietly in the
 * background set `meta: { background: true }` to stay out of it. */
function ActivityBar() {
  const fetching = useIsFetching({ predicate: (q) => q.state.data === undefined && q.meta?.background !== true })
  const saving = useIsMutating()
  const active = fetching + saving > 0
  return (
    <div
      aria-hidden
      className={cn(
        "pointer-events-none absolute top-0 right-0 left-16 z-50 h-0.5 overflow-hidden transition-opacity duration-300",
        active ? "opacity-100 delay-200" : "opacity-0",
      )}
    >
      <div className="h-full w-1/3 bg-primary motion-safe:animate-[activity-sweep_1.1s_ease-in-out_infinite]" />
    </div>
  )
}

const DIM: Record<string, number> = { light: 0.5, medium: 0.65, strong: 0.8 }

/** Settings → Appearance → Background image: an anime's art or the viewer's own picture behind the
 * pages, darkened (and blurred) so text over it stays readable, as on Android. */
function Backdrop() {
  const a = useAppearance()
  const own = useQuery({
    queryKey: ["backdrop-image", a.backdropImage],
    queryFn: api.backdropLoad,
    enabled: a.backdropSource === "image",
    staleTime: Infinity,
  })
  const url = a.backdropSource === "anime" ? a.backdropAnimeUrl : a.backdropSource === "image" ? own.data : null
  if (!url) return null
  return (
    <div aria-hidden className="pointer-events-none absolute inset-0 z-0 overflow-hidden">
      <img src={url} alt="" className={cn("size-full object-cover", a.backdropBlur && "scale-110 blur-xl")} />
      <div className="absolute inset-0 bg-background" style={{ opacity: DIM[a.backdropDim] ?? 0.65 }} />
    </div>
  )
}
