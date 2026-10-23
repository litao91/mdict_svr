/**
 * Theme-aware color adaptation for dictionary markup.
 *
 * Four of the nine dictionaries carry no classes at all: their entire visual
 * structure is `<font color=…>`, `<font size=…>` and inline `style="color:…"`.
 * The sanitizer lifts those colors into a `data-c` attribute with a light and a
 * dark value, because 29 distinct values appear in the corpus and several of them
 * (`#0` 111×, `navy` 225×, `black`, `midnightblue`, `indigo`, `darkred`,
 * `darkgreen`) would be invisible on a dark background.
 *
 * `<font color>` is only a presentational hint, so author CSS beats it — but an
 * inline `style="color:…"` beats author CSS, which is why the color has to be
 * *removed* from the element rather than merely overridden.
 */

/** The CSS named colors that actually occur in the corpus. */
const NAMED: Record<string, string> = {
  black: '#000000',
  white: '#ffffff',
  gray: '#808080',
  grey: '#808080',
  lightgray: '#d3d3d3',
  lightgrey: '#d3d3d3',
  navy: '#000080',
  blue: '#0000ff',
  dodgerblue: '#1e90ff',
  midnightblue: '#191970',
  indigo: '#4b0082',
  darkviolet: '#9400d3',
  green: '#008000',
  darkgreen: '#006400',
  limegreen: '#32cd32',
  olive: '#808000',
  red: '#ff0000',
  darkred: '#8b0000',
  crimson: '#dc143c',
  mediumvioletred: '#c71585',
}

export interface Rgb {
  r: number
  g: number
  b: number
}

/** Parse a `#rgb`/`#rrggbb` hex or one of the named colors above. */
export function parse_color(raw: string): Rgb | null {
  const value = raw.trim().toLowerCase()
  if (!value) return null
  const digits = value.startsWith('#') ? value.slice(1) : (NAMED[value]?.slice(1) ?? '')
  if (!digits || !/^[0-9a-f]+$/.test(digits)) return null
  // Legacy HTML color values are padded rather than rejected: `#0` is black.
  // 4- and 8-digit forms carry an alpha channel, which we ignore.
  const padded = digits.length <= 2 ? digits.padStart(3, '0') : digits
  const short = padded.length === 3 || padded.length === 4
  if (![3, 4, 6, 8].includes(padded.length)) return null
  const channel = (i: number): number =>
    parseInt(short ? padded[i]!.repeat(2) : padded.slice(i * 2, i * 2 + 2), 16)
  const rgb = { r: channel(0), g: channel(1), b: channel(2) }
  return Object.values(rgb).some(Number.isNaN) ? null : rgb
}

interface Hsl {
  h: number
  s: number
  l: number
}

function to_hsl({ r, g, b }: Rgb): Hsl {
  const rn = r / 255
  const gn = g / 255
  const bn = b / 255
  const max = Math.max(rn, gn, bn)
  const min = Math.min(rn, gn, bn)
  const l = (max + min) / 2
  const d = max - min
  if (d === 0) return { h: 0, s: 0, l }
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  const h =
    max === rn
      ? ((gn - bn) / d + (gn < bn ? 6 : 0)) * 60
      : max === gn
        ? ((bn - rn) / d + 2) * 60
        : ((rn - gn) / d + 4) * 60
  return { h, s, l }
}

function from_hsl({ h, s, l }: Hsl): string {
  const pct = (n: number) => `${Math.round(n * 1000) / 10}%`
  return `hsl(${Math.round(h)} ${pct(s)} ${pct(l)})`
}

/** The floor above which a color stays legible on the dark surface. */
const DARK_MIN_L = 0.66
/** Slightly dim near-whites so they do not glare on a dark background. */
const DARK_MAX_L = 0.88

/**
 * Adapt a color for a dark background: lift dark colors above the legibility
 * floor, dim near-whites, and pull very saturated colors back so they do not
 * turn neon. Light mode keeps the publisher's original value untouched.
 */
export function adapt_for_dark(rgb: Rgb): string {
  const { h, s, l } = to_hsl(rgb)
  return from_hsl({
    h,
    s: s > 0.6 ? s * 0.85 : s,
    l: Math.min(Math.max(l, DARK_MIN_L), DARK_MAX_L),
  })
}

export interface ColorPair {
  /** The publisher's own value, normalized to a hex string. */
  light: string
  dark: string
}

/**
 * Both theme values for a color as written in the markup, or `null` when the
 * value cannot be parsed — in which case the caller leaves the original
 * attribute alone rather than silently dropping the color.
 */
export function color_pair(raw: string): ColorPair | null {
  const rgb = parse_color(raw)
  if (!rgb) return null
  const hex = (n: number) => n.toString(16).padStart(2, '0')
  return {
    light: `#${hex(rgb.r)}${hex(rgb.g)}${hex(rgb.b)}`,
    dark: adapt_for_dark(rgb),
  }
}

/**
 * Pull the `color` declaration out of an inline `style`, returning the remaining
 * declarations and the color value. CALD4's layout lives in these styles
 * (`margin-left:20px;display:block`), so everything else must survive.
 */
export function split_style_color(style: string): { rest: string; color: string | null } {
  let color: string | null = null
  const kept: string[] = []
  for (const decl of style.split(';')) {
    const trimmed = decl.trim()
    if (!trimmed) continue
    const idx = trimmed.indexOf(':')
    if (idx < 0) {
      kept.push(trimmed)
      continue
    }
    const prop = trimmed.slice(0, idx).trim().toLowerCase()
    if (prop === 'color' && color === null) {
      color = trimmed.slice(idx + 1).trim()
    } else {
      kept.push(trimmed)
    }
  }
  return { rest: kept.join('; '), color }
}
