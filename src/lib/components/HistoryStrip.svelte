<script lang="ts">
  import { history, currentImage, request } from "../stores";
  import { imageSrc } from "../api";
  import type { GalleryItem } from "../types";
  function open(item: GalleryItem) {
    currentImage.set(imageSrc(item.image_path));
    request.set({ ...item.request });
  }
</script>

<div class="strip">
  {#each $history as item (item.id)}
    <button class="thumb" on:click={() => open(item)} title={item.request.prompt}>
      <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
    </button>
  {:else}
    <span class="empty">No images yet this session.</span>
  {/each}
</div>

<style>
  .strip { display:flex; gap:.4rem; overflow-x:auto; padding:.4rem; min-height:64px; align-items:center; }
  .thumb { padding:0; border:1px solid var(--border); border-radius:6px; overflow:hidden;
    width:56px; height:56px; flex:0 0 auto; cursor:pointer; background:none; }
  .thumb img { width:100%; height:100%; object-fit:cover; }
  .empty { opacity:.5; font-size:.8rem; }
</style>
