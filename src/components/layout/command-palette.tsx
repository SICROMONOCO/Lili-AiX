import { useNavigate } from "@tanstack/react-router"
import { CalendarDaysIcon, CompassIcon, HomeIcon, LibraryIcon, SettingsIcon } from "lucide-react"
import { useState } from "react"

import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command"
import { Spinner } from "@/components/ui/spinner"
import { useSearch } from "@/lib/api"
import { toFailure } from "@/lib/failure"
import { formatLabel } from "@/lib/format"
import { useSettled } from "@/lib/use-settled"

/** Ctrl+K: jump to any title by name, or to a page. */
export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  const navigate = useNavigate()
  const [query, setQuery] = useState("")
  const deferred = useSettled(query.trim())
  const searching = deferred.length >= 2
  const results = useSearch({ query: deferred, page: 1 }, open && searching)

  const go = (to: string, params?: Record<string, string>) => {
    onOpenChange(false)
    setQuery("")
    void navigate({ to, params })
  }

  return (
    <CommandDialog open={open} onOpenChange={onOpenChange}>
      {/* AniList ranks results; cmdk's own fuzzy filter would reorder them. */}
      <Command shouldFilter={false}>
        <CommandInput placeholder="Search anime or jump to a page…" value={query} onValueChange={setQuery} />
        <CommandList>
          {searching && results.isFetching && !results.data && (
            <div className="flex justify-center py-6">
              <Spinner />
            </div>
          )}
          <CommandEmpty>
            {!searching
              ? "Type at least two letters."
              : results.isError
                ? `Search failed: ${toFailure(results.error).message}`
                : results.isFetching
                  ? null
                  : "No titles found."}
          </CommandEmpty>
          {searching && results.data && results.data.results.length > 0 && (
            <CommandGroup heading="Titles">
              {results.data.results.slice(0, 10).map((anime) => (
                <CommandItem
                  key={anime.id}
                  value={`title-${anime.id}`}
                  onSelect={() => go("/anime/$id", { id: String(anime.id) })}
                >
                  {anime.cover && <img src={anime.cover} alt="" className="h-10 w-7 rounded object-cover" />}
                  <div className="flex min-w-0 flex-col">
                    <span className="truncate" title={anime.title}>{anime.title}</span>
                    <span className="truncate text-xs text-muted-foreground">
                      {[formatLabel(anime.format), anime.year].filter(Boolean).join(" · ")}
                    </span>
                  </div>
                </CommandItem>
              ))}
            </CommandGroup>
          )}
          {!searching && (
            <CommandGroup heading="Pages">
              <CommandItem value="home" onSelect={() => go("/")}>
                <HomeIcon />
                Home
              </CommandItem>
              <CommandItem value="browse" onSelect={() => go("/search")}>
                <CompassIcon />
                Browse
              </CommandItem>
              <CommandItem value="schedule" onSelect={() => go("/schedule")}>
                <CalendarDaysIcon />
                Schedule
              </CommandItem>
              <CommandItem value="library" onSelect={() => go("/library")}>
                <LibraryIcon />
                Library
              </CommandItem>
              <CommandItem value="settings" onSelect={() => go("/settings")}>
                <SettingsIcon />
                Settings
              </CommandItem>
            </CommandGroup>
          )}
        </CommandList>
      </Command>
    </CommandDialog>
  )
}
