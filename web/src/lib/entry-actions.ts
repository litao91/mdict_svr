/**
 * Behaviour for an injected definition.
 *
 * The entry HTML is not Svelte-managed — it arrives as a string and goes in via
 * `{@html}` — so nothing inside it can carry a Svelte event handler. Everything
 * here is therefore delegated from the host element, which Svelte *does* own and
 * which `{@html}` never replaces, so one set of listeners survives every
 * re-render.
 */

import { toast } from 'svelte-widgets/toast-queue'
import { tooltip } from 'svelte-widgets/attachments'
import { fetch_markdown } from './api.js'
import { detect_variant, play, reject, toggle } from './audio.svelte.js'
import { open_wordpop } from './wordpop.svelte.js'

export interface EntryCallbacks {
  /** A cross-reference to another headword was activated. */
  on_word: (word: string) => void
  /** An image was clicked; the caller opens the lightbox. */
  on_image: (src: string, alt: string) => void
  /** Hover previews are enabled in settings. */
  previews_enabled: () => boolean
}

/** Definition previews, keyed `word\u0000dict`, so hovering twice costs nothing. */
const preview_cache = new Map<string, string>()

const PREVIEW_MAX = 240

/** First meaningful paragraph of a Markdown definition, trimmed to one line. */
export function excerpt(markdown: string): string {
  const lines = markdown
    .split('\n')
    .map((l) => l.trim())
    // Skip headings, rules, images and blank lines: what is left is prose.
    .filter((l) => l && !/^(#{1,6}\s|---$|!\[|-{2,}$)/.test(l))
  const text = lines.join(' ').replace(/\s+/g, ' ').trim()
  if (text.length <= PREVIEW_MAX) return text
  const cut = text.slice(0, PREVIEW_MAX)
  const last_space = cut.lastIndexOf(' ')
  return `${cut.slice(0, last_space > 80 ? last_space : PREVIEW_MAX).trimEnd()}…`
}

function css_escape(value: string): string {
  if (typeof CSS !== 'undefined' && typeof CSS.escape === 'function') return CSS.escape(value)
  return value.replace(/["\\]/g, '\\$&')
}

/**
 * Wire up one entry host. Returns a teardown function; call it before replacing
 * the host's contents or when the component is destroyed.
 */
export function attach_entry_behavior(host: HTMLElement, cb: EntryCallbacks): () => void {
  let marked: Element | null = null
  let preview_timer: ReturnType<typeof setTimeout> | null = null
  let preview_controller: AbortController | null = null

  const dict = (): string => host.dataset.dict ?? ''
  const word = (): string => host.dataset.word ?? ''

  const clear_marked = () => {
    marked?.classList.remove('is-playing')
    marked = null
  }

  /**
   * Media and `<img>` `error` events do not bubble, but they do propagate in the
   * capture phase — that is how a 404'd oxfordPrimary mp3 or a missing thumbnail
   * becomes a toast instead of a silent broken icon.
   */
  const on_error = (event: Event) => {
    const target = event.target as HTMLElement | null
    if (!target || !host.contains(target)) return
    if (target.tagName === 'IMG') {
      target.classList.add('img-broken')
      const alt = target.getAttribute('alt')
      toast.show(
        alt ? `Image not stored in this dictionary's resources: ${alt}` : 'Image not found',
        { priority: 'warning', dedupe_key: `img:${target.getAttribute('src') ?? ''}` },
      )
    }
  }

  const scroll_to_anchor = (id: string) => {
    if (!id) return
    const esc = css_escape(id)
    // MDict spells in-entry jump targets `<a name="HASH_word">`, not `id=`, so
    // `getElementById` alone misses them.
    const node =
      host.querySelector(`[id="${esc}"],[name="${esc}"]`) ?? document.getElementById(id)
    if (!node) {
      toast.show('That in-entry link points at a section this record does not contain', {
        priority: 'info',
        dedupe_key: 'anchor-missing',
      })
      return
    }
    node.scrollIntoView({ behavior: 'smooth', block: 'start' })
    node.classList.add('anchor-flash')
    window.setTimeout(() => node.classList.remove('anchor-flash'), 1200)
  }

  /** Returns true when the click was handled and must not reach the browser. */
  const handle_anchor = (a: HTMLAnchorElement, event: MouseEvent): boolean => {
    if (a.dataset.audio === '1') {
      event.preventDefault()
      const url = a.dataset.src ?? a.getAttribute('href') ?? ''
      if (a.dataset.playable === '1') {
        clear_marked()
        marked = a
        a.classList.add('is-playing')
        toggle(url, { dict: dict(), label: word(), variant: detect_variant(url, a) })
      } else if (a.dataset.playable === '2') {
        // The browser cannot decode this codec, but the server converts it at
        // the resource URL, so the original href is the right src.
        clear_marked()
        marked = a
        a.classList.add('is-playing')
        play(url, { dict: dict(), label: word(), variant: detect_variant(url, a) }, { converted: true })
      } else {
        reject(url, dict())
      }
      return true
    }
    if (a.dataset.dead === '1') {
      event.preventDefault()
      toast.show(
        "That link points at the publisher's own website, which this server does not host",
        { priority: 'info', dedupe_key: 'dead-link' },
      )
      return true
    }
    const target_word = a.dataset.word
    if (target_word) {
      event.preventDefault()
      clear_marked()
      cb.on_word(target_word)
      return true
    }
    if (a.dataset.anchor !== undefined) {
      event.preventDefault()
      scroll_to_anchor(a.dataset.anchor)
      return true
    }
    const href = a.getAttribute('href') ?? ''
    // External links keep their target=_blank and are left to the browser.
    if (/^https?:/i.test(href) || href.startsWith('mailto:')) return false
    if (href.startsWith('#')) {
      event.preventDefault()
      scroll_to_anchor(href.slice(1))
      return true
    }
    // An unclassified resource ref: let the browser fetch or download it.
    return false
  }

  const on_click = (event: MouseEvent) => {
    // Modifier- and middle-click must keep working as a real browser action.
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return
    }
    const target = event.target as Element | null
    if (!target) return
    const anchor = target.closest('a')
    if (anchor && host.contains(anchor)) {
      if (handle_anchor(anchor as HTMLAnchorElement, event)) return
    }
    const img = target.closest('img')
    if (img && host.contains(img)) {
      const src = img.getAttribute('src')
      if (src) {
        event.preventDefault()
        cb.on_image(src, img.getAttribute('alt') ?? '')
      }
    }
  }

  /**
   * `TooltipOptions.content` is synchronous, so it cannot await a fetch. The
   * preview is therefore warmed on `pointerover` into `data-preview`, and the
   * tooltip reads whatever is there when it opens. `open_delay_ms` is longer than
   * a local lookup takes, so in practice the text is ready.
   */
  const warm_preview = (anchor: HTMLElement) => {
    const target = anchor.dataset.word
    if (!target || anchor.dataset.preview) return
    const key = `${target}\u0000${dict()}`
    const cached = preview_cache.get(key)
    if (cached) {
      anchor.dataset.preview = cached
      return
    }
    preview_controller?.abort()
    preview_controller = new AbortController()
    void fetch_markdown(target, dict(), preview_controller.signal)
      .then((md) => {
        const text = md ? excerpt(md) : ''
        if (text) {
          preview_cache.set(key, text)
          if (anchor.isConnected) anchor.dataset.preview = text
        }
      })
      .catch(() => {
        /* a failed preview is not worth interrupting the reader for */
      })
  }

  const on_pointer_over = (event: PointerEvent) => {
    if (!cb.previews_enabled()) return
    const target = event.target as Element | null
    const anchor = target?.closest('a[data-word]') as HTMLElement | null
    if (!anchor || !host.contains(anchor)) return
    if (preview_timer) clearTimeout(preview_timer)
    preview_timer = setTimeout(() => warm_preview(anchor), 120)
  }

  host.addEventListener('click', on_click)
  host.addEventListener('error', on_error, true)
  host.addEventListener('pointerover', on_pointer_over)

  // `delegate` is what makes the tooltip survive `{@html}` replacing every node
  // underneath: it is bound to the host, not to the anchors.
  /**
   * Double-click selects a word in every browser, so the selection *is* the
   * word under the cursor — no caret arithmetic needed. The popover offers the
   * lookup rather than performing it, because navigating away would cost the
   * reader their place in a 60 KB entry.
   */
  const on_double_click = (event: MouseEvent) => {
    if (event.button !== 0) return
    const selected = window.getSelection()?.toString().trim() ?? ''
    const candidate = /^[\p{L}\p{N}'’-]{1,40}$/u.test(selected) ? selected : ''
    if (!candidate || candidate === word()) return
    event.preventDefault()
    open_wordpop(candidate, event.clientX, event.clientY, dict() || null)
  }
  host.addEventListener('dblclick', on_double_click)

  const detached = tooltip({
    delegate: 'a[data-word]',
    trigger: 'hover',
    open_delay_ms: 450,
    close_delay_ms: 120,
    placement: 'top',
    strategy: 'top-layer',
    show_arrow: true,
    content: (trigger: HTMLElement) => trigger.dataset.preview ?? 'Loading preview…',
  })(host)

  return () => {
    if (preview_timer) clearTimeout(preview_timer)
    preview_controller?.abort()
    host.removeEventListener('click', on_click)
    host.removeEventListener('dblclick', on_double_click)
    host.removeEventListener('error', on_error, true)
    host.removeEventListener('pointerover', on_pointer_over)
    clear_marked()
    // An Attachment may return nothing, or an object with an optional destroy.
    const cleanup = detached as { destroy?: () => void } | void
    if (cleanup && typeof cleanup.destroy === 'function') cleanup.destroy()
  }
}

/** Drop every "now playing" marker, e.g. when the entry changes. */
export function clear_preview_cache(): void {
  preview_cache.clear()
}
