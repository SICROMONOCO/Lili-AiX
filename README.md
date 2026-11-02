# Anilili Desktop

Anilili for Windows (macOS planned): Tauri 3 + Rust, with mpv painting the video under a
React/shadcn interface.

## Features

- **Browse**: trending, this season, popular, top rated and upcoming (AniList); search with
  genre/year/season/format/status/sort filters; a weekly airing schedule; Ctrl+K command palette.
- **Watch**: native mpv playback (HLS/DASH/MP4, hardware decoding, libplacebo rendering).
  - **Servers:** chosen automatically by audio, preferred provider and subtitle language, with
    failover to the next server when one fails.
  - **Subtitles:** soft subtitles in your language, styled ASS via libass.
  - **Skipping:** skip intro/ending (manual or automatic).
  - **Continuity:** resume, autoplay next with a countdown.
  - **Picture:** Anime4K upscaling.
  - **Window:** mini player, fullscreen, screenshots, subtitle delay and size, speed.
  - **Embeds:** embedded players for iframe-only servers, with popups and redirect hijacks
    blocked.
- **Library**: local lists (watching, planning…), history and continue watching. Sign in to
  AniList or MyAnimeList to see your lists and sync progress automatically.
- **Extra providers**: sandboxed WebAssembly modules (wasmtime) add servers next to Miruro, and
  update through a signed channel without an app release. Bundled: ani.pm and AnimePahe direct
  (reached through the app's own browser engine, past Cloudflare's challenge), so playback
  survives a Miruro outage.
- **Downloads**: queue episodes (one, unwatched or all, sub or dub), with taskbar progress.
  Subtitles are saved beside the video, and downloads play offline with progress tracking.
- **Desktop**:
  - Discord status.
  - Media keys and the Windows media overlay.
  - Single instance; "Open with" for video files; drag and drop videos or subtitles.
  - Remembered window size.
  - Signed updates.

See [AGENTS.md](AGENTS.md) for layout, rules and commands.
