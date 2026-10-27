import { afterEach, describe, expect, it, vi } from 'vitest'
import { cached_lookup, fetch_dicts, fetch_entries_except, fetch_entry, fetch_suggest } from '../api.js'

function stub_fetch(respond: (url: string) => Partial<Response> & { body_text?: string }) {
  const spy = vi.fn(async (input: RequestInfo | URL) => {
    const url = String(input)
    const { body_text, ...init } = respond(url)
    return new Response(body_text ?? '', init as ResponseInit)
  })
  vi.stubGlobal('fetch', spy)
  return spy
}

afterEach(() => vi.unstubAllGlobals())

describe('fetch_entry', () => {
  // Every test here uses its own word: a successful lookup is remembered for the
  // life of the module, so a repeated one would never reach `fetch`.
  it('returns the html on success', async () => {
    stub_fetch(() => ({ status: 200, body_text: '<div class="entry">apple</div>' }))
    const out = await fetch_entry('apple', 'OALD9')
    expect(out).toEqual({ kind: 'ok', dict: 'OALD9', html: '<div class="entry">apple</div>' })
  })

  it('treats "not found" as the ordinary silent miss', async () => {
    const spy = stub_fetch(() => ({ status: 404, body_text: '{"error":"not found","word":"xyzzy"}' }))
    expect(await fetch_entry('xyzzy', 'OALD9')).toEqual({ kind: 'miss', dict: 'OALD9' })
    // A miss must not be reported to the reader as a fault.
    expect(spy).toHaveBeenCalledTimes(1)
  })

  it('separates an unknown dictionary from a missing word', async () => {
    // With format=html both are 404; only the JSON body tells them apart.
    stub_fetch(() => ({ status: 404, body_text: '{"error":"unknown dictionary","dict":"Nope"}' }))
    const out = await fetch_entry('apple', 'Nope')
    expect(out.kind).toBe('error')
    if (out.kind === 'error') expect(out.message).toContain('unknown dictionary')
  })

  it('answers a repeated lookup from memory', async () => {
    const spy = stub_fetch(() => ({ status: 200, body_text: '<div class="entry">pear</div>' }))
    await fetch_entry('pear', 'OALD9')
    expect(await fetch_entry('pear', 'OALD9')).toEqual({
      kind: 'ok',
      dict: 'OALD9',
      html: '<div class="entry">pear</div>',
    })
    expect(spy).toHaveBeenCalledTimes(1)
  })

  it('reports an unreachable server as an error, not a miss', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new TypeError('Failed to fetch')
      }),
    )
    const out = await fetch_entry('durian', 'OALD9')
    expect(out).toEqual({ kind: 'error', dict: 'OALD9', message: 'server unreachable' })
  })

  it('retries a failure rather than remembering it', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        if (calls === 1) throw new TypeError('Failed to fetch')
        return new Response('<div class="entry">plum</div>', { status: 200 })
      }),
    )
    expect((await fetch_entry('plum', 'OALD9')).kind).toBe('error')
    expect((await fetch_entry('plum', 'OALD9')).kind).toBe('ok')
    expect(calls).toBe(2)
  })

  it('rethrows aborts so an in-flight lookup can be cancelled', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw Object.assign(new Error('aborted'), { name: 'AbortError' })
      }),
    )
    await expect(
      fetch_entry('rambutan', 'OALD9', new AbortController().signal),
    ).rejects.toMatchObject({
      name: 'AbortError',
    })
  })
})

describe('fetch_entries_except', () => {
  it('names every dictionary to leave out, and splits what comes back', async () => {
    const spy = stub_fetch(() => ({
      status: 200,
      body_text:
        '<section class="dict" data-dict="B"><h3>B</h3><p>b</p></section>\n<hr/>\n' +
        '<section class="dict" data-dict="C"><h3>C</h3><p>c</p></section>',
    }))
    const out = await fetch_entries_except('quince', ['A', 'B'])
    const url = new URL(String(spy.mock.calls[0]![0]), 'http://localhost')
    expect(url.searchParams.getAll('except')).toEqual(['A', 'B'])
    if (out.kind !== 'ok') throw new Error(`expected sections, got ${out.kind}`)
    expect([...out.sections.keys()]).toEqual(['B', 'C'])
    expect(out.sections.get('B')).toBe('<p>b</p>')
    // A combined answer warms the same per-dictionary memory a single one does.
    expect(cached_lookup('quince', 'C')).toEqual({
      kind: 'ok',
      dict: 'C',
      html: '<p>c</p>',
    })
  })
})

describe('fetch_suggest', () => {
  it('returns the server list in order', async () => {
    stub_fetch(() => ({
      status: 200,
      body_text: '{"count":3,"query":"ban","suggestions":["ban","banal","banana"]}',
    }))
    expect(await fetch_suggest('ban', { limit: 3 })).toEqual(['ban', 'banal', 'banana'])
  })

  it('treats no match as an empty list, not an error', async () => {
    stub_fetch(() => ({ status: 200, body_text: '{"count":0,"query":"zz","suggestions":[]}' }))
    expect(await fetch_suggest('zz')).toEqual([])
  })
})

describe('fetch_dicts', () => {
  it('reads the dictionaries array and the transcode capability', async () => {
    stub_fetch(() => ({
      status: 200,
      body_text:
        '{"dictionaries":[{"name":"OALD9","title":"t","entries":1,"records":1,"has_resources":true}],"transcode":true}',
    }))
    const info = await fetch_dicts()
    expect(info.dictionaries).toHaveLength(1)
    expect(info.dictionaries[0]?.has_resources).toBe(true)
    expect(info.transcode).toBe(true)
  })

  it('survives a body without the array', async () => {
    stub_fetch(() => ({ status: 200, body_text: '{}' }))
    const info = await fetch_dicts()
    expect(info.dictionaries).toEqual([])
    expect(info.transcode).toBe(false)
  })
})
