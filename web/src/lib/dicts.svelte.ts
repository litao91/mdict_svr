/**
 * The dictionary list from `/dicts`.
 *
 * `has_resources` is load-bearing rather than informational: it decides whether
 * the sanitizer scopes a relative asset ref to `/res/{dict}/…` or leaves it
 * root-relative for the server's cross-store fallback (see `rewrite_ref`).
 */

import { fetch_dicts, type DictInfo } from './api.js'

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

export function total_entries(): number {
  return dicts.all.reduce((sum, d) => sum + (d.entries || 0), 0)
}
