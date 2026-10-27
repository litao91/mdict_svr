<script lang="ts">
  import { untrack } from 'svelte'
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
    cached_lookup,
    fetch_entries_except,
    fetch_entry,
    fetch_nearby,
    fetch_suggest_full,
    prefetch_entry,
    remember_lookup,
    type CombinedOutcome,
    type LookupOutcome,
  } from '../lib/api.js'
  import { boot } from '../lib/boot.js'
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
  /**
   * The word `results` was fetched for. Effects flush *after* the render that
   * follows a state change, so without this tag that render sanitizes the
   * previous word's HTML under the new word — and caches it there, which makes
   * the wrong entry stick rather than flash.
   */
  let results_word = $state('')
  let nearby_word = $state('')
  // Bound both ways: Tabs normalizes `value` into its item list internally
  // (an absent value snaps to the first tab), and a one-way prop lets that
  // snap diverge from the state that decides which panel renders.
  let tab = $state<string | undefined>(undefined)
  let loading = $state(false)
  let settled = $state(0)
  let total = $state(0)
  let not_found = $state(false)
  let find_open = $state(false)
  let entry_wrap: HTMLElement | null = $state(null)
  let nearby = $state<{ prev: string[]; next: string[] } | null>(null)
  let did_you_mean = $state<string[]>([])

  // Memo caches. Plain Maps, not $state: they are never rendered from directly.
  // Bounded because a session reads hundreds of words and a sanitized entry is
  // the size of the publisher's markup; a miss only costs one re-sanitize.
  const ENTRY_CACHE_MAX = 24
  const entry_cache = new Map<string, SanitizedEntry>()
  let controller: AbortController | null = null
  let request_id = 0

  // Tab order is the reader's order, not the order the fetches happened to
  // settle in, so reordering the sidebar rearranges open tabs immediately.
  const fresh = $derived(results_word === words.word && results_word !== '')

  // Deliberately NOT gated on `fresh`: handing Tabs an empty list for the one
  // stale frame desyncs its internal selection from `value`, leaving the bar
  // pointing at a dictionary whose panel is hidden. Old labels for a frame are
  // harmless; the rendered entry is what `fresh` guards.
  const tab_items = $derived(
    ordered_enabled()
      .filter((d) => results[d.name]?.status === 'ok')
      .map((d) => ({ value: d.name, label: display_name(d.name) })),
  )

  const entry = $derived.by<SanitizedEntry | null>(() => {
    const word = words.word
    const name = tab
    if (!word || !name || results_word !== word) return null
    const result = results[name]
    if (!result || result.status !== 'ok') return null
    const key = `${word}\u0000${name}`
    let cached = entry_cache.get(key)
    if (!cached) {
      const t0 = performance.now()
      cached = sanitize_entry(result.html, name, has_resources(name))
      // Permanent, cheap: this is the main-thread cost of making publisher
      // markup safe, and the number people feel on a 237 KB entry.
      performance.measure('mdict:sanitize', { start: t0, end: performance.now() })
      entry_cache.set(key, cached)
      if (entry_cache.size > ENTRY_CACHE_MAX) {
        const oldest = entry_cache.keys().next()
        if (!oldest.done) entry_cache.delete(oldest.value)
      }
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

  const show_not_found = $derived(not_found && !loading && fresh)

  const errors = $derived.by(() => {
    if (!fresh) return []
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

  /** Record one dictionary's outcome, keeping the progressive-tab rule. */
  function apply(name: string, outcome: LookupOutcome, preferred: string) {
    results[name] =
      outcome.kind === 'ok'
        ? { status: 'ok', html: outcome.html }
        : outcome.kind === 'miss'
          ? { status: 'miss' }
          : { status: 'error', message: outcome.message }
    settled += 1
    if (settled === 1) performance.mark('mdict:first-result')
    if (settled === total) performance.mark('mdict:all-settled')
    // First paint must not wait for the slowest dictionary: show the first
    // match that lands (preferring the requested one), and let the tab bar
    // fill in behind it.
    if (outcome.kind === 'ok') {
      const shown = tab
      const shown_ok = shown !== undefined && results[shown]?.status === 'ok'
      if (!shown_ok && (name === preferred || shown === undefined)) tab = name
    }
  }

  function finish(preferred: string) {
    loading = false
    const targets = ordered_enabled().map((d) => d.name)
    const matched = targets.filter((n) => results[n]?.status === 'ok')
    not_found = matched.length === 0
    // The URL's dict wins (a shared link names it); otherwise the dictionary
    // the reader last chose, so a new word opens where they were reading.
    const wanted = matched.includes(preferred) ? preferred : (matched[0] ?? undefined)
    tab = wanted
    // Keep the URL in step with the tab, without adding a history entry.
    if (wanted && wanted !== words.dict) select_dict(wanted)
  }

  /** The dictionary first paint waits on, asked for alone so nothing queues behind it. */
  async function fetch_one(
    word: string,
    name: string,
    preferred: string,
    signal: AbortSignal,
    id: number,
  ) {
    let outcome: LookupOutcome
    try {
      outcome = await fetch_entry(word, name, signal)
    } catch (err) {
      if ((err as Error)?.name === 'AbortError' || id !== request_id) return
      outcome = { kind: 'error', dict: name, message: 'server unreachable' }
    }
    if (id !== request_id) return
    apply(name, outcome, preferred)
  }

  /** Everything not already in hand, in a single request split by section. */
  async function fetch_rest(
    word: string,
    except: string[],
    targets: string[],
    preferred: string,
    signal: AbortSignal,
    id: number,
  ) {
    let combined: CombinedOutcome
    try {
      combined = await fetch_entries_except(word, except, signal)
    } catch (err) {
      if ((err as Error)?.name === 'AbortError' || id !== request_id) return
      combined = { kind: 'error', message: 'server unreachable' }
    }
    if (id !== request_id) return
    for (const name of targets) {
      if (except.includes(name)) continue
      const html = combined.kind === 'ok' ? combined.sections.get(name) : undefined
      const outcome: LookupOutcome =
        html !== undefined
          ? { kind: 'ok', dict: name, html }
          : combined.kind === 'error'
            ? { kind: 'error', dict: name, message: combined.message }
            : // The server leaves out a dictionary that does not have the word.
              { kind: 'miss', dict: name }
      // `fetch_entries_except` remembers the hits; a miss is worth keeping too,
      // or coming back to this word asks the server the same question again.
      if (outcome.kind === 'miss') remember_lookup(word, name, outcome)
      apply(name, outcome, preferred)
    }
  }

  async function run(word: string) {
    const preferred = words.dict ?? settings.last_dict ?? ''
    performance.mark('mdict:fetch-start')
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
    results_word = word
    total = targets.length
    settled = 0
    loading = targets.length > 0
    if (targets.length === 0) {
      not_found = false
      tab = undefined
      return
    }

    // Whatever is already in hand paints at once: the entry the shell embedded
    // for the URL's dictionary, then anything an earlier visit or a prefetch
    // remembered. A word the reader has just come back to costs no request.
    const seeded = boot && boot.word === word ? boot.entries : undefined
    const have: string[] = []
    for (const name of targets) {
      const html = seeded?.[name]
      const from_boot: LookupOutcome | undefined =
        html !== undefined ? { kind: 'ok', dict: name, html } : undefined
      const outcome = from_boot ?? cached_lookup(word, name)
      if (!outcome) continue
      remember_lookup(word, name, outcome)
      apply(name, outcome, preferred)
      have.push(name)
    }
    const missing = targets.filter((name) => !have.includes(name))
    if (missing.length === 0) {
      finish(preferred)
      return
    }

    // First paint waits on one dictionary. If the preferred one is already on
    // screen there is nothing to wait for and a single combined request covers
    // the rest; if nothing is on screen, the preferred dictionary — or the
    // reader's first, with no preference to honour — is asked for alone so a
    // slow neighbour cannot hold the entry back.
    const first = have.includes(preferred)
      ? null
      : missing.includes(preferred)
        ? preferred
        : missing[0]!
    const jobs: Promise<void>[] = []
    if (first) jobs.push(fetch_one(word, first, preferred, ctrl.signal, id))
    if (missing.some((name) => name !== first)) {
      jobs.push(
        fetch_rest(
          word,
          first ? [...have, first] : have,
          targets,
          preferred,
          ctrl.signal,
          id,
        ),
      )
    }
    await Promise.all(jobs)
    if (id !== request_id) return
    finish(preferred)
  }

  $effect(() => {
    const word = words.word
    // Switching a dictionary off or reordering them must refresh what is on
    // screen, not just the next lookup, so both are read here purely to join
    // this effect's dependency set.
    void settings.enabled_dicts.join('\u0001')
    void settings.dict_order.join('\u0001')
    if (!word) {
      results = {}
      results_word = ''
      nearby_word = ''
      tab = undefined
      not_found = false
      loading = false
      nearby = null
      did_you_mean = []
      return
    }
    results_word = word
    loading = true
    // `run` reads the very state it writes — `tab`, `loading`, `results`, and
    // `words.dict` through `select_dict`. When the shell seeds every entry it
    // runs to completion without awaiting, so all of that happens inside this
    // effect's tracking scope and each write re-invalidates it: an endless
    // lookup/nearby cycle. Untracked, its only dependencies stay the ones the
    // effect reads itself.
    untrack(() => void run(word))
  })

  // The immediate neighbours come straight from the dictionary's sorted index,
  // scoped to the tab being read: exact, and never "unknown" the way the old
  // prefix-shrink through /suggest was.
  const neighbours = $derived<{ prev: string | null; next: string | null }>(
    nearby_word === words.word
      ? { prev: nearby?.prev.at(-1) ?? null, next: nearby?.next[0] ?? null }
      : { prev: null, next: null },
  )

  $effect(() => {
    const word = words.word
    const name = tab
    // Only once the fan-out has settled: fetching for a tab that the next
    // settled result replaces wastes a round trip on every lookup.
    if (!word || !name || loading) {
      return
    }
    let cancelled = false
    void fetch_nearby(word, { dict: name, before: 4, after: 4 })
      .then((found) => {
        if (cancelled) return
        nearby = found ? { prev: found.prev, next: found.next } : null
        nearby_word = word
        // Reading is sequential, and the two words either side of this one are
        // known the moment the index is asked. Fetching them now, behind the
        // entry that is already on screen, is what makes Next feel instant.
        if (found) {
          for (const neighbour of [found.prev.at(-1), found.next[0]]) {
            if (neighbour) prefetch_entry(neighbour, name)
          }
        }
      })
      .catch(() => {
        if (!cancelled) {
          nearby = null
          nearby_word = word
        }
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

  {#if fresh && nearby && (nearby.prev.length > 0 || nearby.next.length > 0)}
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

  {#if show_not_found}
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
      bind:value={tab}
      on_change={(value) => {
        tab = value
        settings.last_dict = value
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
