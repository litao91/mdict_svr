import { describe, expect, it } from 'vitest'
import {
  audio_is_playable,
  encode_path,
  extension_of_url,
  is_audio_ref,
  rewrite_ref,
  sanitize_entry,
  url_is_allowed,
} from '../sanitize.js'
import { CALD4_RECORD, COBUILD8_RECORD, COL_RECORD, OALD9_RECORD, OXFORDPRIMARY_RECORD, OXENCH8_RECORD } from './fixtures.js'

const DICT = 'OALD9'

describe('url_is_allowed', () => {
  it('accepts the shapes dictionary markup actually uses', () => {
    for (const ref of [
      'txbapple.jpg',
      '/media/english/thumb/f/fru/fruit/fruit.jpg',
      '/lookup?word=apple',
      '#49f1b05b_apple',
      'https://example.com/a.png',
      'http://example.com/a.png',
      'mailto:nobody@example.com',
      'data:image/png;base64,AAAA',
    ]) {
      expect(url_is_allowed(ref), ref).toBe(true)
    }
  })

  it('rejects executable and browser-internal schemes', () => {
    for (const ref of [
      'javascript:alert(1)',
      'vbscript:msgbox(1)',
      'chrome://settings',
      'file:///etc/passwd',
      'data:text/html,<script>alert(1)</script>',
      '//evil.example.com/x.js',
    ]) {
      expect(url_is_allowed(ref), ref).toBe(false)
    }
  })

  it('collapses the whitespace browsers ignore, but only that whitespace', () => {
    // WHATWG removes every tab/newline anywhere and trims C0-or-space at the ends.
    expect(url_is_allowed('jav&#9;ascript:alert(1)'.replace('&#9;', '\t'))).toBe(false)
    expect(url_is_allowed('java\nscript:alert(1)')).toBe(false)
    expect(url_is_allowed('  javascript:alert(1)')).toBe(false)
    // A real space inside a query is part of the headword, not obfuscation.
    expect(url_is_allowed("/lookup?word=Adam's apple")).toBe(true)
  })
})

describe('rewrite_ref', () => {
  it('scopes bare relative refs to the dictionary store', () => {
    expect(rewrite_ref('txbapple.jpg', DICT, true)).toBe(`/res/${DICT}/txbapple.jpg`)
    expect(rewrite_ref('audio/三明治.mp3', 'Sound', true)).toBe(
      '/res/Sound/audio/%E4%B8%89%E6%98%8E%E6%B2%BB.mp3',
    )
  })

  it('scopes root-relative asset paths too', () => {
    expect(rewrite_ref('/media/english/thumb/f/fru/fruit/fruit.jpg', DICT, true)).toBe(
      `/res/${DICT}/media/english/thumb/f/fru/fruit/fruit.jpg`,
    )
  })

  it('percent-encodes path segments but never the separators or the query', () => {
    expect(rewrite_ref('flowers 1/2.jpg', DICT, true)).toBe(`/res/${DICT}/flowers%201/2.jpg`)
    expect(rewrite_ref('/lookup?word=Adam’s apple', DICT, true)).toBe(
      `/lookup?word=Adam’s%20apple`,
    )
    expect(encode_path("fruit 1.jpg")).toBe('fruit%201.jpg')
  })

  it('encodes dictionary names, which contain spaces, apostrophes and CJK', () => {
    const dict = "Oxford Advanced Learner's En-Ch Dictionary"
    expect(rewrite_ref('a.png', dict, true)).toBe(
      `/res/${encodeURIComponent(dict)}/a.png`,
    )
    expect(rewrite_ref('a.png', '[英-汉] ◆【新牛津】', true)).toContain(
      `/res/${encodeURIComponent('[英-汉] ◆【新牛津】')}/a.png`,
    )
  })

  it('leaves server routes and dead publisher paths unscoped', () => {
    for (const ref of ['/lookup?word=apple', '/word/apple', '/topic/fruit', '/mywordlist/']) {
      expect(rewrite_ref(ref, DICT, true), ref).toBe(ref)
    }
  })

  it('leaves absolute URLs and fragments alone', () => {
    expect(rewrite_ref('https://example.com/a.png', DICT, true)).toBe('https://example.com/a.png')
    expect(rewrite_ref('#relatedentries', DICT, true)).toBe('#relatedentries')
  })

  it('falls back to the cross-store route when the dictionary has no store', () => {
    // `/res/{dict}/X` on a store-less dictionary answers "dictionary has no
    // resources", whereas an unscoped path gets the server's every-store fallback.
    expect(rewrite_ref('txbapple.jpg', 'oxfordPrimary', false)).toBe('/txbapple.jpg')
    expect(rewrite_ref('/pic/Fruit.bmp', 'oxfordPrimary', false)).toBe('/pic/Fruit.bmp')
  })
})

describe('audio classification', () => {
  it('treats browser-decodable extensions as playable', () => {
    for (const url of ['/a.mp3', '/a.m4a', '/a.wav', '/a.ogg', '/a.flac', '/res/Sound/x.mp3?x=1']) {
      expect(audio_is_playable(url), url).toBe(true)
    }
  })

  it('rejects the codecs no browser ships, above all Ogg-Speex', () => {
    for (const url of ['/ukappen014.spx', '/a.wma', '/a.amr', '/a.mid', '/SOUND/EN_GB/X.SPX']) {
      expect(audio_is_playable(url), url).toBe(false)
    }
  })

  it('reads the extension past query strings and upper case', () => {
    expect(extension_of_url('/res/CALD4/ukappen014.spx')).toBe('spx')
    expect(extension_of_url('/a.mp3?token=1')).toBe('mp3')
    expect(extension_of_url('/no/extension')).toBe('')
  })

  it('recognises the three ways the corpus marks a sound link', () => {
    const make = (html: string) => {
      const doc = new DOMParser().parseFromString(html, 'text/html')
      return doc.querySelector('a')!
    }
    expect(is_audio_ref(make('<a type="sound" href="/x"></a>'), '/x')).toBe(true)
    expect(is_audio_ref(make('<a class="audio_play_button" href="/x"></a>'), '/x')).toBe(true)
    expect(is_audio_ref(make('<a class="fayin" href="/x"></a>'), '/x')).toBe(true)
    expect(is_audio_ref(make('<a href="/x.mp3"></a>'), '/x.mp3')).toBe(true)
    expect(is_audio_ref(make('<a href="/definition/english/apple"></a>'), '/definition/english/apple')).toBe(false)
  })
})

describe('sanitize_entry on real records', () => {
  const parse = (html: string) => new DOMParser().parseFromString(html, 'text/html')

  it('detects each vocabulary', () => {
    expect(sanitize_entry(OALD9_RECORD, 'OALD9', true).vocabulary).toBe('oald9')
    expect(sanitize_entry(OXENCH8_RECORD, 'OxEnCh8', true).vocabulary).toBe('oxench8')
    expect(sanitize_entry(COL_RECORD, 'col', true).vocabulary).toBe('col')
    expect(sanitize_entry(CALD4_RECORD, 'CALD4', true).vocabulary).toBe('plain')
    expect(sanitize_entry(COBUILD8_RECORD, 'collins', false).vocabulary).toBe('plain')
    expect(sanitize_entry(OXFORDPRIMARY_RECORD, 'oxfordPrimary', false).vocabulary).toBe('plain')
  })

  it('drops the publisher stylesheet that would otherwise go global', () => {
    // col.res.qmd really stores result-min.css and every col record links it.
    const out = sanitize_entry(COL_RECORD, 'col', true)
    expect(out.html).not.toContain('result-min.css')
    expect(out.html).not.toContain('<link')
    expect(out.html).not.toContain('yd_col.js')
    expect(out.html).not.toContain('<script')
  })

  it('drops <base>, which would hijack document.baseURI for the whole SPA', () => {
    for (const [name, record] of Object.entries({
      OALD9: OALD9_RECORD,
      col: COL_RECORD,
      CALD4: CALD4_RECORD,
    })) {
      expect(sanitize_entry(record, name, true).html, name).not.toContain('<base')
    }
  })

  it('strips every inline event handler', () => {
    // OALD9 alone carries 55,294 live onclick attributes; {@html} compiles them.
    for (const [name, record] of Object.entries({ OALD9: OALD9_RECORD, OXENCH8: OXENCH8_RECORD, CALD4: CALD4_RECORD })) {
      const doc = parse(sanitize_entry(record, name, true).html)
      const offenders = [...doc.querySelectorAll('*')].filter((el) =>
        [...el.attributes].some((a) => a.name.startsWith('on')),
      )
      expect(offenders, name).toHaveLength(0)
    }
  })

  it('keeps the inline layout styles CALD4 depends on', () => {
    const out = sanitize_entry(CALD4_RECORD, 'CALD4', true)
    expect(out.html).toContain('display:block')
    expect(out.html).toContain('margin-left')
  })

  it('preserves the readable text of every vocabulary', () => {
    const visible_text = (html: string) => {
      const doc = parse(html)
      for (const tag of ['script', 'style']) {
        for (const el of doc.querySelectorAll(tag)) el.remove()
      }
      return (doc.body.textContent ?? '').replace(/\s+/g, ' ').trim()
    }
    for (const [name, record] of Object.entries({
      OALD9: OALD9_RECORD,
      OXENCH8: OXENCH8_RECORD,
      COL: COL_RECORD,
      CALD4: CALD4_RECORD,
      COBUILD8: COBUILD8_RECORD,
      OXFORDPRIMARY: OXFORDPRIMARY_RECORD,
    })) {
      const before = visible_text(record)
      const after = visible_text(sanitize_entry(record, name, true).html)
      expect(after, name).toBe(before)
      expect(after.length, name).toBeGreaterThan(20)
    }
  })

  it('tags playable and undecodable audio separately', () => {
    const oald9 = sanitize_entry(OALD9_RECORD, 'OALD9', true)
    expect(oald9.playable_audio).toBeGreaterThan(0)
    expect(oald9.unsupported_audio).toBe(0)

    const cald4 = sanitize_entry(CALD4_RECORD, 'CALD4', true)
    expect(cald4.unsupported_audio).toBeGreaterThan(0)
    const doc = parse(cald4.html)
    const spx = doc.querySelector('a[data-audio][data-playable="0"]')
    expect(spx?.getAttribute('href')).toMatch(/\.spx$/)
  })

  it('tags cross-references, in-page anchors and dead publisher links', () => {
    const out = sanitize_entry(OALD9_RECORD, 'OALD9', true)
    const doc = parse(out.html)
    expect(doc.querySelectorAll('a[data-word]').length).toBeGreaterThan(0)
    expect(doc.querySelectorAll('a[data-anchor]').length).toBeGreaterThan(0)
    expect(out.dead_links).toBeGreaterThan(0)
    // The dead ones must not look clickable.
    expect(doc.querySelector('a[data-dead]')).not.toBeNull()
  })

  it('lifts publisher colors into data-c and removes them from the element', () => {
    const out = sanitize_entry(COBUILD8_RECORD, 'collins', false)
    const doc = parse(out.html)
    const colored = doc.querySelector('[data-c]')
    expect(colored).not.toBeNull()
    expect(colored?.getAttribute('color')).toBeNull()
    expect(colored?.getAttribute('style') ?? '').not.toContain('color')
  })

  it('numbers headings for the table of contents without trusting publisher ids', () => {
    const out = sanitize_entry(OALD9_RECORD, 'OALD9', true)
    expect(out.headings.length).toBeGreaterThan(0)
    const doc = parse(out.html)
    for (const heading of out.headings) {
      expect(doc.querySelector(`[data-toc-id="${heading.id}"]`), heading.id).not.toBeNull()
    }
  })
})
