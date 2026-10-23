<script lang="ts">
  import { ButtonGroup, Icon } from 'svelte-widgets'
  import {
    Download,
    Pause,
    Play,
    Repeat,
    Stop,
    VolumeOff,
    VolumeOn,
  } from 'svelte-widgets/icons'
  import { pause, player, resume, seek, set_loop, set_rate, set_volume, stop } from '../lib/audio.svelte.js'

  // ButtonGroup is generic over string values, so the rate travels as text.
  const rate_options = [
    { value: '0.75', label: '0.75×' },
    { value: '1', label: '1×' },
    { value: '1.25', label: '1.25×' },
  ]

  const shown = $derived(player.url !== null && player.status !== 'idle')
  const can_control = $derived(player.reason !== 'codec')
  const percent = $derived(player.duration > 0 ? (player.position / player.duration) * 100 : 0)

  function mmss(seconds: number): string {
    if (!Number.isFinite(seconds) || seconds <= 0) return '0:00'
    const m = Math.floor(seconds / 60)
    const s = Math.floor(seconds % 60)
    return `${m}:${String(s).padStart(2, '0')}`
  }

  function on_seek(event: Event) {
    const value = Number((event.target as HTMLInputElement).value)
    if (Number.isFinite(value)) seek(value)
  }

  function on_volume(event: Event) {
    const value = Number((event.target as HTMLInputElement).value)
    if (Number.isFinite(value)) set_volume(value / 100)
  }
</script>

{#if shown}
  <div class="player" role="region" aria-label="Pronunciation audio">
    {#if can_control}
      <button
        class="player__icon"
        type="button"
        aria-label={player.status === 'playing' ? 'Pause' : 'Play'}
        onclick={() => (player.status === 'playing' ? pause() : resume())}
      >
        <Icon icon={player.status === 'playing' ? Pause : Play} width={15} height={15} />
      </button>
    {:else}
      <span class="player__icon player__icon--off" aria-hidden="true">
        <Icon icon={VolumeOff} width={15} height={15} />
      </span>
    {/if}

    <div class="player__body">
      <div class="player__label">
        {#if player.label}
          <span class="player__word">{player.label}</span>
        {/if}
        {#if player.variant}
          <span class="badge">{player.variant === 'uk' ? 'BrE' : 'NAmE'}</span>
        {/if}
        {#if player.dict}
          <span class="player__file">{player.dict}</span>
        {/if}
        <span class="player__file">· {player.url?.split('/').pop()}</span>
      </div>

      {#if player.reason === 'codec'}
        <span class="player__status player__status--warn">
          This browser cannot decode the file’s codec. The server reports it as
          <code>audio/ogg</code>, but it is Ogg-Speex.
        </span>
      {:else if player.status === 'missing'}
        <span class="player__status player__status--warn">
          No such file in this dictionary’s resources.
        </span>
      {:else if player.reason === 'decode'}
        <span class="player__status player__status--warn">The browser could not decode this file.</span>
      {:else if can_control}
        <input
          class="player__seek"
          type="range"
          min="0"
          max={player.duration || 0}
          step="0.01"
          value={player.position}
          aria-label="Seek"
          disabled={player.duration <= 0}
          oninput={on_seek}
        />
      {/if}
    </div>

    {#if can_control && player.duration > 0}
      <span class="player__time">{mmss(player.position)} / {mmss(player.duration)}</span>
    {:else if can_control}
      <span class="player__time">{Math.round(percent)}%</span>
    {/if}

    {#if player.reason === 'codec'}
      <a class="player__link" href={player.url ?? '#'} download title="Save the file instead">
        <Icon icon={Download} width={16} height={16} />
      </a>
    {/if}

    {#if can_control}
      <ButtonGroup
        class="player__rate"
        options={rate_options}
        value={String(player.rate)}
        label="Playback speed"
        on_change={(value: string | null) => {
          const rate = Number(value)
          if (Number.isFinite(rate)) set_rate(rate)
        }}
      />
      <button
        class="player__loop"
        type="button"
        title={player.loop ? 'Stop repeating' : 'Repeat this clip'}
        aria-pressed={player.loop}
        onclick={() => set_loop(!player.loop)}
      >
        <Icon icon={Repeat} width={14} height={14} />
      </button>
      <label class="player__vol" title="Volume">
        <Icon icon={player.volume > 0 ? VolumeOn : VolumeOff} width={15} height={15} />
        <input
          type="range"
          min="0"
          max="100"
          step="1"
          value={Math.round(player.volume * 100)}
          aria-label="Volume"
          oninput={on_volume}
        />
      </label>
      <button class="player__stop" type="button" title="Close player" onclick={stop}>
        <Icon icon={Stop} width={14} height={14} />
      </button>
    {:else}
      <button class="player__stop" type="button" title="Dismiss" onclick={stop}>
        <Icon icon={Stop} width={14} height={14} />
      </button>
    {/if}
  </div>
{/if}

<style>
  .player__icon--off {
    background: var(--bg-sunken);
    color: var(--fg-faint);
    cursor: default;
    border: 1px dashed var(--border-strong);
  }
  .player__link,
  .player__stop,
  .player__vol {
    display: grid;
    place-items: center;
    flex: none;
    color: var(--fg-muted);
  }
  .player__link {
    padding: 0.25rem;
    border-radius: var(--radius-sm);
  }
  .player__link:hover {
    color: var(--accent);
    background: var(--accent-soft);
  }
  .player__stop {
    width: 1.9rem;
    height: 1.9rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
  }
  .player__stop:hover {
    color: var(--fg);
    border-color: var(--border-strong);
  }
  .player__vol {
    display: flex;
    gap: 0.3rem;
    align-items: center;
  }
  /* The class lands on ButtonGroup's root, outside this component's scope. */
  :global(.player__rate) {
    flex: none;
  }
  .player__loop {
    display: grid;
    place-items: center;
    flex: none;
    width: 1.7rem;
    height: 1.7rem;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--fg-faint);
    cursor: pointer;
  }
  .player__loop[aria-pressed='true'] {
    color: var(--accent);
    background: var(--accent-soft);
    border-color: var(--accent);
  }
  .player__vol input {
    width: 4.5rem;
    accent-color: var(--accent);
    cursor: pointer;
  }
  .player__body code {
    font-size: 0.85em;
    background: var(--bg-sunken);
    padding: 0 0.2em;
    border-radius: 3px;
  }
  @media (max-width: 700px) {
    .player__vol {
      display: none;
    }
  }
</style>
