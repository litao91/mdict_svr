<script lang="ts">
  import { Footer } from 'svelte-widgets'
  import { dicts } from '../lib/dicts.svelte.js'

  const links = [
    { href: '/dicts', label: 'Loaded dictionaries (JSON)', title: 'GET /dicts' },
    { href: '/health', label: 'Health', title: 'GET /health' },
  ]

  const note = $derived(
    dicts.loaded
      ? `${dicts.all.length} dictionaries · ${dicts.all
          .reduce((n, d) => n + (d.entries || 0), 0)
          .toLocaleString()} headwords served by mdict_svr`
      : 'Served by mdict_svr',
  )
</script>

<Footer class="app-footer" {links}>
  {#snippet children()}
    <span>{note}</span>
  {/snippet}
</Footer>
