/**
 * Persisted user preferences.
 *
 * Saved through `svelte-widgets/storage`, which turns a disabled, private or full
 * localStorage into a no-op so the UI keeps working in memory.
 */

import { storage_get_json, storage_set_json, storage_remove } from 'svelte-widgets/storage'

const KEY = 'mdict.settings'

export type EntryView = 'styled' | 'markdown'

export interface Settings {
  /** Multiplier applied to the entry's base font size. */
  font_scale: number
  /** Render the publisher's HTML, or the server's Markdown conversion. */
  view: EntryView
  /** Dictionary names switched on. Empty means "all of them". */
  enabled_dicts: string[]
  /** Hover a cross-reference to preview its definition. */
  previews: boolean
  /** Show the pronunciation audio controls inline. */
  show_audio: boolean
}

export const FONT_SCALE_MIN = 0.85
export const FONT_SCALE_MAX = 1.6

const DEFAULTS: Settings = {
  font_scale: 1,
  view: 'styled',
  enabled_dicts: [],
  previews: true,
  show_audio: true,
}

function load(): Settings {
  const stored = storage_get_json(KEY, null)
  if (!stored || typeof stored !== 'object') return { ...DEFAULTS }
  const raw = stored as Partial<Settings>
  return {
    font_scale:
      typeof raw.font_scale === 'number' &&
      Number.isFinite(raw.font_scale) &&
      raw.font_scale >= FONT_SCALE_MIN &&
      raw.font_scale <= FONT_SCALE_MAX
        ? raw.font_scale
        : DEFAULTS.font_scale,
    view: raw.view === 'markdown' ? 'markdown' : 'styled',
    enabled_dicts: Array.isArray(raw.enabled_dicts)
      ? raw.enabled_dicts.filter((d): d is string => typeof d === 'string')
      : [],
    previews: typeof raw.previews === 'boolean' ? raw.previews : DEFAULTS.previews,
    show_audio: typeof raw.show_audio === 'boolean' ? raw.show_audio : DEFAULTS.show_audio,
  }
}

export const settings = $state<Settings>(load())

function snapshot(): Settings {
  return {
    font_scale: settings.font_scale,
    view: settings.view,
    enabled_dicts: [...settings.enabled_dicts],
    previews: settings.previews,
    show_audio: settings.show_audio,
  }
}

// Module-level effects need their own root. The disposer is intentionally
// dropped: this store lives for the lifetime of the page.
if (typeof window !== 'undefined') {
  $effect.root(() => {
    $effect(() => {
      storage_set_json(KEY, snapshot())
    })
  })
}

export function reset_settings(): void {
  Object.assign(settings, DEFAULTS)
}

export function is_dict_enabled(name: string): boolean {
  return settings.enabled_dicts.length === 0 || settings.enabled_dicts.includes(name)
}

export { storage_remove, KEY as SETTINGS_KEY }
