<script lang="ts">
  import { Icon, ThemeToggle } from 'svelte-widgets'
  import { BookOpen, DotsVertical, Home, Search } from 'svelte-widgets/icons'
  import WordSearch from './WordSearch.svelte'
  import { words } from '../lib/words.svelte.js'

  let {
    on_open_settings,
    on_open_palette,
  }: {
    on_open_settings: () => void
    on_open_palette: () => void
  } = $props()
</script>

<header class="topbar">
  <button
    class="brand"
    type="button"
    title="Back to the start page"
    onclick={() => (words.word = '')}
  >
    <Icon icon={BookOpen} width={19} height={19} />
    <span>mdict</span>
  </button>

  <WordSearch class="topbar__search" />

  <div class="topbar__spacer"></div>

  <div class="topbar__actions">
    {#if words.word}
      <button class="icon-btn" type="button" title="Start page" onclick={() => (words.word = '')}>
        <Icon icon={Home} width={17} height={17} />
        <span class="sr-only">Start page</span>
      </button>
    {/if}
    <button class="icon-btn" type="button" title="Search (Ctrl/⌘ K)" onclick={on_open_palette}>
      <Icon icon={Search} width={17} height={17} />
      <span class="sr-only">Search</span>
    </button>
    <ThemeToggle class="icon-btn" tooltip={false} />
    <button class="icon-btn" type="button" title="Settings" onclick={on_open_settings}>
      <Icon icon={DotsVertical} width={17} height={17} />
      <span class="sr-only">Settings</span>
    </button>
  </div>
</header>

<style>
  .icon-btn {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
    transition: background-color 0.12s, color 0.12s;
  }
  .icon-btn:hover {
    background: var(--bg-sunken);
    color: var(--fg);
  }
  .brand {
    padding: 0.2rem 0.35rem;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    cursor: pointer;
  }
  .brand:hover {
    background: var(--bg-sunken);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }
</style>
