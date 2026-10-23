<script lang="ts">
  import {
    ButtonGroup,
    ColorInput,
    Icon,
    JsonTree,
    MultiSelect,
    NumberRangeInput,
    SettingsGroup,
    SettingsSection,
    Sheet,
    Toggle,
  } from 'svelte-widgets'
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

  // Mirrors the `--accent` tokens in styles/base.css; picking either one again
  // clears the override rather than pinning the theme's own color.
  const THEME_ACCENTS = ['#8c4a2f', '#e0906a']
  const ACCENT_PRESETS = [
    '#8c4a2f',
    '#e0906a',
    '#2f6f8c',
    '#4a7a3a',
    '#7a3a5e',
    '#8c6a2f',
    '#3a4a7a',
    '#8c2f3a',
  ]

  const theme_accent = $derived(theme.mode === 'dark' ? THEME_ACCENTS[1] : THEME_ACCENTS[0])

  const dict_options = $derived(dicts.all.map((d) => d.name))

  const server_snapshot = $derived({
    dictionaries: dicts.all,
    transcode: dicts.transcode,
  })

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
      <SettingsGroup title="Appearance" open>
        <SettingsSection title="Theme">
          <ButtonGroup
            options={theme_options}
            label="Theme"
            value={theme.mode}
            on_change={(value: 'light' | 'dark' | 'system' | null) => {
              if (value) apply_theme_mode(value)
            }}
          />
        </SettingsSection>

        <SettingsSection title="Accent colour">
          <ColorInput
            value={settings.accent ?? theme_accent}
            presets={ACCENT_PRESETS}
            commit="change"
            on_commit={(value) => {
              const picked = value.trim().toLowerCase()
              settings.accent = !picked || THEME_ACCENTS.includes(picked) ? null : picked
            }}
          />
          <p class="settings-row__hint">
            Tints links, tabs and highlights. Picking the theme's own colour clears the override.
          </p>
        </SettingsSection>

        <SettingsSection title="Text size">
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
        </SettingsSection>
      </SettingsGroup>

      <SettingsGroup title="Reading" open>
        <SettingsSection title="Default view">
          <ButtonGroup options={view_options} label="Default view" bind:value={settings.view} />
          <p class="settings-row__hint">
            “Entry” renders the publisher's markup, styled here. “Markdown” asks the server for its
            plain-text conversion instead.
          </p>
        </SettingsSection>

        <SettingsSection title="Hover previews">
          <div class="settings-row__head">
            <Toggle bind:checked={settings.previews}>
              {#snippet children()}<span class="sr-only">Hover previews</span>{/snippet}
            </Toggle>
          </div>
          <p class="settings-row__hint">
            Hover a cross-reference to see the first lines of that entry without leaving this one.
          </p>
        </SettingsSection>
      </SettingsGroup>

      <SettingsGroup title="Dictionaries" open>
        <SettingsSection title="Search these">
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
        </SettingsSection>
      </SettingsGroup>

      <SettingsGroup title="Data & server">
        <SettingsSection title="Look-up history">
          <div class="settings-actions">
            <button class="sheet-btn" type="button" onclick={clear_history}>
              Clear look-up history ({words.recent.length})
            </button>
            <button class="sheet-btn" type="button" onclick={reset_settings}>
              Reset settings to defaults
            </button>
          </div>
        </SettingsSection>

        <SettingsSection title="Server metadata">
          <p class="settings-row__hint">
            What <code>/dicts</code> reports, including whether this server can transcode audio
            ({dicts.transcode ? 'yes' : 'no'}).
          </p>
          <div class="json-pane">
            <JsonTree
              value={server_snapshot}
              root_label="server"
              default_fold_level={1}
              download_filename="mdict-dicts.json"
            />
          </div>
        </SettingsSection>
      </SettingsGroup>
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
  .json-pane {
    max-height: 16rem;
    overflow: auto;
    padding: 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-sunken);
    font-size: 0.75rem;
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
  /* SettingsGroup/SettingsSection insert <details>/<section> wrappers between
     the body and the controls; without min-width:0 on every link of that chain
     the MultiSelect's nowrap chips push the sheet past the viewport. */
  .settings-body > :global(*),
  .settings-body :global(details),
  .settings-body :global(section) {
    min-width: 0;
    max-width: 100%;
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
