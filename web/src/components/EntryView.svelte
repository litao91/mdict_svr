<script lang="ts">
  import { FindBar } from 'svelte-widgets'
  import { attach_entry_behavior } from '../lib/entry-actions.js'
  import { settings } from '../lib/settings.svelte.js'
  import type { SanitizedEntry } from '../lib/sanitize.js'

  let {
    entry,
    dict,
    word,
    find_open = $bindable(false),
    on_word,
    on_image,
  }: {
    entry: SanitizedEntry
    dict: string
    word: string
    find_open?: boolean
    on_word: (word: string) => void
    on_image: (src: string, alt: string) => void
  } = $props()

  let host: HTMLDivElement | undefined = $state()

  // `{@html}` replaces every node underneath the host but never the host itself,
  // so the delegated listeners inside `attach_entry_behavior` survive re-renders.
  // This effect exists for the tooltip attachment, which must be torn down and
  // re-created when the entry changes.
  $effect(() => {
    const node = host
    // Read `entry.html` so this effect re-runs when a new entry is rendered.
    void entry.html
    if (!node) return
    return attach_entry_behavior(node, {
      on_word,
      on_image,
      previews_enabled: () => settings.previews,
    })
  })
</script>

{#if find_open && host}
  <FindBar
    root={host}
    label="Find in entry"
    only_within=".entry"
    on_close={() => (find_open = false)}
  />
{/if}

<div
  class="entry dict--{entry.vocabulary}"
  bind:this={host}
  data-dict={dict}
  data-word={word}
  style="--font-scale:{settings.font_scale}"
>
  {@html entry.html}
</div>
