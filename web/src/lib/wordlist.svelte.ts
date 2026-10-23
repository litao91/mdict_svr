/**
 * The reader's own word list — the feature every dictionary product grows
 * eventually (GoldenDict favourites, Cambridge word lists, Pleco's decks) and
 * the reason people keep a dictionary installed: somewhere to put the words
 * they met and want to meet again.
 *
 * History answers "what did I look up"; this answers "what am I learning".
 * Persisted through `svelte-widgets/storage`, which degrades to a no-op when
 * localStorage is unavailable.
 */

import { storage_get_json, storage_set_json } from 'svelte-widgets/storage'

const KEY = 'mdict.wordlist'

export interface WordListItem {
  word: string
  /** Epoch milliseconds; shown as the date the word was kept. */
  added_at: number
}

export type WordlistFormat = 'json' | 'csv' | 'anki'

function load(): WordListItem[] {
  const stored = storage_get_json(KEY, null)
  if (!Array.isArray(stored)) return []
  return stored
    .filter(
      (v): v is WordListItem =>
        !!v &&
        typeof v === 'object' &&
        typeof (v as WordListItem).word === 'string' &&
        (v as WordListItem).word.length > 0 &&
        typeof (v as WordListItem).added_at === 'number',
    )
    .slice(0, 500)
}

export const wordlist = $state<{ items: WordListItem[] }>({ items: load() })

if (typeof window !== 'undefined') {
  $effect.root(() => {
    $effect(() => {
      storage_set_json(KEY, wordlist.items.map((i) => ({ ...i })))
    })
  })
}

export function in_wordlist(word: string): boolean {
  return wordlist.items.some((i) => i.word === word)
}

/** Add or remove; returns whether the word is on the list afterwards. */
export function toggle_wordlist(word: string): boolean {
  const at = wordlist.items.findIndex((i) => i.word === word)
  if (at >= 0) {
    wordlist.items.splice(at, 1)
    return false
  }
  wordlist.items.unshift({ word, added_at: Date.now() })
  return true
}

export function clear_wordlist(): void {
  wordlist.items = []
}

function csv_cell(value: string): string {
  return /[",\n]/.test(value) ? `"${value.replaceAll('"', '""')}"` : value
}

/**
 * Serialize the list. `definition_of` supplies the back side for Anki cards;
 * words without one export with an empty back rather than being dropped, so
 * the deck still matches the list on screen.
 */
export function format_wordlist(
  items: readonly WordListItem[],
  format: WordlistFormat,
  definition_of: (word: string) => string | null = () => null,
): string {
  if (format === 'json') {
    return JSON.stringify(items.map((i) => ({ ...i })), null, 2)
  }
  if (format === 'csv') {
    const rows = items.map((i) =>
      [csv_cell(i.word), new Date(i.added_at).toISOString()].join(','),
    )
    return ['word,added_at', ...rows].join('\n')
  }
  // Anki's TSV import: front, back, tab-separated, one card per line.
  return items
    .map((i) => `${(definition_of(i.word) ?? '').replaceAll('\t', ' ').replaceAll('\n', ' ')}`)
    .map((back, idx) => `${items[idx]!.word}\t${back}`)
    .join('\n')
}

/** Download helper shared by the sidebar's export buttons. */
export function download_wordlist(filename: string, text: string, mime: string): void {
  const blob = new Blob([text], { type: mime })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = filename
  a.click()
  URL.revokeObjectURL(url)
}
