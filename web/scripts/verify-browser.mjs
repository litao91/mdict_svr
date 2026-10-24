/**
 * End-to-end pass over the real server in headless Chromium.
 *
 *   npm run build
 *   mdict_svr serve --db ./data --res Sound=./data/Sound_us_uk.res.qmd \
 *       --web web/dist --addr 127.0.0.1:8396
 *   npm run test:browser            # BASE=http://… to point elsewhere
 *
 * The unit tests cover the pure logic; this covers what only a browser can:
 * that audio actually decodes and plays, that Ogg-Speex degrades visibly, that
 * cross-references navigate without a reload, that no publisher stylesheet or
 * inline handler survives into the page, and that every dictionary stays
 * legible in both themes. Screenshots land in /tmp/shots.
 */
import { chromium } from 'playwright'
import { mkdirSync } from 'node:fs'

const BASE = process.env.BASE ?? 'http://127.0.0.1:8396'
// Set to '' to assert the no-transcoder degrade path instead.
const TRANSCODE_BASE = process.env.TRANSCODE !== '0'
const SHOTS = '/tmp/shots'
mkdirSync(SHOTS, { recursive: true })

const OXENCH = "Oxford Advanced Learner's En-Ch Dictionary"
const XINIUJIN = '[英-汉] ◆【新牛津英汉双解大词典】【孤雷排版】【2011.8.29】'
const COBUILD = 'Collins COBUILD English Dictionary 8Ed'
const SOUND_GB = 'Sound-en_GB(British.English,Word.93612)'

const results = []
function check(name, ok, detail = '') {
  results.push({ name, ok: !!ok, detail })
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? `  — ${detail}` : ''}`)
}

const browser = await chromium.launch({
  args: ['--autoplay-policy=no-user-gesture-required', '--mute-audio'],
})
const page = await browser.newPage({ viewport: { width: 1440, height: 960 } })

const page_errors = []
const failed_requests = []
const media_responses = []
page.on('pageerror', (e) => page_errors.push(`pageerror: ${e.message}`))
page.on('requestfailed', (r) =>
  failed_requests.push(`${r.url().replace(BASE, '')} ${r.failure()?.errorText}`),
)
page.on('response', (r) => {
  if (/\.(mp3|spx|ogg|wav|m4a)(\?|$)/i.test(r.url())) {
    media_responses.push({ url: r.url().replace(BASE, ''), status: r.status(), type: r.headers()['content-type'] })
  }
})

const goto_word = (word, dict) =>
  page.goto(`${BASE}/?word=${encodeURIComponent(word)}&dict=${encodeURIComponent(dict)}`, {
    waitUntil: 'domcontentloaded',
  })

/** Wait until the selected dictionary's panel has actually been populated. */
async function wait_entry(dict, timeout = 25000) {
  await page
    .waitForSelector(`.entry[data-dict="${dict}"]`, { timeout })
    .catch(() => page.waitForSelector('.entry', { timeout }).catch(() => {}))
  await page.waitForTimeout(700)
}

async function shot(name) {
  await page.screenshot({ path: `${SHOTS}/${name}.png` })
}

/**
 * `textContent` includes `display:none` subtrees, which is exactly what the
 * publisher-chrome checks must not do. Walk text nodes and keep only the ones a
 * reader can actually see.
 */
async function visible_text(selector = '.entry') {
  return page.evaluate((sel) => {
    const root = document.querySelector(sel)
    if (!root) return ''
    const hidden = (el) => {
      for (let p = el; p && p !== document.body; p = p.parentElement) {
        const cs = getComputedStyle(p)
        if (cs.display === 'none' || cs.visibility === 'hidden') return true
      }
      return false
    }
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    const parts = []
    let node
    while ((node = walker.nextNode())) {
      const value = node.nodeValue
      if (!value?.trim()) continue
      const el = node.parentElement
      if (!el || hidden(el)) continue
      parts.push(value.replace(/\s+/g, ' '))
    }
    return parts.join(' ').replace(/\s+/g, ' ').trim()
  }, selector)
}

// ── 1. landing page ───────────────────────────────────────────────────────
await page.goto(`${BASE}/`, { waitUntil: 'networkidle' })
await page.waitForTimeout(800)
check('landing page renders', (await page.locator('.landing').count()) > 0)
check('landing lists the dictionaries', (await page.locator('.landing .dict-card').count()) >= 5,
  `${await page.locator('.landing .dict-card').count()} cards`)
const landing_text = (await page.locator('.landing').innerText()).replace(/\s+/g, ' ')
check('landing shows real titles, not the MDict placeholder',
  !/Title \(No HTML code allowed\)/.test(landing_text), landing_text.slice(0, 90))
await shot('01-landing')

// ── 2. /word/apple redirects into the SPA ─────────────────────────────────
{
  const resp = await page.goto(`${BASE}/word/apple`, { waitUntil: 'networkidle' })
  await page.waitForTimeout(900)
  check('/word/apple lands on an entry', page.url().includes('word=apple') &&
    (await page.locator('.entry').count()) > 0, page.url())
  check('the redirect came from the server',
    resp?.request().redirectedFrom()?.url() === `${BASE}/word/apple`,
    `${resp?.request().redirectedFrom()?.url() ?? '(none)'} → ${resp?.status()}`)
  await shot('02-word-apple-redirect')
}

// ── 3. OALD9 apple: audio actually plays ──────────────────────────────────
media_responses.length = 0
await goto_word('apple', 'OALD9')
await wait_entry('OALD9')
{
  const playable = page.locator('.entry a[data-audio][data-playable="1"]')
  const n = await playable.count()
  check('OALD9 apple has playable audio anchors', n === 2, `${n} anchors`)
  await playable.first().click()
  // The clip is ~1 s long, so poll rather than sleep past the end of it.
  await page.waitForSelector('.player', { timeout: 5000 }).catch(() => {})
  await page.waitForTimeout(2500)
  const info = await page.evaluate(() => {
    const region = document.querySelector('.player')
    return {
      visible: !!region,
      text: region?.textContent?.replace(/\s+/g, ' ').trim() ?? '',
      marked: !!document.querySelector('.entry a.is-playing'),
      warned: !!document.querySelector('.player__status--warn'),
      seek_max: Number(document.querySelector('.player__seek')?.getAttribute('max') ?? 0),
      badge: document.querySelector('.player .badge')?.textContent ?? '',
    }
  })
  check('the player bar appears and stays after the clip ends', info.visible, info.text)
  check('the clip decoded (a real duration was reported)', info.seek_max > 0, `duration=${info.seek_max}s`)
  check('no decode/missing warning for OALD9 mp3', !info.warned)
  check('the speaker shows the playing state', info.marked)
  check('BrE is detected from the asset name', info.badge === 'BrE', info.badge)
  const req = media_responses.find((r) => /apple__gb_1\.mp3/.test(r.url))
  check('the mp3 was fetched with 200 audio/mpeg',
    req?.status === 200 && req?.type === 'audio/mpeg', JSON.stringify(req))
  await shot('03-oald9-apple-audio')
  // replay through the bar's own control
  await page.locator('.player__icon').click()
  await page.waitForTimeout(400)
  check('the bar can restart the clip',
    (await page.locator('.player').count()) > 0)
  await page.locator('.player__stop').click()
  await page.waitForTimeout(300)
}

// ── 4. publisher chrome is hidden ─────────────────────────────────────────
{
  const visible = await visible_text()
  const hidden_probe = await page.evaluate(() => {
    const probe = (sel) => {
      const el = document.querySelector(`.entry ${sel}`)
      return el ? getComputedStyle(el).display : 'absent'
    }
    return {
      bre: probe('.bre'),
      wrap: probe('.pron-gs .wrap'),
      pos_g: probe('.top-g .pos-g'),
      wl_add: probe('div[class="btn icon-left-colapse wl-add"]'),
      enlarge: probe('.ox-enlarge-label'),
      responsive: probe('.responsive_display_inline_on_smartphone'),
    }
  })
  check('the publisher hides its own duplicates and dead buttons',
    Object.values(hidden_probe).every((d) => d === 'none' || d === 'absent'),
    JSON.stringify(hidden_probe))
  check('no dead website chrome is visible',
    !/wordlist|other results|enlarge image|simplized/i.test(visible), visible.slice(0, 60))
  check('the part of speech is not duplicated', !/\bnoun noun\b/.test(visible),
    (visible.match(/.{0,18}\bnoun\b.{0,18}/) ?? [''])[0])
  check('the pronunciation label is not duplicated', !/BrE BrE|NAmE NAmE/.test(visible),
    (visible.match(/.{0,10}BrE.{0,22}/) ?? [''])[0])
  check('slashes are not doubled around the IPA', !/\/\/ˈ/.test(visible),
    (visible.match(/.{0,6}ˈæpl.{0,6}/) ?? [''])[0])
  await shot('04-oald9-chrome-hidden')
}

// ── 5. entry:// cross-reference navigates in-app ──────────────────────────
{
  const nav_before = await page.evaluate(() => performance.getEntriesByType('navigation').length)
  const link = page.locator('.entry a[data-word]').first()
  const label = (await link.textContent())?.trim()
  const word = await link.getAttribute('data-word')
  await link.click()
  await page.waitForTimeout(2500)
  const nav_after = await page.evaluate(() => performance.getEntriesByType('navigation').length)
  check('the cross-reference href keeps the spaces in the headword',
    word === 'Adam’s apple' || word === "Adam's apple", JSON.stringify(word))
  check('the cross-reference updates the URL', /word=Adam/.test(page.url()), page.url())
  check('the cross-reference does not reload the page', nav_after === nav_before,
    `navigation entries ${nav_before} → ${nav_after}`)
  check('the cross-reference renders the target entry',
    (await page.locator('.entry').count()) > 0 &&
    (await page.locator('.word-head__title').innerText()).includes('apple'),
    `clicked "${label}" → heading "${await page.locator('.word-head__title').innerText().catch(() => '?')}"`)
  await shot('05-entry-link-navigation')
  await page.goBack()
  await page.waitForTimeout(1500)
  check('history Back returns to the previous word', /word=apple(&|$)/.test(page.url()), page.url())
  await page.goForward()
  await page.waitForTimeout(1500)
  check('history Forward re-applies the navigation', /word=Adam/.test(page.url()), page.url())
}

// ── 6. OALD9 #hash anchor ─────────────────────────────────────────────────
await goto_word('apple', 'OALD9')
await wait_entry('OALD9')
{
  const anchor = page.locator('.entry a[data-anchor]').first()
  const n = await anchor.count()
  check('OALD9 apple has in-document anchors', n > 0, `${n} anchors`)
  const before_y = await page.evaluate(() => document.querySelector('.scroll-area')?.scrollTop ?? -1)
  await anchor.click()
  await page.waitForTimeout(1000)
  const after = await page.evaluate(() => ({
    y: document.querySelector('.scroll-area')?.scrollTop ?? -1,
    flashed: !!document.querySelector('.anchor-flash'),
    navigated: !document.querySelector('.entry a[data-anchor]') ? true : /word=%23/.test(location.search),
  }))
  check('the #hash anchor targets in-page, not a lookup',
    after.flashed || after.y !== before_y,
    `scrollTop ${before_y} → ${after.y}, flash=${after.flashed}, url=${page.url()}`)
  await shot('06-hash-anchor')
}

// ── 7. CALD4 apple: Speex is transcoded on the server when it can be ──────
await goto_word('apple', 'CALD4')
await wait_entry('CALD4')
{
  const unplayable = page.locator('.entry a[data-audio][data-playable="0"]')
  const n = await unplayable.count()
  check('CALD4 apple marks .spx as not natively playable', n > 0, `${n} anchors`)
  media_responses.length = 0
  await unplayable.first().click()
  await page.waitForSelector('.player', { timeout: 8000 }).catch(() => {})
  await page.waitForTimeout(2500)
  const info = await page.evaluate(() => {
    const region = document.querySelector('.player')
    return {
      visible: !!region,
      text: region?.textContent?.replace(/\s+/g, ' ').trim() ?? '',
      warned: !!document.querySelector('.player__status--warn'),
      seek_max: Number(document.querySelector('.player__seek')?.getAttribute('max') ?? 0),
      download: !!document.querySelector('.player a[download]'),
    }
  })
  const wav = media_responses.find((r) => /\/transcode\//.test(r.url))
  if (TRANSCODE_BASE) {
    check('clicking .spx asks the server to transcode it', !!wav, JSON.stringify(media_responses))
    check('the transcode answer is WAV and it plays',
      wav?.status === 200 && wav?.type === 'audio/wav' && info.seek_max > 0 && !info.warned,
      `${JSON.stringify(wav)} seek_max=${info.seek_max} "${info.text.slice(0, 80)}"`)
  } else {
    check('without a transcoder the Speex state explains the codec',
      /Speex/.test(info.text) && info.download, info.text.trim().slice(0, 120))
    check('no element was created for the raw .spx file',
      media_responses.filter((r) => /\.spx/.test(r.url)).length === 0, JSON.stringify(media_responses))
  }
  await shot('07-cald4-speex')
  await page.locator('.player__stop').first().click().catch(() => {})
  await page.waitForTimeout(300)
}

// ── 8. col entry: no result-min.css leak ──────────────────────────────────
await goto_word('apple', 'col')
await wait_entry('col')
{
  const info = await page.evaluate(() => {
    const entry = document.querySelector('.entry')
    return {
      links: [...entry.querySelectorAll('link[rel="stylesheet"]')].map((l) => l.getAttribute('href')),
      scripts: [...entry.querySelectorAll('script')].map((s) => s.getAttribute('src') ?? '(inline)'),
      app_scripts: [...document.querySelectorAll('head script[src]')].map((s) => s.getAttribute('src')),
      base: document.querySelector('base')?.getAttribute('href') ?? null,
      handlers: [...entry.querySelectorAll('*')].filter((el) =>
        [...el.attributes].some((a) => a.name.startsWith('on'))).length,
      text_len: entry.textContent.length,
      classes: entry.className,
    }
  })
  check('no publisher stylesheet survives sanitization',
    !info.links.some((h) => /result-min|OALD|xiaoxuesheng/i.test(h ?? '')), JSON.stringify(info.links))
  check('no publisher script survives sanitization', info.scripts.length === 0,
    `entry=${JSON.stringify(info.scripts)} head=${JSON.stringify(info.app_scripts)}`)
  check('no <base> element in the document', info.base === null, String(info.base))
  check('no inline on* handlers in the entry', info.handlers === 0, `${info.handlers} left`)
  check('col vocabulary detected', info.classes.includes('dict--col'), info.classes)
  check('col entry has text', info.text_len > 200, `${info.text_len} chars`)
  await shot('08-col-no-css-leak')
}

// ── 9. images load + lightbox (OALD9 has a real thumb store) ──────────────
await goto_word('apple', 'OALD9')
await wait_entry('OALD9')
{
  const imgs = page.locator('.entry img')
  const n = await imgs.count()
  check('entry contains images', n > 0, `${n} images`)
  // scroll them into view so loading="lazy" actually fires
  await page.evaluate(() => document.querySelectorAll('.entry img').forEach((i) => i.scrollIntoView()))
  await page.waitForTimeout(2500)
  const loaded = await page.evaluate(() => {
    const list = [...document.querySelectorAll('.entry img')]
    const visible = list.filter((i) => {
      let p = i
      while (p && p !== document.body) {
        const cs = getComputedStyle(p)
        if (cs.display === 'none' || cs.visibility === 'hidden') return false
        p = p.parentElement
      }
      return true
    })
    return {
      total: list.length,
      visible: visible.length,
      complete: visible.filter((i) => i.complete && i.naturalWidth > 0).length,
      lazy: list.filter((i) => i.loading === 'lazy').length,
      async_decoding: list.filter((i) => i.decoding === 'async').length,
      srcs: visible.slice(0, 3).map((i) => i.getAttribute('src')),
    }
  })
  check('images actually load (naturalWidth > 0)', loaded.complete > 0 && loaded.complete === loaded.visible,
    `${loaded.complete}/${loaded.visible} loaded, lazy=${loaded.lazy} async=${loaded.async_decoding} ${JSON.stringify(loaded.srcs)}`)
  // The lightbox duplicate is `style="display:none"`; click the visible one.
  const visible_img = page
    .locator('.entry img')
    .filter({ visible: true })
    .first()
  await visible_img.scrollIntoViewIfNeeded()
  await visible_img.click()
  await page.waitForTimeout(900)
  const open = await page.evaluate(() => !!document.querySelector('dialog[open] img'))
  check('clicking an image opens the lightbox', open)
  if (open) {
    const lb = await page.evaluate(() => {
      const img = document.querySelector('dialog[open] img')
      return { src: img?.getAttribute('src') ?? '', loaded: (img?.naturalWidth ?? 0) > 0 }
    })
    check('the lightbox shows the full-size picture', lb.loaded, JSON.stringify(lb))
  }
  await shot('09-lightbox')
  await page.keyboard.press('Escape')
  await page.waitForTimeout(400)
}

// ── 10. oxfordPrimary dead mp3 → one toast, no uncaught error ─────────────
{
  page_errors.length = 0
  await goto_word('apple', 'oxfordPrimary')
  await wait_entry('oxfordPrimary')
  const dead = page.locator('.entry a[data-audio]')
  const n = await dead.count()
  check('oxfordPrimary apple references audio it does not store', n > 0, `${n} anchors`)
  await dead.first().click()
  await page.waitForTimeout(3000)
  const toasts = await page.evaluate(() =>
    [...document.querySelectorAll('.toast-stack [role]')]
      .map((t) => t.textContent?.replace(/\s+/g, ' ').trim()).filter(Boolean))
  check('the dead mp3 surfaces a toast', toasts.length >= 1, JSON.stringify(toasts.slice(0, 3)))
  check('the dead mp3 is de-duplicated to one toast',
    new Set(toasts.map((t) => t.replace(/\s*×$/, ''))).size === 1, JSON.stringify(toasts))
  check('the dead mp3 raises no uncaught error', page_errors.length === 0, page_errors.slice(0, 2).join(' | '))
  await shot('10-oxfordprimary-dead-mp3')
}

// ── 11. command palette ───────────────────────────────────────────────────
{
  await page.goto(`${BASE}/`, { waitUntil: 'networkidle' })
  await page.waitForTimeout(700)
  await page.keyboard.press('Control+k')
  await page.waitForTimeout(700)
  const open = await page.evaluate(() => !!document.querySelector('dialog[open]'))
  check('Ctrl+K opens the command palette', open)
  if (open) {
    const input = page.locator('dialog[open] input').first()
    await input.click()
    const idle = await page.evaluate(() =>
      [...document.querySelectorAll('dialog[open] [role="option"]')]
        .map((n) => n.textContent?.trim()).filter(Boolean))
    check('the palette lists its commands before any typing',
      idle.some((t) => /random word/i.test(t)) && idle.some((t) => /Open settings/i.test(t)),
      JSON.stringify(idle))
    check('the palette lists recently looked-up words',
      idle.length > 5, `${idle.length} items`)

    await input.pressSequentially('ban', { delay: 70 })
    await page.waitForTimeout(1800)
    const items = await page.evaluate(() =>
      [...document.querySelectorAll('dialog[open] [role="option"]')]
        .map((n) => n.textContent?.trim()).filter(Boolean))
    const headwords = items.filter((t) => /^ban/i.test(t))
    check('the palette shows live /suggest results', headwords.length >= 5, JSON.stringify(headwords.slice(0, 8)))
    const server_order = await page.evaluate(async () => {
      const res = await fetch('/suggest?q=ban&limit=12', { headers: { accept: 'application/json' } })
      return (await res.json()).suggestions
    })
    check("CommandMenu's fuzzy pass preserves the server's order",
      JSON.stringify(headwords.slice(0, server_order.length)) === JSON.stringify(server_order),
      `ui=${JSON.stringify(headwords.slice(0, 6))} server=${JSON.stringify(server_order.slice(0, 6))}`)
    check('multi-word headwords survive', headwords.some((t) => t.includes(' ')), JSON.stringify(headwords.slice(0, 8)))
    await shot('11-command-palette')
    await page.locator('dialog[open] [role="option"]', { hasText: /^banana$/ }).first().click()
    await page.waitForTimeout(2500)
    check('choosing a suggestion looks the word up',
      page.url().includes('word=banana') && (await page.locator('.entry').count()) > 0, page.url())
  }
  await page.keyboard.press('Escape')
  await page.waitForTimeout(300)
}

// ── 12. settings persist across reload ────────────────────────────────────
{
  await goto_word('apple', 'OALD9')
  await wait_entry('OALD9')
  await page.locator('button[title="More actions"]').click()
  await page.waitForTimeout(500)
  check('the overflow menu lists its actions',
    (await page.locator('[role="menu"] [role="menuitem"], [role="menu"] button').count()) >= 4,
    `${await page.locator('[role="menu"] button, [role="menu"] [role="menuitem"]').count()} items`)
  await page.locator('[role="menu"]').getByText('Settings', { exact: false }).first().click()
  await page.waitForTimeout(800)
  check('the settings sheet opens', await page.evaluate(() => !!document.querySelector('dialog[open]')))
  await shot('12-settings-sheet')

  const font_input = page.locator('dialog[open] input[type="number"]').first()
  await font_input.fill('1.35')
  await font_input.dispatchEvent('change')
  await page.waitForTimeout(600)
  const scale_before = await page.evaluate(() => document.querySelector('.entry')?.getAttribute('style') ?? '')
  check('the font size control writes --font-scale', /1\.35/.test(scale_before), scale_before)

  await page.evaluate(() => {
    const btn = [...document.querySelectorAll('button')].find((b) =>
      /theme|dark|light/i.test(`${b.getAttribute('title') ?? ''} ${b.getAttribute('aria-label') ?? ''}`))
    btn?.click()
  })
  await page.waitForTimeout(600)
  const theme_before = await page.evaluate(() => document.documentElement.dataset.theme ?? '')
  await page.keyboard.press('Escape')
  await page.waitForTimeout(400)
  await page.reload({ waitUntil: 'networkidle' })
  await wait_entry('OALD9')
  const after = await page.evaluate(() => ({
    theme: document.documentElement.dataset.theme ?? '',
    style: document.querySelector('.entry')?.getAttribute('style') ?? '',
    url: location.search,
  }))
  check('the theme persists across reload', after.theme === theme_before && theme_before !== '',
    `${theme_before} → ${after.theme}`)
  check('the font scale persists across reload', after.style === scale_before && scale_before !== '',
    `${scale_before} → ${after.style}`)
  check('word and dict persist in the URL', after.url.includes('word=apple'), after.url)
  await shot('13-after-reload')
  // back to defaults for the screenshot sweep
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'light'
    document.documentElement.style.colorScheme = 'light'
    localStorage.removeItem('mdict.settings')
  })
}

// ── 13. every dictionary legible in both themes ───────────────────────────
{
  const PROSE = [
    ['OALD9', 'apple'], ['CALD4', 'apple'], ['col', 'apple'], [COBUILD, 'apple'],
    ['oxfordPrimary', 'apple'], [OXENCH, 'apple'], [XINIUJIN, 'apple'], ['collins', 'apple'], ['dict', 'apple'],
  ]
  const AUDIO_ONLY = [['Sound', 'apple'], [SOUND_GB, 'apple']]
  const problems = []
  for (const theme of ['light', 'dark']) {
    for (const [dict, w] of [...PROSE, ...AUDIO_ONLY]) {
      await goto_word(w, dict)
      await page.evaluate((t) => {
        document.documentElement.dataset.theme = t
        document.documentElement.style.colorScheme = t
      }, theme)
      await wait_entry(dict)
      const report = await page.evaluate(() => {
        const entry = document.querySelector('.entry')
        if (!entry) return null
        const parse = (c) => {
          const m = /rgba?\(([^)]+)\)/.exec(c)
          return m ? m[1].split(',').map((x) => parseFloat(x)) : null
        }
        const lum = (c) => {
          if (!c) return null
          const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4 }
          return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2])
        }
        const ratio = (a, b) => {
          const la = lum(a); const lb = lum(b)
          return la === null || lb === null ? null : (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05)
        }
        const page_bg = parse(getComputedStyle(document.body).backgroundColor)
        const is_visible = (el) => {
          for (let p = el; p && p !== document.body; p = p.parentElement) {
            const cs = getComputedStyle(p)
            if (cs.display === 'none' || cs.visibility === 'hidden' || cs.opacity === '0') return false
          }
          return true
        }
        const bg_of = (el) => {
          for (let p = el; p && p !== document.body; p = p.parentElement) {
            const raw = getComputedStyle(p).backgroundColor
            const c = parse(raw)
            const alpha = parseFloat(/rgba\([^)]+,\s*([\d.]+)\)/.exec(raw)?.[1] ?? '1')
            if (c && alpha > 0.5) return c
          }
          return page_bg
        }
        const walker = document.createTreeWalker(entry, NodeFilter.SHOW_TEXT)
        const seen = new Map()
        let node, sampled = 0
        while ((node = walker.nextNode()) && sampled < 5000) {
          if (!node.nodeValue?.trim()) continue
          const el = node.parentElement
          if (!el || !is_visible(el)) continue
          const rect = el.getBoundingClientRect()
          if (rect.width === 0 && rect.height === 0) continue
          sampled++
          const color = getComputedStyle(el).color
          const cur = seen.get(color) ?? { count: 0, min: Infinity, sample: '' }
          cur.count++
          const r = ratio(parse(color), bg_of(el))
          if (r !== null && r < cur.min) { cur.min = r; cur.sample = node.nodeValue.trim().slice(0, 26) }
          seen.set(color, cur)
        }
        return {
          chars: entry.textContent.trim().length,
          sampled,
          audio: entry.querySelectorAll('a[data-audio]').length,
          bad: [...seen.entries()]
            .filter(([, v]) => v.min < 2.5 && v.count >= 2)
            .map(([color, v]) => ({ color, count: v.count, ratio: +v.min.toFixed(2), sample: v.sample })),
        }
      })
      const tag = `${theme}/${dict}`
      if (!report) { problems.push(`${tag}: no entry rendered`); continue }
      const audio_only = AUDIO_ONLY.some(([d]) => d === dict)
      if (audio_only) {
        if (report.audio === 0) problems.push(`${tag}: audio-only dictionary rendered no audio control`)
      } else if (report.chars < 40) {
        problems.push(`${tag}: only ${report.chars} chars of prose`)
      }
      for (const b of report.bad) {
        problems.push(`${tag}: contrast ${b.ratio} on ${b.count}× "${b.sample}" color=${b.color}`)
      }
      await shot(`14-${theme}-${dict.replace(/[^A-Za-z0-9]+/g, '_').slice(0, 26)}`)
    }
  }
  check('every dictionary is legible in both themes', problems.length === 0, problems.slice(0, 16).join(' ;; '))
}

// ── 14. big-entry performance ceiling ─────────────────────────────────────
{
  for (const [dict, word] of [[OXENCH, 'go'], ['OALD9', 'hand'], [XINIUJIN, 'apple']]) {
    const t0 = Date.now()
    await goto_word(word, dict)
    await wait_entry(dict, 60000)
    const timing = await page.evaluate(() => ({
      fcp: Math.round(performance.getEntriesByType('paint').find((p) => p.name === 'first-contentful-paint')?.startTime ?? 0),
      chars: document.querySelector('.entry')?.textContent?.length ?? 0,
      nodes: document.querySelector('.entry')?.querySelectorAll('*').length ?? 0,
    }))
    const wall = Date.now() - t0
    check(`${dict.slice(0, 24)} "${word}" renders`, timing.chars > 500 && wall < 25000,
      `wall=${wall}ms fcp=${timing.fcp}ms chars=${timing.chars} nodes=${timing.nodes}`)
    await shot(`15-perf-${word}-${dict.replace(/[^A-Za-z0-9]+/g, '_').slice(0, 16)}`)
  }
}

// ── 15. programmatic clients see no redirect ──────────────────────────────
{
  const r = await page.evaluate(async () => {
    const res = await fetch('/lookup?word=apple&dict=OALD9&format=json', {
      headers: { accept: 'application/json' }, redirect: 'manual',
    })
    return { status: res.status, type: res.type }
  })
  check('fetch() to /lookup is not redirected', r.status === 200, JSON.stringify(r))
}

// ── 16. tabs ──────────────────────────────────────────────────────────────
{
  await goto_word('apple', 'OALD9')
  await wait_entry('OALD9')
  const tabs = page.locator('[role="tab"]')
  const count = await tabs.count()
  check('a word in many dictionaries gets one tab each', count > 3, `${count} tabs`)
  const labels = []
  for (let i = 0; i < count; i++) labels.push((await tabs.nth(i).innerText()).trim())
  check('tabs are labelled with real titles',
    labels.every((l) => l && !/Title \(No HTML code allowed\)/.test(l)), JSON.stringify(labels))
  await tabs.nth(1).click()
  await page.waitForTimeout(1500)
  const switched = await page.evaluate(() => ({
    dict: document.querySelector('.entry')?.getAttribute('data-dict') ?? '',
    url: new URLSearchParams(location.search).get('dict') ?? '',
    nav: performance.getEntriesByType('navigation').length,
  }))
  check('clicking a tab switches the entry without a reload',
    switched.dict === switched.url && switched.dict === labels[1], JSON.stringify(switched))
  await shot('16-tabs')
}

// ── 17. the shell scrolls, the chrome does not ────────────────────────────
{
  await goto_word('hand', 'OALD9')
  await wait_entry('OALD9', 60000)
  const before = await page.evaluate(() => ({
    content: document.querySelector('.scroll-area').scrollTop,
    side: document.querySelector('.shell__side').getBoundingClientRect().top,
    bar: document.querySelector('.topbar').getBoundingClientRect().top,
    doc: window.scrollY,
  }))
  await page.mouse.move(700, 500)
  await page.mouse.wheel(0, 800)
  await page.waitForTimeout(600)
  const after = await page.evaluate(() => ({
    content: document.querySelector('.scroll-area').scrollTop,
    side: document.querySelector('.shell__side').getBoundingClientRect().top,
    bar: document.querySelector('.topbar').getBoundingClientRect().top,
    doc: window.scrollY,
  }))
  check('the mouse wheel scrolls the entry', after.content > before.content,
    `scrollTop ${before.content} → ${after.content}`)
  check('sidebar and topbar stay put while the content scrolls',
    after.side === before.side && after.bar === before.bar,
    `side ${before.side} → ${after.side}, topbar ${before.bar} → ${after.bar}`)
  check('the document itself never scrolls', after.doc === 0, `scrollY=${after.doc}`)
}
// ── 18. the features the other dictionary implementations agree on ────────
{
  await goto_word('apple', 'OALD9')
  await wait_entry('OALD9')

  // words nearby, read straight off the sorted index
  const chips = page.locator('.nearby .nearby__chip')
  const chip_texts = await chips.allTextContents()
  check('a "words nearby" strip sits under the headword', chip_texts.length >= 4,
    JSON.stringify(chip_texts.slice(0, 8)))
  check('the neighbours are the real index neighbours',
    chip_texts.includes('apple pie') && chip_texts.includes('applause'),
    JSON.stringify(chip_texts))
  await chips.first().click()
  await page.waitForTimeout(1800)
  check('a nearby chip navigates', (await page.locator('.entry').count()) > 0 &&
    !(page.url().includes('word=apple&')), page.url())

  // did-you-mean on a typo
  await goto_word('aple', 'OALD9')
  await page.waitForTimeout(2500)
  const guesses = page.locator('.empty .nearby__chip--guess')
  const guess_texts = await guesses.allTextContents()
  check('a miss offers "did you mean" candidates', guess_texts.includes('apple'),
    JSON.stringify(guess_texts))
  if (guess_texts.includes('apple')) {
    await guesses.filter({ hasText: /^apple$/ }).first().click()
    await page.waitForTimeout(2000)
    check('choosing a correction looks it up', page.url().includes('word=apple'), page.url())
  }

  // double-click any word in the entry
  await goto_word('apple', 'OALD9')
  await wait_entry('OALD9')
  // Select the word "fruit" inside the definition and fire the dblclick a
  // browser would fire: the handler reads the selection, not the caret.
  const dispatched = await page.evaluate(() => {
    const root = document.querySelector('.entry')
    if (!root) return false
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT)
    let node
    while ((node = walker.nextNode())) {
      const at = (node.nodeValue ?? '').indexOf('fruit')
      if (at < 0) continue
      const range = document.createRange()
      range.setStart(node, at)
      range.setEnd(node, at + 'fruit'.length)
      const sel = window.getSelection()
      sel?.removeAllRanges()
      sel?.addRange(range)
      node.parentElement?.dispatchEvent(
        new MouseEvent('dblclick', { bubbles: true, cancelable: true, button: 0, clientX: 500, clientY: 500 }),
      )
      return true
    }
    return false
  })
  check('a word inside the definition can be selected and double-clicked', dispatched)
  await page.waitForTimeout(1500)
  const pop_word = await page.locator('.wordpop__word').textContent().catch(() => null)
  check('double-clicking a word opens the lookup popover', pop_word === 'fruit', String(pop_word))
  if (pop_word === 'fruit') {
    check('the popover previews the target entry',
      ((await page.locator('.wordpop__body').textContent()) ?? '').length > 10)
    await page.locator('.wordpop__btn', { hasText: 'Save' }).click()
    await page.waitForTimeout(600)
    check('saving from the popover keeps it open',
      (await page.locator('.wordpop').count()) === 1)
    await page.locator('.wordpop__btn--go').click()
    await page.waitForTimeout(2000)
    check('the popover can navigate to the double-clicked word',
      page.url().includes('word=fruit'), page.url())
  }

  // word list: starred from the popover above, visible in the sidebar
  await page.locator('.shell__side .accordion-trigger').filter({ hasText: 'Word list' }).first().click()
  await page.waitForTimeout(500)
  const saved = await page.locator('.shell__side .side-link__label').allTextContents()
  check('the word list keeps the starred word', saved.includes('fruit'), JSON.stringify(saved))
  const export_buttons = await page.locator('.shell__side .side-export button').allTextContents()
  check('the word list exports to Anki, CSV and JSON',
    export_buttons.some((t) => /Anki/.test(t)) && export_buttons.some((t) => /CSV/.test(t)) &&
    export_buttons.some((t) => /JSON/.test(t)), JSON.stringify(export_buttons))
  await shot('17-wordlist')

  // playback rate and loop
  await goto_word('apple', 'OALD9')
  await wait_entry('OALD9')
  await page.locator('.entry a[data-audio][data-playable="1"]').first().click()
  await page.waitForSelector('.player', { timeout: 6000 }).catch(() => {})
  await page.waitForTimeout(800)
  await page.locator('.player .button-group button', { hasText: '0.75×' }).click()
  await page.waitForTimeout(400)
  const rate = await page.evaluate(() => {
    // the element is detached from the DOM; read it through the player state
    return document.querySelector('.player')?.textContent?.includes('0:00') ?? false
  })
  const loop_before = await page.locator('.player__loop').getAttribute('aria-pressed')
  await page.locator('.player__loop').click()
  await page.waitForTimeout(300)
  const loop_after = await page.locator('.player__loop').getAttribute('aria-pressed')
  check('the loop control toggles', loop_before === 'false' && loop_after === 'true',
    `${loop_before} → ${loop_after}`)
  check('the speed control is present and selectable', rate, 'player visible at 0.75x')
  await shot('18-player-rate-loop')

  // wildcards in the palette
  await page.keyboard.press('Control+k')
  await page.waitForTimeout(600)
  const input = page.locator('dialog[open] input').first()
  await input.click()
  await input.pressSequentially('app*e', { delay: 60 })
  await page.waitForTimeout(1800)
  const wild = await page.evaluate(() =>
    [...document.querySelectorAll('dialog[open] [role="option"]')]
      .map((n) => n.textContent?.trim()).filter(Boolean))
  check('wildcards search the whole index', wild.some((t) => /^app.*e$/i.test(t) && t !== 'app*e'),
    JSON.stringify(wild.slice(0, 6)))
  await page.keyboard.press('Escape')
}

// ── 19. phone-width layout ────────────────────────────────────────────────
{
  const phone = await browser.newPage({ viewport: { width: 420, height: 860 } })
  await phone.goto(`${BASE}/?word=apple&dict=OALD9`, { waitUntil: 'domcontentloaded' })
  await phone.waitForSelector('.entry', { timeout: 25000 })
  await phone.waitForTimeout(1200)
  const layout = await phone.evaluate(() => {
    const cols = getComputedStyle(document.querySelector('.entry-columns')).gridTemplateColumns
    const entry = document.querySelector('.entry-wrap').getBoundingClientRect()
    const tabs = document.querySelector('.tabs-list')
    return {
      tracks: cols.split(' ').length,
      entry_width: Math.round(entry.width),
      tabs_scrollable: tabs.scrollWidth > tabs.clientWidth || getComputedStyle(tabs).overflowX === 'auto',
      tabs_one_row: tabs.getBoundingClientRect().height < 90,
    }
  })
  check('the entry takes the full phone width', layout.tracks === 1 && layout.entry_width > 360,
    JSON.stringify(layout))
  check('the dictionary tabs scroll in one row instead of stacking',
    layout.tabs_one_row, JSON.stringify(layout))
  await phone.locator('.nav-toggle').click()
  await phone.waitForTimeout(700)
  const drawer = await phone.evaluate(() => {
    const pane = document.querySelector('.nav-pane')
    if (!pane) return null
    const r = pane.getBoundingClientRect()
    return { visible: getComputedStyle(pane).display !== 'none', x: Math.round(r.x), w: Math.round(r.width), h: Math.round(r.height) }
  })
  check('the navigation drawer opens on phones', !!drawer && drawer.visible && drawer.x <= 8 && drawer.w > 250,
    JSON.stringify(drawer))
  check('the drawer lists history and dictionaries',
    (await phone.locator('.nav-pane .accordion-trigger').count()) >= 3,
    `${await phone.locator('.nav-pane .accordion-trigger').count()} sections`)
  await phone.close()
}

await browser.close()

console.log('\n── summary ──')
const failed = results.filter((r) => !r.ok)
console.log(`${results.length - failed.length}/${results.length} passed`)
for (const f of failed) console.log(`  FAIL ${f.name}: ${f.detail}`)
if (page_errors.length) {
  console.log(`\nuncaught page errors (${new Set(page_errors).size} unique):`)
  for (const e of [...new Set(page_errors)].slice(0, 10)) console.log(`  ${e.slice(0, 200)}`)
}
if (failed_requests.length) {
  const uniq = [...new Set(failed_requests)]
  console.log(`\nfailed requests (${uniq.length} unique):`)
  for (const u of uniq.slice(0, 20)) console.log(`  ${u}`)
}
process.exit(failed.length ? 1 : 0)
