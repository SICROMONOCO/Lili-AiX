// A server's languages, as Android's server picker shows them (SourceFilter.kt, ProviderCatalog's
// declared languages): the subtitle tracks it carries, and for subtitles burnt into the picture or a
// dub, the language the provider is known for. "Preferred server languages" (Settings → Servers)
// narrows which of them the player's server menu tags and filters by.

import type { Server } from "@/lib/types"

/** Android's `SourceFilterLanguage`: stored value (ISO 639-2), name, and the tag on a server row. */
export const SOURCE_LANGUAGES: { value: string; label: string; tag: string }[] = [
  { value: "ara", label: "Arabic", tag: "AR" },
  { value: "ben", label: "Bengali", tag: "BN" },
  { value: "zho", label: "Chinese", tag: "ZH" },
  { value: "nld", label: "Dutch", tag: "NL" },
  { value: "eng", label: "English", tag: "EN" },
  { value: "fra", label: "French", tag: "FR" },
  { value: "deu", label: "German", tag: "DE" },
  { value: "hin", label: "Hindi", tag: "HI" },
  { value: "ind", label: "Indonesian", tag: "ID" },
  { value: "ita", label: "Italian", tag: "IT" },
  { value: "jpn", label: "Japanese", tag: "JA" },
  { value: "kan", label: "Kannada", tag: "KN" },
  { value: "kor", label: "Korean", tag: "KO" },
  { value: "mal", label: "Malayalam", tag: "ML" },
  { value: "mar", label: "Marathi", tag: "MR" },
  { value: "fas", label: "Persian", tag: "FA" },
  { value: "pol", label: "Polish", tag: "PL" },
  { value: "por", label: "Portuguese", tag: "PT" },
  { value: "rus", label: "Russian", tag: "RU" },
  { value: "spa", label: "Spanish", tag: "ES" },
  { value: "tgl", label: "Tagalog", tag: "TL" },
  { value: "tam", label: "Tamil", tag: "TA" },
  { value: "tel", label: "Telugu", tag: "TE" },
  { value: "tha", label: "Thai", tag: "TH" },
  { value: "tur", label: "Turkish", tag: "TR" },
  { value: "ukr", label: "Ukrainian", tag: "UK" },
  { value: "vie", label: "Vietnamese", tag: "VI" },
]

/** ISO 639-2/B codes some servers send, for the /T ones above. */
const ALIASES: Record<string, string> = { chi: "zho", ger: "deu", fre: "fra", dut: "nld", per: "fas", fil: "tgl" }

/** A subtitle track's language as one of [SOURCE_LANGUAGES]: by code (`en`, `eng`, `pt-BR`) or by name. */
export function sourceLanguage(language: string, label = ""): string | null {
  const code = language.trim().toLowerCase().split(/[-_]/)[0]
  const byCode = SOURCE_LANGUAGES.find((l) => l.tag.toLowerCase() === code || l.value === (ALIASES[code] ?? code))
  if (byCode) return byCode.value
  const text = `${language} ${label}`.toLowerCase()
  return SOURCE_LANGUAGES.find((l) => text.includes(l.label.toLowerCase()))?.value ?? null
}

/**
 * The provider's own language for what it burns in or dubs (Android's `declaredLanguage`): AnimeRift
 * is Arabic; everything else the app reaches is English.
 */
function declaredLanguage(server: Server): string {
  return server.provider === "rift" ? "ara" : "eng"
}

/** The languages a server offers, as stored values, in [SOURCE_LANGUAGES] order. */
export function serverLanguages(server: Server): string[] {
  const found = new Set<string>()
  if (server.track === "ssub") {
    for (const s of server.subtitles) {
      const language = sourceLanguage(s.language, s.label)
      if (language) found.add(language)
    }
  }
  if (server.track !== "ssub" || found.size === 0) found.add(declaredLanguage(server))
  return SOURCE_LANGUAGES.filter((l) => found.has(l.value)).map((l) => l.value)
}

/** What the server menu shows of `languages`: the preferred ones, or every one when none is preferred. */
export function visibleLanguages(languages: string[], preferred: string[]): string[] {
  return preferred.length === 0 ? languages : languages.filter((l) => preferred.includes(l))
}

export const languageTag = (value: string) => SOURCE_LANGUAGES.find((l) => l.value === value)?.tag ?? value.slice(0, 2).toUpperCase()
export const languageName = (value: string) => SOURCE_LANGUAGES.find((l) => l.value === value)?.label ?? value
