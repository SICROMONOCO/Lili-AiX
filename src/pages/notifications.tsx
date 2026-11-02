// Notifications, as Android's page (ui/notifications/NotificationsScreen.kt): the AniList feed in
// tabs, grouped by age, unread ones outlined. The desktop adds its own log beside the feed: new
// episodes of library titles, reminders and finished downloads, so the page has something to say
// to a viewer who isn't on AniList.

import { Link, useNavigate } from "@tanstack/react-router"
import { useQueryClient } from "@tanstack/react-query"
import { BellIcon, CheckCheckIcon, EllipsisIcon, RefreshCwIcon } from "lucide-react"
import { useMemo, useState } from "react"

import { ErrorNotice } from "@/components/layout/error-notice"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { Skeleton } from "@/components/ui/skeleton"
import { Toggle } from "@/components/ui/toggle"
import { api, keys, useNotifications, useNotificationsUnread } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import type { FeedItem, Notice } from "@/lib/types"
import { cn } from "@/lib/utils"

type Tab = "all" | "airing" | "social" | "media" | "downloads"
const TABS: { value: Tab; label: string }[] = [
  { value: "all", label: "All" },
  { value: "airing", label: "Airing" },
  { value: "social", label: "Social" },
  { value: "media", label: "Media" },
  { value: "downloads", label: "Downloads" },
]

/** A feed item or a log entry, as one card. */
type Item = {
  key: string
  tab: Exclude<Tab, "all">
  createdAt: number
  title: string
  detail: string | null
  badge: string | null
  image: string | null
  banner: string | null
  unread: boolean
  route: string | null
}

const fromFeed = (f: FeedItem): Item => ({
  key: `feed:${f.id}`,
  tab: f.kind,
  createdAt: f.createdAt,
  title: f.title,
  detail: f.kind === "airing" ? (f.episode != null ? `Episode ${f.episode} is out` : null) : f.detail,
  badge: f.badge,
  image: f.image,
  banner: f.banner,
  unread: f.unread,
  route: f.mediaId != null ? `/anime/${f.mediaId}` : null,
})

const fromNotice = (n: Notice): Item => ({
  key: `notice:${n.id}`,
  tab: n.kind === "airing" || n.kind === "reminder" ? "airing" : "downloads",
  createdAt: n.createdAt,
  title: n.title,
  detail: n.body,
  badge: n.kind === "reminder" ? "REMINDER" : n.kind === "airing" ? "NEW EPISODE" : n.kind === "download" ? "DOWNLOADS" : null,
  image: n.image,
  banner: null,
  unread: !n.read,
  route: n.route,
})

export function NotificationsPage() {
  const client = useQueryClient()
  const page = useNotifications()
  const unread = useNotificationsUnread().data ?? 0
  const [tab, setTab] = useState<Tab>("all")
  const [marking, setMarking] = useState(false)

  const items = useMemo(() => {
    const all = [...(page.data?.feed ?? []).map(fromFeed), ...(page.data?.notices ?? []).map(fromNotice)]
    // The same episode from both the feed and a local alert would be two cards; the feed's wins.
    const seen = new Set(all.filter((i) => i.key.startsWith("feed:") && i.tab === "airing").map((i) => `${i.route}|${i.detail}`))
    return all
      .filter((i) => !(i.key.startsWith("notice:") && i.tab === "airing" && seen.has(`${i.route}|${i.detail}`)))
      .sort((a, b) => b.createdAt - a.createdAt)
  }, [page.data])
  const shown = items.filter((i) => tab === "all" || i.tab === tab)
  // Ages are measured from when the list was fetched, so a re-render never regroups it.
  const fetchedAt = page.dataUpdatedAt / 1000
  const sections = useMemo(() => {
    const now = fetchedAt
    const groups = new Map<string, Item[]>()
    for (const item of shown) {
      const age = now - item.createdAt
      const name = age < 7 * 86400 ? "This week" : age < 30 * 86400 ? "Last 30 days" : "Over a month ago"
      groups.set(name, [...(groups.get(name) ?? []), item])
    }
    return [...groups.entries()]
  }, [shown, fetchedAt])

  const markAllRead = () => {
    setMarking(true)
    // The answer still carries this visit's unread outlines (they were read before the reset);
    // only the count goes to nothing.
    void api
      .notifications(true)
      .then((data) => client.setQueryData(keys.notifications, data))
      .catch((e) => toastFailure(e))
      .finally(() => setMarking(false))
  }

  return (
    <div className="flex max-w-4xl flex-col gap-5 p-8">
      <div className="flex flex-wrap items-center gap-2">
        <h1 className="font-heading text-2xl font-bold">Notifications</h1>
        <div className="ml-auto flex items-center gap-2">
          <Button variant="ghost" size="icon" aria-label="Refresh" onClick={() => void page.refetch()} loading={page.isFetching}>
            <RefreshCwIcon />
          </Button>
          <Button variant="outline" disabled={unread === 0} loading={marking} onClick={markAllRead}>
            <CheckCheckIcon data-icon="inline-start" />
            Mark all read
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger render={<Button variant="ghost" size="icon" aria-label="More" />}>
              <EllipsisIcon />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuGroup>
                <DropdownMenuItem
                  disabled={!page.data?.notices.length}
                  onClick={() =>
                    api
                      .notificationsClear()
                      .then(() => client.invalidateQueries({ queryKey: keys.notifications }))
                      .catch((e) => toastFailure(e))
                  }
                >
                  Clear the app's own notifications
                </DropdownMenuItem>
                <DropdownMenuItem render={<Link to="/settings" search={{ section: "notifications" }} />}>Notification settings</DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>

      <div className="flex flex-wrap gap-2">
        {TABS.map((t) => {
          const count = items.filter((i) => i.unread && (t.value === "all" || i.tab === t.value)).length
          return (
            <Toggle key={t.value} variant="outline" size="sm" pressed={tab === t.value} onPressedChange={() => setTab(t.value)}>
              {t.label}
              {count > 0 && <Badge className="h-4 px-1 text-[10px]">{count}</Badge>}
            </Toggle>
          )
        })}
      </div>

      {page.data && !page.data.anilist && (
        <p className="text-sm text-muted-foreground">
          New episodes and activity from your account appear here when you're signed in to AniList.{" "}
          <Link to="/settings" search={{ section: "accounts" }} className="text-foreground underline underline-offset-4">
            Sign in
          </Link>
        </p>
      )}
      {page.data?.feedError && <ErrorNotice error={page.data.feedError} onRetry={() => void page.refetch()} />}

      {page.isPending ? (
        <div className="flex flex-col gap-3">
          {Array.from({ length: 5 }, (_, i) => (
            <Skeleton key={i} className="h-24 rounded-xl" />
          ))}
        </div>
      ) : page.isError ? (
        <ErrorNotice error={page.error} onRetry={() => void page.refetch()} />
      ) : shown.length === 0 ? (
        <Empty className="border">
          <EmptyHeader>
            <EmptyMedia variant="icon">
              <BellIcon />
            </EmptyMedia>
            <EmptyTitle>Nothing here yet</EmptyTitle>
            <EmptyDescription>New episodes of titles in your library, reminders from the schedule and finished downloads show up here.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      ) : (
        sections.map(([name, list]) => (
          <section key={name} className="flex flex-col gap-2.5">
            <h2 className="pt-1 text-xs font-semibold tracking-wide text-muted-foreground uppercase">{name}</h2>
            {list.map((item) => (
              <Card key={item.key} item={item} />
            ))}
          </section>
        ))
      )}
    </div>
  )
}

function Card({ item }: { item: Item }) {
  const navigate = useNavigate()
  const open = item.route ? () => void navigate({ to: item.route! }) : undefined
  return (
    <button
      type="button"
      disabled={!open}
      onClick={open}
      className={cn(
        "surface relative flex w-full items-center gap-3 overflow-hidden rounded-xl p-2.5 text-left transition outline-none focus-visible:ring-2 focus-visible:ring-ring enabled:hover:bg-muted",
        item.unread && "border-primary",
      )}
    >
      {item.banner && <img src={item.banner} alt="" className="pointer-events-none absolute inset-0 size-full object-cover opacity-15" />}
      {item.image ? (
        <img src={item.image} alt="" loading="lazy" className="relative h-19 w-14 shrink-0 rounded-lg object-cover" />
      ) : (
        <div className="relative flex h-19 w-14 shrink-0 items-center justify-center rounded-lg bg-muted">
          <BellIcon className="size-5 text-muted-foreground" />
        </div>
      )}
      <div className="relative flex min-w-0 flex-1 flex-col gap-1">
        <span className="line-clamp-2 font-semibold" title={item.title}>{item.title}</span>
        {item.detail && <span className="line-clamp-2 text-sm text-muted-foreground">{item.detail}</span>}
        {item.badge && (
          <Badge variant="secondary" className="self-start">
            {item.badge}
          </Badge>
        )}
      </div>
      <span className="relative shrink-0 self-end text-xs font-medium text-muted-foreground">
        {new Date(item.createdAt * 1000).toLocaleDateString(undefined, { day: "numeric", month: "short" })}
      </span>
    </button>
  )
}
