<script lang="ts">
  import { untrack } from "svelte";
  import { editModel, deleteModelEntry, pickModelFile } from "../api";
  import { refreshLibrary } from "../stores";
  import { VAE_FORMATS, PREDICTIONS, SAMPLERS } from "../types";
  import type { LibraryEntry, ManifestFlags, ModelComponents, GenDefaults, ModelRef, Sampler } from "../types";

  let { entry, onClose }: { entry: LibraryEntry; onClose: () => void } = $props();

  // Flatten the resolved model ref into six editable absolute-path slots.
  function slotsFrom(m: ModelRef) {
    if (m.type === "single_file") {
      return { diffusion_model: m.path, vae: "", clip_l: "", clip_g: "", t5xxl: "", llm: "" };
    }
    return {
      diffusion_model: m.diffusion_model,
      vae: m.vae ?? "", clip_l: m.clip_l ?? "", clip_g: m.clip_g ?? "",
      t5xxl: m.t5xxl ?? "", llm: m.llm ?? "",
    };
  }

  // Seeded once from the entry (caller remounts per-entry — no re-derivation).
  let name = $state(untrack(() => entry.name));
  let family = $state(untrack(() => entry.family));
  let vaeFormat = $state(untrack(() => entry.flags.vae_format ?? ""));
  let prediction = $state(untrack(() => entry.flags.prediction ?? ""));
  let slots = $state(untrack(() => slotsFrom(entry.model)));
  let overrideOn = $state(untrack(() => entry.recommended_settings != null));
  let rec = $state<GenDefaults>(
    untrack(() => entry.recommended_settings ?? { steps: 20, cfg_scale: 7, sampler: "euler_a" as Sampler, width: 512, height: 512 }),
  );
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmingDelete = $state(false);

  const FAMILIES = ["sd15", "sdxl", "flux1", "flux2", "sd3", "qwen-image", "custom"];
  type SlotKey = "diffusion_model" | "vae" | "clip_l" | "clip_g" | "t5xxl" | "llm";
  const OPTIONAL_ROLES: { key: SlotKey; label: string }[] = [
    { key: "vae", label: "VAE" },
    { key: "clip_l", label: "CLIP-L" },
    { key: "clip_g", label: "CLIP-G" },
    { key: "t5xxl", label: "T5-XXL" },
    { key: "llm", label: "LLM" },
  ];

  async function pick(key: SlotKey) {
    const p = await pickModelFile();
    if (p) slots[key] = p;
  }

  async function save() {
    busy = true; error = null;
    try {
      const flags: ManifestFlags = {
        vae_format: vaeFormat === "" ? null : vaeFormat,
        prediction: prediction === "" ? null : prediction,
      };
      const components: ModelComponents = {
        diffusion_model: slots.diffusion_model,
        vae: slots.vae || null,
        clip_l: slots.clip_l || null,
        clip_g: slots.clip_g || null,
        t5xxl: slots.t5xxl || null,
        llm: slots.llm || null,
      };
      await editModel(entry.id, name, family, flags, components, overrideOn ? rec : null);
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
    {#if entry.broken}<p class="warn">⚠ Some component files are missing. Re-pick them below, then Save.</p>{/if}
    {#if error}<p class="error">{error}</p>{/if}

    <label>Name<input bind:value={name} /></label>
    <label>Family
      <select bind:value={family}>
        {#each FAMILIES as f}<option value={f}>{f}</option>{/each}
      </select>
    </label>

    <div class="section">Components</div>
    <div class="slot">
      <span class="slot-label">Diffusion model</span>
      <span class="path" title={slots.diffusion_model}>{slots.diffusion_model || "(none)"}</span>
      <button disabled={busy} onclick={() => pick("diffusion_model")}>Change…</button>
    </div>
    {#each OPTIONAL_ROLES as role}
      <div class="slot">
        <span class="slot-label">{role.label}</span>
        <span class="path" title={slots[role.key]}>{slots[role.key] || "(none)"}</span>
        <button disabled={busy} onclick={() => pick(role.key)}>Change…</button>
        <button disabled={busy || !slots[role.key]} onclick={() => (slots[role.key] = "")} aria-label="Clear">×</button>
      </div>
    {/each}

    <div class="section">Engine flags</div>
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

    <div class="section">
      <label class="inline"><input type="checkbox" bind:checked={overrideOn} /> Custom recommended settings</label>
    </div>
    {#if overrideOn}
      <div class="rec-grid">
        <label>Steps<input type="number" min="1" max="150" bind:value={rec.steps} /></label>
        <label>CFG<input type="number" min="1" max="30" step="0.5" bind:value={rec.cfg_scale} /></label>
        <label>Width<input type="number" min="64" max="2048" step="64" bind:value={rec.width} /></label>
        <label>Height<input type="number" min="64" max="2048" step="64" bind:value={rec.height} /></label>
        <label class="wide">Sampler
          <select bind:value={rec.sampler}>
            {#each SAMPLERS as s}<option value={s.value}>{s.label}</option>{/each}
          </select>
        </label>
      </div>
    {/if}

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
  .dialog { background: var(--dialog-bg); border: 1px solid var(--border); border-radius: 10px; width: min(520px, 94vw); max-height: 90vh; overflow: auto; padding: 14px; display: flex; flex-direction: column; gap: 10px; color: var(--text); }
  header { display: flex; justify-content: space-between; align-items: center; }
  .x { background: none; border: none; cursor: pointer; color: inherit; }
  label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
  label.inline { flex-direction: row; align-items: center; gap: 6px; }
  .section { font-size: 12px; font-weight: 600; opacity: 0.75; border-top: 1px solid var(--border); padding-top: 8px; }
  .slot { display: flex; align-items: center; gap: 8px; font-size: 12px; }
  .slot-label { flex: 0 0 96px; }
  .path { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; opacity: 0.85; font-family: monospace; }
  .rec-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 8px; }
  .rec-grid .wide { grid-column: 1 / -1; }
  .footer { display: flex; gap: 8px; align-items: center; margin-top: 6px; }
  .spacer { flex: 1; }
  .danger { color: var(--danger); }
  .warn { font-size: 12px; color: var(--warn); }
  .error { color: var(--danger); font-size: 12px; }
  input, select { font: inherit; padding: 4px; }
</style>
