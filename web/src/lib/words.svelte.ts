/**
 * The current headword, the selected dictionary tab, and the recent-words list.
 *
 * State lives in the **query string** (`/?word=apple&dict=OALD9`), never in the
 * path. That is what lets the SPA and the dictionary assets share an origin:
 * definitions reference files at arbitrary root paths (`/media/english/thumb/…`,
 * `/txbapple.jpg`), and a path-based route like `/word/apple` would be ambiguous
 * with them. `/word/{word}` is also already an API route.
 */

import { create_recent_list, storage_set_json } from 'svelte-widgets/storage'
import { url_with_params, type UrlParamEntry } from 'svelte-widgets/url-params'

const RECENT_KEY = 'mdict.recent'
const MAX_RECENT = 40

interface WordsState {
  /** The committed headword. Empty means "show the landing page". */
  word: string
  /** Selected dictionary tab, or null for whichever matched first. */
  dict: string | null
  recent: string[]
  /** Bumped on every navigation so effects that cannot read `word` still re-run. */
  seq: number
}

export const words = $state<WordsState>({ word: '', dict: null, recent: [], seq: 0 })

const recent_list = create_recent_list<string>({
  storage_key: RECENT_KEY,
  max_items: MAX_RECENT,
  key_of: (w) => w,
  is_valid: (v): v is string => typeof v === 'string' && v.length > 0,
})

function entries(): UrlParamEntry[] {
  return [
    ['word', words.word, ''],
    ['dict', words.dict ?? '', ''],
  ]
}

function write_url(replace: boolean): void {
  if (typeof window === 'undefined') return
  const next = url_with_params(entries(), window.location)
  const current = `${window.location.pathname}${window.location.search}${window.location.hash}`
  if (next === current) return
  window.history[replace ? 'replaceState' : 'pushState'](null, '', next)
}

/** Look up a word. Pushes a history entry so Back returns to the previous word. */
export function navigate(
  word: string,
  opts: { dict?: string | null; replace?: boolean } = {},
): void {
  const next = word.trim()
  words.word = next
  if (opts.dict !== undefined) words.dict = opts.dict
  if (next) remember(next)
  words.seq += 1
  write_url(opts.replace ?? false)
}

/** Switching tabs is not a navigation; it must not pile up history entries. */
export function select_dict(dict: string | null): void {
  words.dict = dict
  write_url(true)
}

export function home(): void {
  words.word = ''
  words.dict = null
  words.seq += 1
  write_url(false)
}

export function remember(word: string): void {
  words.recent = recent_list.remember(word, words.recent)
}

export function forget(word: string): void {
  words.recent = recent_list.forget(word, words.recent)
}

export function clear_history(): void {
  words.recent = []
  storage_set_json(RECENT_KEY, [])
}

/** Read the initial word from the URL and keep back/forward working. */
export function init_from_url(): void {
  if (typeof window === 'undefined') return
  read_url()
  if (words.word) remember(words.word)
  window.addEventListener('popstate', () => {
    read_url()
    words.seq += 1
  })
}

function read_url(): void {
  const params = new URLSearchParams(window.location.search)
  words.word = (params.get('word') ?? '').trim()
  words.dict = params.get('dict')
  words.recent = recent_list.load()
}
