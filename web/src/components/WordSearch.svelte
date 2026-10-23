<script lang="ts" module>
  const is_mac =
    typeof navigator !== 'undefined' && /mac|iphone|ipad|ipod/i.test(navigator.platform ?? navigator.userAgent)
  const id_list = 'word-suggest'
</script>

<script lang="ts">
  import { onMount } from 'svelte'
  import { Icon } from 'svelte-widgets'
  import { Close, Search } from 'svelte-widgets/icons'
  import { fetch_suggest } from '../lib/api.js'
  import { navigate } from '../lib/words.svelte.js'

  let {
    placeholder = 'Look up a word…',
    focus_on_mount = false,
    dict = null as string | null,
    class: klass = '',
  }: {
    placeholder?: string
    /**
     * Focus the field once mounted. Deliberately not the `autofocus` attribute:
     * that fires before the reader knows where they are and cannot be made
     * conditional on anything.
     */
    focus_on_mount?: boolean
    /** Scope suggestions to one dictionary; null searches all of them. */
    dict?: string | null
    class?: string
  } = $props()

  onMount(() => {
    if (focus_on_mount) input_el?.focus()
  })

  let input_el: HTMLInputElement | undefined = $state()
  let text = $state('')
  let results = $state<string[]>([])
  let open = $state(false)
  let active = $state(0)
  let busy = $state(false)

  let controller: AbortController | null = null
  let timer: ReturnType<typeof setTimeout> | null = null

  const show_list = $derived(open && text.trim().length > 0)

  function schedule(query: string) {
    if (timer) clearTimeout(timer)
    const q = query.trim()
    if (!q) {
      results = []
      busy = false
      return
    }
    busy = true
    timer = setTimeout(() => void load(q), 110)
  }

  async function load(query: string) {
    controller?.abort()
    controller = new AbortController()
    try {
      const list = await fetch_suggest(query, { dict: dict ?? undefined, limit: 12, signal: controller.signal })
      // A slower response for an older query must not overwrite a newer one.
      if (query === text.trim()) {
        results = list
        active = Math.max(0, list.indexOf(query))
      }
    } catch (err) {
      if ((err as Error)?.name !== 'AbortError') results = []
    } finally {
      busy = false
    }
  }

  function choose(word: string) {
    const picked = word.trim()
    if (!picked) return
    close()
    navigate(picked)
  }

  function close() {
    open = false
    if (timer) clearTimeout(timer)
    controller?.abort()
  }

  function on_keydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      close()
      return
    }
    if (!show_list) {
      if (event.key === 'Enter') choose(text)
      return
    }
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      active = (active + 1) % results.length
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      active = (active - 1 + results.length) % results.length
    } else if (event.key === 'Enter') {
      event.preventDefault()
      choose(results[active] ?? text)
    } else if (event.key === 'Tab') {
      close()
    }
  }

  /** Split a suggestion so the typed prefix can be highlighted. */
  function parts(word: string, query: string): [string, string] {
    const q = query.trim().toLowerCase()
    const i = word.toLowerCase().indexOf(q)
    return i === 0 ? [word.slice(0, q.length), word.slice(q.length)] : ['', word]
  }
</script>

<div class="search {klass}">
  <div class="search__field">
    <Icon icon={Search} width={16} height={16} />
    <input
      class="search__input"
      bind:this={input_el}
      bind:value={text}
      {placeholder}
      type="search"
      role="combobox"
      aria-expanded={show_list}
      aria-controls={id_list}
      aria-autocomplete="list"
      aria-activedescendant={show_list && results[active] ? `${id_list}-${active}` : undefined}
      oninput={() => {
        open = true
        schedule(text)
      }}
      onfocus={() => {
        open = true
        if (text.trim()) schedule(text)
      }}
      onkeydown={on_keydown}
    />
    {#if text}
      <button
        class="search__clear"
        type="button"
        aria-label="Clear"
        onclick={() => {
          text = ''
          results = []
          input_el?.focus()
        }}
      >
        <Icon icon={Close} width={14} height={14} />
      </button>
    {/if}
    <kbd class="search__kbd">{is_mac ? '⌘' : 'Ctrl'} K</kbd>
  </div>

  {#if show_list}
    <ul class="search__results" id={id_list} role="listbox" aria-label="Suggestions">
      {#each results as word, i (word)}
        {@const [hit, rest] = parts(word, text)}
        <li role="option" id="{id_list}-{i}" aria-selected={i === active}>
          <button
            class="search__result"
            type="button"
            onmouseenter={() => (active = i)}
            onclick={() => choose(word)}
          >
            <mark>{hit}</mark>{rest}
          </button>
        </li>
      {/each}
      {#if !busy && results.length === 0}
        <li class="search__empty">No headword starts with “{text.trim()}”</li>
      {/if}
    </ul>
  {/if}
</div>

<style>
  .search__clear {
    flex: none;
    display: grid;
    place-items: center;
    padding: 0.1rem;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--fg-faint);
    cursor: pointer;
    line-height: 0;
  }
  .search__clear:hover {
    color: var(--fg);
    background: var(--bg-sunken);
  }
</style>
