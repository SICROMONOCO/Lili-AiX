// Mirrors of the Rust types the commands return (crates/core/src/model.rs, src-tauri/src/*.rs).
// Hand-written until tauri-specta supports Tauri 3: keep in step with the Rust side.

export type NextAiring = { episode: number; airingAt: number }

export type AnimeCard = {
  id: number
  title: string
  titleRomaji: string | null
  cover: string | null
  color: string | null
  banner: string | null
  format: string | null
  status: string | null
  episodes: number | null
  score: number | null
  year: number | null
  season: string | null
  genres: string[]
  nextAiring: NextAiring | null
  isAdult: boolean
}

export type AnimeDetail = AnimeCard & {
  malId: number | null
  titleNative: string | null
  synonyms: string[]
  description: string | null
  duration: number | null
  source: string | null
  studios: string[]
  startDate: string | null
  endDate: string | null
  trailer: { site: string; id: string } | null
  relations: { kind: string; anime: AnimeCard }[]
  recommendations: AnimeCard[]
  characters: { name: string; image: string | null; role: string; voiceActor: string | null }[]
  tags: string[]
  /** The series' own wordmark (transparent PNG) from the media index. */
  logo: string | null
}

/** One title of a franchise's watch guide (Rust `franchise::WatchOrderEntry`). */
export type WatchOrderEntry = {
  anime: AnimeCard
  shelf: "seasons" | "movies" | "ovas" | "specials" | "other"
  /** Franchise-wide, in airing order. */
  position: number
  /** Franchise-wide, in the story's own order (prequels first wherever they aired); equals
   * `position` when the story and the calendar agree. */
  chronological: number
  seasonNumber: number | null
  /** "Side story", "Summary"… */
  relation: string | null
  isCurrent: boolean
}

export type Home = {
  trending: AnimeCard[]
  seasonal: AnimeCard[]
  popular: AnimeCard[]
  topRated: AnimeCard[]
  upcoming: AnimeCard[]
  /** Android's New Episodes: titles with an episode just out. */
  newest: AnimeCard[]
  movies: AnimeCard[]
}

/** One further page of a Home row (Rust `model::HomeRowPage`). */
export type HomeRowPage = { cards: AnimeCard[]; hasMore: boolean }

export type SearchFilters = {
  query?: string | null
  genres?: string[]
  year?: number | null
  season?: string | null
  format?: string | null
  status?: string | null
  sort?: string | null
  page?: number | null
  /** The rest of Android's DiscoverFilters (Rust `SearchFilters`). */
  tags?: string[]
  excludedGenres?: string[]
  excludedTags?: string[]
  yearFrom?: number | null
  yearTo?: number | null
  country?: string | null
  source?: string | null
  episodesMin?: number | null
  episodesMax?: number | null
  minimumScore?: number | null
  dubbedOnly?: boolean
  /** Android's WatchedFilter: null shows everything. */
  hideWatched?: "completed" | "started" | null
}

/** One of AniList's tags (Rust `anilist::Tag`). */
export type Tag = { name: string; description: string | null; category: string | null; isAdult: boolean }

/**
 * A misspelled search and what it most likely meant (Rust `model::SearchCorrection`). `applied`:
 * the page holds `suggested`'s results; otherwise `suggested` is only offered ("Did you mean").
 */
export type SearchCorrection = { typed: string; suggested: string; applied: boolean }

export type SearchPage = { results: AnimeCard[]; hasNext: boolean; correction?: SearchCorrection | null }

export type AiringEntry = { airingAt: number; episode: number; anime: AnimeCard }

export type Window = { start: number; end: number }
export type SkipTimes = { intro: Window | null; outro: Window | null; recap?: Window | null }

export type Episode = {
  number: number
  title: string | null
  synopsis: string | null
  thumbnail: string | null
  airedOn: string | null
  durationSeconds: number | null
  filler: boolean
  skip: SkipTimes | null
  /** Set on the season's next episode while it is locked: when it airs, Unix seconds (see
   * `episodeLock`). */
  airsAt?: number | null
}

export type EpisodeList = { episodes: Episode[]; subCount: number | null; dubCount: number | null }

export type Audio = "sub" | "dub"

export type Stream = { url: string; format: string; quality: string | null; height: number | null }
export type Subtitle = { url: string; label: string; language: string }

export type Server = {
  id: string
  provider: string
  label: string
  name: string
  track: "sub" | "ssub" | "dub" | string
  audio: Audio
  headers: Record<string, string>
  streams: Stream[]
  embed: string | null
  subtitles: Subtitle[]
  /** The seek bar's previews: a WebVTT storyboard, fetched with `headers` (Rust `storyboard`). */
  thumbnails: string | null
}

/** One storyboard cue (Rust `storyboard::Frame`): a picture, or a tile of a sprite sheet. */
export type StoryboardFrame = { start: number; end: number; image: string; rect: { x: number; y: number; w: number; h: number } | null }

export type Sources = { servers: Server[]; skip: SkipTimes | null }

export type WatchInfo = {
  server: Server
  servers: Server[]
  skip: SkipTimes | null
  start: number
  proxied: boolean
}

export type LibraryStatus = "watching" | "planning" | "completed" | "paused" | "dropped"

export type LibraryEntry = {
  anime: AnimeCard
  status: LibraryStatus
  updatedAt: number
  /** The signed-in account has this entry as it is here. */
  synced: boolean
  /** Episodes watched, as the account counts them. */
  progress?: number | null
  /** 0..10, from the account. */
  score?: number | null
  /** The account's own status (REPEATING is filed under watching here). */
  accountStatus?: string | null
}

export type Progress = { animeId: number; episode: number; position: number; duration: number; updatedAt: number }

export type ContinueEntry = { anime: AnimeCard; progress: Progress }

export type PlayerContentScale = "fit" | "crop" | "fill"

export type Settings = {
  audio: Audio
  subtitleLanguages: string[]
  autoplayNext: boolean
  autoSkipIntro: boolean
  autoSkipOutro: boolean
  resume: boolean
  showAdult: boolean
  preferredProvider: string | null
  /** Android's Preferred server languages, its stored values (`eng`…); empty shows every language. */
  preferredSourceLanguages: string[]
  /** Android's DNS over HTTPS: `auto`, `none`, or a provider (`cloudflare`, `quad9`…). */
  dohProvider: string
  /** Android's Server priority: provider keys in the order they are tried; empty is Automatic. */
  serverPriority: string[]
  /** Android's Enabled servers: provider keys switched off. */
  disabledProviders: string[]
  volume: number
  syncAnilist: boolean
  hardwareDecoding: boolean
  discordPresence: boolean
  downloadDir: string | null
  /** The download dialog's starting quality, as Android stores it. */
  downloadQuality: DownloadQuality
  /** The download dialog's "Hard subs only", remembered. */
  downloadHardSubs: boolean
  /** Android's skip button: a fixed jump for shows without skip timings, this many seconds. */
  showSkipButton: boolean
  skipButtonSeconds: number
  /** Android's Buffer size and Video cache, by their stored values. */
  playbackBufferSize: "auto" | "small" | "medium" | "large" | "very_large"
  videoCacheSize: "auto" | "off" | "256" | "512" | "1024" | "2048"
  /** New episode alerts: the AniList feed and local release alerts as Windows notifications. */
  releaseNotifications: boolean
  /** A notification when a batch of downloads finishes. */
  downloadNotifications: boolean
  subtitleScale: number
  /** Android's Show subtitles by default, and Also on dubbed audio. */
  subtitlesByDefault: boolean
  subtitlesWithDub: boolean
  captions: Captions
  upscaler: "off" | "fast" | "quality"
  /** Android's Pause details: the episode's billing over a long pause. */
  pauseDetailsOverlay: boolean
  /** Android's Clock and finishing time, top right of the player. */
  playerClock: boolean
  /** Android's Screen size. */
  playerContentScale: PlayerContentScale
  /** Android's Check for updates on launch. */
  checkUpdatesOnLaunch: boolean
  appearance: Appearance
}

/** One OpenSubtitles file a search found (Rust `opensubtitles::SubtitleFile`). */
export type SubtitleFile = {
  fileId: number
  /** ISO 639-1, sometimes with a region (`pt-br`). */
  language: string
  release: string
  fileName: string
  downloads: number
  hearingImpaired: boolean
  trusted: boolean
  machineTranslated: boolean
}

export type SubtitleLink = { url: string; fileName: string; remaining: number | null }

export type OpenSubtitlesAccount = { configured: boolean; username: string | null }

/** Android's Caption appearance (Rust `Captions`), under its stored values; looks: src/lib/captions.ts. */
export type Captions = {
  font: "sans" | "serif" | "mono" | "condensed" | "casual" | "cursive"
  bold: boolean
  textColor: "white" | "yellow" | "cyan" | "green" | "magenta"
  /** 0..100. */
  textOpacity: number
  backgroundColor: "black" | "gray" | "navy" | "white"
  /** 0..100; 0 draws no box. */
  backgroundOpacity: number
  edgeStyle: "none" | "outline" | "shadow"
  position: "bottom" | "top"
  /** The gap to that edge, as a percentage of the picture's height. */
  edgeMargin: number
  /** Across the picture: 0 the left edge, 50 centred, 100 the right (Android's anchor). */
  horizontalAnchor: number
}

/** Android's Appearance choices, as the values it stores (Rust `Appearance`; looks: src/lib/theme.ts). */
export type Appearance = {
  background: string
  accent: string
  customSeed: string
  style: string
  contrast: string
  playerAccent: boolean
  showColourPages: boolean
  backdropSource: string
  backdropAnimeUrl: string | null
  backdropAnimeTitle: string | null
  backdropImage: string | null
  backdropDim: string
  backdropBlur: boolean
  uiScale: string
  clockFormat: string
  hiddenPosterElements: string[]
  blurEpisodeImages: boolean
  showSeriesLogos: boolean
  /** Liquid glass per part (Android's GlassSection; `glassPages` is the desktop's other pages). */
  glassNavigation: boolean
  glassHome: boolean
  glassDetails: boolean
  glassBrowse: boolean
  glassPopups: boolean
  glassRefresh: boolean
  glassPages: boolean
  homeLayout: string | null
}

export type DownloadRecord = {
  id: number
  animeId: number
  episode: number
  audio: Audio
  title: string
  cover: string | null
  status: "queued" | "downloading" | "done" | "failed" | "cancelled"
  progress: number
  bytes: number
  path: string | null
  error: string | null
  createdAt: number
  updatedAt: number
  /** The server to try first; null follows the playback preference. */
  provider: string | null
  /** The tallest rendition wanted; null takes the best. */
  maxHeight: number | null
  matchQuality: boolean
  /** "Hard subs only": only servers with the subtitles burned into the picture. */
  hardSubs: boolean
  /** What was fetched: the rendition's height (when known) and the server's name. */
  height: number | null
  server: string | null
  /** Live, from the worker's progress events: bytes a second, and seconds left. */
  speed?: number
  eta?: number | null
}

/** Android's download qualities, by their stored values. */
export type DownloadQuality = "best" | "1080" | "720" | "480" | "360"

/** The download dialog's choices (Rust `DownloadOptions`). */
export type DownloadOptions = { provider: string | null; maxHeight: number | null; matchQuality: boolean; hardSubs: boolean }

/** The queue and the disk it writes to (Rust `downloads::QueueState`). */
export type QueueState = { paused: boolean; pending: number; folder: string; freeBytes: number | null; headroomBytes: number }

/** What a server really offers for a sample episode (Rust `downloads::QualityCheck`). */
export type QualityCheck = {
  server: string
  provider: string
  offered: number[]
  delivered: number | null
  fits: boolean | null
  alternative: { server: string; provider: string; height: number } | null
}

/** One item of the AniList notification feed (Rust `anilist::FeedItem`, Android's `AppNotification`). */
export type FeedItem = {
  id: number
  kind: "airing" | "media" | "social"
  createdAt: number
  title: string
  badge: string | null
  detail: string | null
  episode: number | null
  mediaId: number | null
  image: string | null
  banner: string | null
  unread: boolean
}

/** One entry of the app's own notification log (Rust `model::Notice`). */
export type Notice = {
  id: number
  kind: "airing" | "reminder" | "download" | "app"
  title: string
  body: string | null
  route: string | null
  image: string | null
  animeId: number | null
  createdAt: number
  read: boolean
}

export type NotificationsPage = { feed: FeedItem[]; feedError: CommandError | null; anilist: boolean; notices: Notice[] }

export type Update = {
  version: string
  changelog: string
  url: string
  sizeBytes: number
  sha256: string
  publishedAt: number
}

export type UpdateStatus = { current: string; update: Update | null }

export type MalUser = { id: number; name: string; picture: string | null }

/** The connected Simkl account (`simkl.rs`): only its name, when Simkl gave one. */
export type SimklAccount = { name: string | null }

/** A Simkl sign-in waiting for approval at simkl.com/pin. `expiresAt` is Unix seconds. */
export type SimklCode = { userCode: string; verificationUri: string; verificationUriComplete: string; expiresAt: number }

export type PluginInfo = { id: string; name: string; version: string; hosts: string[] }

export type Viewer = { id: number; name: string; avatar: string | null; showAdult: boolean }

export type ListEntry = {
  anime: AnimeCard
  status: string
  progress: number
  score: number
  updatedAt: number
  /** Main animation studios (AniList lists only). */
  studios?: string[]
}

/** A card with its synopsis (plain text with AniList's line breaks), for the dice. */
export type DicePick = AnimeCard & { description: string | null }

/** Home's recommendation rows (Rust `recs::HomeRecs`). */
export type HomeRecs = { forYou: AnimeCard[]; because: { seed: AnimeCard; items: AnimeCard[] } | null }

/** The signed-in account as the Profile page shows it (Rust `AccountProfile`). */
export type AccountProfile = {
  service: "anilist" | "mal"
  id: number
  name: string
  avatar: string | null
  banner: string | null
  /** Unix seconds. */
  joined: number | null
  url: string
  titles: number
  episodes: number
  minutes: number
  /** 0..10. */
  meanScore: number | null
  /** Unix seconds at UTC midnight; empty when the service keeps no history (MyAnimeList). */
  activity: { day: number; amount: number }[]
}

/** What the viewer can do about a failure (Rust `catalogue::Next`). */
export type FailureNext = "nothing" | "retry" | "otherServer" | "signIn" | "diagnostics" | "reinstall"

/** A failure from a command: catalogue code and message, technical detail, next step. */
/** A backup file's header and counts, read before restoring it. */
export type BackupInfo = { createdAt: number; appVersion: string; summary: string; services: string[] }
export type BackupRestored = { summary: string; warnings: string[] }
export type BackupExported = { path: string; summary: string }

export type CommandError = { code: string; message: string; detail: string; next: FailureNext }

export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error
  if (error && typeof error === "object" && "message" in error) {
    const e = error as CommandError
    return e.code ? `${e.message} (${e.code})` : e.message
  }
  return String(error)
}
