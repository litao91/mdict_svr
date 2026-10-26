<script lang="ts">
  import {
    Icon,
    PrevNext,
    Progress,
    Spinner,
    StatusMessage,
    SplitPane,
    Tabs,
    TaskStatus,
    Toc,
  } from 'svelte-widgets'
  import { BookSearch, Headphones, LinkVariant, MusicOff } from 'svelte-widgets/icons'
  import EntryToolbar from './EntryToolbar.svelte'
  import EntryView from './EntryView.svelte'
  import {
    fetch_entry,
    fetch_nearby,
    fetch_suggest_full,
    type LookupOutcome,
  } from '../lib/api.js'
  import { dicts, display_name, has_resources, info as dict_info, load_dicts, ordered_enabled } from '../lib/dicts.svelte.js'
  import { sanitize_entry, type SanitizedEntry } from '../lib/sanitize.js'
  import { settings } from '../lib/settings.svelte.js'
  import { navigate, select_dict, words } from '../lib/words.svelte.js'

  let {
    on_image,
  }: {
    on_image: (src: string, alt: string) => void
  } = $props()

  type Result =
    | { status: 'loading' }
    | { status: 'ok'; html: string }
    | { status: 'miss' }
    | { status: 'error'; message: string }

  let results = $state<Record<string, Result>>({})
  let tab = $state<string | null>(null)
  let loading = $state(false)
  let settled = $state(0)
  let total = $state(0)
  let not_found = $state(false)
  let find_open = $state(false)
  let entry_wrap: HTMLElement | null = $state(null)
  let nearby = $state<{ prev: string[]; next: string[] } | null>(null)
  let did_you_mean = $state<string[]>([])

  // Memo caches. Plain Maps, not $state: they are never rendered from directly.
  const entry_cache = new Map<string, SanitizedEntry>()
  let controller: AbortController | null = null
  let request_id = 0

  // Tab order is the reader's order, not the order the fetches happened to
  // settle in, so reordering the sidebar rearranges open tabs immediately.
  const tab_items = $derived(
    ordered_enabled()
      .filter((d) => results[d.name]?.status === 'ok')
      .map((d) => ({ value: d.name, label: display_name(d.name) })),
  )

  const entry = $derived.by<SanitizedEntry | null>(() => {
    const word = words.word
    const name = tab
    if (!word || !name) return null
    const result = results[name]
    if (!result || result.status !== 'ok') return null
    const key = `${word}\u0000${name}`
    let cached = entry_cache.get(key)
    if (!cached) {
      cached = sanitize_entry(result.html, name, has_resources(name))
      entry_cache.set(key, cached)
    }
    return cached
  })

  const toc_items = $derived(
    entry && entry.headings.length >= 2
      ? entry.headings.map((h) => ({ id: h.id, level: h.level, title: h.text }))
      : [],
  )

  /** One TaskStatus row per searched dictionary, while the fan-out is in flight. */
  const task_rows = $derived(
    Object.entries(results).map(([name, r]) => ({
      name,
      label:
        r.status === 'miss' ? `${display_name(name)} — no match` : display_name(name),
      state:
        r.status === 'loading'
          ? ('running' as const)
          : r.status === 'ok'
            ? ('success' as const)
            : r.status === 'miss'
              ? ('cancelled' as const)
              : ('error' as const),
    })),
  )

  const errors = $derived.by(() => {
    const out: string[] = []
    for (const [name, r] of Object.entries(results)) {
      if (r.status === 'error') out.push(`${display_name(name)}: ${r.message}`)
    }
    return out
  })

  const audio_summary = $derived.by(() => {
    if (!entry) return null
    const { playable_audio, unsupported_audio } = entry
    if (playable_audio === 0 && unsupported_audio === 0) return null
    return { playable_audio, unsupported_audio }
  })

  /**
   * Fetch every enabled dictionary in parallel so the tab list is known, but
   * sanitize only the selected one — a single OxEnCh8 entry is 237 KB of markup
   * and parsing nine of them would dominate the interaction.
   */
  async function run(word: string) {
    const id = ++request_id
    controller?.abort()
    const ctrl = new AbortController()
    controller = ctrl

    if (!dicts.loaded) await load_dicts()
    if (id !== request_id) return

    const targets = ordered_enabled().map((d) => d.name)
    const next: Record<string, Result> = {}
    for (const name of targets) next[name] = { status: 'loading' }
    results = next
    total = targets.length
    settled = 0
    loading = targets.length > 0
    if (targets.length === 0) {
      not_found = false
      tab = null
      return
    }

    await Promise.all(
      targets.map(async (name) => {
        let outcome: LookupOutcome
        try {
          outcome = await fetch_entry(word, name, ctrl.signal)
        } catch (err) {
          if ((err as Error)?.name === 'AbortError') return
          outcome = { kind: 'error', dict: name, message: 'server unreachable' }
        }
        if (id !== request_id) return
        results[name] =
          outcome.kind === 'ok'
            ? { status: 'ok', html: outcome.html }
            : outcome.kind === 'miss'
              ? { status: 'miss' }
              : { status: 'error', message: outcome.message }
        settled += 1
        // First paint must not wait for the slowest of N dictionaries: show
        // the first match that lands (preferring the requested dictionary),
        // and let the tab bar fill in as the rest settle.
        if (outcome.kind === 'ok') {
          const shown = tab
          const shown_ok = shown !== null && results[shown]?.status === 'ok'
          if (!shown_ok && (name === words.dict || shown === null)) tab = name
        }
      }),
    )
    if (id !== request_id) return

    loading = false
    const matched = targets.filter((n) => results[n]?.status === 'ok')
    not_found = matched.length === 0
    const wanted = words.dict && matched.includes(words.dict) ? words.dict : (matched[0] ?? null)
    tab = wanted
    // Keep the URL in step with the tab, without adding a history entry.
    if (wanted && wanted !== words.dict) select_dict(wanted)
  }

  $effect(() => {
    const word = words.word
    // Switching a dictionary off or reordering them must refresh what is on
    // screen, not just the next lookup, so both are dependencies of the run.
    const selection = settings.enabled_dicts.join('\u0001')
    const order = settings.dict_order.join('\u0001')
    void selection
    void order
    if (!word) {
      results = {}
      tab = null
      not_found = false
      loading = false
      nearby = null
      did_you_mean = []
      return
    }
    void run(word)
  })

  // The immediate neighbours come straight from the dictionary's sorted index,
  // scoped to the tab being read: exact, and never "unknown" the way the old
  // prefix-shrink through /suggest was.
  const neighbours = $derived<{ prev: string | null; next: string | null }>({
    prev: nearby?.prev.at(-1) ?? null,
    next: nearby?.next[0] ?? null,
  })

  $effect(() => {
    const word = words.word
    const name = tab
    if (!word || !name) {
      nearby = null
      return
    }
    let cancelled = false
    void fetch_nearby(word, { dict: name, before: 4, after: 4 })
      .then((found) => {
        if (!cancelled) nearby = found ? { prev: found.prev, next: found.next } : null
      })
      .catch(() => {
        if (!cancelled) nearby = null
      })
    return () => {
      cancelled = true
    }
  })

  // A miss is usually a typo, and a typo is otherwise a dead end.
  $effect(() => {
    const word = words.word
    if (!not_found || loading || !word) {
      did_you_mean = []
      return
    }
    let cancelled = false
    void fetch_suggest_full(word, { fuzzy: true, limit: 6 })
      .then((r) => {
        if (!cancelled) did_you_mean = r.fuzzy.filter((w) => w !== word).slice(0, 5)
      })
      .catch(() => {
        if (!cancelled) did_you_mean = []
      })
    return () => {
      cancelled = true
    }
  })

  /** PrevNext renders real anchors; intercept so navigation stays in-app. */
  function on_nav_click(event: MouseEvent) {
    if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey || event.button !== 0) return
    const href = (event.currentTarget as HTMLAnchorElement | null)?.getAttribute('href')
    if (!href) return
    const word = new URL(href, window.location.origin).searchParams.get('word')
    if (!word) return
    event.preventDefault()
    navigate(word)
  }

  type NavItem = { href: string; label: string; word: string | null }

  // PrevNext locates `current` by href inside `items` and wraps around the ends,
  // so it always gets a three-slot list with the current word in the middle. A
  // missing neighbour is a `word: null` placeholder the snippet renders as
  // nothing — that keeps index/total stable instead of wrapping prev onto next.
  const nav_current = $derived(`/?word=${encodeURIComponent(words.word)}`)

  const nav_items = $derived<NavItem[]>([
    neighbours.prev
      ? { href: `/?word=${encodeURIComponent(neighbours.prev)}`, label: neighbours.prev, word: neighbours.prev }
      : { href: '', label: '', word: null },
    { href: nav_current, label: words.word, word: words.word },
    neighbours.next
      ? { href: `/?word=${encodeURIComponent(neighbours.next)}`, label: neighbours.next, word: neighbours.next }
      : { href: '', label: '', word: null },
  ])
</script>

<div class="word-view">
  <div class="word-head">
    <h1 class="word-head__title">{words.word}</h1>
    {#if tab_items.length > 1}
      <span class="word-head__count">in {tab_items.length} of {total} dictionaries</span>
    {/if}
  </div>

  {#if nearby && (nearby.prev.length > 0 || nearby.next.length > 0)}
    <nav class="nearby" aria-label="Words nearby">
      {#each [...nearby.prev, ...nearby.next] as w (w)}
        <button class="nearby__chip" type="button" onclick={() => navigate(w)}>{w}</button>
      {/each}
    </nav>
  {/if}

  {#if loading && total > 0}
    <Progress
      class="load-progress"
      value={settled}
      max={total}
      label="Searching dictionaries"
    />
    <div class="lookup-tasks">
      {#each task_rows as row (row.name)}
        <TaskStatus state={row.state} label={row.label} />
      {/each}
    </div>
  {/if}

  {#if errors.length > 0}
    <StatusMessage type="warning" message={errors.join(' · ')} />
  {/if}

  {#if not_found && !loading}
    <div class="empty">
      <Icon icon={BookSearch} width={34} height={34} />
      <p><strong>“{words.word}” is not in any searched dictionary.</strong></p>
      {#if did_you_mean.length > 0}
        <p class="empty__didyoumean">
          Did you mean
          {#each did_you_mean as candidate, i (candidate)}
            {#if i > 0}, {/if}
            <button class="nearby__chip nearby__chip--guess" type="button" onclick={() => navigate(candidate)}>
              {candidate}
            </button>
          {/each}
          ?
        </p>
      {:else}
        <p class="empty__hint">
          {#if total < dicts.all.length}
            Only {total} of {dicts.all.length} dictionaries are switched on — enable more in
            settings.
          {:else}
            Try a different spelling, or check that the dictionaries were imported with their
            companion <code>.mdd</code> files.
          {/if}
        </p>
      {/if}
    </div>
  {:else if tab_items.length > 0}
    <Tabs
      items={tab_items}
      value={tab ?? undefined}
      on_change={(value) => {
        tab = value
        select_dict(value)
      }}
      label="Dictionaries with this word"
    >
      {#snippet panel({ item })}
        {#if item.value === tab}
          <EntryToolbar
            bind:find_open
            word={words.word}
            dict={item.value}
            fullscreen_target={entry_wrap}
          />

          <div class="word-meta">
            <span class="badge" title={dict_info(item.value)?.source ?? ''}>
              <Icon icon={BookSearch} width={12} height={12} />
              {dict_info(item.value)?.entries?.toLocaleString() ?? '?'} entries
            </span>
            {#if audio_summary}
              <span
                class="badge"
                class:badge--on={audio_summary.playable_audio > 0}
                title={
                  audio_summary.unsupported_audio > 0
                    ? `${audio_summary.unsupported_audio} pronunciation file(s) use a codec no browser can decode`
                    : 'Pronunciation audio available'
                }
              >
                <Icon
                  icon={audio_summary.playable_audio > 0 ? Headphones : MusicOff}
                  width={12}
                  height={12}
                />
                {audio_summary.playable_audio > 0
                  ? `${audio_summary.playable_audio} audio`
                  : 'audio unsupported'}
              </span>
            {/if}
            {#if entry && entry.word_links > 0}
              <span class="badge" title="Cross-references that open another headword">
                <Icon icon={LinkVariant} width={12} height={12} />
                {entry.word_links} links
              </span>
            {/if}
            {#if entry && entry.unsupported_audio > 0}
              <span class="badge" title="Ogg-Speex or another codec browsers cannot play">
                <Icon icon={MusicOff} width={12} height={12} />
                {entry.unsupported_audio} unplayable
              </span>
            {/if}
          </div>

          <div class="entry-columns" class:entry-columns--solo={toc_items.length < 2}>
            <div class="entry-wrap" bind:this={entry_wrap}>
              {#if entry}
                <EntryView {entry} dict={item.value} word={words.word} bind:find_open {on_image} on_word={navigate} />
              {/if}
            </div>

            {#if toc_items.length >= 2}
              <aside class="entry-toc">
                <Toc items={toc_items} desktop={true} min_items={2} />
              </aside>
              <!-- Divider only: it writes --split-pane-size onto .entry-columns,
                   which is what sizes the two grid tracks. -->
              <SplitPane
                orientation="horizontal"
                ratio={0.78}
                min_ratio={0.55}
                max_ratio={0.88}
                aria-label="Resize the entry and its outline"
              />
            {/if}
          </div>
        {/if}
      {/snippet}
    </Tabs>

    {#if neighbours.prev || neighbours.next}
      <div class="word-nav">
        <PrevNext items={nav_items} current={nav_current}>
          {#snippet children({ item, kind })}
            {#if item.word}
              <div class={kind}>
                <span>{kind === 'prev' ? 'Previous' : 'Next'}</span>
                <a
                  href={item.href}
                  title="Alphabetical neighbour"
                  onclick={on_nav_click}
                >{item.label}</a>
              </div>
            {/if}
          {/snippet}
        </PrevNext>
      </div>
    {/if}
  {:else if loading}
    <div class="spinner-wrap"><Spinner text="Looking up “{words.word}”…" /></div>
  {/if}
</div>

<style>
  .word-view {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
  }
  .word-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
    margin-bottom: 0.9rem;
  }
  .nearby {
    display: flex;
    flex-wrap: wrap;
    gap: 0.3rem;
    margin: -0.2rem 0 0.6rem;
  }
  .nearby__chip {
    padding: 0.12rem 0.55rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg-raised);
    color: var(--fg-muted);
    font: inherit;
    font-size: 0.76rem;
    cursor: pointer;
    transition: color 0.12s, border-color 0.12s, background-color 0.12s;
  }
  .nearby__chip:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .nearby__chip--guess {
    color: var(--accent);
    font-weight: 600;
  }
  .empty__didyoumean {
    margin: 0.4rem 0 0;
    color: var(--fg-muted);
  }
  .lookup-tasks {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(13rem, 1fr));
    gap: 0.25rem 1.2rem;
    margin-top: 0.5rem;
  }

  .entry-columns {
    position: relative;
    display: grid;
    grid-template-columns: var(--split-pane-size, minmax(0, 1fr)) 15rem;
    gap: 0 1.4rem;
    align-items: start;
  }
  .entry-columns--solo {
    grid-template-columns: minmax(0, 1fr);
  }
  .entry-wrap {
    min-width: 0;
  }
  .entry-toc {
    position: sticky;
    top: 1rem;
    min-width: 0;
    max-height: calc(100vh - 12rem);
    overflow-y: auto;
    font-size: 0.82rem;
  }
  .word-nav {
    margin-top: 2rem;
    padding-top: 1rem;
    border-top: 1px solid var(--border);
  }
  @media (max-width: 1100px) {
    .entry-toc {
      display: none;
    }
  }

  /* Below the ToC's breakpoint the second grid track and the divider would
     squeeze the entry into a column; the outline is hidden, so collapse. */
  @media (max-width: 1100px) {
    .entry-columns {
      grid-template-columns: minmax(0, 1fr);
    }
    .entry-columns :global(.pane-divider) {
      display: none;
    }
  }

  /* Eleven dictionary names do not fit a phone: scroll the tab row instead of
     stacking it into a four-line block. */
  @media (max-width: 700px) {
    .word-view :global(.tabs-list) {
      flex-wrap: nowrap;
      overflow-x: auto;
      scrollbar-width: thin;
    }
    .word-view :global(.tabs-tab) {
      flex: none;
      white-space: nowrap;
    }
    .nearby {
      max-height: 4.6rem;
      overflow-y: auto;
    }
  }
</style>
