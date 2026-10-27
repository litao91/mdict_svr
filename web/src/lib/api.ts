/**
 * Thin typed wrapper over the mdict_svr HTTP API.
 *
 * Two details of the server shape the design here:
 *
 * - A definition's relative asset refs can only be rewritten once the dictionary
 *   it came from is known, so a response always says which dictionary each part
 *   belongs to — either because one was asked for by name, or because the server
 *   wrapped each match in a `<section data-dict>`.
 * - A `404` is ambiguous: with `format=html` both "this dictionary does not have
 *   the word" and "unknown dictionary" are 404s. The first is a normal, silent
 *   miss; the second is a bug. So `ok` is checked *and* the JSON `error` field is
 *   read before deciding which.
 */

export interface DictInfo {
  name: string
  title: string
  /** Absent from the shell-embedded boot payload; fetched with `/dicts`. */
  description?: string
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
  /** How the list was produced: `prefix`, `wildcard`, `fuzzy` or `prefix+fuzzy`. */
  mode: string
  /** Only the typo-tolerant additions, for a "did you mean" affordance. */
  fuzzy: string[]
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

export interface ServerInfo {
  dictionaries: DictInfo[]
  /** The server has an ffmpeg configured, so `/transcode` can re-encode audio
      no browser can play (Ogg-Speex). Without it those stay download-only. */
  transcode: boolean
}

/** `GET /dicts` — the loaded dictionaries and what the server can do. */
export async function fetch_dicts(signal?: AbortSignal): Promise<ServerInfo> {
  const res = await fetch('/dicts', { signal, headers: { accept: 'application/json' } })
  if (!res.ok) throw new Error(await error_body(res))
  const body = (await res.json()) as Partial<ServerInfo>
  return { dictionaries: body.dictionaries ?? [], transcode: body.transcode === true }
}

/** `GET /suggest` — prefix autocomplete. No match is `200` with an empty list. */
export async function fetch_suggest(
  query: string,
  opts: { dict?: string; limit?: number; signal?: AbortSignal; fuzzy?: boolean } = {},
): Promise<string[]> {
  return (await fetch_suggest_full(query, opts)).suggestions
}

/**
 * The whole `/suggest` body. `fuzzy` costs a full index scan server-side, so it
 * is opt-in: as-you-type leaves it off, a settled input or a miss turns it on.
 * `fuzzy_results` carries only the typo-tolerant additions, for "did you mean".
 */
export async function fetch_suggest_full(
  query: string,
  opts: { dict?: string; limit?: number; signal?: AbortSignal; fuzzy?: boolean } = {},
): Promise<SuggestResponse> {
  const params = new URLSearchParams({ q: query, limit: String(opts.limit ?? 20) })
  if (opts.dict) params.set('dict', opts.dict)
  if (opts.fuzzy) params.set('fuzzy', 'true')
  const res = await fetch(`/suggest?${params}`, {
    signal: opts.signal,
    headers: { accept: 'application/json' },
  })
  if (!res.ok) throw new Error(await error_body(res))
  const body = (await res.json()) as Partial<SuggestResponse>
  return {
    query: body.query ?? query,
    count: body.count ?? 0,
    mode: body.mode ?? 'prefix',
    fuzzy: body.fuzzy ?? [],
    suggestions: body.suggestions ?? [],
  }
}

export interface NearbyResponse {
  word: string
  dict: string
  exact: boolean
  prev: string[]
  next: string[]
}

/**
 * `GET /nearby` — the headwords either side of `word` in one dictionary's
 * sorted index. Exact and O(log n), unlike shrinking prefixes through
 * `/suggest`, which is forward-only and gives up when no window contains the
 * word.
 */
export async function fetch_nearby(
  word: string,
  opts: { dict?: string; before?: number; after?: number; signal?: AbortSignal } = {},
): Promise<NearbyResponse | null> {
  const params = new URLSearchParams({
    word,
    before: String(opts.before ?? 5),
    after: String(opts.after ?? 5),
  })
  if (opts.dict) params.set('dict', opts.dict)
  const res = await fetch(`/nearby?${params}`, {
    signal: opts.signal,
    headers: { accept: 'application/json' },
  })
  if (!res.ok) return null
  const body = (await res.json()) as Partial<NearbyResponse>
  if (!Array.isArray(body.prev) || !Array.isArray(body.next)) return null
  return {
    word: body.word ?? word,
    dict: body.dict ?? '',
    exact: body.exact === true,
    prev: body.prev,
    next: body.next,
  }
}

/**
 * One dictionary's answer for one word, remembered so the reading loop does not
 * pay a round trip for a word it has already seen: Back and Forward, a tab the
 * reader returns to, and the neighbours fetched ahead while they read.
 *
 * Bounded by bytes, not entries — a definition runs to hundreds of kilobytes of
 * publisher markup, so a count says nothing about what this costs. Oldest first
 * out; `Map` iterates in insertion order and a hit re-inserts, which makes this
 * an LRU without a second data structure.
 */
const LOOKUP_CACHE_BYTES = 12 << 20
const lookups = new Map<string, LookupOutcome>()
let lookup_bytes = 0

const size_of = (outcome: LookupOutcome) =>
  outcome.kind === 'ok' ? outcome.html.length * 2 : 64

export function remember_lookup(word: string, dict: string, outcome: LookupOutcome): void {
  // A failure is worth retrying, so it is the one thing not worth keeping.
  if (outcome.kind === 'error') return
  const key = `${word}\u0000${dict}`
  const cost = size_of(outcome)
  const previous = lookups.get(key)
  if (previous) lookup_bytes -= size_of(previous)
  lookups.delete(key)
  lookups.set(key, outcome)
  lookup_bytes += cost
  for (const oldest of lookups.keys()) {
    if (lookup_bytes <= LOOKUP_CACHE_BYTES) break
    const gone = lookups.get(oldest)
    if (gone) lookup_bytes -= size_of(gone)
    lookups.delete(oldest)
  }
}

export function cached_lookup(word: string, dict: string): LookupOutcome | undefined {
  return lookups.get(`${word}\u0000${dict}`)
}

/** `GET /lookup` for one dictionary, as HTML. */
export async function fetch_entry(
  word: string,
  dict: string,
  signal?: AbortSignal,
): Promise<LookupOutcome> {
  const hit = cached_lookup(word, dict)
  if (hit) return hit
  const params = new URLSearchParams({ word, dict, format: 'html' })
  let res: Response
  try {
    res = await fetch(`/lookup?${params}`, { signal, headers: { accept: 'text/html' } })
  } catch (err) {
    if ((err as Error)?.name === 'AbortError') throw err
    return { kind: 'error', dict, message: 'server unreachable' }
  }
  let outcome: LookupOutcome
  if (res.ok) {
    outcome = { kind: 'ok', dict, html: await res.text() }
  } else {
    const message = await error_body(res)
    // "not found" is the normal case for a word this dictionary lacks.
    outcome = message === 'not found' ? { kind: 'miss', dict } : { kind: 'error', dict, message }
  }
  remember_lookup(word, dict, outcome)
  return outcome
}

/**
 * Warm the cache for a word the reader is about to open. Deliberately without an
 * abort signal: it belongs to no navigation, so leaving the page it was started
 * from must not cancel it.
 */
export function prefetch_entry(word: string, dict: string): void {
  if (!word || !dict || cached_lookup(word, dict)) return
  void fetch_entry(word, dict).catch(() => {})
}

/**
 * Split a multi-dictionary response into per-dictionary HTML. The server wraps
 * each match in `<section class="dict" data-dict="NAME">` (always, when the
 * request carries `except`), which is what makes one request serve the whole
 * tab bar.
 */
export function split_sections(html: string): Map<string, string> {
  const doc = new DOMParser().parseFromString(html, 'text/html')
  const out = new Map<string, string>()
  for (const section of doc.querySelectorAll('section.dict[data-dict]')) {
    section.querySelector(':scope > h3')?.remove()
    const name = section.getAttribute('data-dict')
    if (name) out.set(name, section.innerHTML)
  }
  return out
}

export type CombinedOutcome =
  | { kind: 'ok'; sections: Map<string, string> }
  | { kind: 'miss' }
  | { kind: 'error'; message: string }

/**
 * Every dictionary except the ones named, in a single request. One request per
 * dictionary queues across HTTP/1.1's six sockets, so the second wave waits a
 * full round trip for nothing; this and the preferred dictionary's own request
 * are the whole fan-out.
 */
export async function fetch_entries_except(
  word: string,
  except: readonly string[],
  signal?: AbortSignal,
): Promise<CombinedOutcome> {
  const params = new URLSearchParams({ word, format: 'html' })
  for (const name of except) params.append('except', name)
  let res: Response
  try {
    res = await fetch(`/lookup?${params}`, { signal })
  } catch (err) {
    if ((err as Error)?.name === 'AbortError') throw err
    return { kind: 'error', message: 'server unreachable' }
  }
  if (res.ok) {
    const sections = split_sections(await res.text())
    // One combined answer is every dictionary's answer, so it warms the same
    // cache a single lookup would.
    for (const [dict, html] of sections) {
      remember_lookup(word, dict, { kind: 'ok', dict, html })
    }
    return { kind: 'ok', sections }
  }
  const message = await error_body(res)
  return message === 'not found' ? { kind: 'miss' } : { kind: 'error', message }
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
