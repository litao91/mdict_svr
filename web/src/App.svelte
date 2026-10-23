<script lang="ts">
  import { onMount } from 'svelte'
  import { GitHubCorner, Toast } from 'svelte-widgets'
  import { toast } from 'svelte-widgets/toast-queue'
  import { watch_theme } from 'svelte-widgets/theme'
  import AudioPlayer from './components/AudioPlayer.svelte'
  import CommandPalette from './components/CommandPalette.svelte'
  import FooterBar from './components/FooterBar.svelte'
  import Header from './components/Header.svelte'
  import ImageLightbox from './components/ImageLightbox.svelte'
  import LandingPage from './components/LandingPage.svelte'
  import SettingsSheet from './components/SettingsSheet.svelte'
  import Sidebar from './components/Sidebar.svelte'
  import WordView from './components/WordView.svelte'
  import { load_dicts } from './lib/dicts.svelte.js'
  import { init_from_url, words } from './lib/words.svelte.js'

  let settings_open = $state(false)
  let palette_open = $state(false)
  let lightbox = $state<{ src: string; alt: string } | null>(null)

  // Read the word from the URL synchronously, before the first render, so a
  // shared link lands on the entry instead of flashing the start page first.
  init_from_url()

  onMount(() => {
    const stop_theme = watch_theme()
    void load_dicts()
    return stop_theme
  })

  /** ⌘K / Ctrl+K opens the palette; `/` does too when nothing is focused. */
  function on_keydown(event: KeyboardEvent) {
    const key = event.key.toLowerCase()
    if ((event.metaKey || event.ctrlKey) && key === 'k') {
      event.preventDefault()
      palette_open = !palette_open
      return
    }
    if (key === '/' && !event.metaKey && !event.ctrlKey && !event.altKey) {
      const target = event.target as HTMLElement | null
      const typing =
        target &&
        (target.isContentEditable ||
          ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName))
      if (!typing) {
        event.preventDefault()
        palette_open = true
      }
    }
  }

  function on_image(src: string, alt: string) {
    lightbox = { src, alt }
  }
</script>

<svelte:window onkeydown={on_keydown} />

<GitHubCorner
  href="https://github.com/litao91/mdict_svr"
  target="_blank"
  title="mdict_svr on GitHub"
  aria_label="Source on GitHub"
  corner="top-right"
/>

<Header
  on_open_settings={() => (settings_open = true)}
  on_open_palette={() => (palette_open = true)}
/>

<div class="shell">
  <div class="shell__side">
    <Sidebar />
  </div>
  <main class="shell__main">
    <div class="scroll-area">
      {#if words.word}
        <WordView {on_image} />
      {:else}
        <LandingPage />
      {/if}
    </div>
    <FooterBar />
  </main>
</div>

<AudioPlayer />
<SettingsSheet bind:open={settings_open} />
<ImageLightbox bind:image={lightbox} />
<CommandPalette bind:open={palette_open} on_open_settings={() => (settings_open = true)} />
<Toast store={toast} position="bottom-right" pause_on_hover />

<style>
  .shell__side {
    flex: none;
    width: 15.5rem;
    border-right: 1px solid var(--border);
    background: color-mix(in srgb, var(--bg-raised) 55%, var(--bg));
    /* Scrolls on its own so a long history never pushes the shell taller. */
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  @media (max-width: 900px) {
    .shell__side {
      display: none;
    }
  }
</style>
