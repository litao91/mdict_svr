/**
 * The payload the server embeds in the SPA shell (`src/web.rs`).
 *
 * Without it, boot costs two round trips before anything can render: `/dicts`,
 * then the lookup the URL already named. The server knows both answers while it
 * writes the shell, so it inlines them and the app starts sanitizing the entry
 * the reader asked for before it has fetched anything at all. The remaining
 * dictionaries still come over the wire — inlining those too would put the whole
 * fan-out back on the critical path to the first paint.
 */

import type { DictInfo } from './api.js'

export interface BootPayload {
  dictionaries: DictInfo[]
  transcode: boolean
  /** Present only when the URL named a word the server could find. */
  word?: string
  /** Definition HTML for `word`, keyed by dictionary name. */
  entries?: Record<string, string>
}

function read_boot(): BootPayload | null {
  if (typeof document === 'undefined') return null
  const el = document.getElementById('mdict-boot')
  if (!el || el.tagName !== 'SCRIPT') return null
  try {
    const parsed = JSON.parse(el.textContent ?? '') as Partial<BootPayload>
    if (!Array.isArray(parsed.dictionaries)) return null
    return {
      dictionaries: parsed.dictionaries as DictInfo[],
      transcode: parsed.transcode === true,
      word: typeof parsed.word === 'string' ? parsed.word : undefined,
      entries:
        parsed.entries && typeof parsed.entries === 'object'
          ? (parsed.entries as Record<string, string>)
          : undefined,
    }
  } catch {
    return null
  }
}

export const boot: BootPayload | null = read_boot()
