/**
 * Turning a stored dictionary record into something safe to inject into the SPA.
 *
 * The server hands back the publisher's markup almost verbatim, and injecting it
 * unprocessed breaks the app in three concrete ways:
 *
 * 1. It opens with a server-injected `<base href="/">`. A `<base>` anywhere in
 *    the document sets `document.baseURI` for the *whole* SPA, not just the entry.
 * 2. `col.res.qmd` genuinely contains `result-min.css` (3,376 bytes) and col's
 *    records carry `<link rel="stylesheet" href="result-min.css"/>`, so that
 *    stylesheet really would load and restyle the app globally.
 * 3. OALD9 alone has 55,294 live `onclick` attributes, and inline handler
 *    attributes *are* compiled when inserted via `{@html}`.
 *
 * Parsing with `DOMParser` rather than assigning `innerHTML` matters: a parsed
 * document has no browsing context, so nothing executes and nothing loads while
 * we walk it.
 */

import { color_pair, split_style_color } from './colors.js'
import { dicts } from './dicts.svelte.js'

/** Dropped along with their entire subtree. */
const DROP_TAGS = new Set([
  'script',
  'style',
  'link',
  'meta',
  'base',
  'head',
  'title',
  'iframe',
  'object',
  'embed',
  'noscript',
  'template',
])

/** Dropped wherever they appear, regardless of element. */
const DROP_ATTRS = new Set(['srcdoc', 'formaction', 'xlink:href'])

/** Attributes whose value is a URL and so gets scheme-checked and rewritten. */
const URL_ATTRS = ['href', 'src', 'poster'] as const

/**
 * Server routes. The definition HTML contains `/lookup?word=…` links produced by
 * `rewrite_urls`, and rewriting those into `/res/{dict}/lookup?word=…` would turn
 * every cross-reference into a dead asset request.
 */
const API_PREFIXES = ['/lookup', '/word/', '/suggest', '/dicts', '/info', '/health', '/res/']

/**
 * Root-relative paths that appear in definitions but route nowhere on this
 * server — they are oxford.com destinations. `src/md/filter.rs` unwraps the same
 * set when producing Markdown. Clicking one would render a 404 JSON body.
 */
const DEAD_PATH_PREFIXES = ['/topic/', '/mywordlist/', '/wordlist/', '/definition/', '/pronunciation/']

const ALLOWED_SCHEMES = new Set(['http:', 'https:', 'mailto:'])

/**
 * WHATWG's URL parser removes every ASCII tab and newline anywhere in a URL and
 * trims C0-control-or-space from both ends. Doing exactly that — and no more — is
 * what makes `jav&#9;ascript:` collapse to `javascript:` and get rejected, while a
 * real space inside a query survives. Stripping all whitespace instead would
 * silently rewrite `entry://Adam's apple` to a headword that does not exist.
 */
function strip_url_junk(raw: string): string {
  return raw
    .replace(/[\t\n\r]/g, '')
    .replace(/^[\u0000-\u0020\u007f]+|[\u0000-\u0020\u007f]+$/g, '')
}

function has_scheme(ref: string): boolean {
  return /^[a-z][a-z0-9+.-]*:/i.test(ref)
}

/**
 * Is this URL allowed to appear in the output at all? Relative and root-relative
 * refs are the normal case; `data:` is allowed only for images.
 */
export function url_is_allowed(raw: string): boolean {
  const value = strip_url_junk(raw)
  if (!value) return false
  if (value.startsWith('#')) return true
  if (value.startsWith('/')) return !value.startsWith('//')
  if (!has_scheme(value)) return true
  const scheme = value.slice(0, value.indexOf(':') + 1).toLowerCase()
  if (scheme === 'data:') return /^data:image\//i.test(value)
  return ALLOWED_SCHEMES.has(scheme)
}

function encode_segment(segment: string): string {
  return encodeURIComponent(segment)
}

/** Percent-encode each path segment, leaving `/` as a separator. */
export function encode_path(path: string): string {
  return path.split('/').map(encode_segment).join('/')
}

/**
 * Resolve one asset reference against a dictionary.
 *
 * `has_resources` decides the shape. `/res/{dict}/X` on a dictionary with no
 * attached store returns `{"error":"dictionary has no resources"}`, whereas an
 * unscoped `/X` hits the server's fallback route, which searches *every* loaded
 * store. So dictionaries without a store keep root-relative refs and get a chance
 * at a sibling's copy — which is not hypothetical: under `serve --db ./data`,
 * `Sound_us_uk.res.qmd` is skipped by the case-sensitive stem pairing in
 * `src/main.rs`, so `Sound` reports `has_resources: false` despite holding
 * 248,363 audio files.
 */
export function rewrite_ref(ref: string, dict: string, has_resources: boolean): string {
  // Applied on the way out, not the way in: `encode_path` already turns a space
  // into `%20`, and encoding first would double it to `%2520`.
  return resolve_ref(strip_url_junk(ref), dict, has_resources).replaceAll(' ', '%20')
}

function resolve_ref(trimmed: string, dict: string, has_resources: boolean): string {
  if (!trimmed) return trimmed
  if (trimmed.startsWith('#')) return trimmed
  // Anything with a scheme, or protocol-relative, is passed through untouched;
  // `url_is_allowed` has already decided whether it may exist at all.
  if (has_scheme(trimmed) || trimmed.startsWith('//')) return trimmed

  // Split off query/fragment so they are not percent-encoded as path segments.
  const cut = trimmed.search(/[?#]/)
  const raw_path = cut < 0 ? trimmed : trimmed.slice(0, cut)
  const tail = cut < 0 ? '' : trimmed.slice(cut)
  const rooted = raw_path.startsWith('/') ? raw_path : `/${raw_path}`
  // Server routes, and the oxford.com paths that route nowhere here. Neither is
  // an asset, so scoping them under `/res/{dict}/` would turn a cross-reference or
  // a detectable dead link into a bogus resource request.
  if (API_PREFIXES.some((prefix) => rooted.startsWith(prefix))) return rooted + tail
  if (DEAD_PATH_PREFIXES.some((prefix) => rooted.startsWith(prefix))) return rooted + tail
  if (!has_resources) return rooted + tail
  return `/res/${encode_segment(dict)}${encode_path(rooted)}${tail}`
}

/**
 * Codecs the server re-encodes to WAV when it has a decoder configured — the
 * mirror of `undecodable_audio` in src/server.rs. With one, these anchors play
 * from their original URL; without, they are the download-only case.
 */
const SERVER_CONVERTIBLE = new Set(['spx', 'wma', 'amr', 'mid'])

/** Browser-decodable audio extensions. */
const PLAYABLE_AUDIO = new Set(['mp3', 'm4a', 'mp4', 'wav', 'webm', 'flac', 'ogg', 'oga', 'opus'])

/**
 * Audio no browser can decode. `.spx` is Ogg-Speex: CALD4, Sound and Sound-en_GB
 * store all 281 of their sampled pronunciation files in it. `mime_guess` maps
 * `.spx` to `audio/ogg`, so the server answers `200 OK, Content-Type: audio/ogg`
 * and an `<audio>` element fails at *decode* time — the HTTP status gives no
 * warning, which is why this is decided by extension up front.
 */
const UNPLAYABLE_AUDIO = new Set(['spx', 'wma', 'amr', 'aac', 'mid', 'midi'])

/** Lowercase file extension of a URL, ignoring any query or fragment. */
export function extension_of_url(url: string): string {
  const path = url.split(/[?#]/)[0] ?? ''
  const dot = path.lastIndexOf('.')
  return dot < 0 ? '' : path.slice(dot + 1).toLowerCase()
}

/** Mirrors `is_audio` in `src/md/filter.rs`. */
export function is_audio_ref(el: Element, url: string): boolean {
  if (el.getAttribute('type')?.toLowerCase() === 'sound') return true
  const classes = (el.getAttribute('class') ?? '').split(/\s+/)
  if (classes.some((c) => c === 'sound' || c === 'fayin' || c === 'audio_play_button')) return true
  const ext = extension_of_url(url)
  return PLAYABLE_AUDIO.has(ext) || UNPLAYABLE_AUDIO.has(ext)
}

/**
 * Can a browser actually decode this? Unknown extensions are treated as playable
 * and left to the `<audio>` error event, which reports
 * `MEDIA_ERR_SRC_NOT_SUPPORTED`.
 */
export function audio_is_playable(url: string): boolean {
  return !UNPLAYABLE_AUDIO.has(extension_of_url(url))
}

export interface TocHeading {
  id: string
  text: string
  level: number
}

/**
 * Which markup vocabulary a record uses. Detected from the content rather than
 * the dictionary name, because the name is user-chosen at launch (`--db NAME=…`)
 * and two of the dictionaries in `./data` are byte-identical duplicates stored
 * under different names.
 */
export type Vocabulary = 'oald9' | 'oxench8' | 'col' | 'plain'

export function detect_vocabulary(doc: Document): Vocabulary {
  if (doc.querySelector('.oald8c, .no-dot-h, .phon-gb, .z_ei-g')) return 'oxench8'
  if (doc.querySelector('#collinsResult, .collinsMajorTrans, .collinsOrder')) return 'col'
  if (doc.querySelector('.h-g, .pron-gs, .sn-gs, .webtop-g')) return 'oald9'
  return 'plain'
}

export interface SanitizedEntry {
  /** Safe to pass to `{@html}`. */
  html: string
  vocabulary: Vocabulary
  /** Headings found, in document order, for the table of contents. */
  headings: TocHeading[]
  /** Anchors turned into playable audio controls. */
  playable_audio: number
  /** Anchors whose audio is a codec no browser can decode. */
  unsupported_audio: number
  /** Anchors pointing at a path this server does not route. */
  dead_links: number
  /** Cross-reference anchors that navigate to another headword. */
  word_links: number
}

/** Class names that act as section headings in the class-based vocabularies. */
const HEADING_CLASSES = new Set(['heading', 'revout'])
const HEADING_TAGS = new Set(['h1', 'h2', 'h3', 'h4', 'h5', 'h6'])

function heading_level(el: Element): number | null {
  const tag = el.tagName.toLowerCase()
  if (HEADING_TAGS.has(tag)) return Number(tag.slice(1))
  const classes = (el.getAttribute('class') ?? '').split(/\s+/)
  if (classes.some((c) => HEADING_CLASSES.has(c))) return 3
  return null
}

/** Set a custom property on any element, HTML or SVG. */
function set_property(el: Element, name: string, value: string): void {
  const style = (el as Partial<HTMLElement | SVGElement>).style
  style?.setProperty(name, value)
}

/**
 * Lift a color out of `font[color]` or an inline `style`, into a `data-c` pair.
 *
 * `<font color>` is only a presentational hint, so author CSS beats it — but an
 * inline `style="color:…"` beats author CSS. That is why the color has to be
 * *removed* from the element rather than merely overridden, and why the rest of
 * the inline style must survive: CALD4's entire layout is
 * `<span style="margin-left:20px;display:block;">`.
 */
function lift_color(el: Element): void {
  const tag = el.tagName.toLowerCase()
  let raw: string | null = null
  if (tag === 'font') {
    raw = el.getAttribute('color')
    if (raw !== null) el.removeAttribute('color')
  }
  const style = el.getAttribute('style')
  if (style) {
    const { rest, color } = split_style_color(style)
    if (color) {
      raw ??= color
      if (rest) el.setAttribute('style', rest)
      else el.removeAttribute('style')
    }
  }
  if (raw === null) return
  const pair = color_pair(raw)
  if (!pair) return // unparseable: leave whatever the element already had
  // Both values go into custom properties rather than one into `data-c`, because
  // CSS `attr()` only works in `content` — it cannot supply a `color`.
  el.setAttribute('data-c', pair.light)
  set_property(el, '--c-light', pair.light)
  set_property(el, '--c-dark', pair.dark)
}

/**
 * Speaker glyphs for audio links.
 *
 * The publishers' own icons (`snd_uk.png`, `uk_pron.png`, `snd_us.png`) differ
 * per dictionary and are not always present in the resource store, so audio
 * anchors get one uniform icon instead. An unsupported codec gets a crossed-out
 * speaker, so the difference is visible before you click rather than discovered
 * as silence.
 *
 * Built with DOM APIs rather than an HTML string: no markup parsing, so nothing
 * here can be a sink no matter what the record contained.
 */
const SVG_NS = 'http://www.w3.org/2000/svg'
const PATH_PLAYABLE =
  'M3 9.5v5h3.6L11 18.6V5.4L6.6 9.5H3Zm12.4 2.5a4 4 0 0 0-2-3.46v6.92a4 4 0 0 0 2-3.46Zm-2-7.36v1.62a5.75 5.75 0 0 1 0 11.48v1.62a7.37 7.37 0 0 0 0-14.72Z'
const PATH_UNSUPPORTED =
  'M3 9.5v5h3.6L11 18.6V5.4L6.6 9.5H3Zm17.7-.9-1.2-1.2-3 3-3-3-1.2 1.2 3 3-3 3 1.2 1.2 3-3 3 3 1.2-1.2-3-3Z'

function audio_icon(doc: Document, playable: boolean): SVGElement {
  const svg = doc.createElementNS(SVG_NS, 'svg')
  svg.setAttribute('class', playable ? 'audio-icon' : 'audio-icon audio-icon--off')
  svg.setAttribute('viewBox', '0 0 24 24')
  svg.setAttribute('width', '13')
  svg.setAttribute('height', '13')
  svg.setAttribute('aria-hidden', 'true')
  svg.setAttribute('focusable', 'false')
  const path = doc.createElementNS(SVG_NS, 'path')
  path.setAttribute('d', playable ? PATH_PLAYABLE : PATH_UNSUPPORTED)
  svg.append(path)
  return svg
}

function give_audio_icon(el: Element, playable: boolean): void {
  const icon = () => audio_icon(el.ownerDocument, playable)
  // Anchors that wrap real text keep it: oxfordPrimary links the headword itself
  // to its recording, and the whole Sound dictionary is
  // `<a href="sound://…">英式发音apple</a>`.
  if ((el.textContent ?? '').trim().length > 0) {
    el.prepend(icon())
    return
  }
  el.replaceChildren(icon())
}

/**
 * Tag an anchor so click delegation is a lookup rather than a re-parse.
 *
 * MDict's in-entry jump targets are spelled `<a name="HASH_word">`, and
 * `rewrite_urls` turns `entry://#HASH_word` into `/lookup?word=#HASH_word` — so a
 * `word` starting with `#` is a same-document anchor, not a headword, and looking
 * it up would 404.
 */
function tag_anchor(el: Element, url: string, stats: SanitizedEntry): void {
  if (is_audio_ref(el, url)) {
    const native = audio_is_playable(url)
    // '2' = the browser cannot decode it, but the server converts it on the
    // fly, so clicking plays the original URL. The marker describes what
    // happens on click, not what the browser supports.
    const converted =
      !native && dicts.transcode && SERVER_CONVERTIBLE.has(extension_of_url(url))
    const state = native ? '1' : converted ? '2' : '0'
    el.setAttribute('data-audio', '1')
    el.setAttribute('data-playable', state)
    el.setAttribute('data-src', url)
    el.setAttribute('role', 'button')
    el.setAttribute(
      'title',
      native ? 'Play pronunciation' : converted ? 'Play pronunciation (converted by the server)' : 'Audio this browser cannot decode',
    )
    if (state === '0') stats.unsupported_audio += 1
    else stats.playable_audio += 1
    give_audio_icon(el, state !== '0')
    return
  }

  const path = url.split('?')[0] ?? ''
  if (path === '/lookup' || path === '/lookup/') {
    const word = new URLSearchParams(url.split('?')[1] ?? '').get('word') ?? ''
    if (word.startsWith('#')) {
      el.setAttribute('data-anchor', word.slice(1))
      el.setAttribute('href', `#${word.slice(1)}`)
    } else if (word) {
      el.setAttribute('data-word', word)
      stats.word_links += 1
    }
    return
  }
  if (url.startsWith('/') && DEAD_PATH_PREFIXES.some((p) => url.startsWith(p))) {
    el.setAttribute('data-dead', '1')
    stats.dead_links += 1
    return
  }
  if (/^https?:/i.test(url)) {
    el.setAttribute('target', '_blank')
    el.setAttribute('rel', 'noopener noreferrer')
  }
}

/**
 * Sanitize one dictionary's record for injection.
 *
 * `dict` and `has_resources` come from `/dicts`; `has_resources` changes how
 * relative refs are resolved (see `rewrite_ref`).
 */
export function sanitize_entry(
  raw_html: string,
  dict: string,
  has_resources: boolean,
): SanitizedEntry {
  const doc = new DOMParser().parseFromString(raw_html, 'text/html')
  const stats: SanitizedEntry = {
    html: '',
    vocabulary: detect_vocabulary(doc),
    headings: [],
    playable_audio: 0,
    unsupported_audio: 0,
    dead_links: 0,
    word_links: 0,
  }
  let heading_seq = 0

  const walk = (parent: Element): void => {
    // Snapshot first: we remove nodes while iterating.
    for (const child of Array.from(parent.children)) {
      const tag = child.tagName.toLowerCase()
      if (DROP_TAGS.has(tag)) {
        child.remove()
        continue
      }

      for (const attr of Array.from(child.attributes)) {
        const name = attr.name.toLowerCase()
        if (name.startsWith('on') || DROP_ATTRS.has(name)) {
          child.removeAttribute(attr.name)
        }
      }

      lift_color(child)

      // URL attributes: scheme-check, then rewrite.
      let dropped_src = false
      for (const name of URL_ATTRS) {
        const value = child.getAttribute(name)
        if (value === null) continue
        if (!url_is_allowed(value)) {
          child.removeAttribute(name)
          if (name === 'src') dropped_src = true
          continue
        }
        child.setAttribute(name, rewrite_ref(value, dict, has_resources))
      }
      const srcset = child.getAttribute('srcset')
      if (srcset) {
        if (!url_is_allowed(srcset.split(',')[0] ?? '')) {
          child.removeAttribute('srcset')
        } else {
          child.setAttribute(
            'srcset',
            srcset
              .split(',')
              .map((candidate) => {
                const [ref, ...rest] = candidate.trim().split(/\s+/)
                const kept = rest.length ? ` ${rest.join(' ')}` : ''
                return ref && url_is_allowed(ref)
                  ? rewrite_ref(ref, dict, has_resources) + kept
                  : candidate.trim()
              })
              .filter(Boolean)
              .join(', '),
          )
        }
      }

      if (tag === 'img') {
        // A 237 KB entry can carry hundreds of images; never load them eagerly.
        child.setAttribute('loading', 'lazy')
        child.setAttribute('decoding', 'async')
        if (dropped_src || !child.getAttribute('src')) {
          child.remove()
          continue
        }
      } else if (tag === 'a') {
        const href = child.getAttribute('href')
        if (href) tag_anchor(child, href, stats)
      }

      const level = heading_level(child)
      if (level !== null) {
        // Only assign an id when the publisher did not supply one, so an
        // `<a href="#…">` or `<a name="…">` target is never clobbered.
        const existing = child.getAttribute('id')
        const id = existing && existing.trim() ? existing.trim() : `toc-${heading_seq++}`
        if (!existing) child.setAttribute('id', id)
        child.setAttribute('data-toc-id', id)
        const text = (child.textContent ?? '').trim()
        if (text) stats.headings.push({ id, text, level })
      }

      walk(child)
    }
  }

  walk(doc.body)
  stats.html = doc.body.innerHTML
  return stats
}
