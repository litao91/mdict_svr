<script lang="ts">
  import { Accordion, ActionButton, Icon } from 'svelte-widgets'
  import { ChevronDown, ChevronRight, Close, Dice, History, Notebook } from 'svelte-widgets/icons'
  import { toast } from 'svelte-widgets/toast-queue'
  import { fetch_suggest } from '../lib/api.js'
  import { dicts, display_name } from '../lib/dicts.svelte.js'
  import { clear_history, forget, navigate, select_dict, words } from '../lib/words.svelte.js'

  let open_section = $state<string | null>('recent')

  const sections = [
    { value: 'recent', label: 'Recent' },
    { value: 'libraries', label: 'Dictionaries' },
  ]

  /**
   * `/suggest` is a forward-only prefix scan, so there is no "random entry"
   * endpoint. Pick a random prefix, take a random item from the window it
   * returns, and retry on a dead prefix.
   */
  async function random_word(): Promise<string> {
    const letters = 'abcdefghijklmnopqrstuvwxyz'
    const pick = () => letters[Math.floor(Math.random() * letters.length)]!
    for (let attempt = 0; attempt < 6; attempt++) {
      const prefix = attempt < 3 ? pick() : pick() + pick()
      const list = await fetch_suggest(prefix, { limit: 100 })
      const usable = list.filter((w) => /^[a-z]/i.test(w))
      if (usable.length > 0) return usable[Math.floor(Math.random() * usable.length)]!
    }
    throw new Error('no headword found')
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
        <Icon icon={item.value === 'recent' ? History : Notebook} width={13} height={13} />
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
            action={random_word}
            on_success={(word) => navigate(word)}
            on_error={(error) => {
              toast.show(`Could not pick a random word: ${(error as Error).message}`, {
                priority: 'error',
              })
            }}
            labels={{ ready: 'Random word' }}
            icons={{ ready: Dice }}
          />
        </div>
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
  .side-random {
    margin-top: 0.7rem;
    padding: 0 0.35rem;
  }
</style>
