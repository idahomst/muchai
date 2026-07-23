<script lang="ts">
  import { get } from "svelte/store";
  import { currentImage, currentItem, history, request, livePreview } from "../stores";
  import { deleteImage, listHistory, imageSrc } from "../api";

  // The live draft (if any) wins over the settled image. When a draft 404s
  // (engine hasn't written the first frame yet) we clear it so the fallback
  // shows and never render a broken-image icon.
  const shown = $derived($livePreview ?? $currentImage);
  const isPreview = $derived($livePreview !== null);

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
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      confirming = false; // close the prompt whether the delete succeeded or failed
    }
  }

  function cancel() {
    confirming = false;
    error = null;
  }
</script>

<div class="preview">
  {#if shown}
    <img src={shown} alt={isPreview ? "generation preview" : "generated result"}
      onerror={() => { if (isPreview) livePreview.set(null); }} />
    {#if isPreview}
      <span class="badge">Preview</span>
    {:else}
      <div class="actions">
        {#if confirming}
          <span class="ask">Move to trash?</span>
          <button class="del" onclick={doDelete} disabled={busy}>Delete</button>
          <button onclick={cancel} disabled={busy}>Cancel</button>
        {:else}
          <button class="del" onclick={() => (confirming = true)} disabled={!$currentItem}>Delete</button>
        {/if}
      </div>
      {#if error}<span class="err">{error}</span>{/if}
    {/if}
  {:else}
    <div class="empty">
      <p class="empty-title">Your image will appear here.</p>
      <p class="empty-sub">Pick a model, write a prompt, then press Generate.</p>
    </div>
  {/if}
</div>

<style>
  .preview { flex:1; display:flex; align-items:center; justify-content:center; position:relative;
    background:var(--overlay-soft); border-radius:8px; overflow:hidden; }
  img { max-width:100%; max-height:100%; object-fit:contain; }
  .actions { position:absolute; top:.5rem; right:.5rem; display:flex; gap:.4rem; align-items:center; }
  .actions button { font:inherit; font-size:.75rem; padding:.25rem .6rem; cursor:pointer; }
  .actions button:disabled { opacity:.5; cursor:default; }
  .del { background:var(--danger-bg); color:var(--on-accent); border:none; border-radius:5px; }
  .ask { font-size:.75rem; background:var(--overlay); color:var(--on-accent); padding:.25rem .5rem; border-radius:5px; }
  .badge { position:absolute; top:.5rem; right:.5rem; font-size:.7rem; letter-spacing:.03em;
    background:var(--overlay); color:var(--on-accent); padding:.25rem .55rem; border-radius:5px; }
  .err { position:absolute; bottom:.5rem; left:.5rem; right:.5rem; color:var(--danger-soft); font-size:.75rem;
    background:var(--overlay); padding:.3rem .5rem; border-radius:5px; }
  .empty { opacity:.55; text-align:center; padding:1rem; }
  .empty-title { margin:0 0 .3rem; font-size:.95rem; }
  .empty-sub { margin:0; font-size:.8rem; opacity:.85; }
</style>
