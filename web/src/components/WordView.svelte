<script lang="ts">
  import { Icon, PrevNext, Progress, Spinner, StatusMessage, Tabs, Toc } from 'svelte-widgets'
  import { BookSearch, Headphones, LinkVariant, MusicOff } from 'svelte-widgets/icons'
  import EntryToolbar from './EntryToolbar.svelte'
  import EntryView from './EntryView.svelte'
  import { fetch_entry, fetch_markdown, fetch_suggest, type LookupOutcome } from '../lib/api.js'
  import { dicts, display_name, has_resources, info as dict_info, load_dicts } from '../lib/dicts.svelte.js'
  import { sanitize_entry, type SanitizedEntry } from '../lib/sanitize.js'
  import { is_dict_enabled, settings } from '../lib/settings.svelte.js'
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
  let markdown = $state<string | null>(null)
  let find_open = $state(false)
  let entry_wrap: HTMLElement | null = $state(null)
  let neighbours = $state<{ prev: string | null; next: string | null }>({ prev: null, next: null })

  // Memo caches. Plain Maps, not $state: they are never rendered from directly.
  const entry_cache = new Map<string, SanitizedEntry>()
  const md_cache = new Map<string, string>()
  let controller: AbortController | null = null
  let request_id = 0

  const tab_items = $derived(
    Object.entries(results)
      .filter(([, r]) => r.status === 'ok')
      .map(([name]) => ({ value: name, label: display_name(name) })),
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

    const targets = dicts.all.filter((d) => is_dict_enabled(d.name)).map((d) => d.name)
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
    if (!word) {
      results = {}
      tab = null
      not_found = false
      loading = false
      neighbours = { prev: null, next: null }
      return
    }
    void run(word)
  })

  /**
   * `/suggest` is a forward-only prefix scan capped at 100 with no cursor, so
   * there is no way to ask for "the headword before this one" directly. Shrink
   * the prefix until the current word sits inside a window that is not truncated
   * at the boundary we need. `prev` stays null when no window contains it.
   */
  async function find_neighbours(word: string) {
    const lower = word.toLowerCase()
    for (let len = word.length; len >= 1; len--) {
      let list: string[]
      try {
        list = await fetch_suggest(word.slice(0, len), { limit: 100 })
      } catch {
        return { prev: null, next: null }
      }
      const i = list.indexOf(lower)
      if (i < 0) continue
      const at_edge = (i === 0 || i === list.length - 1) && list.length >= 100
      if (at_edge && len > 1) continue
      return { prev: i > 0 ? (list[i - 1] ?? null) : null, next: list[i + 1] ?? null }
    }
    return { prev: null, next: null }
  }

  $effect(() => {
    const word = words.word
    if (!word) return
    let cancelled = false
    void find_neighbours(word).then((found) => {
      if (!cancelled) neighbours = found
    })
    return () => {
      cancelled = true
    }
  })

  // Markdown is fetched for the selected tab only, on demand: the server runs the
  // conversion in a blocking task, so requesting nine of them up front would be
  // wasteful when the reader usually looks at one.
  $effect(() => {
    const word = words.word
    const name = tab
    if (!word || !name) {
      markdown = null
      return
    }
    const key = `${word}\u0000${name}`
    const cached = md_cache.get(key)
    if (cached !== undefined) {
      markdown = cached || null
      return
    }
    markdown = null
    let cancelled = false
    void fetch_markdown(word, name).then((md) => {
      md_cache.set(key, md ?? '')
      if (!cancelled && words.word === word && tab === name) markdown = md || null
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

  {#if loading && total > 0}
    <Progress
      class="load-progress"
      value={settled}
      max={total}
      label="Searching dictionaries"
    />
  {/if}

  {#if errors.length > 0}
    <StatusMessage type="warning" message={errors.join(' · ')} />
  {/if}

  {#if not_found && !loading}
    <div class="empty">
      <Icon icon={BookSearch} width={34} height={34} />
      <p><strong>“{words.word}” is not in any searched dictionary.</strong></p>
      <p class="empty__hint">
        {#if total < dicts.all.length}
          Only {total} of {dicts.all.length} dictionaries are switched on — enable more in
          settings.
        {:else}
          Try a different spelling, or check that the dictionaries were imported with their
          companion <code>.mdd</code> files.
        {/if}
      </p>
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
            bind:view={settings.view}
            bind:find_open
            {markdown}
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

          <div class="entry-columns">
            <div class="entry-wrap" bind:this={entry_wrap}>
              {#if settings.view === 'markdown'}
                {#if markdown === null}
                  <div class="spinner-wrap"><Spinner text="Converting to Markdown…" /></div>
                {:else}
                  <pre class="md-view" style="--font-scale:{settings.font_scale}">{markdown}</pre>
                {/if}
              {:else if entry}
                <EntryView {entry} dict={item.value} word={words.word} bind:find_open {on_image} on_word={navigate} />
              {/if}
            </div>

            {#if toc_items.length >= 2}
              <aside class="entry-toc">
                <Toc items={toc_items} desktop={true} min_items={2} />
              </aside>
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
  .entry-columns {
    display: flex;
    align-items: flex-start;
    gap: 2rem;
  }
  .entry-wrap {
    flex: 1;
    min-width: 0;
  }
  .entry-toc {
    position: sticky;
    top: 1rem;
    flex: none;
    width: 15rem;
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
</style>
