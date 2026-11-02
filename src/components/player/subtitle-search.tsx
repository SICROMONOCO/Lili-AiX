// The player's OpenSubtitles search (Android's OpenSubtitlesPanel): the episode's title and number
// in the viewer's subtitle languages, the most downloaded first; Add fetches a link and hands it to
// mpv as a track. Searching needs no account; adding needs one (Settings → Subtitles).

import { useQuery } from "@tanstack/react-query"
import { Link } from "@tanstack/react-router"
import { DownloadIcon, SearchIcon, ShieldCheckIcon } from "lucide-react"
import { useDeferredValue, useState } from "react"
import { toast } from "sonner"

import { Alert, AlertDescription } from "@/components/ui/alert"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Skeleton } from "@/components/ui/skeleton"
import { Spinner } from "@/components/ui/spinner"
import { api, useOpenSubtitles, useSettings } from "@/lib/api"
import { toastFailure } from "@/lib/failure"
import { episodeLabel } from "@/lib/format"
import { player } from "@/lib/player"
import { errorMessage } from "@/lib/types"
import { cn } from "@/lib/utils"

/** The languages offered as chips: the viewer's own first, then the commonest on OpenSubtitles. */
const COMMON = ["en", "es", "pt", "fr", "de", "it", "ar", "ru", "id", "ja"]
const NAMES = new Intl.DisplayNames(undefined, { type: "language" })
const languageName = (code: string) => {
  try {
    return NAMES.of(code) ?? code
  } catch {
    return code
  }
}

export function SubtitleSearch({
  open,
  onOpenChange,
  title,
  episode,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  title: string
  episode: number
}) {
  const settings = useSettings().data
  const account = useOpenSubtitles().data
  const own = settings?.subtitleLanguages.slice(0, 2) ?? ["en"]
  const [text, setText] = useState(title)
  const [languages, setLanguages] = useState<string[]>(own)
  const [adding, setAdding] = useState<number | null>(null)
  const query = useDeferredValue(text.trim())
  const results = useQuery({
    queryKey: ["opensubtitles", query, languages, episode],
    queryFn: () => api.opensubtitlesSearch(query, languages, episode),
    enabled: open && !!account?.configured && query.length > 1,
    staleTime: 10 * 60_000,
    retry: false,
  })
  const offered = [...new Set([...own, ...COMMON])]
  const toggle = (code: string) =>
    setLanguages(languages.includes(code) ? languages.filter((l) => l !== code) : [...languages, code].slice(-3))

  const add = (fileId: number) => {
    setAdding(fileId)
    api
      .opensubtitlesDownload(fileId)
      .then(async (link) => {
        await player.control({ type: "addSubtitle", url: link.url })
        toast.success("Subtitles added", {
          description: link.remaining != null ? `${link.remaining} OpenSubtitles downloads left today` : link.fileName || undefined,
        })
        onOpenChange(false)
      })
      .catch((e) => toastFailure(e))
      .finally(() => setAdding(null))
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="flex flex-col sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Search OpenSubtitles</DialogTitle>
          <DialogDescription>
            {episodeLabel(episode)}. Subtitles made for another release may need a shift (Z and X) to line up.
          </DialogDescription>
        </DialogHeader>

        {account && !account.configured ? (
          <Alert>
            <AlertDescription>OpenSubtitles isn't available in this build of the app.</AlertDescription>
          </Alert>
        ) : (
          <>
            {account && !account.username && (
              <Alert>
                <AlertDescription>
                  Searching works as it is; adding a file needs a free OpenSubtitles account.{" "}
                  <Link
                    to="/settings"
                    search={{ section: "subtitles" }}
                    className="text-foreground underline underline-offset-4"
                    onClick={() => onOpenChange(false)}
                  >
                    Sign in under Settings → Subtitles
                  </Link>
                  .
                </AlertDescription>
              </Alert>
            )}
            <InputGroup>
              <InputGroupAddon>
                <SearchIcon />
              </InputGroupAddon>
              <InputGroupInput value={text} onChange={(e) => setText(e.target.value)} placeholder="Title" aria-label="Title to search for" />
              {results.isFetching && (
                <InputGroupAddon align="inline-end">
                  <Spinner />
                </InputGroupAddon>
              )}
            </InputGroup>
            <div className="flex flex-wrap gap-1.5" role="group" aria-label="Languages, three at most">
              {offered.map((code) => {
                const on = languages.includes(code)
                return (
                  <button
                    key={code}
                    type="button"
                    aria-pressed={on}
                    onClick={() => toggle(code)}
                    className={cn(
                      "h-7 rounded-full border px-3 text-xs transition",
                      on ? "border-primary bg-primary text-primary-foreground" : "surface-control hover:bg-muted",
                    )}
                  >
                    {languageName(code)}
                  </button>
                )
              })}
            </div>

            <ScrollArea className="h-[min(50vh,26rem)] rounded-lg border">
              {results.isError ? (
                <p className="p-4 text-sm text-muted-foreground">{errorMessage(results.error)}</p>
              ) : results.isPending && query.length > 1 ? (
                <div className="flex flex-col gap-2 p-2">
                  {Array.from({ length: 5 }, (_, i) => (
                    <Skeleton key={i} className="h-14 rounded-lg" />
                  ))}
                </div>
              ) : !results.data?.length ? (
                <Empty>
                  <EmptyHeader>
                    <EmptyTitle>Nothing found</EmptyTitle>
                    <EmptyDescription>Try the title as it's known in English or Japanese, or another language.</EmptyDescription>
                  </EmptyHeader>
                </Empty>
              ) : (
                <ul className="flex flex-col divide-y">
                  {results.data.map((file) => (
                    <li key={file.fileId} className="flex items-center gap-3 px-3 py-2.5">
                      <Badge variant="outline" className="w-10 justify-center uppercase">
                        {file.language.slice(0, 5)}
                      </Badge>
                      <div className="flex min-w-0 flex-1 flex-col gap-1">
                        <span className="truncate text-sm font-medium" title={file.release}>
                          {file.release}
                        </span>
                        <span className="flex flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                          <span className="flex items-center gap-1">
                            <DownloadIcon className="size-3" />
                            {file.downloads.toLocaleString()}
                          </span>
                          {file.trusted && (
                            <Badge variant="secondary" className="gap-1">
                              <ShieldCheckIcon />
                              Trusted
                            </Badge>
                          )}
                          {file.hearingImpaired && <Badge variant="secondary">Hearing impaired</Badge>}
                          {file.machineTranslated && <Badge variant="secondary">Machine translated</Badge>}
                        </span>
                      </div>
                      <Button size="sm" loading={adding === file.fileId} disabled={adding != null || !account?.username} onClick={() => add(file.fileId)}>
                        Add
                      </Button>
                    </li>
                  ))}
                </ul>
              )}
            </ScrollArea>
          </>
        )}
      </DialogContent>
    </Dialog>
  )
}
