<script lang="ts">
  import { get } from "svelte/store";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { request, isEditingModel } from "../stores";
  import { pickRefImage, pickRefImageDialog, imageSrc } from "../api";
  import type { RefImageInfo } from "../types";
  import { HELP } from "../helpText";
  import InfoHint from "./InfoHint.svelte";

  let refs = $state<string[]>([]);
  request.subscribe((r) => (refs = r.ref_images));

  let canEdit = $state(false);
  isEditingModel.subscribe((v) => (canEdit = v));

  // Dimensions of the current reference, filled by the last successful pick.
  // Null after a reload (the request persists paths, not sizes) — the panel
  // still shows the file, just without the size line, rather than re-reading
  // every image at startup.
  let info = $state<RefImageInfo | null>(null);
  let error = $state<string | null>(null);
  let note = $state<string | null>(null);
  let dragOver = $state(false);
  let busy = $state(false);

  const current = $derived(refs[0] ?? null);
  const filename = $derived(current ? (current.split(/[/\\]/).pop() ?? current) : "");

  // Drop the remembered dimensions when the path changes underneath us — a
  // replayed history item sets ref_images directly, and showing the previous
  // image's size next to a different filename is worse than showing none.
  $effect(() => {
    const path = current;
    if (info && info.path !== path) info = null;
  });

  async function accept(path: string) {
    busy = true;
    error = null;
    try {
      const got = await pickRefImage(path);
      info = got;
      // The suggested size is applied, not merely offered: a mismatched aspect
      // ratio is the single most common way an edit comes back stretched, and
      // the note below makes the change visible rather than silent.
      request.update((r) => ({
        ...r,
        ref_images: [got.path],
        width: got.suggested_width,
        height: got.suggested_height,
      }));
      note = `Size matched to your image — ${got.suggested_width}×${got.suggested_height}`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function choose() {
    const dir = current ? current.replace(/[/\\][^/\\]*$/, "") : undefined;
    const picked = await pickRefImageDialog(dir);
    if (picked) await accept(picked);
  }

  function clear() {
    request.update((r) => ({ ...r, ref_images: [] }));
    info = null;
    note = null;
    error = null;
  }

  // Tauri delivers native file drops as a webview event, not as a DOM drop —
  // the DOM event carries no usable path. The hover/cancel phases only drive
  // the highlight. Registered once and torn down with the component.
  $effect(() => {
    let un: (() => void) | null = null;
    let disposed = false;
    getCurrentWebview()
      .onDragDropEvent((e) => {
        if (!get(isEditingModel)) return;
        if (e.payload.type === "over") dragOver = true;
        else if (e.payload.type === "leave") dragOver = false;
        else if (e.payload.type === "drop") {
          dragOver = false;
          const first = e.payload.paths[0];
          if (first) void accept(first);
        }
      })
      .then((f) => {
        if (disposed) f();
        else un = f;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  });
</script>

{#if canEdit}
  <div class="head">
    <p class="section">Image to edit</p>
    <InfoHint text={HELP.refImage} label="About the image to edit" />
    {#if current}
      <button class="clear" type="button" onclick={clear}>Clear</button>
    {/if}
  </div>

  {#if current}
    <div class="picked">
      <img class="thumb" src={imageSrc(current)} alt="Reference" />
      <div class="meta">
        <span class="name" title={current}>{filename}</span>
        {#if info}
          <span class="dims">{info.width}×{info.height}</span>
        {/if}
        <button class="change" type="button" onclick={choose} disabled={busy}>Change…</button>
      </div>
    </div>
  {:else}
    <!-- Not a DOM drop target: the drop is handled by the webview event above,
         so this only needs to look like one and offer the picker. -->
    <div class="drop" class:over={dragOver}>
      <p>Drop an image here</p>
      <button class="choose" type="button" onclick={choose} disabled={busy}>Choose a file…</button>
    </div>
  {/if}

  {#if note}
    <div class="notice" role="status">
      <span>{note}</span>
      <button class="notice-x" aria-label="Dismiss" onclick={() => (note = null)}>✕</button>
    </div>
  {/if}
  {#if error}
    <div class="notice err" role="alert">
      <span>{error}</span>
      <button class="notice-x" aria-label="Dismiss" onclick={() => (error = null)}>✕</button>
    </div>
  {/if}
{/if}

<style>
  .head { display:flex; align-items:center; margin-bottom:12px; }
  .section { font-size:10.5px; letter-spacing:.05em; text-transform:uppercase;
    font-weight:600; color:var(--text-muted); margin:0; cursor:default; }
  .clear { margin-left:auto; font:inherit; font-size:10.5px; font-weight:550;
    color:var(--text-muted); cursor:pointer; padding:2px 6px; border-radius:5px;
    background:transparent; border:none; }
  .clear:hover { background:var(--card-hover); color:var(--text); }

  .drop { display:flex; flex-direction:column; align-items:center; gap:8px;
    padding:18px 12px; border:1px dashed var(--border-strong);
    border-radius:var(--radius-sm); color:var(--text-muted); font-size:12px; }
  .drop.over { border-color:var(--accent); background:var(--accent-soft); }
  .drop p { margin:0; }
  .choose { font:inherit; font-size:12px; padding:5px 10px; cursor:pointer;
    border-radius:var(--radius-sm); border:1px solid var(--border);
    background:var(--card); color:var(--text); }
  .choose:hover:not(:disabled) { background:var(--card-hover); }
  .choose:disabled { opacity:.5; cursor:default; }

  .picked { display:flex; gap:10px; align-items:flex-start; }
  .thumb { width:72px; height:72px; object-fit:cover; flex:0 0 auto;
    border-radius:var(--radius-sm); border:1px solid var(--border);
    background:var(--card); }
  .meta { display:flex; flex-direction:column; gap:4px; min-width:0; }
  .name { font-size:12px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .dims { font-size:11px; color:var(--text-muted); }
  .change { align-self:flex-start; font:inherit; font-size:11px; padding:3px 8px;
    cursor:pointer; border-radius:5px; border:1px solid var(--border);
    background:var(--card); color:var(--text); }
  .change:hover:not(:disabled) { background:var(--card-hover); }
  .change:disabled { opacity:.5; cursor:default; }

  .notice { display:flex; align-items:flex-start; gap:8px; margin-top:10px;
    padding:8px 10px; border-radius:var(--radius-sm); font-size:12px; line-height:1.4;
    background:var(--card); color:var(--text-muted); }
  .notice.err { background:var(--warn-tint); color:var(--warn); }
  .notice span { flex:1; }
  .notice-x { flex:0 0 auto; width:18px; height:18px; display:grid; place-items:center;
    border:none; background:transparent; color:inherit; cursor:pointer;
    font-size:11px; border-radius:4px; }

  /* The panel emits nothing at all for a non-edit model, so its own spacing —
     not a divider in the parent — is what separates it from the prompt below. */
  .head { margin-top:0; }
  .drop, .picked { margin-bottom:16px; }
  .notice:last-child { margin-bottom:16px; }
</style>
