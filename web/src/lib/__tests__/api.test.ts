import { afterEach, describe, expect, it, vi } from 'vitest'
import { fetch_dicts, fetch_entry, fetch_suggest } from '../api.js'

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

  it('reports an unreachable server as an error, not a miss', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw new TypeError('Failed to fetch')
      }),
    )
    const out = await fetch_entry('apple', 'OALD9')
    expect(out).toEqual({ kind: 'error', dict: 'OALD9', message: 'server unreachable' })
  })

  it('rethrows aborts so an in-flight lookup can be cancelled', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        throw Object.assign(new Error('aborted'), { name: 'AbortError' })
      }),
    )
    await expect(fetch_entry('apple', 'OALD9', new AbortController().signal)).rejects.toMatchObject({
      name: 'AbortError',
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
