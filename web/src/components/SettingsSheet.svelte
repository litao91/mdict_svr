<script lang="ts">
  import { ButtonGroup, Icon, MultiSelect, NumberRangeInput, Sheet, Toggle } from 'svelte-widgets'
  import { Close } from 'svelte-widgets/icons'
  import { apply_theme_mode, theme } from 'svelte-widgets/theme'
  import { dicts, display_name } from '../lib/dicts.svelte.js'
  import { FONT_SCALE_MAX, FONT_SCALE_MIN, reset_settings, settings } from '../lib/settings.svelte.js'
  import { clear_history, words } from '../lib/words.svelte.js'

  let { open = $bindable(false) }: { open?: boolean } = $props()

  const theme_options = [
    { value: 'light' as const, label: 'Light' },
    { value: 'system' as const, label: 'System' },
    { value: 'dark' as const, label: 'Dark' },
  ]

  const view_options = [
    { value: 'styled' as const, label: 'Entry' },
    { value: 'markdown' as const, label: 'Markdown' },
  ]

  const dict_options = $derived(dicts.all.map((d) => d.name))

  /**
   * MultiSelect's own value, kept separate from `settings.enabled_dicts` because
   * an empty selection means "all of them" — a distinction the widget cannot
   * express. Deselecting everything therefore reads back as "all".
   */
  let selected_dicts = $state<string[]>([])
  let hydrated = false

  $effect(() => {
    if (hydrated) return
    if (!dicts.loaded) return
    hydrated = true
    selected_dicts =
      settings.enabled_dicts.length === 0 ? [...dict_options] : [...settings.enabled_dicts]
  })

  $effect(() => {
    if (!hydrated) return
    settings.enabled_dicts =
      selected_dicts.length === 0 || selected_dicts.length === dict_options.length
        ? []
        : [...selected_dicts]
  })
</script>

<Sheet bind:open side="right" backdrop_dim backdrop_blur>
  {#snippet header({ close })}
    <div class="sheet-head">
      <h2 class="sheet-title">Settings</h2>
      <button class="sheet-close" type="button" aria-label="Close settings" onclick={close}>
        <Icon icon={Close} width={16} height={16} />
      </button>
    </div>
  {/snippet}

  {#snippet children()}
    <div class="settings-body">
      <div class="settings-group">
        <h3 class="settings-group__title">Appearance</h3>

        <div class="settings-row">
          <div class="settings-row__head">
            <span class="settings-row__label">Theme</span>
          </div>
          <ButtonGroup
            options={theme_options}
            label="Theme"
            value={theme.mode}
            on_change={(value: 'light' | 'dark' | 'system' | null) => {
              if (value) apply_theme_mode(value)
            }}
          />
        </div>

        <div class="settings-row">
          <div class="settings-row__head">
            <span class="settings-row__label">Text size</span>
          </div>
          <NumberRangeInput
            value={settings.font_scale}
            min={FONT_SCALE_MIN}
            max={FONT_SCALE_MAX}
            step={0.05}
            label="Text size"
            commit="change"
            on_commit={(value) => {
              if (typeof value === 'number') settings.font_scale = value
            }}
          />
          <p class="settings-row__hint">Scales the definition text only, not the interface.</p>
        </div>
      </div>

      <div class="settings-group">
        <h3 class="settings-group__title">Reading</h3>

        <div class="settings-row">
          <div class="settings-row__head">
            <span class="settings-row__label">Default view</span>
          </div>
          <ButtonGroup options={view_options} label="Default view" bind:value={settings.view} />
          <p class="settings-row__hint">
            “Entry” renders the publisher's markup, styled here. “Markdown” asks the server for its
            plain-text conversion instead.
          </p>
        </div>

        <div class="settings-row">
          <div class="settings-row__head">
            <span class="settings-row__label">Hover previews</span>
            <Toggle bind:checked={settings.previews}>
              {#snippet children()}<span class="sr-only">Hover previews</span>{/snippet}
            </Toggle>
          </div>
          <p class="settings-row__hint">
            Hover a cross-reference to see the first lines of that entry without leaving this one.
          </p>
        </div>
      </div>

      <div class="settings-group">
        <h3 class="settings-group__title">Dictionaries</h3>

        <div class="settings-row">
          <div class="settings-row__head">
            <span class="settings-row__label">Search these</span>
          </div>
          <MultiSelect
            options={dict_options}
            mode="multiple"
            bind:value={selected_dicts}
            placeholder="All dictionaries"
          >
            {#snippet children({ option })}
              <!-- Dictionary names run to 40+ characters; without a truncating
                   wrapper the chip's `white-space: nowrap` widens the whole
                   sheet past the viewport. -->
              <span class="ms-label" title={String(option)}>{option}</span>
            {/snippet}
          </MultiSelect>
          <p class="settings-row__hint">
            {settings.enabled_dicts.length === 0
              ? `Searching all ${dict_options.length} dictionaries.`
              : `Searching ${settings.enabled_dicts.length} of ${dict_options.length}.`}
            Deselecting everything is the same as selecting all of it.
            {#if words.dict}
              Currently scoped to <strong>{display_name(words.dict)}</strong> — pick a different
              dictionary in the sidebar to widen it.
            {/if}
          </p>
        </div>
      </div>

      <div class="settings-group">
        <h3 class="settings-group__title">Data</h3>
        <div class="settings-actions">
          <button class="sheet-btn" type="button" onclick={clear_history}>
            Clear look-up history ({words.recent.length})
          </button>
          <button class="sheet-btn" type="button" onclick={reset_settings}>
            Reset settings to defaults
          </button>
        </div>
      </div>
    </div>
  {/snippet}
</Sheet>

<style>
  .sheet-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }
  .sheet-title {
    margin: 0;
    font-size: 1.05rem;
    font-weight: 650;
  }
  .sheet-close {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
  }
  .sheet-close:hover {
    color: var(--fg);
    border-color: var(--border-strong);
  }
  .settings-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }
  .sheet-btn {
    padding: 0.4rem 0.75rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-raised);
    color: var(--fg-muted);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
  }
  .sheet-btn:hover {
    color: var(--fg);
    border-color: var(--border-strong);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }
  /* MultiSelect chips are `white-space: nowrap` flex items, so a 40-character
     dictionary name widens the sheet past the viewport unless the chip and its
     label are both allowed to shrink. */
  :global(.multiselect),
  :global(.multiselect > ul.selected),
  :global(.multiselect > ul.selected > li) {
    min-width: 0;
    max-width: 100%;
  }
  .ms-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
