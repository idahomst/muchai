<script lang="ts">
  import { history, currentImage, currentItem, request, selectedModelId } from "../stores";
  import { imageSrc } from "../api";
  import type { GalleryItem } from "../types";

  type Group = { batchId: string; items: GalleryItem[] };

  // Group the flat (newest-first) history by batch. First-encounter order is
  // preserved, so groups stay in recency order; members within a batch are
  // ordered by batch_index (disk read order isn't guaranteed).
  function groupByBatch(items: GalleryItem[]): Group[] {
    const groups: Group[] = [];
    const byKey = new Map<string, Group>();
    for (const it of items) {
      const key = it.batch_id || it.id;
      let g = byKey.get(key);
      if (!g) { g = { batchId: key, items: [] }; byKey.set(key, g); groups.push(g); }
      g.items.push(it);
    }
    for (const g of groups) g.items.sort((a, b) => a.batch_index - b.batch_index);
    return groups;
  }

  $: groups = groupByBatch($history);

  function open(item: GalleryItem) {
    currentImage.set(imageSrc(item.image_path));
    currentItem.set(item);
    // Replaying history is ad-hoc: use the frozen ref verbatim, never re-resolve
    // against a since-changed manifest.
    request.set({ ...item.request, model_id: null });
    selectedModelId.set(null);
  }
</script>

<div class="strip">
  {#each groups as g (g.batchId)}
    {#if g.items.length > 1}
      <div class="batch">
        {#each g.items as item (item.id)}
          <button class="thumb" class:selected={item.id === $currentItem?.id}
                  on:click={() => open(item)} title={item.request.prompt}>
            <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
            <span class="idx">{item.batch_index + 1}</span>
          </button>
        {/each}
      </div>
    {:else}
      {@const item = g.items[0]}
      <button class="thumb" class:selected={item.id === $currentItem?.id}
              on:click={() => open(item)} title={item.request.prompt}>
        <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
      </button>
    {/if}
  {:else}
    <span class="empty">No images yet this session.</span>
  {/each}
</div>

<style>
  /* Thin auto-hide scrollbar so the strip stays quiet until hovered. */
  .strip { display:flex; gap:8px; overflow-x:auto; padding:6px 2px 8px; min-height:66px; align-items:center;
    scrollbar-width:thin; scrollbar-color:var(--border-strong) transparent; }
  .strip::-webkit-scrollbar { height:6px; }
  .strip::-webkit-scrollbar-thumb { background:var(--border-strong); border-radius:6px; }
  .strip::-webkit-scrollbar-thumb:hover { background:var(--text-faint); }
  .batch { display:flex; gap:8px; position:relative; padding-bottom:6px; }
  .batch::after { content:''; position:absolute; left:0; right:0; bottom:0; height:3px;
    background:var(--accent); border-radius:2px; }
  .thumb { padding:0; border:1px solid var(--border); border-radius:8px; overflow:hidden;
    width:58px; height:58px; flex:0 0 auto; cursor:pointer; background:none; position:relative; }
  .thumb img { width:100%; height:100%; object-fit:cover; display:block; }
  .thumb:hover { border-color:var(--border-strong); }
  /* Violet ring on the active thumb (matches focus rings elsewhere). */
  .thumb.selected { border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
  .idx { position:absolute; bottom:1px; left:1px; background:var(--overlay); color:var(--on-accent);
    font-size:.55rem; line-height:1.3; border-radius:3px; padding:0 .25rem; }
  .empty { opacity:.5; font-size:.8rem; }
</style>
