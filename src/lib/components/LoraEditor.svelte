<script lang="ts">
  import { untrack } from "svelte";
  import { editLora, deleteLora, listFamilies } from "../api";
  import { refreshLoras, request } from "../stores";
  import type { LoraInfo } from "../types";

  let { lora, onClose }: { lora: LoraInfo; onClose: () => void } = $props();

  // Seeded once from the LoRA — the caller remounts per-LoRA, so there is
  // nothing to re-derive. Same contract as ModelEditor.
  let name = $state(untrack(() => lora.display_name));
  let family = $state(untrack(() => lora.family));
  let baseModel = $state(untrack(() => lora.base_model));
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmingDelete = $state(false);

  // Family strings come from the backend list, so the dropdown always matches
  // what the rest of the app calls a family.
  let families = $state<string[]>([]);
  $effect(() => {
    listFamilies().then((fs) => (families = fs)).catch(() => (families = []));
  });

  async function save() {
    busy = true; error = null;
    try {
      await editLora(lora.id, name, family, baseModel);
      await refreshLoras();
      onClose();
    } catch (e) { error = String(e); } finally { busy = false; }
  }

  async function doDelete() {
    busy = true; error = null;
    try {
      await deleteLora(lora.id);
      // Drop it from the run as well, or the next generate fails pre-flight on
      // a LoRA that no longer exists — and with the sidebar showing only what
      // is switched on, there would be no row left to turn off.
      request.update((r) => ({ ...r, loras: r.loras.filter((s) => s.name !== lora.name) }));
      await refreshLoras();
      onClose();
    } catch (e) { error = String(e); } finally { busy = false; }
  }
</script>

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onClose(); }} role="presentation">
  <div class="modal" role="dialog" aria-modal="true">
    <div class="modal-head">
      <span class="modal-title">Edit LoRA</span>
      <button class="modal-x" onclick={onClose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      {#if lora.broken}<p class="warn">⚠ This LoRA's file is missing. Re-add it, or delete the entry here.</p>{/if}
      {#if error}<p class="err">{error}</p>{/if}

      <div class="dlg-field">
        <p class="microlabel">Name</p>
        <input class="dlg-input" bind:value={name} />
      </div>
      <div class="dlg-field">
        <p class="microlabel">Family</p>
        <select class="dlg-select" bind:value={family}>
          <option value="">Unknown</option>
          {#each families as f}<option value={f}>{f}</option>{/each}
        </select>
      </div>
      <div class="dlg-field last">
        <p class="microlabel">Base model</p>
        <!-- Free text on purpose. It is a note to yourself, shown verbatim and
             never matched against anything, so anything that helps you
             recognise the right base model is a valid value. Civitai fills it
             at add-time; a local file or an older entry starts empty. -->
        <input class="dlg-input" bind:value={baseModel} placeholder="e.g. Flux.2 Klein 4B" />
        <p class="hint">Finer-grained than the family — often the only way to tell a 4B LoRA
          from a 9B one. Shown to you, never acted on.</p>
      </div>

      <!-- The pool filename is the engine's identity for this LoRA and is
           immutable. Showing it here is how the user finds out an add was
           auto-suffixed after a name collision. -->
      <p class="poolname">Stored as <code>{lora.name}.safetensors</code></p>
    </div>

    <div class="modal-foot">
      {#if confirmingDelete}
        <!-- Not "goes to trash": loras::remove unlinks the file outright. -->
        <span class="warn">Delete “{lora.display_name}”? The file is removed, not trashed.</span>
        <button class="btn btn-danger spacer" disabled={busy} onclick={doDelete}>Confirm delete</button>
        <button class="btn btn-ghost" disabled={busy} onclick={() => (confirmingDelete = false)}>Cancel</button>
      {:else}
        <button class="btn btn-danger" onclick={() => (confirmingDelete = true)}>Delete…</button>
        <button class="btn btn-ghost spacer" disabled={busy} onclick={onClose}>Cancel</button>
        <button class="btn btn-primary" disabled={busy} onclick={save}>Save</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .warn { font-size: 12px; color: var(--warn); }
  .err { color: var(--danger); font-size: 12px; margin: 0 0 10px; }
  .hint { margin: 6px 0 0; font-size: 11px; line-height: 1.45; color: var(--text-muted); }
  .poolname { margin: 18px 0 0; font-size: 11px; color: var(--text-muted); overflow-wrap: anywhere; }
  .poolname code { font-family: var(--mono, ui-monospace, monospace); font-size: 11px; }
</style>
