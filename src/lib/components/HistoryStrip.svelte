<script lang="ts">
  import { history, currentImage, currentItem, pendingRun, livePreview, viewingLive } from "../stores";
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

  // The pending tile sits at the head of the strip, which is off-screen if the
  // user had scrolled off to older images. Bring it into view when a run
  // starts, so the thing that just became the selected tile is visible.
  let strip: HTMLDivElement;
  $: if ($pendingRun && strip) strip.scrollTo({ left: 0, behavior: "smooth" });

  // Viewing only. Clicking a thumbnail used to overwrite the whole left panel,
  // which destroyed whatever the user was in the middle of composing just to
  // look at an earlier image. Reusing an image's settings is now the explicit
  // Load button in ParamsPanel.
  function open(item: GalleryItem) {
    currentImage.set(imageSrc(item.image_path));
    currentItem.set(item);
    // Leaving the run in flight. The draft keeps updating behind the pending
    // tile; this only stops it claiming the image area.
    viewingLive.set(false);
  }
</script>

<div class="strip" bind:this={strip}>
  <!-- The run in flight, at the head of the strip where its results will land.
       One tile per run, not per batch image: the engine writes a single draft
       file, so there is only ever one live frame to show. -->
  {#if $pendingRun}
    <button class="thumb pending" class:selected={$viewingLive}
            on:click={() => viewingLive.set(true)}
            title="Generating: {$pendingRun.prompt}">
      {#if $livePreview}
        <img src={$livePreview} alt="generation preview" />
      {:else}
        <span class="spinner" aria-hidden="true"></span>
      {/if}
      <span class="idx">live</span>
    </button>
  {/if}

  {#each groups as g (g.batchId)}
    {#if g.items.length > 1}
      <div class="batch">
        {#each g.items as item (item.id)}
          <button class="thumb" class:selected={!$viewingLive && item.id === $currentItem?.id}
                  on:click={() => open(item)} title={item.request.prompt}>
            <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
            <span class="idx">{item.batch_index + 1}</span>
          </button>
        {/each}
      </div>
    {:else}
      {@const item = g.items[0]}
      <button class="thumb" class:selected={!$viewingLive && item.id === $currentItem?.id}
              on:click={() => open(item)} title={item.request.prompt}>
        <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
      </button>
    {/if}
  {:else}
    {#if !$pendingRun}<span class="empty">No images yet this session.</span>{/if}
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
  /* Dashed while there is nothing to show yet, so the slot reads as "reserved"
     rather than as an image that failed to load. */
  .thumb.pending { border-style:dashed; border-color:var(--accent); display:grid; place-items:center;
    background:var(--card); }
  .spinner { width:18px; height:18px; border-radius:50%; border:2px solid var(--border-strong);
    border-top-color:var(--accent); animation:spin .8s linear infinite; }
  @keyframes spin { to { transform:rotate(360deg); } }
  .empty { opacity:.5; font-size:.8rem; }
</style>
