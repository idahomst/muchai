<script lang="ts">
  import { get } from "svelte/store";
  import { currentImage, currentItem, history, request } from "../stores";
  import { deleteImage, listHistory, imageSrc } from "../api";

  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function doDelete() {
    const item = get(currentItem);
    if (!item || busy) return;
    busy = true;
    error = null;
    try {
      const idx = get(history).findIndex((x) => x.id === item.id);
      await deleteImage(item.image_path);
      const next = await listHistory();
      history.set(next);
      // Select the item that slid into the deleted slot, else the new last, else clear.
      const pick = next[Math.min(idx, next.length - 1)] ?? null;
      if (pick) {
        currentImage.set(imageSrc(pick.image_path));
        currentItem.set(pick);
        request.set({ ...pick.request });
      } else {
        currentImage.set(null);
        currentItem.set(null);
      }
      confirming = false;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="preview">
  {#if $currentImage}
    <img src={$currentImage} alt="generated result" />
    <div class="actions">
      {#if confirming}
        <span class="ask">Move to trash?</span>
        <button class="del" onclick={doDelete} disabled={busy}>Delete</button>
        <button onclick={() => (confirming = false)} disabled={busy}>Cancel</button>
      {:else}
        <button class="del" onclick={() => (confirming = true)} disabled={!$currentItem}>Delete</button>
      {/if}
    </div>
    {#if error}<span class="err">{error}</span>{/if}
  {:else}
    <p class="empty">Your generated image will appear here.</p>
  {/if}
</div>

<style>
  .preview { flex:1; display:flex; align-items:center; justify-content:center; position:relative;
    background:rgba(0,0,0,.2); border-radius:8px; overflow:hidden; }
  img { max-width:100%; max-height:100%; object-fit:contain; }
  .actions { position:absolute; top:.5rem; right:.5rem; display:flex; gap:.4rem; align-items:center; }
  .actions button { font:inherit; font-size:.75rem; padding:.25rem .6rem; cursor:pointer; }
  .actions button:disabled { opacity:.5; cursor:default; }
  .del { background:rgba(180,40,40,.85); color:#fff; border:none; border-radius:5px; }
  .ask { font-size:.75rem; background:rgba(0,0,0,.6); color:#fff; padding:.25rem .5rem; border-radius:5px; }
  .err { position:absolute; bottom:.5rem; left:.5rem; right:.5rem; color:#ffb4b4; font-size:.75rem;
    background:rgba(0,0,0,.6); padding:.3rem .5rem; border-radius:5px; }
  .empty { opacity:.5; }
</style>
