// Failures as the UI shows them: the catalogue message, its code, and the next step.
// Codes and messages come from Rust (crates/core/src/catalogue.rs); docs/ERROR_CODES.md lists them.

import { listen } from "@tauri-apps/api/event"
import { useEffect } from "react"
import { toast } from "sonner"

import { api } from "@/lib/api"
import type { CommandError, FailureNext } from "@/lib/types"

export type Failure = {
  code: string | null
  message: string
  detail: string
  next: FailureNext
}

/** Any thrown value as a failure; non-catalogue errors (a JS exception) keep their own text. */
export function toFailure(error: unknown): Failure {
  if (error && typeof error === "object" && "message" in error) {
    const e = error as Partial<CommandError>
    return {
      code: e.code ?? null,
      message: String(e.message),
      detail: e.detail ?? "",
      next: e.next ?? "nothing",
    }
  }
  return { code: null, message: String(error), detail: "", next: "nothing" }
}

/** Writes the diagnostics file and offers to show it. */
export async function saveDiagnostics() {
  try {
    const path = await api.diagnosticsSave()
    toast.success("Diagnostics saved to Downloads", {
      description: "Nothing was sent anywhere. Share the file if you're reporting a problem.",
      action: { label: "Show", onClick: () => void api.revealPath(path) },
    })
  } catch (e) {
    toastFailure(e)
  }
}

/** A failure as a toast: the message, the code to quote, and "Save diagnostics" where it helps. */
export function toastFailure(error: unknown) {
  const f = toFailure(error)
  toast.error(f.message, {
    description: f.code ?? undefined,
    action: f.next === "diagnostics" ? { label: "Save diagnostics", onClick: () => void saveDiagnostics() } : undefined,
  })
}

/** Background failures (sign-in windows, sync, downloads) arrive as `failure` events. */
export function useFailureEvents() {
  useEffect(() => {
    const unlisten = listen<CommandError>("failure", (e) => toastFailure(e.payload))
    return () => {
      void unlisten.then((fn) => fn())
    }
  }, [])
}
