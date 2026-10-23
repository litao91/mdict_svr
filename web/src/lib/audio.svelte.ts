/**
 * The single shared audio player.
 *
 * Two constraints shape this module:
 *
 * - `play()` must be called **synchronously inside the click handler**. Any
 *   `await` before it loses the user gesture and Safari/Firefox refuse to start
 *   playback. So the element's `src` is set and `play()` called before anything
 *   asynchronous happens.
 * - The `HTMLAudioElement` is held outside `$state` on purpose. `$state`
 *   deep-proxies its value, and a proxied `HTMLMediaElement` breaks `play()` and
 *   event-target identity checks.
 *
 * Codec detection happens *before* the element is created, because the server
 * cannot be trusted to signal the problem: `mime_guess` maps `.spx` to
 * `audio/ogg`, so Ogg-Speex files arrive as `200 OK, Content-Type: audio/ogg`
 * and only fail when the decoder runs.
 */

import { toast } from 'svelte-widgets/toast-queue'
import { audio_is_playable, extension_of_url } from './sanitize.js'

export type AudioStatus =
  | 'idle'
  | 'loading'
  | 'playing'
  | 'paused'
  | 'error'
  | 'unsupported'
  | 'missing'

export type AudioVariant = 'uk' | 'us' | null

export interface NowPlaying {
  status: AudioStatus
  url: string | null
  dict: string | null
  /** Human-readable name for the bottom bar, e.g. the headword. */
  label: string | null
  variant: AudioVariant
  reason: 'codec' | 'http' | 'decode' | null
  position: number
  duration: number
  volume: number
}

export const player = $state<NowPlaying>({
  status: 'idle',
  url: null,
  dict: null,
  label: null,
  variant: null,
  reason: null,
  position: 0,
  duration: 0,
  volume: 1,
})

let el: HTMLAudioElement | null = null
let wired = false

function element(): HTMLAudioElement | null {
  if (typeof Audio === 'undefined') return null
  if (!el) el = new Audio()
  if (!wired) {
    wired = true
    el.addEventListener('playing', () => {
      player.status = 'playing'
      player.reason = null
    })
    el.addEventListener('waiting', () => (player.status = 'loading'))
    el.addEventListener('pause', () => {
      if (player.status === 'playing') player.status = 'paused'
    })
    el.addEventListener('ended', () => {
      // Pronunciation clips are about a second long, so going back to `idle`
      // would tear the bar down before the reader registered that it played.
      // `paused` keeps it up with a Play button; `play()` on an ended element
      // seeks to the start again.
      player.status = 'paused'
      player.position = 0
    })
    el.addEventListener('timeupdate', () => {
      player.position = el?.currentTime ?? 0
      player.duration = Number.isFinite(el?.duration ?? NaN) ? (el!.duration as number) : 0
    })
    el.addEventListener('loadedmetadata', () => {
      player.duration = Number.isFinite(el?.duration ?? NaN) ? (el!.duration as number) : 0
    })
    el.addEventListener('error', () => void diagnose())
  }
  return el
}

/**
 * Work out *why* playback failed. The `<audio>` element only reports a decode
 * error, which is what both a missing file and an undecodable codec look like,
 * so a lightweight fetch separates them. Audio files here are 3–6 KB, and this
 * only runs on failure.
 */
async function diagnose(): Promise<void> {
  const url = player.url
  if (!url) return
  player.status = 'error'
  try {
    const res = await fetch(url, { headers: { accept: '*/*' } })
    if (!res.ok) {
      player.status = 'missing'
      player.reason = 'http'
      toast.show(`No audio file stored for this entry (HTTP ${res.status})`, {
        priority: 'warning',
        dedupe_key: `missing:${url}`,
      })
      return
    }
    await res.body?.cancel()
  } catch {
    /* the server may be gone; fall through to the generic decode message */
  }
  player.reason = 'decode'
  toast.show('This browser could not decode the audio file', {
    priority: 'error',
    dedupe_key: `decode:${url}`,
  })
}

/** BrE vs NAmE, inferred from the asset name — there is no variant API. */
export function detect_variant(url: string, anchor: Element | null): AudioVariant {
  const markup = anchor ? `${anchor.getAttribute('class') ?? ''} ${anchor.innerHTML.slice(0, 500)}` : ''
  const haystack = `${url} ${markup}`.toLowerCase()
  if (/(\b|_|\/|-)(uk|gb|brit)(\b|_|\/|-)|snd_uk|pron-uk|_gb_/.test(haystack)) return 'uk'
  if (/(\b|_|\/|-)(us|n_am|amer)(\b|_|\/|-)|snd_us|pron-us|_us_/.test(haystack)) return 'us'
  return null
}

export interface PlayMeta {
  dict?: string | null
  label?: string | null
  variant?: AudioVariant
}

/**
 * Start (or restart) playback of a reference the sanitizer already marked
 * playable. Unsupported codecs never reach here — see `reject`.
 */
export function play(url: string, meta: PlayMeta = {}): void {
  // Defensive: the sanitizer already marks unplayable refs, but `play` is public
  // and a codec check here costs nothing.
  if (!audio_is_playable(url)) {
    reject(url, meta.dict ?? null)
    return
  }
  const audio = element()
  if (!audio) {
    toast.show('Audio is not available in this environment', { priority: 'error' })
    return
  }
  player.url = url
  player.dict = meta.dict ?? null
  player.label = meta.label ?? file_label(url)
  player.variant = meta.variant ?? null
  player.reason = null
  player.position = 0
  player.duration = 0
  player.status = 'loading'
  // Synchronous, inside the click handler, so the user gesture is still valid.
  audio.src = url
  audio.volume = player.volume
  void audio.play().catch(() => {
    /* the error listener does the real diagnosis */
  })
}

/** Click on a reference whose codec no browser can decode: explain, then offer the file. */
export function reject(url: string, dict: string | null): void {
  player.status = 'unsupported'
  player.url = url
  player.dict = dict
  player.label = file_label(url)
  player.reason = 'codec'
  const ext = extension_of_url(url)
  toast.show(
    ext === 'spx'
      ? 'Ogg-Speex audio — no browser can decode this format'
      : `Unsupported audio format (.${ext || 'unknown'})`,
    {
      priority: 'warning',
      dedupe_key: `codec:${ext}`,
      action: {
        label: 'Download',
        on_click: () => {
          const a = document.createElement('a')
          a.href = url
          a.download = file_label(url)
          a.click()
        },
      },
    },
  )
}

export function toggle(url: string, meta: PlayMeta = {}): void {
  const audio = element()
  if (player.url === url && audio && !audio.paused && player.status === 'playing') {
    audio.pause()
    player.status = 'paused'
    return
  }
  if (player.url === url && audio && player.status === 'paused') {
    player.status = 'loading'
    void audio.play().catch(() => {})
    return
  }
  play(url, meta)
}

export function pause(): void {
  el?.pause()
  if (player.status === 'playing') player.status = 'paused'
}

export function resume(): void {
  if (!el || !player.url) return
  player.status = 'loading'
  void el.play().catch(() => {})
}

export function stop(): void {
  if (!el) return
  el.pause()
  el.removeAttribute('src')
  el.load()
  player.status = 'idle'
  player.position = 0
  player.url = null
}

export function seek(seconds: number): void {
  if (!el) return
  // The server sends no Accept-Ranges, so seeking is best-effort: it works once
  // the whole (small) file is buffered and does nothing before that.
  el.currentTime = seconds
  player.position = seconds
}

export function set_volume(value: number): void {
  player.volume = Math.min(1, Math.max(0, value))
  if (el) el.volume = player.volume
}

/** Whether this browser claims it can handle Ogg-Speex. It cannot, anywhere. */
export function browser_reports_speex_support(): boolean {
  if (typeof Audio === 'undefined') return false
  return new Audio().canPlayType('audio/ogg; codecs="speex"') !== ''
}

function file_label(url: string): string {
  const path = url.split(/[?#]/)[0] ?? url
  return decodeURIComponent(path.slice(path.lastIndexOf('/') + 1)) || url
}
