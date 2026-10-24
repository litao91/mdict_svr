<script lang="ts">
  import { ActionMenu, DraggablePane, Icon, ThemeToggle } from 'svelte-widgets'
  import type { CmdAction } from 'svelte-widgets'
  import type { IconData } from 'svelte-widgets/icons'
  import {
    BookOpen,
    Close,
    Copy,
    DotsVertical,
    History,
    Home,
    Menu,
    Search,
    Shuffle,
    TuneVariant,
  } from 'svelte-widgets/icons'
  import { toast } from 'svelte-widgets/toast-queue'
  import Sidebar from './Sidebar.svelte'
  import WordSearch from './WordSearch.svelte'
  import { clear_history, navigate, random_word, words } from '../lib/words.svelte.js'

  let {
    on_open_settings,
    on_open_palette,
  }: {
    on_open_settings: () => void
    on_open_palette: () => void
  } = $props()

  // Under 900px the static sidebar rail is hidden; this pane is the only way to
  // reach history and the dictionary list there.
  let nav_open = $state(false)

  type MenuAction = CmdAction & { icon: IconData }

  const menu_actions = $derived<MenuAction[]>([
    {
      id: 'menu-settings',
      label: 'Settings',
      icon: TuneVariant,
      action: () => on_open_settings(),
    },
    {
      id: 'menu-random',
      label: 'Look up a random word',
      icon: Shuffle,
      action: () => {
        void random_word().then((w) => w && navigate(w))
      },
    },
    {
      id: 'menu-copy',
      label: 'Copy a link to this entry',
      icon: Copy,
      disabled: !words.word,
      action: () => {
        void navigator.clipboard
          ?.writeText(window.location.href)
          .then(() => toast.show('Link copied', { priority: 'success', dedupe_key: 'copy-link' }))
          .catch(() => toast.show('The browser refused to copy', { priority: 'warning' }))
      },
    },
    {
      id: 'menu-clear',
      label: `Clear look-up history (${words.recent.length})`,
      icon: History,
      disabled: words.recent.length === 0,
      action: () => {
        clear_history()
        toast.show('Look-up history cleared', { priority: 'success', dedupe_key: 'clear-history' })
      },
    },
  ])
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
    <!-- DraggablePane renders its own toggle <button> around this snippet, so
         the snippet must not contain a second button: nesting them makes one
         click toggle twice and the pane never opens. -->
    <DraggablePane
      align="start"
      position="fixed"
      resize="width"
      max_width="82vw"
      bind:open={nav_open}
      toggle_props={{ class: 'nav-toggle', 'aria-label': 'Dictionaries and history' }}
      pane_props={{ class: 'nav-pane', 'aria-label': 'Dictionaries and look-up history' }}
    >
      {#snippet toggle(state)}
        <Icon icon={state.open ? Close : Menu} width={17} height={17} />
        <span class="sr-only">Dictionaries and history</span>
      {/snippet}
      {#snippet children()}
        <Sidebar />
      {/snippet}
    </DraggablePane>
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
    <ActionMenu actions={menu_actions} align="end" offset={6}>
      {#snippet trigger({ onclick, ...aria })}
        <button class="icon-btn" type="button" title="More actions" {onclick} {...aria}>
          <Icon icon={DotsVertical} width={17} height={17} />
          <span class="sr-only">More actions</span>
        </button>
      {/snippet}
      {#snippet item({ action })}
        <span class="menu-row">
          <Icon icon={action.icon} width={15} height={15} />
          {action.label}
        </span>
      {/snippet}
    </ActionMenu>
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
    display: flex;
    align-items: center;
    gap: 0.35rem;
    padding: 0.2rem 0.35rem;
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg);
    font-weight: 650;
    letter-spacing: -0.01em;
    cursor: pointer;
  }
  .brand:hover {
    background: var(--bg-sunken);
  }
  /* The toggle is DraggablePane's own button; it only exists on narrow
     screens, where the static sidebar rail is hidden. */
  :global(.nav-toggle) {
    display: none;
    place-items: center;
    width: 2rem;
    height: 2rem;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-muted);
    cursor: pointer;
  }
  :global(.nav-toggle:hover) {
    background: var(--bg-sunken);
    color: var(--fg);
  }
  @media (max-width: 900px) {
    :global(.nav-toggle) {
      display: grid;
    }
  }
  /* DraggablePane positions itself inline from its toggle; as the phone's only
     navigation it must read as a drawer, so pin it under the topbar. The
     !important beats the component's inline top/left. */
  :global(.nav-pane) {
    top: 3.4rem !important;
    left: 0.5rem !important;
    width: min(20rem, calc(100vw - 1rem));
    max-height: calc(100vh - 4.4rem);
    overflow-y: auto;
    background: var(--bg-raised);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
    padding: 0.75rem 0.5rem;
  }
  .menu-row {
    display: flex;
    align-items: center;
    gap: 0.5rem;
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
