<script lang="ts">
  import { CommandMenu } from 'svelte-widgets'
  import type { CmdAction } from 'svelte-widgets'
  import { apply_theme_mode, theme } from 'svelte-widgets/theme'
  import { fetch_suggest } from '../lib/api.js'
  import { in_wordlist, toggle_wordlist } from '../lib/wordlist.svelte.js'
  import { clear_history, home, navigate, random_word, words } from '../lib/words.svelte.js'

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
      action: () => {
        void random_word().then((w) => w && navigate(w))
      },
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
    {
      id: 'cmd-star',
      label: words.word
        ? in_wordlist(words.word)
          ? `Remove “${words.word}” from your word list`
          : `Keep “${words.word}” in your word list`
        : 'Keep the current word in your word list',
      group: 'Commands',
      keywords: ['star', 'save', 'favourite', 'favorite', 'anki'],
      disabled: !words.word,
      action: () => {
        if (words.word) toggle_wordlist(words.word)
      },
    },
  ]

  const recents: CmdAction[] = words.recent.slice(0, 6).map((word) => ({
    id: `recent:${word}`,
    label: word,
    group: 'Recent',
    action: () => navigate(word),
  }))

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
            // CommandMenu re-filters our server-side results with its own
            // subsequence matcher, which a literal `*` can never satisfy. Tagging
            // each wildcard hit with the raw query keeps the server's answer.
            const wildcard = query.includes('*') || query.includes('?')
            word_actions = list.map((word) => ({
              id: `word:${word}`,
              label: word,
              group: 'Headwords',
              keywords: wildcard ? [query] : undefined,
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
