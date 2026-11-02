// The dice's draw pile, a port of Android's RandomAnimeDeck: a shuffled page of the pool, dealt one
// title at a time. Kept at module level, so closing the dialog keeps the deck and what it has shown.
//
// A roll costs no request while the pile lasts (a page holds 50), and only an empty pile fetches
// another page, chosen at random from those not yet dealt: nothing is shown twice until every page
// has been through. Every third card comes from the viewer's recommendations instead, only one in a
// few, or the dice would stop being a dice. A taste source that comes back empty (no history yet,
// no graph offline) isn't asked again until the pool has gone round once.

import { api } from "@/lib/api"
import type { DicePick } from "@/lib/types"

/** `anilili_core::anilist::RANDOM_POOL_PAGES`. */
const POOL_PAGES = 70
const TASTE_EVERY = 3
/** Bounds one draw: a pool that keeps answering empty is down, not unlucky. */
const MAX_FETCHES_PER_DRAW = 4

class Deck {
  private pageCount = POOL_PAGES
  private dealtPages = new Set<number>()
  private shown = new Set<number>()
  private pile: DicePick[] = []
  private tastePile: DicePick[] = []
  private tasteDry = false
  private dealt = 0
  private busy: Promise<DicePick | null> | null = null

  private tasteTurn(count: number) {
    return count % TASTE_EVERY === TASTE_EVERY - 1
  }

  /** The title the next draw will deal, when the pile already holds it: for preloading its art. */
  upNext(): DicePick | undefined {
    return (this.tasteTurn(this.dealt) ? this.tastePile[0] : undefined) ?? this.pile[0]
  }

  /** Deals one title. A draw already under way is shared rather than stacked. */
  draw(): Promise<DicePick | null> {
    this.busy ??= this.deal().finally(() => {
      this.busy = null
    })
    return this.busy
  }

  private async deal(): Promise<DicePick | null> {
    if (this.tasteTurn(this.dealt)) {
      const pick = await this.drawTaste()
      if (pick) {
        this.shown.add(pick.id)
        this.dealt++
        return pick
      }
    }
    this.dealt++
    for (let i = 0; i < MAX_FETCHES_PER_DRAW; i++) {
      if (this.pile.length === 0) await this.refill()
      const pick = this.pile.shift()
      if (pick) {
        this.shown.add(pick.id)
        return pick
      }
    }
    return null
  }

  private async drawTaste(): Promise<DicePick | null> {
    if (this.tastePile.length === 0 && !this.tasteDry) {
      const fresh = (await api.diceTaste().catch(() => [])).filter((p) => !this.shown.has(p.id))
      if (fresh.length === 0) this.tasteDry = true
      else this.tastePile.push(...fresh)
    }
    while (this.tastePile.length > 0) {
      const pick = this.tastePile.shift()!
      if (!this.shown.has(pick.id)) return pick
    }
    return null
  }

  private async refill() {
    if (this.dealtPages.size >= this.pageCount) {
      // The whole pool has been through the pile: start again rather than run dry.
      this.dealtPages.clear()
      this.shown.clear()
      this.tasteDry = false
    }
    const left = Array.from({ length: this.pageCount }, (_, i) => i + 1).filter((p) => !this.dealtPages.has(p))
    const page = left[Math.floor(Math.random() * left.length)]
    const items = await api.dicePool(page)
    if (items.length === 0) {
      // The pool has shrunk below this page since it was measured: everything past it is empty too.
      this.pageCount = Math.max(1, page - 1)
      for (const p of this.dealtPages) if (p > this.pageCount) this.dealtPages.delete(p)
      if (page === 1) this.dealtPages.add(page)
      return
    }
    this.dealtPages.add(page)
    // The page comes shuffled, and without titles already started unless that left nothing.
    this.pile.push(...items.filter((p) => !this.shown.has(p.id)))
  }
}

export const deck = new Deck()

// ---- what the dialog shows: Android's RandomAnimeViewModel -------------------------------------

export type DiceState = { pick: DicePick | null; upNext: DicePick | null; rolling: boolean; failure: unknown }

let state: DiceState = { pick: null, upNext: null, rolling: false, failure: null }
const listeners = new Set<() => void>()

function set(next: Partial<DiceState>) {
  state = { ...state, ...next }
  for (const listener of listeners) listener()
}

export const dice = {
  subscribe(listener: () => void) {
    listeners.add(listener)
    return () => void listeners.delete(listener)
  },
  snapshot: () => state,

  /** Deals the next title; a draw under way is left to finish. */
  async roll() {
    if (state.rolling) return
    set({ rolling: true, failure: null })
    try {
      const pick = await deck.draw()
      const upNext = deck.upNext() ?? null
      set({ pick, upNext, rolling: false, failure: pick ? null : new Error("The pool returned no titles.") })
      // Warms the card under the top one, so a swipe uncovers a picture rather than a blank.
      if (upNext?.cover) new Image().src = upNext.cover
    } catch (failure) {
      set({ pick: null, upNext: null, rolling: false, failure })
    }
  },

  /** Opening the dialog deals a card, unless it still holds the one it was closed on. */
  ensurePick() {
    if (!state.pick) void dice.roll()
  },

  /** The viewer took the pick to its page: the next opening should deal something new. */
  opened() {
    if (state.pick) void api.diceFeedback(state.pick.id, true).catch(() => {})
    void dice.roll()
  },

  /** Swiped away: a light "not this" for the taste model. */
  skipped() {
    if (state.pick) void api.diceFeedback(state.pick.id, false).catch(() => {})
    void dice.roll()
  },
}
