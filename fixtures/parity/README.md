# Parity corpus

Cases both Anilili apps must answer the same way, in plain JSON so neither app's language
owns them. Each file says where its answers come from and which code reads it.

| File | What | Desktop test |
| --- | --- | --- |
| `titles.json` | Season numbers, base series titles and title scores: how a provider picks a site's entry for an AniList title. Android's answers (`NativeProviderParsersTest.kt`). | `providers/kit` `matches_titles_as_android_does` |
| `links.json` | `anilili://` links read, refused and written (`docs/LINKS.md`). Android reads none yet. | `crates/core` `links` tests |
| `backup-sealed.json` | A backup whose logins were sealed by an independent implementation; both apps must open it. | `crates/core` `backup::opens_logins_sealed_elsewhere` |

Android doesn't read this directory yet. Its tests keep their cases inline, which is where
`titles.json` came from.

**Adding a case.** When one app's behaviour changes on purpose, add the case here with that
app's answer, then make the other app pass it. A case one app fails is a disagreement to
settle, not a test to delete. Keep `note` for the reason behind a surprising answer.

The commits to look at come from `node scripts/port-ledger.mjs`: Android changes since the
last sync point (`docs/android-sync.json`), grouped by the desktop code that mirrors them.
