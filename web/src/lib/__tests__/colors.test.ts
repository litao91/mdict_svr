import { describe, expect, it } from 'vitest'
import { adapt_for_dark, color_pair, parse_color, split_style_color } from '../colors.js'

/**
 * Every distinct color value that occurs in ./data, plus the legacy spellings.
 * The dark half of each pair has to land inside the legibility band: `#0` (111x),
 * `navy` (225x), `black`, `midnightblue`, `indigo`, `darkred` and `darkgreen`
 * are all invisible on a dark background if passed through untouched.
 */
const CORPUS_COLORS = [
  'black',
  'white',
  'gray',
  'grey',
  'lightgray',
  'lightgrey',
  'navy',
  'blue',
  'dodgerblue',
  'midnightblue',
  'indigo',
  'darkviolet',
  'green',
  'darkgreen',
  'limegreen',
  'olive',
  'red',
  'darkred',
  'crimson',
  'mediumvioletred',
  '#0',
  '#000',
  '#000000',
  '#fff',
  '#ffffff',
  '#ff0000',
  '#0000ff',
  '#000080',
  '#008000',
  '#800080',
]

const lightness_of = (hsl: string): number => {
  const match = /hsl\(-?\d+(?:\.\d+)? [\d.]+% ([\d.]+)%\)/.exec(hsl)
  expect(match, hsl).not.toBeNull()
  return Number(match![1])
}

describe('parse_color', () => {
  it('parses hex, short hex and the legacy padded forms', () => {
    expect(parse_color('#0')).toEqual({ r: 0, g: 0, b: 0 })
    expect(parse_color('#000')).toEqual({ r: 0, g: 0, b: 0 })
    expect(parse_color('#fff')).toEqual({ r: 255, g: 255, b: 255 })
    expect(parse_color('#ff0000')).toEqual({ r: 255, g: 0, b: 0 })
    expect(parse_color('NAVY')).toEqual({ r: 0, g: 0, b: 128 })
  })

  it('rejects anything that is not a color rather than guessing', () => {
    for (const raw of ['', 'transparent', 'inherit', '#12345', 'rgb(1,2,3)', 'rebeccapurple']) {
      expect(parse_color(raw), raw).toBeNull()
    }
  })
})

describe('color_pair over the whole corpus palette', () => {
  it('keeps the publisher value verbatim in light mode', () => {
    for (const raw of CORPUS_COLORS) {
      const pair = color_pair(raw)
      expect(pair, raw).not.toBeNull()
      expect(pair!.light, raw).toMatch(/^#[0-9a-f]{6}$/)
    }
    expect(color_pair('navy')!.light).toBe('#000080')
    expect(color_pair('#0')!.light).toBe('#000000')
  })

  it('lifts every dark-unreadable value into the legible band', () => {
    for (const raw of CORPUS_COLORS) {
      const light = lightness_of(adapt_for_dark(parse_color(raw)!))
      expect(light, `${raw} → ${light}`).toBeGreaterThanOrEqual(66)
      expect(light, `${raw} → ${light}`).toBeLessThanOrEqual(88)
    }
  })

  it('desaturates the neon end so dark mode does not glare', () => {
    const saturated = adapt_for_dark(parse_color('#ff0000')!)
    const match = /hsl\(\d+ ([\d.]+)% [\d.]+%\)/.exec(saturated)
    expect(match).not.toBeNull()
    expect(Number(match![1])).toBeLessThan(100)
  })

  it('returns null for unparseable values so the caller keeps the attribute', () => {
    expect(color_pair('transparent')).toBeNull()
    expect(color_pair('')).toBeNull()
  })
})

describe('split_style_color', () => {
  it('removes only the color declaration', () => {
    const { rest, color } = split_style_color('margin-left:20px;color:blue;display:block;')
    expect(color).toBe('blue')
    expect(rest).toBe('margin-left:20px; display:block')
  })

  it('leaves CALD4 layout styles intact when there is no color', () => {
    const { rest, color } = split_style_color('margin-left:20px;display:block;')
    expect(color).toBeNull()
    expect(rest).toBe('margin-left:20px; display:block')
  })

  it('does not mistake background-color for color', () => {
    const { rest, color } = split_style_color('background-color:red;color:green')
    expect(color).toBe('green')
    expect(rest).toBe('background-color:red')
  })
})
