<script lang="ts">
  import { onMount } from 'svelte'
  import { GitHubCorner, Toast } from 'svelte-widgets'
  import { toast } from 'svelte-widgets/toast-queue'
  import { watch_theme } from 'svelte-widgets/theme'
  import AudioPlayer from './components/AudioPlayer.svelte'
  import FooterBar from './components/FooterBar.svelte'
  import Header from './components/Header.svelte'
  import LandingPage from './components/LandingPage.svelte'
  import Sidebar from './components/Sidebar.svelte'
  import WordPopover from './components/WordPopover.svelte'
  import WordView from './components/WordView.svelte'
  import { load_dicts } from './lib/dicts.svelte.js'
  import { init_from_url, words } from './lib/words.svelte.js'

  type PaletteMod = typeof import('./components/CommandPalette.svelte').default
  type LightboxMod = typeof import('./components/ImageLightbox.svelte').default
  type SettingsMod = typeof import('./components/SettingsSheet.svelte').default

  // The overlays are most of the bundle and none of the first paint, so each is
  // its own chunk, imported the first time it is opened.
  let CommandPalette = $state<PaletteMod | null>(null)
  let ImageLightbox = $state<LightboxMod | null>(null)
  let SettingsSheet = $state<SettingsMod | null>(null)

  let settings_open = $state(false)
  let palette_open = $state(false)
  let lightbox = $state<{ src: string; alt: string } | null>(null)

  // Read the word from the URL synchronously, before the first render, so a
  // shared link lands on the entry instead of flashing the start page first.
  init_from_url()

  onMount(() => {
    const stop_theme = watch_theme()
    void load_dicts()
    warm_overlays()
    return stop_theme
  })

  /**
   * Fetch the overlay chunks without competing with the entry's first paint.
   * Splitting them out of boot is only a win if opening one is still instant,
   * so they arrive as soon as the browser has nothing better to do.
   */
  function warm_overlays() {
    const when_idle = window.requestIdleCallback
      ? (fn: () => void) => window.requestIdleCallback(fn, { timeout: 3000 })
      : (fn: () => void) => window.setTimeout(fn, 1000)
    when_idle(() => {
      void import('./components/CommandPalette.svelte')
      void import('./components/ImageLightbox.svelte')
      void import('./components/SettingsSheet.svelte')
    })
  }

  function open_settings() {
    settings_open = true
    void import('./components/SettingsSheet.svelte').then((m) => (SettingsSheet ??= m.default))
  }

  function open_palette() {
    palette_open = true
    void import('./components/CommandPalette.svelte').then((m) => (CommandPalette ??= m.default))
  }

  /** ⌘K / Ctrl+K opens the palette; `/` does too when nothing is focused. */
  function on_keydown(event: KeyboardEvent) {
    const key = event.key.toLowerCase()
    if ((event.metaKey || event.ctrlKey) && key === 'k') {
      event.preventDefault()
      if (palette_open) palette_open = false
      else open_palette()
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
        open_palette()
      }
    }
  }

  function on_image(src: string, alt: string) {
    lightbox = { src, alt }
    void import('./components/ImageLightbox.svelte').then((m) => (ImageLightbox ??= m.default))
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

<Header on_open_settings={open_settings} on_open_palette={open_palette} />

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
<WordPopover />
{#if SettingsSheet}
  <SettingsSheet bind:open={settings_open} />
{/if}
{#if ImageLightbox}
  <ImageLightbox bind:image={lightbox} />
{/if}
{#if CommandPalette}
  <CommandPalette bind:open={palette_open} on_open_settings={open_settings} />
{/if}
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
