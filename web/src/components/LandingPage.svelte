<script lang="ts">
  import { Heading, Icon, StatGrid, StatusMessage } from 'svelte-widgets'
  import { Database, Headphones, ImageOff, Notebook } from 'svelte-widgets/icons'
  import WordSearch from './WordSearch.svelte'
  import { dicts, display_name } from '../lib/dicts.svelte.js'
  import { navigate, words } from '../lib/words.svelte.js'
  import { is_dict_enabled } from '../lib/settings.svelte.js'

  const enabled = $derived(dicts.all.filter((d) => is_dict_enabled(d.name)))
  const total_entries = $derived(enabled.reduce((n, d) => n + (d.entries || 0), 0))
  const with_audio = $derived(enabled.filter((d) => d.has_resources).length)

  const stats = $derived([
    { label: 'Dictionaries', value: enabled.length, hint: 'switched on' },
    { label: 'Headwords', value: total_entries.toLocaleString(), hint: 'searchable' },
    { label: 'With assets', value: with_audio, hint: 'audio and images' },
    { label: 'Recent', value: words.recent.length, hint: 'in your history' },
  ])

  const cards = $derived(
    dicts.all.map((d) => ({
      name: d.name,
      title: display_name(d.name),
      entries: d.entries,
      has_resources: d.has_resources,
      source: d.source,
      enabled: is_dict_enabled(d.name),
    })),
  )
</script>

<div class="landing">
  <div class="hero">
    <Heading id="start" level={1} class="hero__title" link={false}>
      {#snippet children()}Look up a word{/snippet}
    </Heading>
    <p class="hero__sub">
      Every dictionary the server has loaded, with pronunciation audio that plays in place and
      cross-references that stay inside the app.
    </p>
    <WordSearch class="hero__search" placeholder="Try “apple”, “run” or 苹果…" focus_on_mount />
  </div>

  {#if dicts.error}
    <StatusMessage
      type="error"
      message={`Could not reach mdict_svr — is the server running? ${dicts.error}`}
    />
  {:else if dicts.loaded && dicts.all.length === 0}
    <StatusMessage type="warning" message="The server is running but has no dictionaries loaded." />
  {/if}

  {#if dicts.loaded && dicts.all.length > 0}
    <StatGrid items={stats} />

    <section class="cards">
      <h2 class="cards__title">
        <Icon icon={Notebook} width={15} height={15} />
        Dictionaries
      </h2>
      <div class="cards__grid">
        {#each cards as card (card.name)}
          <button
            class="dict-card"
            type="button"
            class:dict-card--off={!card.enabled}
            title={card.source || card.title}
            onclick={() => navigate(words.word || 'apple', { dict: card.name })}
          >
            <h3 class="dict-card__name">{card.title}</h3>
            <div class="dict-card__meta">
              <span><Icon icon={Database} width={11} height={11} /> {card.entries.toLocaleString()}</span>
              {#if card.has_resources}
                <span class="badge badge--on">
                  <Icon icon={Headphones} width={11} height={11} /> assets
                </span>
              {:else}
                <span class="badge">
                  <Icon icon={ImageOff} width={11} height={11} /> no assets
                </span>
              {/if}
              {#if !card.enabled}
                <span class="badge">off</span>
              {/if}
            </div>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if words.recent.length > 0}
    <section class="cards">
      <h2 class="cards__title">Recently looked up</h2>
      <div class="chips">
        {#each words.recent.slice(0, 18) as word (word)}
          <button class="chip" type="button" onclick={() => navigate(word)}>{word}</button>
        {/each}
      </div>
    </section>
  {/if}
</div>

<style>
  .cards {
    display: flex;
    flex-direction: column;
    gap: 0.7rem;
  }
  .cards__title {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    margin: 0;
    font-size: 0.72rem;
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--fg-faint);
  }
  .cards__grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
    gap: 0.7rem;
  }
  .dict-card {
    cursor: pointer;
    font: inherit;
    text-align: left;
    transition: transform 0.12s, box-shadow 0.12s, border-color 0.12s;
  }
  .dict-card:hover {
    transform: translateY(-1px);
    box-shadow: var(--shadow-md);
    border-color: var(--border-strong);
  }
  .dict-card--off {
    opacity: 0.55;
  }
  .dict-card__meta span {
    display: inline-flex;
    align-items: center;
    gap: 0.25rem;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }
  .chip {
    padding: 0.25rem 0.7rem;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--bg-raised);
    color: var(--fg-muted);
    font: inherit;
    font-size: 0.85rem;
    cursor: pointer;
    transition: color 0.12s, border-color 0.12s, background-color 0.12s;
  }
  .chip:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: var(--accent-soft);
  }
</style>
