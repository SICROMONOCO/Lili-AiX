// Miruro's servers, by provider key and the name the site shows them under. Shared by Settings'
// preferred server and the download dialog's first choice. `softSubOnly`: the server publishes
// only a clean picture with subtitle tracks, so "Hard subs only" downloads never take it.

export const MIRURO_PROVIDERS: { label: string; value: string; softSubOnly?: boolean }[] = [
  { label: "bee", value: "anikoto", softSubOnly: true },
  { label: "sun", value: "icarus", softSubOnly: true },
  { label: "hop", value: "kickassanime", softSubOnly: true },
  { label: "kiwi", value: "animepahe" },
  { label: "knob", value: "aniwaves" },
  { label: "pewe", value: "anidbapp" },
  { label: "moo", value: "animegg" },
]

/** A provider key as the app shows it: Miruro's server name, else the extra provider's name. */
export function providerLabel(key: string, extras: { id: string; name: string }[] = []): string {
  return MIRURO_PROVIDERS.find((p) => p.value === key)?.label ?? extras.find((p) => p.id === key)?.name ?? key
}
