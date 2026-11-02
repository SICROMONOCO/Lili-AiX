// `anilili://` links, as docs/LINKS.md writes them down (parsed in crates/core/src/links.rs).

import { toast } from "sonner"

import { toastFailure } from "@/lib/failure"

export const animeLink = (id: number) => `anilili://anime/${id}`

export const watchLink = (id: number, episode: number, start?: number) =>
  `anilili://watch/${id}/${episode}${start && start >= 1 ? `?t=${Math.floor(start)}` : ""}`

export function copyLink(link: string) {
  navigator.clipboard
    .writeText(link)
    .then(() => toast.success("Link copied", { description: "Opens in Anilili on any computer that has it." }))
    .catch(toastFailure)
}
