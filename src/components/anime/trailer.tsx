// The title's official trailer (AniList's `trailer`: YouTube or Dailymotion), as Android's
// TrailerCard on the title page. Android hands the link to the YouTube app; here it plays in a
// dialog, through the site's own embed player, mounted only while the dialog is open so closing it
// stops the sound.

import { PlayIcon, XIcon } from "lucide-react"

import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog"

export type Trailer = { site: string; id: string }

/** The embed and still for a trailer, or null for a site we can't play. */
export function trailerSources(trailer: Trailer | null | undefined): { embed: string; thumbnail: string } | null {
  if (!trailer?.id) return null
  const id = encodeURIComponent(trailer.id)
  switch (trailer.site.toLowerCase()) {
    case "youtube":
      return {
        embed: `https://www.youtube-nocookie.com/embed/${id}?autoplay=1&rel=0&playsinline=1`,
        thumbnail: `https://i.ytimg.com/vi/${id}/hqdefault.jpg`,
      }
    case "dailymotion":
      return {
        embed: `https://www.dailymotion.com/embed/video/${id}?autoplay=1`,
        thumbnail: `https://www.dailymotion.com/thumbnail/video/${id}`,
      }
    default:
      return null
  }
}

/** The still with a play mark, for the title page's side column. */
export function TrailerCard({ trailer, onOpen }: { trailer: Trailer; onOpen: () => void }) {
  const sources = trailerSources(trailer)
  if (!sources) return null
  return (
    <div className="flex flex-col gap-2">
      <h3 className="font-heading font-medium">Trailer</h3>
      <button
        type="button"
        onClick={onOpen}
        aria-label="Play the trailer"
        className="group relative aspect-video w-full overflow-hidden rounded-lg bg-muted outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
      >
        <img src={sources.thumbnail} alt="" loading="lazy" className="size-full object-cover transition-transform group-hover:scale-105" />
        <span className="absolute inset-0 flex items-center justify-center bg-black/25 transition-colors group-hover:bg-black/40">
          <span className="flex size-12 items-center justify-center rounded-full bg-black/60 text-white ring-1 ring-white/30 [&_svg]:size-5">
            <PlayIcon className="fill-current" />
          </span>
        </span>
      </button>
    </div>
  )
}

export function TrailerDialog({ trailer, title, open, onOpenChange }: { trailer: Trailer; title: string; open: boolean; onOpenChange: (open: boolean) => void }) {
  const sources = trailerSources(trailer)
  if (!sources) return null
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="gap-0 overflow-hidden bg-black p-0 sm:max-w-5xl" showCloseButton={false}>
        <DialogTitle className="sr-only">{title} trailer</DialogTitle>
        {/* Above the embed, in a chip that holds over a bright frame (the stock one vanished). */}
        <button
          type="button"
          aria-label="Close"
          onClick={() => onOpenChange(false)}
          className="absolute top-0 right-0 z-10 m-2 flex size-8 items-center justify-center rounded-full bg-black/70 text-white ring-1 ring-white/25 outline-none hover:bg-black/90 focus-visible:ring-3 focus-visible:ring-white/50 [&_svg]:size-4"
        >
          <XIcon />
        </button>
        <iframe
          src={sources.embed}
          title={`${title} trailer`}
          className="aspect-video w-full"
          allow="autoplay; encrypted-media; picture-in-picture; fullscreen"
          allowFullScreen
          referrerPolicy="strict-origin-when-cross-origin"
        />
      </DialogContent>
    </Dialog>
  )
}
