// Typed wrappers over the Rust commands, and the React Query hooks the pages use.

import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"
import { keepPreviousData, useInfiniteQuery, useMutation, useQuery, useQueryClient } from "@tanstack/react-query"
import { useEffect } from "react"

import { DEFAULT_APPEARANCE } from "@/lib/theme"
import type {
  AccountProfile,
  AiringEntry,
  AnimeCard,
  AnimeDetail,
  Appearance,
  Audio,
  BackupExported,
  BackupInfo,
  BackupRestored,
  ContinueEntry,
  DownloadOptions,
  DownloadRecord,
  NotificationsPage,
  QualityCheck,
  QueueState,
  Update,
  UpdateStatus,
  EpisodeList,
  HomeRecs,
  Home,
  HomeRowPage,
  LibraryEntry,
  LibraryStatus,
  ListEntry,
  MalUser,
  SimklAccount,
  SimklCode,
  DicePick,
  PluginInfo,
  Progress,
  SearchFilters,
  SearchPage,
  Sources,
  Settings,
  OpenSubtitlesAccount,
  SkipTimes,
  SubtitleFile,
  SubtitleLink,
  StoryboardFrame,
  Tag,
  Viewer,
  WatchInfo,
  WatchOrderEntry,
} from "@/lib/types"

export const api = {
  home: () => invoke<Home>("home"),
  /** The next page of a Home row (`home` opens each with its first): one of `trending`, `seasonal`,
   * `popular`, `top_rated`, `upcoming`, `movies`, `newest`. */
  homeMore: (row: string, page: number) => invoke<HomeRowPage>("home_more", { row, page }),
  /** `exact`: take the query as typed, with no spelling correction ("Search instead for"). */
  search: (filters: SearchFilters, exact?: boolean) => invoke<SearchPage>("search", { filters, exact: exact ?? null }),
  detail: (id: number) => invoke<AnimeDetail>("anime_detail", { id }),
  animeLogo: (id: number) => invoke<string | null>("anime_logo", { id }),
  dubTitles: () => invoke<number[]>("dub_titles"),
  animeTags: () => invoke<Tag[]>("anime_tags"),
  schedule: (from: number, to: number) => invoke<AiringEntry[]>("schedule", { from, to }),
  episodes: (id: number) => invoke<EpisodeList>("episodes", { id }),
  sources: (id: number, episode: number) => invoke<Sources>("sources", { id, episode, refresh: false }),
  skipTimes: (animeId: number, episode: number, duration: number) =>
    invoke<SkipTimes | null>("skip_times", { animeId, episode, duration }),
  storyboard: (url: string, headers: Record<string, string>) => invoke<StoryboardFrame[]>("storyboard", { url, headers }),
  storyboardImage: (url: string, headers: Record<string, string>) => invoke<ArrayBuffer>("storyboard_image", { url, headers }),
  watch: (request: {
    animeId: number
    episode: number
    serverId?: string | null
    audio?: Audio | null
    start?: number | null
    refresh?: boolean
    title?: string | null
    /** The episode's own title, for Discord. */
    episodeTitle?: string | null
  }) => invoke<WatchInfo>("watch", { request }),
  library: () => invoke<LibraryEntry[]>("library_list"),
  /** Asks for a sync with the account; dropped within 30 s of the last (src-tauri library::sync_with_account). */
  librarySync: () => invoke<void>("library_sync"),
  libraryStatus: (id: number) => invoke<LibraryStatus | null>("library_status", { id }),
  librarySet: (anime: AnimeCard, status: LibraryStatus | null) => invoke<void>("library_set", { anime, status }),
  progressFor: (id: number) => invoke<Progress[]>("progress_for", { id }),
  continueWatching: () => invoke<ContinueEntry[]>("continue_watching"),
  clearHistory: (id: number | null) => invoke<void>("clear_history", { id }),
  continueHide: (id: number) => invoke<void>("continue_hide", { id }),
  clearCache: () => invoke<void>("clear_cache"),
  settings: () => invoke<Settings>("settings_get"),
  saveSettings: (settings: Settings) => invoke<void>("settings_set", { settings }),
  authStatus: () => invoke<Viewer | null>("auth_status"),
  login: () => invoke<void>("auth_login"),
  logout: () => invoke<void>("auth_logout"),
  anilistList: () => invoke<ListEntry[]>("anilist_list"),
  accountList: () => invoke<ListEntry[]>("account_list"),
  malStatus: () => invoke<MalUser | null>("mal_status"),
  accountProfile: () => invoke<AccountProfile | null>("account_profile"),
  recsHome: () => invoke<HomeRecs>("recs_home"),
  watchOrder: (id: number) => invoke<WatchOrderEntry[]>("watch_order", { id }),
  backdropSave: (dataUrl: string) => invoke<void>("backdrop_save", { dataUrl }),
  backdropLoad: () => invoke<string | null>("backdrop_load"),
  dicePool: (page: number) => invoke<DicePick[]>("dice_pool", { page }),
  diceTaste: () => invoke<DicePick[]>("dice_taste"),
  diceFeedback: (id: number, opened: boolean) => invoke<void>("dice_feedback", { id, opened }),
  accountRefresh: () => invoke<void>("account_refresh"),
  openAccountPage: (url: string) => invoke<void>("open_account_page", { url }),
  openDiscord: () => invoke<void>("open_discord"),
  dnsEngaged: () => invoke<boolean>("dns_engaged"),
  opensubtitlesAccount: () => invoke<OpenSubtitlesAccount>("opensubtitles_account"),
  opensubtitlesLogin: (username: string, password: string) => invoke<string>("opensubtitles_login", { username, password }),
  opensubtitlesLogout: () => invoke<void>("opensubtitles_logout"),
  openOpenSubtitlesSignup: () => invoke<void>("opensubtitles_signup"),
  opensubtitlesSearch: (query: string, languages: string[], episode: number | null) =>
    invoke<SubtitleFile[]>("opensubtitles_search", { query, languages, season: null, episode }),
  opensubtitlesDownload: (fileId: number) => invoke<SubtitleLink>("opensubtitles_download", { fileId }),
  /** Discord status for an embedded player (its clock once the page reports one); `null` takes it down. */
  embedPresence: (
    embed: {
      animeId: number
      episode: number
      title: string | null
      episodeTitle: string | null
      position?: number
      duration?: number
      paused?: boolean
    } | null,
  ) => invoke<void>("embed_presence", { embed }),
  malLogin: () => invoke<void>("mal_login"),
  malLogout: () => invoke<void>("mal_logout"),
  simklStatus: () => invoke<SimklAccount | null>("simkl_status"),
  /** Gets a code and opens simkl.com/pin with it; `simkl-changed` follows once it is approved. */
  simklLogin: () => invoke<SimklCode>("simkl_login"),
  simklLoginCancel: () => invoke<void>("simkl_login_cancel"),
  simklLogout: () => invoke<void>("simkl_logout"),
  downloads: () => invoke<DownloadRecord[]>("downloads_list"),
  download: (anime: AnimeCard, episodes: number[], audio?: Audio | null, options?: DownloadOptions | null) =>
    invoke<number>("download_enqueue", { anime, episodes, audio: audio ?? null, options: options ?? null }),
  downloadsState: () => invoke<QueueState>("downloads_state"),
  downloadsPause: (paused: boolean) => invoke<void>("downloads_pause", { paused }),
  downloadsCancelAll: () => invoke<void>("downloads_cancel_all"),
  downloadsRetryFailed: () => invoke<number>("downloads_retry_failed"),
  downloadsClearUnfinished: () => invoke<number>("downloads_clear_unfinished"),
  downloadsOpenFolder: () => invoke<void>("downloads_open_folder"),
  downloadCheck: (request: { animeId: number; episode: number; audio: Audio; provider: string | null; maxHeight: number | null; hardSubs: boolean }) =>
    invoke<QualityCheck>("download_check", { request }),
  notifications: (markRead: boolean) => invoke<NotificationsPage>("notifications_list", { markRead }),
  notificationsUnread: () => invoke<number>("notifications_unread"),
  notificationsClear: () => invoke<void>("notifications_clear"),
  reminders: () => invoke<string[]>("reminders_list"),
  reminderToggle: (entry: AiringEntry) => invoke<boolean>("reminder_toggle", { entry }),
  notificationTest: () => invoke<void>("notification_test"),
  downloadCancel: (id: number) => invoke<void>("download_cancel", { id }),
  downloadRetry: (id: number) => invoke<void>("download_retry", { id }),
  downloadRemove: (id: number, deleteFile: boolean) => invoke<void>("download_remove", { id, deleteFile }),
  downloadReveal: (id: number) => invoke<void>("download_reveal", { id }),
  downloadPlay: (id: number) => invoke<DownloadRecord>("download_play", { id }),
  updateCheck: () => invoke<UpdateStatus>("update_check"),
  updateNotify: (update: Update) => invoke<void>("update_notify", { update }),
  updateDownload: (update: Update) => invoke<void>("update_download", { update }),
  updateApply: () => invoke<void>("update_apply"),
  providers: () => invoke<PluginInfo[]>("providers_list"),
  providersUpdate: () => invoke<string[]>("providers_update"),
  diagnosticsSave: () => invoke<string>("diagnostics_save"),
  diagnosticsOpenLogs: () => invoke<void>("diagnostics_open_logs"),
  revealPath: (path: string) => invoke<void>("reveal_path", { path }),
  backupSuggestPassphrase: () => invoke<string>("backup_suggest_passphrase"),
  backupExport: (passphrase: string | null) => invoke<BackupExported>("backup_export", { passphrase }),
  backupInspect: (text: string) => invoke<BackupInfo>("backup_inspect", { text }),
  backupRestore: (text: string, passphrase: string | null) => invoke<BackupRestored>("backup_restore", { text, passphrase }),
}

export const keys = {
  home: ["home"] as const,
  search: (f: SearchFilters) => ["search", f] as const,
  detail: (id: number) => ["detail", id] as const,
  episodes: (id: number) => ["episodes", id] as const,
  schedule: (from: number, to: number) => ["schedule", from, to] as const,
  library: ["library"] as const,
  libraryStatus: (id: number) => ["library-status", id] as const,
  progress: (id: number) => ["progress", id] as const,
  continue: ["continue"] as const,
  settings: ["settings"] as const,
  viewer: ["viewer"] as const,
  anilistList: ["anilist-list"] as const,
  malUser: ["mal-user"] as const,
  simkl: ["simkl-account"] as const,
  openSubtitles: ["opensubtitles-account"] as const,
  account: ["account-profile"] as const,
  recs: ["recs-home"] as const,
  downloads: ["downloads"] as const,
  queue: ["downloads-state"] as const,
  notifications: ["notifications"] as const,
  unread: ["notifications-unread"] as const,
  reminders: ["reminders"] as const,
}

/** "For you" and "Because you watched": ready when the graph and the list are. */
export const useRecs = () => useQuery({ queryKey: keys.recs, queryFn: api.recsHome, staleTime: 10 * 60_000, retry: false })

/** A title's franchise as a watch guide, for its page's shelf tabs. */
export const useWatchOrder = (id: number) =>
  useQuery({ queryKey: ["watch-order", id], queryFn: () => api.watchOrder(id), staleTime: 60 * 60_000 })

export const useHome = () => useQuery({ queryKey: keys.home, queryFn: api.home, staleTime: 10 * 60_000 })

/** A Home row's further pages, asked for as its shelf is scrolled to its end. `first` is the page
 * `home` opened the row with: when a refetch changes its titles, the extras start over (their
 * ordering moved with them). The query sleeps until `started` — the row's end first coming into
 * view — because an enabled infinite query fetches its opening page by itself, which here would
 * be the row's *next* one. "New episodes" pages from 1 again: its window is "now", so an early
 * page can repeat what the row shows; the row drops ids it has. */
export function useHomeRowMore(row: string, first: AnimeCard[] | undefined, started: boolean) {
  const firstPage = row === "newest" ? 1 : 2
  return useInfiniteQuery({
    queryKey: ["home-row", row, first?.map((a) => a.id)],
    enabled: !!first && started,
    initialPageParam: firstPage,
    queryFn: ({ pageParam }) => api.homeMore(row, pageParam),
    getNextPageParam: (last, pages) => (last.hasMore ? firstPage + pages.length : undefined),
    staleTime: 10 * 60_000,
    retry: false,
    meta: { background: true },
  })
}

export const useSearch = (filters: SearchFilters, enabled = true) =>
  useQuery({
    queryKey: keys.search(filters),
    queryFn: () => api.search(filters),
    enabled,
    placeholderData: keepPreviousData,
    staleTime: 5 * 60_000,
  })

export const useDetail = (id: number) =>
  useQuery({ queryKey: keys.detail(id), queryFn: () => api.detail(id), staleTime: 30 * 60_000 })

export const useEpisodes = (id: number) =>
  useQuery({ queryKey: keys.episodes(id), queryFn: () => api.episodes(id), staleTime: 10 * 60_000, retry: 1 })

export const useSchedule = (from: number, to: number) =>
  useQuery({ queryKey: keys.schedule(from, to), queryFn: () => api.schedule(from, to), staleTime: 15 * 60_000 })

export const useLibrary = () => useQuery({ queryKey: keys.library, queryFn: api.library })

export const useLibraryStatus = (id: number) =>
  useQuery({ queryKey: keys.libraryStatus(id), queryFn: () => api.libraryStatus(id) })

export const useProgress = (id: number) => useQuery({ queryKey: keys.progress(id), queryFn: () => api.progressFor(id) })

/** Titles with a known English dub (the media index's, refreshed daily), as a set. */
export const useDubTitles = () =>
  useQuery({ queryKey: ["dub-titles"], queryFn: api.dubTitles, staleTime: 6 * 60 * 60_000, select: toSet, meta: { background: true } })
const toSet = (ids: number[]) => new Set(ids)

export const useContinue = () => useQuery({ queryKey: keys.continue, queryFn: api.continueWatching })

export const useSettings = () => useQuery({ queryKey: keys.settings, queryFn: api.settings, staleTime: Infinity })

export const useViewer = () => {
  const client = useQueryClient()
  useEffect(() => {
    const unlisten = listen<Viewer | null>("auth-changed", (e) => {
      client.setQueryData(keys.viewer, e.payload)
      void client.invalidateQueries({ queryKey: keys.anilistList })
      void client.invalidateQueries({ queryKey: keys.account })
    })
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [client])
  return useQuery({ queryKey: keys.viewer, queryFn: api.authStatus, staleTime: Infinity, retry: false })
}

/** The signed-in account's list: AniList if signed in there, else MyAnimeList. */
export const useAnilistList = (enabled: boolean) =>
  useQuery({ queryKey: keys.anilistList, queryFn: api.accountList, enabled, staleTime: 2 * 60_000 })

/** The signed-in account for the Profile page (AniList first, as the list is). */
export const useAccountProfile = (enabled: boolean) =>
  useQuery({ queryKey: keys.account, queryFn: api.accountProfile, enabled, staleTime: 2 * 60_000 })

export const useOpenSubtitles = () => useQuery({ queryKey: keys.openSubtitles, queryFn: api.opensubtitlesAccount, staleTime: Infinity })

export const useMalUser = () => {
  const client = useQueryClient()
  useEffect(() => {
    const unlisten = listen<MalUser | null>("mal-changed", (e) => {
      client.setQueryData(keys.malUser, e.payload)
      void client.invalidateQueries({ queryKey: keys.anilistList })
      void client.invalidateQueries({ queryKey: keys.account })
    })
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [client])
  return useQuery({ queryKey: keys.malUser, queryFn: api.malStatus, staleTime: Infinity, retry: false })
}

/** The Simkl tracker's account, kept current by `simkl-changed` (a sign-in, a sign-out, a grant gone). */
export const useSimklAccount = () => {
  const client = useQueryClient()
  useEffect(() => {
    const unlisten = listen<SimklAccount | null>("simkl-changed", (e) => client.setQueryData(keys.simkl, e.payload))
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [client])
  return useQuery({ queryKey: keys.simkl, queryFn: api.simklStatus, staleTime: Infinity, retry: false })
}

/** How the app looks (defaults until settings load). */
export const useAppearance = (): Appearance => useSettings().data?.appearance ?? DEFAULT_APPEARANCE

/** Saves a change to the appearance on top of the rest of the settings. */
export function useSetAppearance() {
  const settings = useSettings().data
  const save = useSaveSettings()
  return (patch: Partial<Appearance>) => {
    if (settings) save.mutate({ ...settings, appearance: { ...settings.appearance, ...patch } })
  }
}

export function useSaveSettings() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: api.saveSettings,
    onMutate: (settings) => {
      const showAdult = client.getQueryData<Settings>(keys.settings)?.showAdult
      client.setQueryData(keys.settings, settings)
      return { showAdult }
    },
    onSuccess: (_data, settings, previous) => {
      if (previous?.showAdult !== settings.showAdult) {
        // Drop old pages as well as the first results, after the backend has saved the setting.
        return client.resetQueries({
          predicate: ({ queryKey }) =>
            [
              "home",
              "home-row",
              "search",
              "anime-tags",
              "schedule",
              "recs-home",
              "watch-order",
            ].includes(String(queryKey[0])),
        })
      }
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: keys.settings })
      // The download folder may have moved, and its disk with it.
      void client.invalidateQueries({ queryKey: keys.queue })
    },
  })
}

export function useSetLibrary() {
  const client = useQueryClient()
  return useMutation({
    mutationFn: ({ anime, status }: { anime: AnimeCard; status: LibraryStatus | null }) => api.librarySet(anime, status),
    onSuccess: (_, { anime }) => {
      // The account changed with it (src-tauri library_set): its list and stats are stale too.
      void client.invalidateQueries({ queryKey: keys.library })
      void client.invalidateQueries({ queryKey: keys.libraryStatus(anime.id) })
      void client.invalidateQueries({ queryKey: keys.anilistList })
      void client.invalidateQueries({ queryKey: keys.account })
    },
  })
}

/** Keeps "continue watching" and per-title progress fresh after an episode is watched. */
export function useProgressRefresh() {
  const client = useQueryClient()
  useEffect(() => {
    const unlisten = listen<{ animeId: number }>("progress-finished", (e) => {
      void client.invalidateQueries({ queryKey: keys.progress(e.payload.animeId) })
      void client.invalidateQueries({ queryKey: keys.continue })
      void client.invalidateQueries({ queryKey: keys.anilistList })
    })
    // A sync with the account changed the library (src-tauri library::sync_with_account).
    const synced = listen("library-changed", () => {
      void client.invalidateQueries({ queryKey: keys.library })
      void client.invalidateQueries({ queryKey: ["library-status"] })
      // The account list it fetched moves Continue watching's rows on, and the title pages' offers.
      void client.invalidateQueries({ queryKey: keys.continue })
      void client.invalidateQueries({ queryKey: keys.anilistList })
    })
    return () => {
      void unlisten.then((fn) => fn())
      void synced.then((fn) => fn())
    }
  }, [client])
}

/** The download queue, kept live by the worker's events. */
export function useDownloads() {
  const client = useQueryClient()
  useEffect(() => {
    const changed = listen("downloads-changed", () => {
      void client.invalidateQueries({ queryKey: keys.downloads })
      void client.invalidateQueries({ queryKey: keys.queue })
    })
    const progress = listen<{ id: number; progress: number; bytes: number; speed: number; eta: number | null }>(
      "download-progress",
      ({ payload }) => {
        client.setQueryData<DownloadRecord[]>(keys.downloads, (rows) =>
          rows?.map((r) =>
            r.id === payload.id
              ? { ...r, status: "downloading", progress: payload.progress, bytes: payload.bytes, speed: payload.speed, eta: payload.eta }
              : r,
          ),
        )
      },
    )
    return () => {
      void changed.then((fn) => fn())
      void progress.then((fn) => fn())
    }
  }, [client])
  return useQuery({ queryKey: keys.downloads, queryFn: api.downloads })
}

/** Paused or not, and the disk the queue writes to. `downloads-changed` refreshes it. */
export const useQueueState = () => useQuery({ queryKey: keys.queue, queryFn: api.downloadsState, staleTime: 30_000, meta: { background: true } })

/** The bell's count: AniList's unread notifications plus the app's own, kept live by the backend.
 * `notifications-changed` means there is something new to list; `notifications-unread` only that
 * the count moved, which the list's own fetch reports (refetching on it would never stop). */
export function useNotificationsUnread() {
  const client = useQueryClient()
  useEffect(() => {
    const changed = listen<number>("notifications-changed", (e) => {
      client.setQueryData(keys.unread, e.payload)
      void client.invalidateQueries({ queryKey: keys.notifications })
    })
    const counted = listen<number>("notifications-unread", (e) => client.setQueryData(keys.unread, e.payload))
    return () => {
      void changed.then((fn) => fn())
      void counted.then((fn) => fn())
    }
  }, [client])
  return useQuery({ queryKey: keys.unread, queryFn: api.notificationsUnread, staleTime: Infinity, meta: { background: true } })
}

export const useNotifications = () =>
  useQuery({ queryKey: keys.notifications, queryFn: () => api.notifications(false), staleTime: 60_000 })

/** The schedule's "airs soon" reminders, as `animeId:episode:airingAt` keys. */
export const useReminders = () => useQuery({ queryKey: keys.reminders, queryFn: api.reminders, staleTime: Infinity, meta: { background: true } })

/**
 * One quiet check per launch when `enabled` (Settings → App → Check for updates on launch); the
 * result is shared by the shell's notification and Settings. Disabled, `refetch` still checks.
 */
export const useUpdateCheck = (enabled = true) =>
  useQuery({ queryKey: ["update"], queryFn: api.updateCheck, staleTime: 6 * 60 * 60_000, retry: false, enabled, meta: { background: true } })
