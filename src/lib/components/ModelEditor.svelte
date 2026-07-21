<script lang="ts">
  import { untrack } from "svelte";
  import { editModel, deleteModelEntry } from "../api";
  import { refreshLibrary } from "../stores";
  import { VAE_FORMATS, PREDICTIONS } from "../types";
  import type { LibraryEntry, ManifestFlags } from "../types";

  let { entry, onClose }: { entry: LibraryEntry; onClose: () => void } = $props();

  // Local editable copies, seeded once from the entry passed in (not re-derived
  // as `entry` changes — the dialog is remounted per-entry by its caller).
  let name = $state(untrack(() => entry.name));
  let family = $state(untrack(() => entry.family));
  // "" == default (null). Pre-load from the manifest flags carried on the entry.
  let vaeFormat = $state(untrack(() => entry.flags.vae_format ?? ""));
  let prediction = $state(untrack(() => entry.flags.prediction ?? ""));
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmingDelete = $state(false);

  const FAMILIES = ["sd15", "sdxl", "flux1", "flux2", "sd3", "qwen-image", "custom"];

  async function save() {
    busy = true; error = null;
    try {
      const flags: ManifestFlags = {
        vae_format: vaeFormat === "" ? null : vaeFormat,
        prediction: prediction === "" ? null : prediction,
      };
      await editModel(entry.id, name, family, flags);
      await refreshLibrary();
      onClose();
    } catch (e) { error = String(e); } finally { busy = false; }
  }

  async function doDelete() {
    busy = true; error = null;
    try {
      await deleteModelEntry(entry.id);
      await refreshLibrary();
      onClose();
    } catch (e) { error = String(e); } finally { busy = false; }
  }
</script>

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onClose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true">
    <header><b>Edit model</b><button class="x" onclick={onClose} aria-label="Close">✕</button></header>
    {#if error}<p class="error">{error}</p>{/if}

    <label>Name<input bind:value={name} /></label>
    <label>Family
      <select bind:value={family}>
        {#each FAMILIES as f}<option value={f}>{f}</option>{/each}
      </select>
    </label>
    <label>VAE format
      <select bind:value={vaeFormat}>
        <option value="">Default</option>
        {#each VAE_FORMATS.filter((v) => v) as v}<option value={v}>{v}</option>{/each}
      </select>
    </label>
    <label>Prediction
      <select bind:value={prediction}>
        <option value="">Default</option>
        {#each PREDICTIONS.filter((p) => p) as p}<option value={p}>{p}</option>{/each}
      </select>
    </label>

    <div class="footer">
      {#if confirmingDelete}
        <span class="warn">Delete "{entry.name}"? Files go to trash.</span>
        <button class="danger" disabled={busy} onclick={doDelete}>Confirm delete</button>
        <button disabled={busy} onclick={() => (confirmingDelete = false)}>Cancel</button>
      {:else}
        <button class="danger" onclick={() => (confirmingDelete = true)}>Delete…</button>
        <span class="spacer"></span>
        <button disabled={busy} onclick={save}>Save</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: var(--backdrop); display: flex; align-items: center; justify-content: center; z-index: 50; }
  .dialog { background: var(--dialog-bg); border: 1px solid var(--border); border-radius: 10px; width: min(440px, 92vw); padding: 14px; display: flex; flex-direction: column; gap: 10px; color: var(--text); }
  header { display: flex; justify-content: space-between; align-items: center; }
  .x { background: none; border: none; cursor: pointer; color: inherit; }
  label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
  .footer { display: flex; gap: 8px; align-items: center; margin-top: 6px; }
  .spacer { flex: 1; }
  .danger { color: var(--danger); }
  .warn { font-size: 12px; color: var(--warn); }
  .error { color: var(--danger); font-size: 12px; }
</style>
