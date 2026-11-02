// Caption appearance (Settings → Subtitles): Android's choices (CaptionStyle.kt) and how the
// settings preview draws them. The player's own captions are mpv's, styled in src-tauri/src/player.rs
// (`apply_captions`); keep the faces and colours here in step with it.

import type { CSSProperties } from "react"

import type { Captions } from "@/lib/types"

export const CAPTION_FONTS: { value: Captions["font"]; label: string; css: string }[] = [
  { value: "sans", label: "Sans", css: 'Arial, "Helvetica Neue", sans-serif' },
  { value: "serif", label: "Serif", css: "Georgia, serif" },
  { value: "mono", label: "Mono", css: "Consolas, Menlo, monospace" },
  { value: "condensed", label: "Narrow", css: '"Bahnschrift Condensed", "Avenir Next Condensed", "Arial Narrow", sans-serif' },
  { value: "casual", label: "Casual", css: '"Comic Sans MS", cursive' },
  { value: "cursive", label: "Script", css: '"Segoe Script", "Snell Roundhand", cursive' },
]

export const CAPTION_TEXT_COLORS: { value: Captions["textColor"]; label: string; hex: string }[] = [
  { value: "white", label: "White", hex: "#ffffff" },
  { value: "yellow", label: "Yellow", hex: "#ffeb3b" },
  { value: "cyan", label: "Cyan", hex: "#4dd0e1" },
  { value: "green", label: "Green", hex: "#81c784" },
  { value: "magenta", label: "Magenta", hex: "#ff80ab" },
]

export const CAPTION_BACKGROUNDS: { value: Captions["backgroundColor"]; label: string; hex: string }[] = [
  { value: "black", label: "Black", hex: "#000000" },
  { value: "gray", label: "Gray", hex: "#4a4a4a" },
  { value: "navy", label: "Navy", hex: "#102a43" },
  { value: "white", label: "White", hex: "#ffffff" },
]

export const CAPTION_EDGES: { value: Captions["edgeStyle"]; label: string }[] = [
  { value: "none", label: "None" },
  { value: "outline", label: "Outline" },
  { value: "shadow", label: "Shadow" },
]

/** The desktop's defaults (Rust `Captions::default`): the look it always had. */
export const DEFAULT_CAPTIONS: Captions = {
  font: "sans",
  bold: false,
  textColor: "white",
  textOpacity: 100,
  backgroundColor: "black",
  backgroundOpacity: 0,
  edgeStyle: "outline",
  position: "bottom",
  edgeMargin: 3,
  horizontalAnchor: 50,
}

/**
 * Android's five steps across the picture. mpv aligns text left, centre or right with a margin, so
 * these are what it can do (player.rs `caption_across`): against an edge, pushed in, or centred.
 */
export const CAPTION_ACROSS: { value: number; label: string; short: string }[] = [
  { value: 0, label: "Left edge", short: "⇤" },
  { value: 25, label: "Left", short: "←" },
  { value: 50, label: "Centre", short: "Centre" },
  { value: 75, label: "Right", short: "→" },
  { value: 100, label: "Right edge", short: "⇥" },
]

/** The step an anchor lands on, as player.rs rounds it. */
export function captionAcross(anchor: number): number {
  return anchor <= 12 ? 0 : anchor <= 37 ? 25 : anchor <= 62 ? 50 : anchor <= 87 ? 75 : 100
}

/** The preview's line placement for an anchor: alignment and the inset from that side, in mpv units. */
export function captionAcrossStyle(anchor: number): { justify: "flex-start" | "center" | "flex-end"; inset: number } {
  const step = captionAcross(anchor)
  if (step === 50) return { justify: "center", inset: 19 }
  return { justify: step < 50 ? "flex-start" : "flex-end", inset: step === 0 || step === 100 ? 19 : 150 }
}

/** Starting points, one click to a known-good look: the desktop's own (Android's editor has none). */
export const CAPTION_PRESETS: { label: string; captions: Partial<Captions> }[] = [
  { label: "Classic", captions: DEFAULT_CAPTIONS },
  { label: "Boxed", captions: { edgeStyle: "none", backgroundColor: "black", backgroundOpacity: 60, bold: true } },
  { label: "Cinema", captions: { textColor: "yellow", edgeStyle: "shadow", backgroundOpacity: 0, bold: true } },
  { label: "High contrast", captions: { textColor: "white", backgroundColor: "black", backgroundOpacity: 100, edgeStyle: "none", bold: true } },
]

function rgba(hex: string, opacity: number): string {
  const n = Number.parseInt(hex.slice(1), 16)
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${Math.min(100, Math.max(0, opacity)) / 100})`
}

/**
 * The preview's caption, as mpv draws it: `height` is the preview's height in pixels, since mpv
 * sizes text against the picture (its font size 44 at a 720-line picture, times the scale).
 */
export function captionStyle(c: Captions, scale: number, height: number): { box: CSSProperties; text: CSSProperties } {
  const text = CAPTION_TEXT_COLORS.find((x) => x.value === c.textColor)?.hex ?? "#ffffff"
  const back = CAPTION_BACKGROUNDS.find((x) => x.value === c.backgroundColor)?.hex ?? "#000000"
  const px = (units: number) => (units * height) / 720
  const edge = rgba("#000000", c.textOpacity)
  const outline = c.edgeStyle === "outline" ? px(2.5) : 0
  const boxed = c.backgroundOpacity > 0
  const shadow = !boxed && c.edgeStyle === "shadow" ? `${px(2)}px ${px(2)}px 0 ${rgba("#000000", c.textOpacity * 0.8)}` : undefined
  return {
    box: {
      backgroundColor: boxed ? rgba(back, c.backgroundOpacity) : undefined,
      padding: boxed ? `${px(6)}px ${px(10)}px` : undefined,
      borderRadius: boxed ? px(4) : undefined,
    },
    text: {
      fontFamily: CAPTION_FONTS.find((x) => x.value === c.font)?.css,
      fontWeight: c.bold ? 700 : 400,
      // libass sizes a line by its full height, CSS by the em, so the same number reads larger here.
      fontSize: px(44 * scale) * 0.8,
      lineHeight: 1.2,
      color: rgba(text, c.textOpacity),
      WebkitTextStroke: outline ? `${outline}px ${edge}` : undefined,
      paintOrder: "stroke fill",
      textShadow: shadow,
    },
  }
}
