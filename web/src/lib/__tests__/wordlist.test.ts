import { describe, expect, it } from 'vitest'
import { format_wordlist, type WordListItem } from '../wordlist.svelte.js'

const ITEMS: WordListItem[] = [
  { word: 'apple', added_at: Date.parse('2026-01-02T03:04:05Z') },
  { word: 'a, "quoted" word', added_at: Date.parse('2026-02-03T04:05:06Z') },
]

describe('format_wordlist', () => {
  it('json round-trips the items', () => {
    expect(JSON.parse(format_wordlist(ITEMS, 'json'))).toEqual(ITEMS)
  })

  it('csv quotes cells that contain commas or quotes', () => {
    const [head, first, second] = format_wordlist(ITEMS, 'csv').split('\n')
    expect(head).toBe('word,added_at')
    expect(first).toBe('apple,2026-01-02T03:04:05.000Z')
    expect(second).toBe('"a, ""quoted"" word",2026-02-03T04:05:06.000Z')
  })

  it('anki puts the definition on the back and flattens newlines', () => {
    const out = format_wordlist(ITEMS, 'anki', (w) =>
      w === 'apple' ? 'a round fruit\nwith shiny skin' : null,
    )
    const [first, second] = out.split('\n')
    expect(first).toBe('apple\ta round fruit with shiny skin')
    // A word with no fetched definition still exports, with an empty back.
    expect(second).toBe('a, "quoted" word\t')
  })

  it('anki never emits a tab or newline inside a field', () => {
    const out = format_wordlist([{ word: 'x', added_at: 0 }], 'anki', () => 'a\tb\nc')
    expect(out).toBe('x\ta b c')
  })

  it('an empty list still produces a header for csv', () => {
    expect(format_wordlist([], 'csv')).toBe('word,added_at')
    expect(format_wordlist([], 'json')).toBe('[]')
    expect(format_wordlist([], 'anki')).toBe('')
  })
})
