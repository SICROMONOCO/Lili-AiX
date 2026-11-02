// Settings as the television lays them out (Android's TvSettingsTwoPane): a rail of categories down
// the left, and the open category's rows beside it. These are the categories and the block a
// category's rows are grouped in; the page itself is src/pages/settings.tsx.

import {
  BellIcon,
  CaptionsIcon,
  DatabaseIcon,
  DownloadIcon,
  EyeIcon,
  InfoIcon,
  type LucideIcon,
  PaletteIcon,
  PlayCircleIcon,
  ServerIcon,
  SlidersHorizontalIcon,
  UserIcon,
} from "lucide-react"

import { cn } from "@/lib/utils"

export type SectionId =
  | "player"
  | "subtitles"
  | "downloads"
  | "servers"
  | "accounts"
  | "notifications"
  | "content"
  | "appearance"
  | "data"
  | "app"
  | "about"

/** Android's categories, in its order (SettingsScreen.kt `sectionTiles`). */
export const SECTIONS: { id: SectionId; title: string; icon: LucideIcon; summary: string }[] = [
  { id: "player", title: "Player", icon: PlayCircleIcon, summary: "Playback, skipping, and how the player looks" },
  { id: "subtitles", title: "Subtitles", icon: CaptionsIcon, summary: "When they show, which language, and how they look" },
  { id: "downloads", title: "Downloads", icon: DownloadIcon, summary: "Where episodes are saved, and at what quality" },
  { id: "servers", title: "Servers", icon: ServerIcon, summary: "Which servers are used, and in what order" },
  { id: "accounts", title: "Accounts & sync", icon: UserIcon, summary: "AniList and MAL sign-in, and what is synced" },
  { id: "notifications", title: "Notifications", icon: BellIcon, summary: "New episodes, your AniList feed and finished downloads" },
  { id: "content", title: "Content", icon: EyeIcon, summary: "What the catalogue shows, and Discord status" },
  { id: "appearance", title: "Appearance", icon: PaletteIcon, summary: "Theme, colours, layout, and how cards look" },
  { id: "data", title: "Data", icon: DatabaseIcon, summary: "Video cache, clearing data, and backups" },
  { id: "app", title: "App", icon: SlidersHorizontalIcon, summary: "Updates and diagnostics" },
  { id: "about", title: "About", icon: InfoIcon, summary: "Version, keyboard shortcuts, and the people behind it" },
]

/** `?section=` values from before the categories followed Android's. */
const RENAMED: Record<string, SectionId> = { playback: "player", layout: "appearance", backup: "data" }

export function sectionFor(id: string | undefined): SectionId | undefined {
  if (!id) return undefined
  if (SECTIONS.some((s) => s.id === id)) return id as SectionId
  return RENAMED[id]
}

/**
 * One block of a category's rows, under an optional heading (a search result can point at it).
 * `action` sits at the heading's end: a switch over the whole block, ruled off from the rows it
 * governs.
 */
export function SettingsGroup({
  title,
  description,
  action,
  className,
  children,
}: {
  title?: string
  description?: React.ReactNode
  action?: React.ReactNode
  className?: string
  children: React.ReactNode
}) {
  return (
    <section className={cn("surface flex flex-col gap-4 rounded-2xl p-5", className)}>
      {title && (
        <header className={cn("flex items-center gap-4", action != null && "border-b pb-4")}>
          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
            <h3 className="font-heading font-semibold">{title}</h3>
            {description && <p className="text-sm text-muted-foreground">{description}</p>}
          </div>
          {action}
        </header>
      )}
      {children}
    </section>
  )
}
