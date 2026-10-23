<script lang="ts">
  import { Icon, Spinner } from 'svelte-widgets'
  import { BookOpen, Star, StarOutline } from 'svelte-widgets/icons'
  import { toast } from 'svelte-widgets/toast-queue'
  import { in_wordlist, toggle_wordlist } from '../lib/wordlist.svelte.js'
  import { close_wordpop, wordpop } from '../lib/wordpop.svelte.js'
  import { navigate } from '../lib/words.svelte.js'

  const WIDTH = 19 * 16
  const HEIGHT_ESTIMATE = 150

  const pop = $derived(wordpop.current)
  const starred = $derived(pop ? in_wordlist(pop.word) : false)

  /** Clamp into the viewport so a double-click near an edge stays readable. */
  const style = $derived.by(() => {
    if (!pop) return ''
    const x = Math.min(Math.max(8, pop.x), window.innerWidth - WIDTH - 8)
    const below = pop.y + HEIGHT_ESTIMATE + 16 < window.innerHeight
    const y = below ? pop.y + 14 : Math.max(8, pop.y - HEIGHT_ESTIMATE - 6)
    return `left:${x}px;top:${y}px;width:${WIDTH}px`
  })

  function on_look_up() {
    if (!pop) return
    const word = pop.word
    close_wordpop()
    navigate(word)
  }

  function on_star() {
    if (!pop) return
    const added = toggle_wordlist(pop.word)
    toast.show(added ? `“${pop.word}” added to your word list` : `“${pop.word}” removed from your word list`, {
      priority: 'success',
      dedupe_key: `wordlist:${pop.word}`,
    })
  }

  $effect(() => {
    if (!wordpop.current) return
    const on_key = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close_wordpop()
    }
    const on_down = (e: PointerEvent) => {
      const target = e.target as Node | null
      if (target && (target as Element).closest?.('.wordpop')) return
      close_wordpop()
    }
    window.addEventListener('keydown', on_key)
    window.addEventListener('pointerdown', on_down, true)
    return () => {
      window.removeEventListener('keydown', on_key)
      window.removeEventListener('pointerdown', on_down, true)
    }
  })
</script>

{#if pop}
  <div class="wordpop" style={style} role="dialog" aria-label={`Look up ${pop.word}`}>
    <div class="wordpop__head">
      <strong class="wordpop__word">{pop.word}</strong>
      <button class="wordpop__close" type="button" title="Dismiss" onclick={close_wordpop}>×</button>
    </div>
    <div class="wordpop__body">
      {#if pop.loading}
        <Spinner text="Fetching a preview…" />
      {:else if pop.preview}
        <p>{pop.preview}</p>
      {:else}
        <p class="wordpop__miss">No entry for this word in {pop.dict ?? 'any dictionary'}.</p>
      {/if}
    </div>
    <div class="wordpop__actions">
      <button class="wordpop__btn wordpop__btn--go" type="button" onclick={on_look_up}>
        <Icon icon={BookOpen} width={14} height={14} />
        Look up
      </button>
      <button
        class="wordpop__btn"
        type="button"
        title={starred ? 'Remove from your word list' : 'Add to your word list'}
        onclick={on_star}
      >
        <Icon icon={starred ? Star : StarOutline} width={14} height={14} />
        {starred ? 'Saved' : 'Save'}
      </button>
    </div>
  </div>
{/if}

<style>
  .wordpop {
    position: fixed;
    z-index: 70;
    display: flex;
    flex-direction: column;
    gap: 0.45rem;
    padding: 0.6rem 0.7rem;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius);
    background: var(--bg-raised);
    box-shadow: var(--shadow-lg);
  }
  .wordpop__head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 0.5rem;
  }
  .wordpop__word {
    font-family: var(--read-font);
    font-size: 1.02rem;
    color: var(--fg);
  }
  .wordpop__close {
    border: 0;
    background: transparent;
    color: var(--fg-faint);
    font-size: 1rem;
    line-height: 1;
    cursor: pointer;
  }
  .wordpop__close:hover {
    color: var(--fg);
  }
  .wordpop__body {
    min-height: 2.4rem;
    color: var(--fg-muted);
    font-size: 0.8rem;
    line-height: 1.5;
  }
  .wordpop__body p {
    margin: 0;
  }
  .wordpop__miss {
    color: var(--fg-faint);
    font-style: italic;
  }
  .wordpop__actions {
    display: flex;
    gap: 0.4rem;
  }
  .wordpop__btn {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.28rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-sunken);
    color: var(--fg-muted);
    font: inherit;
    font-size: 0.78rem;
    cursor: pointer;
  }
  .wordpop__btn:hover {
    color: var(--fg);
    border-color: var(--border-strong);
  }
  .wordpop__btn--go {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
  }
  .wordpop__btn--go:hover {
    color: var(--accent-fg);
    filter: brightness(1.08);
  }
</style>
