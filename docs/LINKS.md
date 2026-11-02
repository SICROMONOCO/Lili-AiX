# `anilili://` links

A link opens the same page in either Anilili app. The desktop app defines the format
first (2026-10-04); the Android app doesn't read these links yet. Change this file
only together with both apps' parsers: `crates/core/src/links.rs` here.

| Link | Opens |
| --- | --- |
| `anilili://anime/<id>` | The title's page. |
| `anilili://watch/<id>/<episode>` | That episode, from the saved position. |
| `anilili://watch/<id>/<episode>?t=<seconds>` | That episode, from `t` seconds in. |
| `anilili://search?q=<text>` | Search for `text`, URL-encoded. |

- `<id>` is the AniList id, as everywhere the two apps meet. It is a positive integer.
- `<episode>` is AniList's episode number. It can be fractional, written as the number
  itself (`6.5`, never `6.50`; `3`, never `3.0`).
- `t` is whole seconds when written. A reader accepts a fraction. It ignores `t` when it
  isn't a positive number, and ignores any query parameter it doesn't know, so a newer
  app can add some.
- Readers also accept `anilili:///anime/1` and `anilili:anime/1`, a trailing `/`, and
  any letter case in the scheme. Writers use the forms in the table.
- Anything else is `DLINK-01` ("That Anilili link isn't one this app understands"),
  including a known page with a missing or invalid id or episode.

## On Windows

The installer registers the scheme for the current user
(`HKCU\Software\Classes\anilili`, from `plugins.deep-link` in `tauri.conf.json`), and
the uninstaller removes it. Windows starts the app with the link as an argument. A
running app receives it through single-instance and opens it in the existing window.

A dev build isn't registered: passing a link as an argument has the same effect.

```bash
target/debug/anilili-desktop.exe anilili://anime/154587
```
