<script lang="ts">
  import { CopyButton, FullscreenButton, Icon, NumberRangeInput } from 'svelte-widgets'
  import { Star, StarOutline, TextSearch } from 'svelte-widgets/icons'
  import { fetch_markdown } from '../lib/api.js'
  import { toast } from 'svelte-widgets/toast-queue'
  import { FONT_SCALE_MAX, FONT_SCALE_MIN, settings } from '../lib/settings.svelte.js'
  import { in_wordlist, toggle_wordlist } from '../lib/wordlist.svelte.js'

  let {
    find_open = $bindable(false),
    word,
    dict,
    fullscreen_target = null as HTMLElement | null,
  }: {
    find_open?: boolean
    word: string
    dict: string
    fullscreen_target?: HTMLElement | null
  } = $props()

  // The server's Markdown conversion costs ~150 ms on a large entry, so it is
  // fetched on intent — hovering or focusing the copy button — not per tab.
  let markdown = $state<string | null>(null)
  let fetching = false

  function prefetch_markdown() {
    if (markdown !== null || fetching) return
    fetching = true
    void fetch_markdown(word, dict)
      .then((md) => (markdown = md))
      .finally(() => (fetching = false))
  }

  const starred = $derived(in_wordlist(word))

  function on_star() {
    const added = toggle_wordlist(word)
    toast.show(added ? `“${word}” added to your word list` : `“${word}” removed from your word list`, {
      priority: 'success',
      dedupe_key: `wordlist:${word}`,
    })
  }
</script>

<div class="toolbar">
  <button
    class="tool-btn"
    type="button"
    title="Find in entry (opens a search bar scoped to this definition)"
    aria-pressed={find_open}
    onclick={() => (find_open = !find_open)}
  >
    <Icon icon={TextSearch} width={16} height={16} />
  </button>

  <!-- svelte-ignore a11y_no_static_element_interactions -- the handlers only
       prefetch; the interactive element remains the CopyButton inside. -->
  <span onpointerenter={prefetch_markdown} onfocusin={prefetch_markdown} class="copy-wrap">
    <CopyButton
      class="tool-btn"
      content={markdown ?? `Loading the Markdown for “${word}”…`}
      disabled={markdown === null}
      title={markdown === null ? 'Preparing the Markdown…' : `Copy “${word}” from ${dict} as Markdown`}
    />
  </span>

  <button
    class="tool-btn"
    type="button"
    title={starred ? `Remove “${word}” from your word list` : `Keep “${word}” in your word list`}
    aria-pressed={starred}
    onclick={on_star}
  >
    <Icon icon={starred ? Star : StarOutline} width={16} height={16} />
  </button>

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
  .copy-wrap {
    display: contents;
  }
  .font-scale {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 11rem;
  }
  @media (max-width: 600px) {
    .copy-wrap {
    display: contents;
  }
  .font-scale {
      display: none;
    }
  }
</style>
