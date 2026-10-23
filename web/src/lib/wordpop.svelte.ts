/**
 * The double-click lookup popover.
 *
 * GoldenDict's scan popup and mdict-go-web's double-click lookup exist because
 * reading a definition produces more words worth defining, and making the
 * reader type them into the search box breaks the reading. This holds the one
 * open popover and fetches its preview.
 */

import { fetch_markdown } from './api.js'

export interface WordPop {
  word: string
  /** Viewport coordinates of the double-click, for positioning. */
  x: number
  y: number
  dict: string | null
  preview: string | null
  loading: boolean
}

export const wordpop = $state<{ current: WordPop | null }>({ current: null })

let seq = 0

export function open_wordpop(word: string, x: number, y: number, dict: string | null): void {
  const id = ++seq
  wordpop.current = { word, x, y, dict, preview: null, loading: true }
  if (!dict) {
    if (wordpop.current) wordpop.current.loading = false
    return
  }
  void fetch_markdown(word, dict)
    .then((md) => {
      if (wordpop.current?.word !== word || seq !== id) return
      // The preview is prose, not source: drop the Markdown link/audio markup
      // (`[audio](/res/…/x.mp3)`) before showing it.
      const lines = (md ?? '')
        .replace(/\[([^\]]*)\]\([^)]*\)/g, '$1')
        .replace(/[*_`]/g, '')
        .split('\n')
        .map((l) => l.trim())
        .filter((l) => l && !l.startsWith('#') && !l.startsWith('→'))
      wordpop.current.preview =
        lines.slice(0, 2).join(' ').replace(/\s+/g, ' ').slice(0, 180) || null
      wordpop.current.loading = false
    })
    .catch(() => {
      if (wordpop.current?.word === word && seq === id) {
        wordpop.current.preview = null
        wordpop.current.loading = false
      }
    })
}

export function close_wordpop(): void {
  seq += 1
  wordpop.current = null
}
