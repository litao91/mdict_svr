<script lang="ts">
  import { Dialog } from 'svelte-widgets'

  let {
    image = $bindable(null),
  }: {
    image?: { src: string; alt: string } | null
  } = $props()
</script>

<Dialog
  open={image !== null}
  backdrop_dim
  backdrop_blur
  closedby="any"
  on_close={() => (image = null)}
>
  {#snippet children({ close })}
    <div class="lightbox" onclick={close} role="presentation">
      {#if image}
        <img class="lightbox-img" src={image.src} alt={image.alt || 'Dictionary illustration'} />
        <p class="lightbox-cap">{image.alt || decodeURIComponent(image.src.split('/').pop() ?? image.src)}</p>
      {/if}
    </div>
  {/snippet}
</Dialog>

<style>
  .lightbox {
    display: flex;
    flex-direction: column;
    gap: 0.5rem;
    cursor: zoom-out;
  }
</style>
