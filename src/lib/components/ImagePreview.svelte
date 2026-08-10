<script lang="ts">
  import { get } from "svelte/store";
  import { currentImage, currentItem, history, livePreview, library, selectedModelId,
           editFamilies, request, catalogHighlightId, revealModelPicker } from "../stores";
  import { deleteImage, listHistory, imageSrc, openFolder, pickRefImage } from "../api";

  let { onNeedEditModel }: { onNeedEditModel: () => void } = $props();

  // The live draft (if any) wins over the settled image. When a draft 404s
  // (engine hasn't written the first frame yet) we clear it so the fallback
  // shows and never render a broken-image icon.
  const shown = $derived($livePreview ?? $currentImage);
  const isPreview = $derived($livePreview !== null);

  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  // Reset the transient confirm/error whenever the selected image changes
  // (e.g. picking another filmstrip thumb) — otherwise an open "Move to trash?"
  // prompt would silently retarget the newly-selected image. Reading the id
  // registers it as the effect's dependency.
  $effect(() => {
    void $currentItem?.id;
    confirming = false;
    error = null;
  });

  // Strip the trailing path segment to get the containing directory. Open-folder
  // opens that dir in the OS file manager (open_path on the file itself would
  // launch an image viewer instead).
  const parentDir = (p: string) => p.replace(/[/\\][^/\\]*$/, "");

  async function openContainingFolder() {
    const item = get(currentItem);
    if (!item) return;
    error = null;
    try {
      await openFolder(parentDir(item.image_path));
    } catch (e) {
      error = String(e);
    }
  }

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
        // Viewing only, like every other selection — the left panel is the
        // user's working state and a delete must not rewrite it.
        currentImage.set(imageSrc(pick.image_path));
        currentItem.set(pick);
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

  /** The catalog entry offered when the user wants to edit but owns no edit
   *  model. Matches the id in resources/catalog.json. */
  const EDIT_CATALOG_ID = "qwen-image-edit-2511-q3ks";

  const editModels = $derived($library.filter((e) => $editFamilies.includes(e.family)));
  const selectedIsEditModel = $derived(
    editModels.some((e) => e.id === $selectedModelId),
  );

  /** Set the shown image as the reference and make sure an edit model is
   *  selected. Also the iteration loop: the result lands in the gallery, and
   *  pressing this again edits that. */
  async function editThisImage() {
    const item = get(currentItem);
    if (!item || busy) return;
    error = null;
    // No edit model at all: the catalog is the only way forward, so open it
    // pointed at the entry rather than leaving the user to find it.
    if (editModels.length === 0) {
      catalogHighlightId.set(EDIT_CATALOG_ID);
      onNeedEditModel();
      return;
    }
    busy = true;
    try {
      const info = await pickRefImage(item.image_path);
      if (!selectedIsEditModel) {
        const pick = editModels[0];
        selectedModelId.set(pick.id);
        request.update((r) => ({ ...r, model: pick.model, model_id: pick.id }));
        // Several to choose from: show the picker so the automatic choice is
        // visible and changeable, rather than silently deciding for them.
        if (editModels.length > 1) revealModelPicker.set(true);
      }
      request.update((r) => ({
        ...r,
        ref_images: [info.path],
        width: info.suggested_width,
        height: info.suggested_height,
      }));
      document.getElementById("prompt")?.focus();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="preview">
  {#if shown}
    <img class="photo" src={shown} alt={isPreview ? "generation preview" : "generated result"}
      onerror={() => { if (isPreview) livePreview.set(null); }} />
    {#if isPreview}
      <span class="badge">Preview</span>
    {:else}
      <div class="toolbar">
        {#if confirming}
          <span class="ask">Move to trash?</span>
          <button class="tool danger" onclick={doDelete} disabled={busy}
            aria-label="Confirm delete" title="Confirm delete">Delete</button>
          <button class="tool" onclick={cancel} disabled={busy}
            aria-label="Cancel" title="Cancel">Cancel</button>
        {:else}
          <button class="tool" onclick={editThisImage} disabled={!$currentItem || busy}
            aria-label="Edit this image" title="Edit this image">
            <span aria-hidden="true">✎</span>
          </button>
          <button class="tool" onclick={openContainingFolder} disabled={!$currentItem}
            aria-label="Open containing folder" title="Open folder">
            <span aria-hidden="true">🗀</span>
          </button>
          <button class="tool danger" onclick={() => (confirming = true)} disabled={!$currentItem}
            aria-label="Delete image" title="Delete">
            <span aria-hidden="true">🗑</span>
          </button>
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
  /* Matte: a subtle two-tone checker so any output aspect ratio reads
     intentionally; the image floats on it with a soft shadow + hairline. */
  /* No overflow:hidden — the background checker already clips to the rounded
     corners, and clipping would crop the image's soft drop shadow. */
  /* min-height:0 is load-bearing: without it the flex item's default
     min-height:auto refuses to shrink below the image's intrinsic (width-driven)
     height, so a wide window makes the image push Parameters + gallery off-screen.
     With it, the flex algorithm caps .preview's height and the image's
     max-height:100% resolves against that bound instead of driving it. */
  .preview { flex:1; min-height:0; display:flex; align-items:center; justify-content:center; position:relative;
    padding:24px; border-radius:8px;
    background:repeating-conic-gradient(var(--matte-2) 0% 25%, var(--matte) 0% 50%);
    background-size:22px 22px; }
  .photo { max-width:100%; max-height:100%; object-fit:contain; border-radius:8px;
    /* Black shadow sits on image pixels — theme-independent, like the mockup. */
    box-shadow:0 10px 30px rgba(0,0,0,.45), 0 0 0 1px var(--border-subtle); }

  /* Quiet floating toolbar, top-right. Ghost by default; trash reddens on hover. */
  .toolbar { position:absolute; top:14px; right:14px; display:flex; gap:6px; align-items:center; }
  /* The pill floats over arbitrary image pixels, so it can't lean on theme
     surface/text tokens — it uses a fixed dark scrim + light glyph + blur so
     it stays legible over both bright and dark images, in either theme. */
  .tool { min-width:32px; height:32px; padding:0 .5rem; border-radius:8px; display:grid; place-items:center;
    cursor:pointer; font:inherit; font-size:13px; color:rgba(255,255,255,.9);
    background:rgba(0,0,0,.55); border:1px solid rgba(255,255,255,.18);
    backdrop-filter:blur(8px); -webkit-backdrop-filter:blur(8px);
    box-shadow:0 2px 8px rgba(0,0,0,.35); }
  .tool:hover:not(:disabled) { background:rgba(0,0,0,.78); color:#fff; border-color:rgba(255,255,255,.32); }
  .tool:disabled { opacity:.5; cursor:default; }
  .tool.danger:hover:not(:disabled) { color:var(--danger-soft); border-color:var(--danger); }

  .ask { font-size:.75rem; background:var(--overlay); color:var(--on-accent);
    padding:.3rem .55rem; border-radius:8px; }
  .badge { position:absolute; top:14px; right:14px; font-size:.7rem; letter-spacing:.03em;
    background:var(--overlay); color:var(--on-accent); padding:.25rem .55rem; border-radius:5px; }
  .err { position:absolute; bottom:.5rem; left:.5rem; right:.5rem; color:var(--danger-soft); font-size:.75rem;
    background:var(--overlay); padding:.3rem .5rem; border-radius:5px; }
  .empty { opacity:.55; text-align:center; padding:1rem; }
  .empty-title { margin:0 0 .3rem; font-size:.95rem; }
  .empty-sub { margin:0; font-size:.8rem; opacity:.85; }
</style>
