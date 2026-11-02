import { Link } from "@tanstack/react-router"
import { StarIcon } from "lucide-react"

import { AnimeContextMenu, type CardAction } from "@/components/anime/anime-actions"
import { PosterTag } from "@/components/anime/poster-tag"
import { useAppearance, useDubTitles } from "@/lib/api"
import { countdown, formatLabel } from "@/lib/format"
import type { AnimeCard as Card } from "@/lib/types"
import { cn } from "@/lib/utils"

type Props = {
  anime: Card
  className?: string
  /** Extra line under the title (e.g. "Episode 4 · 12m left"). */
  caption?: string
  /** 0..1 watched fraction, drawn as a bar over the poster. */
  progress?: number
  /** The row's own right-click actions, ahead of the list menu. */
  actions?: CardAction[]
}

export function AnimeCard({ anime, className, caption, progress, actions }: Props) {
  // Card details (Settings → Appearance): which badges and captions posters draw.
  const hidden = new Set(useAppearance().hiddenPosterElements)
  // Only where the dub index positively found one, as Android: unknown is far commoner than absent.
  const dubbed = useDubTitles().data?.has(anime.id) ?? false
  const meta = [formatLabel(anime.format), anime.year, anime.episodes ? `${anime.episodes} eps` : null]
    .filter(Boolean)
    .join(" · ")
  return (
    <AnimeContextMenu anime={anime} actions={actions}>
    <Link
      to="/anime/$id"
      params={{ id: String(anime.id) }}
      className={cn("group flex w-40 shrink-0 flex-col gap-2 outline-none", className)}
    >
      <div
        className="relative aspect-[2/3] overflow-hidden rounded-lg bg-muted ring-ring/60 transition group-hover:ring-2 group-focus-visible:ring-2"
        style={{ backgroundColor: anime.color ?? undefined }}
      >
        {anime.cover && (
          <img
            src={anime.cover}
            alt=""
            loading="lazy"
            decoding="async"
            className="size-full object-cover transition duration-300 group-hover:scale-105"
          />
        )}
        {/* Android's corners: the score top right on every card, 18+ opposite it, the dub bottom left. */}
        {anime.score != null && !hidden.has("score") && (
          <PosterTag icon={StarIcon} outlined={false} label={`Rated ${anime.score} percent`} className="absolute top-1.5 right-1.5">
            {anime.score}%
          </PosterTag>
        )}
        {anime.isAdult && !hidden.has("adult") && (
          <PosterTag tone="danger" label="Adult content" className="absolute top-1.5 left-1.5">
            18+
          </PosterTag>
        )}
        {dubbed && !hidden.has("dub") && (
          <PosterTag label="English dub available" className="absolute bottom-1.5 left-1.5">
            DUB
          </PosterTag>
        )}
        {anime.nextAiring && (
          <PosterTag tone="accent" className="absolute right-1.5 bottom-1.5">
            Ep {anime.nextAiring.episode} {countdown(anime.nextAiring.airingAt)}
          </PosterTag>
        )}
        {progress != null && progress > 0 && !hidden.has("progress") && (
          <div className="absolute inset-x-0 bottom-0 h-1 bg-background/60">
            <div className="h-full bg-primary" style={{ width: `${Math.min(100, progress * 100)}%` }} />
          </div>
        )}
      </div>
      {(!hidden.has("title") || !hidden.has("details")) && (
        <div className="flex min-w-0 flex-col gap-0.5">
          {!hidden.has("title") && <span className="line-clamp-2 text-sm leading-snug font-medium" title={anime.title}>{anime.title}</span>}
          {/* A caption is the row's own information (progress, time left), not the details line. */}
          {(caption || !hidden.has("details")) && <span className="truncate text-xs text-muted-foreground">{caption ?? meta}</span>}
        </div>
      )}
    </Link>
    </AnimeContextMenu>
  )
}
