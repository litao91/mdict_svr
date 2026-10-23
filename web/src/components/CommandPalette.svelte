<script lang="ts">
  import { CommandMenu } from 'svelte-widgets'
  import type { CmdAction } from 'svelte-widgets'
  import { apply_theme_mode, theme } from 'svelte-widgets/theme'
  import { fetch_suggest } from '../lib/api.js'
  import { clear_history, home, navigate, words } from '../lib/words.svelte.js'

  let {
    open = $bindable(false),
    on_open_settings,
  }: {
    open?: boolean
    on_open_settings: () => void
  } = $props()

  let search_text = $state('')
  let word_actions = $state<CmdAction[]>([])
  let controller: AbortController | null = null
  let timer: ReturnType<typeof setTimeout> | null = null

  /** Fixed commands, always available regardless of the query. */
  const commands: CmdAction[] = [
    {
      id: 'cmd-random',
      label: 'Look up a random word',
      group: 'Commands',
      keywords: ['shuffle', 'surprise', 'dice'],
      action: () => void random_word(),
    },
    {
      id: 'cmd-theme',
      label: 'Cycle theme (light → system → dark)',
      group: 'Commands',
      keywords: ['dark', 'light', 'appearance'],
      action: () => apply_theme_mode(theme.mode === 'light' ? 'system' : theme.mode === 'system' ? 'dark' : 'light'),
    },
    {
      id: 'cmd-settings',
      label: 'Open settings',
      group: 'Commands',
      keywords: ['preferences', 'dictionaries', 'font'],
      action: () => on_open_settings(),
    },
    {
      id: 'cmd-home',
      label: 'Go to the start page',
      group: 'Commands',
      keywords: ['home', 'clear'],
      action: () => home(),
    },
    {
      id: 'cmd-clear',
      label: 'Clear look-up history',
      group: 'Commands',
      keywords: ['recent', 'forget'],
      action: () => clear_history(),
    },
  ]

  const recents: CmdAction[] = words.recent.slice(0, 6).map((word) => ({
    id: `recent:${word}`,
    label: word,
    group: 'Recent',
    action: () => navigate(word),
  }))

  async function random_word() {
    const letters = 'abcdefghijklmnopqrstuvwxyz'
    const pick = () => letters[Math.floor(Math.random() * letters.length)]!
    for (let attempt = 0; attempt < 6; attempt++) {
      const list = await fetch_suggest(attempt < 3 ? pick() : pick() + pick(), { limit: 100 })
      const usable = list.filter((w) => /^[a-z]/i.test(w))
      if (usable.length > 0) {
        navigate(usable[Math.floor(Math.random() * usable.length)]!)
        return
      }
    }
  }

  // Live headword completion. `/suggest` is a prefix scan, so the results are
  // already exact matches; CommandMenu's own fuzzy pass runs over them on top.
  $effect(() => {
    const query = search_text.trim()
    if (timer) clearTimeout(timer)
    if (!query) {
      word_actions = []
      return
    }
    timer = setTimeout(() => {
      controller?.abort()
      controller = new AbortController()
      void fetch_suggest(query, { limit: 12, signal: controller.signal })
        .then((list) => {
          if (query === search_text.trim()) {
            word_actions = list.map((word) => ({
              id: `word:${word}`,
              label: word,
              group: 'Headwords',
              action: () => navigate(word),
            }))
          }
        })
        .catch((err: unknown) => {
          if ((err as Error)?.name !== 'AbortError') word_actions = []
        })
    }, 110)
  })

  const actions = $derived([...word_actions, ...recents, ...commands])
</script>

<CommandMenu
  bind:open
  bind:search_text
  {actions}
  placeholder="Look up a word or run a command…"
  aria_label="Dictionary command palette"
  input_aria_label="Search headwords and commands"
  recent_actions_key="mdict.palette.recent"
  max_recent={6}
  backdrop_dim
  backdrop_blur
/>
