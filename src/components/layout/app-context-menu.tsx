// The app's own right-click menu, in place of the webview's (Back, Reload, Inspect…): Refresh,
// About and the keyboard shortcuts everywhere, plus Cut/Copy/Paste where there is text to act on.
// A card's own menu (anime-actions.tsx) takes the click first and this one stays shut.

import { useQueryClient } from "@tanstack/react-query"
import { useRouter } from "@tanstack/react-router"
import { ClipboardPasteIcon, CopyIcon, ExternalLinkIcon, HeartIcon, InfoIcon, KeyboardIcon, RefreshCwIcon, ScissorsIcon } from "lucide-react"
import { useState } from "react"

import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuGroup,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/components/ui/context-menu"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { api } from "@/lib/api"
import { toastFailure } from "@/lib/failure"

/** The shortcut modifier as the platform writes it. */
const MOD = /Mac/i.test(navigator.platform) ? "⌘" : "Ctrl+"

/** Opens the keyboard shortcuts (layout/shortcuts-dialog.tsx listens for it). */
export const SHORTCUTS_EVENT = "anilili:shortcuts"

/** What the click landed on: an editable field, and whatever text is selected. */
type Target = { editable: HTMLInputElement | HTMLTextAreaElement | HTMLElement | null; selection: string }

export function AppContextMenu({ children }: { children: React.ReactNode }) {
  const [target, setTarget] = useState<Target>({ editable: null, selection: "" })
  const [about, setAbout] = useState(false)
  const client = useQueryClient()
  const router = useRouter()

  // Everything on screen asked for again: the page's queries and the route's loaders. Kinder than a
  // reload, which would also stop whatever is playing and lose the page's place.
  const refresh = () => {
    void client.invalidateQueries()
    void router.invalidate()
  }

  return (
    <>
      <ContextMenu>
        <ContextMenuTrigger
          className="contents select-auto"
          onContextMenu={(e) => {
            const el = e.target as HTMLElement
            const editable = el.closest<HTMLElement>("input, textarea, [contenteditable=true]")
            setTarget({ editable, selection: window.getSelection()?.toString() ?? "" })
          }}
        >
          {children}
        </ContextMenuTrigger>
        <ContextMenuContent className="w-56">
          {(target.editable || target.selection) && (
            <>
              <ContextMenuGroup>
                {target.editable && (
                  <ContextMenuItem onClick={() => editCommand(target.editable!, "cut")}>
                    <ScissorsIcon />
                    Cut
                    <ContextMenuShortcut>{MOD}X</ContextMenuShortcut>
                  </ContextMenuItem>
                )}
                <ContextMenuItem
                  onClick={() =>
                    target.editable ? editCommand(target.editable, "copy") : void navigator.clipboard.writeText(target.selection)
                  }
                >
                  <CopyIcon />
                  Copy
                  <ContextMenuShortcut>{MOD}C</ContextMenuShortcut>
                </ContextMenuItem>
                {target.editable && (
                  <ContextMenuItem onClick={() => void paste(target.editable!)}>
                    <ClipboardPasteIcon />
                    Paste
                    <ContextMenuShortcut>{MOD}V</ContextMenuShortcut>
                  </ContextMenuItem>
                )}
              </ContextMenuGroup>
              <ContextMenuSeparator />
            </>
          )}
          <ContextMenuGroup>
            <ContextMenuItem onClick={refresh}>
              <RefreshCwIcon />
              Refresh
            </ContextMenuItem>
            <ContextMenuItem onClick={() => window.dispatchEvent(new Event(SHORTCUTS_EVENT))}>
              <KeyboardIcon />
              Keyboard shortcuts
              <ContextMenuShortcut>?</ContextMenuShortcut>
            </ContextMenuItem>
            <ContextMenuItem onClick={() => setAbout(true)}>
              <InfoIcon />
              About Anilili
            </ContextMenuItem>
          </ContextMenuGroup>
        </ContextMenuContent>
      </ContextMenu>
      <AboutDialog open={about} onOpenChange={setAbout} />
    </>
  )
}

/** Cut or copy the field's selection; the webview still honours these for a focused field. */
function editCommand(el: HTMLElement, command: "cut" | "copy") {
  el.focus()
  document.execCommand(command)
}

/** Paste at the caret, through the input's own setter so React sees the change. */
async function paste(el: HTMLElement) {
  const text = await navigator.clipboard.readText().catch(() => "")
  if (!text) return
  el.focus()
  document.execCommand("insertText", false, text)
}

/**
 * About, as Android's About section has it: the Discord server, then the sign-off. Version and
 * updates live in Settings, as Android keeps them under App.
 */
function AboutDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void }) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>About</DialogTitle>
          <DialogDescription className="sr-only">The Discord server and who makes Anilili</DialogDescription>
        </DialogHeader>
        <button
          type="button"
          onClick={() => void api.openDiscord().catch((e) => toastFailure(e))}
          className="flex items-center gap-3 rounded-lg px-3 py-2.5 text-left transition-colors hover:bg-muted focus-visible:bg-muted focus-visible:outline-none"
        >
          <DiscordGlyph className="size-[22px] shrink-0" />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="text-sm font-medium">Discord server</span>
            <span className="text-sm text-muted-foreground">Questions, source problems and release news</span>
          </span>
          <ExternalLinkIcon className="size-4 shrink-0 text-muted-foreground" />
        </button>
        <div className="flex flex-col gap-3.5 px-3 pt-2 pb-1">
          <p className="text-sm text-muted-foreground">
            Anilili is a one-person project: an anime app that finds an episode across dozens of sources, remembers where you
            stopped, and keeps your list in step with AniList or MyAnimeList. No adverts, and no account of its own.
          </p>
          <p className="flex items-center gap-2 text-sm font-bold">
            <HeartIcon className="size-4 text-primary" />
            Made with love by lili
          </p>
        </div>
      </DialogContent>
    </Dialog>
  )
}

/** Android's Discord badge (ic_brand_discord): a disc with the mark knocked out, in the text colour. */
export function DiscordGlyph({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 512 512" aria-hidden="true" className={className}>
      <path
        fill="currentColor"
        fillRule="evenodd"
        d="M256 0c141.385 0 256 114.615 256 256S397.385 512 256 512 0 397.385 0 256 114.615 0 256 0Zm104.932 160.621a250.428 250.428 0 0 0-62.383-19.182 173.883 173.883 0 0 0-7.966 16.243 232.557 232.557 0 0 0-34.619-2.603c-11.569 0-23.195.879-34.622 2.58-2.334-5.509-5.044-10.971-7.986-16.223a252.58 252.58 0 0 0-62.397 19.222c-39.483 58.408-50.183 115.357-44.833 171.497a251.49 251.49 0 0 0 76.502 38.398c6.169-8.327 11.695-17.192 16.386-26.417a161.682 161.682 0 0 1-25.813-12.319c2.164-1.569 4.281-3.186 6.325-4.756 23.912 11.231 50.039 17.088 76.473 17.088 26.436 0 52.562-5.857 76.475-17.089 2.069 1.688 4.186 3.305 6.325 4.755a162.693 162.693 0 0 1-25.86 12.352 183.969 183.969 0 0 0 16.387 26.397 250.498 250.498 0 0 0 76.553-38.392l-.006.007c6.277-65.104-10.725-121.53-44.941-171.558ZM205.78 297.63c-14.908 0-27.226-13.53-27.226-30.175 0-16.645 11.889-30.293 27.178-30.293 15.29 0 27.511 13.648 27.25 30.293-.262 16.645-12.008 30.175-27.202 30.175Zm100.439 0c-14.933 0-27.202-13.53-27.202-30.175 0-16.645 11.889-30.293 27.202-30.293 15.313 0 27.44 13.648 27.178 30.293-.261 16.645-11.984 30.175-27.178 30.175Z"
      />
    </svg>
  )
}
