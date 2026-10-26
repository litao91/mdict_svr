/**
 * The dictionary list from `/dicts`.
 *
 * `has_resources` is load-bearing rather than informational: it decides whether
 * the sanitizer scopes a relative asset ref to `/res/{dict}/…` or leaves it
 * root-relative for the server's cross-store fallback (see `rewrite_ref`).
 */

import { fetch_dicts, type DictInfo } from './api.js'
import { is_dict_enabled, settings } from './settings.svelte.js'

interface DictsState {
  all: DictInfo[]
  loading: boolean
  error: string | null
  loaded: boolean
  /** The server can re-encode audio no browser can play; see `/transcode`. */
  transcode: boolean
}

export const dicts = $state<DictsState>({
  all: [],
  loading: false,
  error: null,
  loaded: false,
  transcode: false,
})

let inflight: Promise<void> | null = null

/** Idempotent: concurrent callers share one request. */
export function load_dicts(): Promise<void> {
  if (dicts.loaded) return Promise.resolve()
  if (inflight) return inflight
  dicts.loading = true
  dicts.error = null
  inflight = fetch_dicts()
    .then((info) => {
      dicts.all = info.dictionaries
      dicts.transcode = info.transcode
      dicts.loaded = true
    })
    .catch((err: unknown) => {
      dicts.error = err instanceof Error ? err.message : 'could not reach the server'
    })
    .finally(() => {
      dicts.loading = false
      inflight = null
    })
  return inflight
}

export function info(name: string): DictInfo | undefined {
  return dicts.all.find((d) => d.name === name)
}

export function has_resources(name: string): boolean {
  return info(name)?.has_resources ?? false
}

/**
 * MDict writes this literal string into the header when the compiler left the
 * title field blank — four of the eleven dictionaries in `./data` carry it.
 */
const PLACEHOLDER_TITLE = 'Title (No HTML code allowed)'

/** Display name: the MDict header `Title` when there is a real one, else the key. */
export function display_name(name: string): string {
  const title = info(name)?.title?.trim()
  return title && title !== PLACEHOLDER_TITLE ? title : name
}

/**
 * The reader's order: names in `settings.dict_order` first, in that order, then
 * everything the server loaded in registry order. The sort is stable, so
 * unlisted dictionaries keep their relative position.
 */
export function ordered(): DictInfo[] {
  const rank = new Map(settings.dict_order.map((name, i) => [name, i]))
  return [...dicts.all].sort(
    (a, b) => (rank.get(a.name) ?? Number.MAX_SAFE_INTEGER) - (rank.get(b.name) ?? Number.MAX_SAFE_INTEGER),
  )
}

/** Dictionaries that take part in a lookup, in the reader's order. */
export function ordered_enabled(): DictInfo[] {
  return ordered().filter((d) => is_dict_enabled(d.name))
}

/** Persist a full ordering (every loaded name, in the new sequence). */
export function set_dict_order(names: readonly string[]): void {
  settings.dict_order = [...names]
}

/** Move one dictionary by `delta` positions within the reader's order. */
export function move_dict(name: string, delta: number): void {
  const names = ordered().map((d) => d.name)
  const at = names.indexOf(name)
  const to = at + delta
  if (at < 0 || to < 0 || to >= names.length) return
  names.splice(at, 1)
  names.splice(to, 0, name)
  set_dict_order(names)
}

/** Move the dictionary at `from` to position `to` (drag and drop). */
export function move_dict_to(from: number, to: number): void {
  const names = ordered().map((d) => d.name)
  if (from < 0 || from >= names.length || to < 0 || to >= names.length || from === to) return
  const [name] = names.splice(from, 1)
  names.splice(to, 0, name!)
  set_dict_order(names)
}

/**
 * Switch one dictionary on or off. `enabled_dicts` stays empty while everything
 * is on; disabling the first one materializes the full list minus it. The last
 * enabled dictionary cannot be switched off — an empty list means "all".
 */
export function toggle_dict(name: string): boolean {
  const all = ordered().map((d) => d.name)
  // Copy: splicing an alias of `all` would shrink both and make the
  // length check below read "everything is still on".
  const current = settings.enabled_dicts.length === 0 ? [...all] : [...settings.enabled_dicts]
  const at = current.indexOf(name)
  if (at >= 0) {
    if (current.length === 1) return false
    current.splice(at, 1)
  } else {
    current.push(name)
  }
  settings.enabled_dicts = current.length === all.length ? [] : current
  return at < 0
}

export function total_entries(): number {
  return dicts.all.reduce((sum, d) => sum + (d.entries || 0), 0)
}
