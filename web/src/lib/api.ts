/**
 * Thin typed wrapper over the mdict_svr HTTP API.
 *
 * Two details of the server shape the design here:
 *
 * - Every request is made **per dictionary** (`?dict=NAME`) rather than once for
 *   all of them, because rewriting a definition's relative asset refs needs to
 *   know which dictionary it came from. It also means tabs can be filled lazily
 *   instead of rendering nine entries of up to 237 KB each up front.
 * - A `404` is ambiguous: with `format=html` both "this dictionary does not have
 *   the word" and "unknown dictionary" are 404s. The first is a normal, silent
 *   miss; the second is a bug. So `ok` is checked *and* the JSON `error` field is
 *   read before deciding which.
 */

export interface DictInfo {
  name: string
  title: string
  description: string
  creation_date: string
  source: string
  entries: number
  records: number
  case_sensitive: boolean
  has_resources: boolean
}

export interface SuggestResponse {
  query: string
  count: number
  suggestions: string[]
}

/** Outcome of looking one word up in one dictionary. */
export type LookupOutcome =
  | { kind: 'ok'; dict: string; html: string }
  /** This dictionary simply does not have the word. Not an error. */
  | { kind: 'miss'; dict: string }
  | { kind: 'error'; dict: string; message: string }

/**
 * The message behind a non-2xx response. Parsed from the body rather than the
 * `Content-Type` header: a JSON error that arrives without the header must still
 * read as "not found", otherwise an ordinary miss surfaces as a fault toast.
 */
async function error_body(res: Response): Promise<string> {
  const text = await res.text().catch(() => '')
  try {
    const body = JSON.parse(text) as { error?: unknown }
    if (typeof body.error === 'string' && body.error) return body.error
  } catch {
    /* a non-JSON error body is not worth failing over */
  }
  return `HTTP ${res.status}`
}

/** `GET /dicts` — the loaded dictionaries and their metadata. */
export async function fetch_dicts(signal?: AbortSignal): Promise<DictInfo[]> {
  const res = await fetch('/dicts', { signal, headers: { accept: 'application/json' } })
  if (!res.ok) throw new Error(await error_body(res))
  const body = (await res.json()) as { dictionaries?: DictInfo[] }
  return body.dictionaries ?? []
}

/** `GET /suggest` — prefix autocomplete. No match is `200` with an empty list. */
export async function fetch_suggest(
  query: string,
  opts: { dict?: string; limit?: number; signal?: AbortSignal } = {},
): Promise<string[]> {
  const params = new URLSearchParams({ q: query, limit: String(opts.limit ?? 20) })
  if (opts.dict) params.set('dict', opts.dict)
  const res = await fetch(`/suggest?${params}`, {
    signal: opts.signal,
    headers: { accept: 'application/json' },
  })
  if (!res.ok) throw new Error(await error_body(res))
  const body = (await res.json()) as Partial<SuggestResponse>
  return body.suggestions ?? []
}

/** `GET /lookup` for one dictionary, as HTML. */
export async function fetch_entry(
  word: string,
  dict: string,
  signal?: AbortSignal,
): Promise<LookupOutcome> {
  const params = new URLSearchParams({ word, dict, format: 'html' })
  let res: Response
  try {
    res = await fetch(`/lookup?${params}`, { signal, headers: { accept: 'text/html' } })
  } catch (err) {
    if ((err as Error)?.name === 'AbortError') throw err
    return { kind: 'error', dict, message: 'server unreachable' }
  }
  if (res.ok) {
    return { kind: 'ok', dict, html: await res.text() }
  }
  const message = await error_body(res)
  // "not found" is the normal case for a word this dictionary lacks.
  return message === 'not found'
    ? { kind: 'miss', dict }
    : { kind: 'error', dict, message }
}

/** `GET /lookup` as Markdown — used by the copy button and hover previews. */
export async function fetch_markdown(
  word: string,
  dict: string,
  signal?: AbortSignal,
): Promise<string | null> {
  const params = new URLSearchParams({ word, dict, format: 'md' })
  const res = await fetch(`/lookup?${params}`, { signal, headers: { accept: 'text/markdown' } })
  return res.ok ? await res.text() : null
}
