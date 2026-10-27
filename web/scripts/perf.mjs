/**
 * Throttled performance report: the cold load, then the reading loop.
 *
 *   npm run build && npm run perf            # against BASE, default 100ms/10Mbps
 *   LATENCY=200 DOWN_MBPS=3 npm run perf     # a bad connection
 *
 * Localhost latency is zero, which hides the things a real reader pays for:
 * round trips, socket queuing on the lookup fan-out, and transfer size. This
 * emulates a network and reports the waterfall, then the wall-clock cost of the
 * interactions that follow it.
 */
import { chromium } from 'playwright'

const BASE = process.env.BASE ?? 'http://127.0.0.1:8396'
const LATENCY = Number(process.env.LATENCY ?? 100)
const DOWN_MBPS = Number(process.env.DOWN_MBPS ?? 10)
const WORDS = [
  ['hand', 'OALD9'],
  ['go', "Oxford Advanced Learner's En-Ch Dictionary"],
]

const browser = await chromium.launch()

/** A fresh page on the emulated connection. */
async function throttled_page() {
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 960 } })
  const page = await ctx.newPage()
  const cdp = await ctx.newCDPSession(page)
  await cdp.send('Network.enable')
  await cdp.send('Network.emulateNetworkConditions', {
    offline: false,
    latency: LATENCY,
    downloadThroughput: (DOWN_MBPS * 1_000_000) / 8,
    uploadThroughput: (DOWN_MBPS * 1_000_000) / 8,
  })
  return { ctx, page }
}

console.log(`${BASE} @ ${LATENCY}ms RTT, ${DOWN_MBPS} Mbps`)

for (const [word, dict] of WORDS) {
  const { ctx, page } = await throttled_page()
  await page.goto(`${BASE}/?word=${encodeURIComponent(word)}&dict=${encodeURIComponent(dict)}`, {
    waitUntil: 'domcontentloaded',
  })
  await page.waitForSelector('.entry', { timeout: 60000 })
  await page.waitForTimeout(2500)

  const report = await page.evaluate(() => {
    const mark = (n) => Math.round(performance.getEntriesByName(n).at(-1)?.startTime ?? -1)
    const nav = performance.getEntriesByType('navigation')[0]
    const paints = performance.getEntriesByType('paint')
    const resources = performance
      .getEntriesByType('resource')
      .map((r) => ({
        name: r.name.replace(location.origin, ''),
        start: Math.round(r.startTime),
        dur: Math.round(r.duration),
        kb: Math.round((r.transferSize || r.encodedBodySize || 0) / 1024),
      }))
    return {
      boot: {
        domContentLoaded: Math.round(nav.domContentLoadedEventEnd),
        fcp: Math.round(paints.find((p) => p.name === 'first-contentful-paint')?.startTime ?? -1),
        // What the first paint is waiting on, before the idle chunks arrive.
        critical_kb: resources
          .filter((r) => r.name.startsWith('/assets/') && r.start < 200)
          .reduce((a, b) => a + b.kb, 0),
      },
      marks: {
        fetch_start: mark('mdict:fetch-start'),
        first_result: mark('mdict:first-result'),
        all_settled: mark('mdict:all-settled'),
      },
      sanitize_ms: performance
        .getEntriesByName('mdict:sanitize')
        .map((m) => Math.round(m.duration)),
      requests: [...resources].sort((a, b) => a.start - b.start),
    }
  })

  const m = report.marks
  console.log(`\n── cold load: ${dict} "${word}" ──`)
  console.log(
    `boot: DCL=${report.boot.domContentLoaded}ms fcp=${report.boot.fcp}ms critical=${report.boot.critical_kb}KB`,
  )
  console.log(
    `lookup: fetch@${m.fetch_start}ms first-result@${m.first_result}ms all-settled@${m.all_settled}ms`,
  )
  console.log(`sanitize (main thread): ${report.sanitize_ms.join(', ')} ms`)
  console.log(`requests (${report.requests.length}):`)
  for (const r of report.requests.slice(0, 10)) {
    console.log(
      `  t+${String(r.start).padStart(5)} ${String(r.dur).padStart(5)}ms ${String(r.kb).padStart(5)}KB  ${r.name.slice(0, 74)}`,
    )
  }
  if (report.requests.length > 10) console.log(`  … ${report.requests.length - 10} more`)
  await ctx.close()
}

// A dictionary is read, not opened once, so the interactions after the first
// paint are measured the same way: what the next word costs, and what going back
// costs.
{
  const { ctx, page } = await throttled_page()
  const lookups = []
  page.on('request', (r) => {
    if (r.url().includes('/lookup')) lookups.push(new URL(r.url()).searchParams)
  })

  async function step(label, fn) {
    lookups.length = 0
    const t0 = Date.now()
    await fn()
    const asked = lookups.map(
      (q) => `${q.get('word')}${q.has('dict') ? ` [${q.get('dict')}]` : ' [all]'}`,
    )
    console.log(
      `${label.padEnd(30)} ${String(Date.now() - t0).padStart(5)} ms   ${asked.length} lookups${
        asked.length ? `: ${asked.join(', ')}` : ''
      }`,
    )
    return lookups.slice()
  }

  console.log('\n── reading loop ──')
  await page.goto(`${BASE}/?word=hand&dict=OALD9`, { waitUntil: 'domcontentloaded' })
  await page.waitForSelector('.entry', { timeout: 60000 })
  // The neighbours are fetched behind the entry already on screen; let them land.
  await page.waitForTimeout(2000)

  let back_to = 'hand'
  for (let i = 0; i < 2; i++) {
    const next = page.locator('.word-nav .next a')
    const word = (await next.textContent()) ?? ''
    const leaving = (await page.locator('.word-head__title').textContent()) ?? ''
    const asked = await step(`Next → "${word}"`, async () => {
      await next.click()
      await page.waitForFunction(
        (w) => document.querySelector('.word-head__title')?.textContent === w,
        word,
        { timeout: 60000 },
      )
      await page.waitForSelector('.entry', { timeout: 60000 })
    })
    // A per-dictionary request here means the prefetch did not cover the click.
    const own = asked.filter((q) => q.get('word') === word && q.has('dict'))
    if (own.length) console.log(`  ^^ fetched ${word} per dictionary instead of from memory`)
    back_to = leaving
    await page.waitForTimeout(1500)
  }

  await step(`Back → "${back_to}"`, async () => {
    await page.goBack()
    await page.waitForFunction(
      (w) => document.querySelector('.word-head__title')?.textContent === w,
      back_to,
      { timeout: 60000 },
    )
    await page.waitForSelector('.entry', { timeout: 60000 })
  })

  await step('a word nothing has seen', async () => {
    await page.evaluate(() => {
      window.history.pushState(null, '', '/?word=zebra&dict=OALD9')
      window.dispatchEvent(new PopStateEvent('popstate'))
    })
    await page.waitForFunction(
      () => document.querySelector('.word-head__title')?.textContent === 'zebra',
      null,
      { timeout: 60000 },
    )
    await page.waitForSelector('.entry', { timeout: 60000 })
  })

  const tabs = page.locator('[role="tab"]')
  if ((await tabs.count()) > 1) {
    const label = ((await tabs.nth(1).textContent()) ?? '').slice(0, 24)
    await step(`tab → "${label}"`, async () => {
      await tabs.nth(1).click()
      await page.waitForTimeout(80)
      await page.waitForSelector('.entry', { timeout: 60000 })
    })
  }
  await ctx.close()
}

await browser.close()
