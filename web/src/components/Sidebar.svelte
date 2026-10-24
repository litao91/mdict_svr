<script lang="ts">
  import { Accordion, ActionButton, Icon } from 'svelte-widgets'
  import {
    ChevronDown,
    ChevronRight,
    Close,
    Dice,
    Download,
    History,
    Notebook,
    Star,
  } from 'svelte-widgets/icons'
  import { toast } from 'svelte-widgets/toast-queue'
  import { fetch_markdown } from '../lib/api.js'
  import { dicts, display_name } from '../lib/dicts.svelte.js'
  import {
    clear_wordlist,
    download_wordlist,
    format_wordlist,
    toggle_wordlist,
    wordlist,
    type WordlistFormat,
  } from '../lib/wordlist.svelte.js'
  import { clear_history, forget, navigate, random_word, select_dict, words } from '../lib/words.svelte.js'

  let open_section = $state<string | null>('recent')
  let exporting = $state(false)

  const sections = [
    { value: 'recent', label: 'Recent' },
    { value: 'wordlist', label: 'Word list' },
    { value: 'libraries', label: 'Dictionaries' },
  ]

  const section_icon = (value: string) =>
    value === 'recent' ? History : value === 'wordlist' ? Star : Notebook

  /**
   * Anki cards want a back side, so the TSV export fetches each word's Markdown
   * first. Capped: a 500-word list must not fire 500 conversions at once.
   */
  async function export_wordlist(format: WordlistFormat) {
    if (exporting) return
    exporting = true
    try {
      const definitions = new Map<string, string>()
      if (format === 'anki') {
        for (const item of wordlist.items.slice(0, 60)) {
          const scope = words.dict ?? dicts.all[0]?.name ?? ''
          if (!scope) break
          const md = await fetch_markdown(item.word, scope).catch(() => null)
          if (md) {
            definitions.set(
              item.word,
              md.split('\n').map((l) => l.trim()).filter(Boolean).slice(0, 2).join(' '),
            )
          }
        }
      }
      const mime =
        format === 'json' ? 'application/json' : format === 'csv' ? 'text/csv' : 'text/tab-separated-values'
      download_wordlist(
        `mdict-wordlist.${format === 'anki' ? 'txt' : format}`,
        format_wordlist(wordlist.items, format, (w) => definitions.get(w) ?? null),
        mime,
      )
      toast.show(`Word list exported as ${format.toUpperCase()}`, { priority: 'success' })
    } finally {
      exporting = false
    }
  }

  /** Scope the search to one dictionary, or back to all of them. */
  function scope(name: string | null) {
    select_dict(name)
    if (words.word) navigate(words.word, { dict: name, replace: true })
  }

  const total_entries = $derived(dicts.all.reduce((n, d) => n + (d.entries || 0), 0))
</script>

<aside class="sidebar">
  <Accordion
    items={sections}
    collapsible
    heading_level={2}
    value={open_section}
    on_change={(value: string | null) => (open_section = value)}
  >
    {#snippet trigger({ item, open })}
      <span class="side-block__title">
        <Icon icon={section_icon(item.value)} width={13} height={13} />
        {item.label}
        <span class="side-block__chevron">
          <Icon icon={open ? ChevronDown : ChevronRight} width={12} height={12} />
        </span>
      </span>
    {/snippet}

    {#snippet panel({ item })}
      {#if item.value === 'recent'}
        {#if words.recent.length === 0}
          <p class="side-empty">Words you look up collect here.</p>
        {:else}
          <ul class="side-list">
            {#each words.recent as word (word)}
              <li class="side-row">
                <button
                  class="side-link"
                  type="button"
                  aria-current={word === words.word}
                  onclick={() => navigate(word)}
                >
                  <span class="side-link__label">{word}</span>
                </button>
                <button
                  class="side-drop"
                  type="button"
                  title="Forget “{word}”"
                  aria-label="Forget {word}"
                  onclick={() => forget(word)}
                >
                  <Icon icon={Close} width={12} height={12} />
                </button>
              </li>
            {/each}
          </ul>
          <button class="side-clear" type="button" onclick={clear_history}>Clear history</button>
        {/if}

        <div class="side-random">
          <ActionButton
            action={async () => (await random_word()) ?? ''}
            on_success={(picked) => {
              if (picked) navigate(picked)
            }}
            on_error={(error) => {
              toast.show(`Could not pick a random word: ${(error as Error).message}`, {
                priority: 'error',
              })
            }}
            labels={{ ready: 'Random word' }}
            icons={{ ready: Dice }}
          />
        </div>
      {:else if item.value === 'wordlist'}
        {#if wordlist.items.length === 0}
          <p class="side-empty">
            Star a word — from the toolbar or by double-clicking it in an entry — and it is kept
            here.
          </p>
        {:else}
          <ul class="side-list">
            {#each wordlist.items as entry (entry.word)}
              <li class="side-row">
                <button
                  class="side-link"
                  type="button"
                  aria-current={entry.word === words.word}
                  onclick={() => navigate(entry.word)}
                >
                  <span class="side-link__label">{entry.word}</span>
                </button>
                <button
                  class="side-drop"
                  type="button"
                  title="Remove “{entry.word}”"
                  aria-label="Remove {entry.word}"
                  onclick={() => toggle_wordlist(entry.word)}
                >
                  <Icon icon={Close} width={12} height={12} />
                </button>
              </li>
            {/each}
          </ul>
          <div class="side-export">
            <span class="side-export__label">Export</span>
            <button class="side-clear" type="button" disabled={exporting} onclick={() => export_wordlist('anki')}>
              <Icon icon={Download} width={11} height={11} /> Anki
            </button>
            <button class="side-clear" type="button" disabled={exporting} onclick={() => export_wordlist('csv')}>CSV</button>
            <button class="side-clear" type="button" disabled={exporting} onclick={() => export_wordlist('json')}>JSON</button>
            <button class="side-clear" type="button" onclick={clear_wordlist}>Clear</button>
          </div>
        {/if}
      {:else}
        <ul class="side-list">
          {#each dicts.all as d (d.name)}
            <li class="side-row">
              <button
                class="side-link"
                type="button"
                aria-current={words.dict === d.name}
                title={d.title || d.name}
                onclick={() => scope(words.dict === d.name ? null : d.name)}
              >
                <span class="side-link__label">{display_name(d.name)}</span>
                <span class="side-count">{d.entries.toLocaleString()}</span>
              </button>
            </li>
          {/each}
        </ul>
        <p class="side-empty">
          {dicts.all.length} loaded · {total_entries.toLocaleString()} entries. Click to search one
          dictionary only.
        </p>
      {/if}
    {/snippet}
  </Accordion>
</aside>

<style>
  .side-block__title {
    display: flex;
    align-items: center;
    gap: 0.35rem;
  }
  .side-block__chevron {
    margin-left: auto;
    display: grid;
    place-items: center;
    color: var(--fg-faint);
  }
  .side-row {
    display: flex;
    align-items: center;
    gap: 0.1rem;
  }
  .side-row .side-link {
    flex: 1;
    min-width: 0;
  }
  .side-drop {
    flex: none;
    display: grid;
    place-items: center;
    width: 1.4rem;
    height: 1.4rem;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--fg-faint);
    opacity: 0;
    cursor: pointer;
  }
  .side-row:hover .side-drop {
    opacity: 1;
  }
  .side-drop:hover {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .side-count {
    flex: none;
    font-size: 0.7rem;
    color: var(--fg-faint);
    font-variant-numeric: tabular-nums;
  }
  .side-empty {
    margin: 0.25rem 0.5rem;
    font-size: 0.78rem;
    color: var(--fg-faint);
    line-height: 1.5;
  }
  .side-clear {
    margin: 0.35rem 0.5rem 0;
    padding: 0;
    border: 0;
    background: none;
    color: var(--fg-faint);
    font: inherit;
    font-size: 0.75rem;
    text-align: left;
    cursor: pointer;
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  .side-clear:hover {
    color: var(--accent);
  }
  .side-export {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.15rem 0.6rem;
    margin: 0.35rem 0.5rem 0;
  }
  .side-export__label {
    font-size: 0.72rem;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--fg-faint);
  }
  .side-export .side-clear {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
    margin: 0;
  }
  .side-random {
    margin-top: 0.7rem;
    padding: 0 0.35rem;
  }
  /* ActionButton ships its own bordered pill; in the rail it should read as a
     quiet row like the history links above it. */
  .side-random :global(button) {
    width: 100%;
    justify-content: flex-start;
    padding: 0.35rem 0.5rem;
    border-color: transparent;
    background: transparent;
    color: var(--fg-muted);
  }
  .side-random :global(button:hover) {
    background: var(--bg-sunken);
    color: var(--fg);
  }
</style>
