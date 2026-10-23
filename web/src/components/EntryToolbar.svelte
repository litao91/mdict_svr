<script lang="ts">
  import { ButtonGroup, CopyButton, FullscreenButton, Icon, NumberRangeInput } from 'svelte-widgets'
  import { FormatText, Markdown, TextSearch } from 'svelte-widgets/icons'
  import { FONT_SCALE_MAX, FONT_SCALE_MIN, settings } from '../lib/settings.svelte.js'
  import type { EntryView as ViewMode } from '../lib/settings.svelte.js'

  let {
    view = $bindable<ViewMode>('styled'),
    find_open = $bindable(false),
    markdown = null as string | null,
    word,
    dict,
    fullscreen_target = null as HTMLElement | null,
  }: {
    view?: ViewMode
    find_open?: boolean
    markdown?: string | null
    word: string
    dict: string
    fullscreen_target?: HTMLElement | null
  } = $props()

  const view_options = [
    { value: 'styled' as const, label: 'Entry', icon: FormatText },
    { value: 'markdown' as const, label: 'Markdown', icon: Markdown },
  ]
</script>

<div class="toolbar">
  <ButtonGroup options={view_options} label="Entry format" bind:value={view} />

  <button
    class="tool-btn"
    type="button"
    title="Find in entry (opens a search bar scoped to this definition)"
    aria-pressed={find_open}
    onclick={() => (find_open = !find_open)}
  >
    <Icon icon={TextSearch} width={16} height={16} />
  </button>

  <CopyButton
    class="tool-btn"
    content={markdown ?? `Loading the Markdown for “${word}”…`}
    disabled={markdown === null}
    title={markdown === null ? 'Markdown still loading' : `Copy “${word}” from ${dict} as Markdown`}
  />

  <FullscreenButton wrapper={fullscreen_target ?? undefined} placement="inline" class="tool-btn" />

  <div class="toolbar__spacer"></div>

  <div class="font-scale">
    <NumberRangeInput
      value={settings.font_scale}
      min={FONT_SCALE_MIN}
      max={FONT_SCALE_MAX}
      step={0.05}
      label="Text size"
      commit="change"
      title="Definition text size"
      on_commit={(value) => {
        if (typeof value === 'number') settings.font_scale = value
      }}
    />
  </div>
</div>

<style>
  .tool-btn {
    display: grid;
    place-items: center;
    min-width: 2rem;
    height: 2rem;
    padding: 0 0.4rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--fg-muted);
    cursor: pointer;
    transition: color 0.12s, border-color 0.12s, background-color 0.12s;
  }
  .tool-btn:hover:not(:disabled) {
    color: var(--fg);
    border-color: var(--border-strong);
  }
  .tool-btn:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .tool-btn[aria-pressed='true'] {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .font-scale {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 11rem;
  }
</style>
