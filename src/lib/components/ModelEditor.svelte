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

  const FAMILIES = ["sd15", "sdxl", "flux1", "flux2", "sd3", "qwen-image", "z-image", "custom"];
  type SlotKey = "diffusion_model" | "vae" | "clip_l" | "clip_g" | "t5xxl" | "llm";
  const OPTIONAL_ROLES: { key: SlotKey; label: string }[] = [
    { key: "vae", label: "VAE" },
    { key: "clip_l", label: "CLIP-L" },
    { key: "clip_g", label: "CLIP-G" },
    { key: "t5xxl", label: "T5-XXL" },
    { key: "llm", label: "LLM" },
  ];

  // Parent folder of an absolute path (POSIX or Windows separator), or undefined.
  function parentDir(p: string): string | undefined {
    const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
    return i > 0 ? p.slice(0, i) : undefined;
  }

  async function pick(key: SlotKey) {
    // Start in the slot's current folder, falling back to the diffusion model's
    // folder so component re-picks land next to the model, not in the CWD.
    const cur = slots[key] || slots.diffusion_model;
    const p = await pickModelFile(cur ? parentDir(cur) : undefined);
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

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onClose(); }} role="presentation">
  <div class="modal" role="dialog" aria-modal="true">
    <div class="modal-head">
      <span class="modal-title">Edit model</span>
      <button class="modal-x" onclick={onClose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      {#if entry.broken}<p class="warn">⚠ Some component files are missing. Re-pick them below, then Save.</p>{/if}
      {#if error}<p class="err">{error}</p>{/if}

      <div class="dlg-field">
        <p class="microlabel">Name</p>
        <input class="dlg-input" bind:value={name} />
      </div>
      <div class="dlg-field">
        <p class="microlabel">Family</p>
        <select class="dlg-select" bind:value={family}>
          {#each FAMILIES as f}<option value={f}>{f}</option>{/each}
        </select>
      </div>

      <p class="section-hdr">Components</p>
      <div class="comp">
        <span class="ck">Diffusion</span>
        <span class="cpath" class:none={!slots.diffusion_model} title={slots.diffusion_model}>
          {slots.diffusion_model ? "‏" + slots.diffusion_model : "Not set"}</span>
        <button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => pick("diffusion_model")}>Change…</button>
        <span class="rm" aria-hidden="true"></span>
      </div>
      {#each OPTIONAL_ROLES as role}
        <div class="comp">
          <span class="ck">{role.label}</span>
          <span class="cpath" class:none={!slots[role.key]} title={slots[role.key]}>
            {slots[role.key] ? "‏" + slots[role.key] : "Not set"}</span>
          <button class="btn btn-ghost btn-sm" disabled={busy} onclick={() => pick(role.key)}>Change…</button>
          {#if slots[role.key]}
            <button class="rm" disabled={busy} onclick={() => (slots[role.key] = "")}
              aria-label="Remove {role.label}" title="Remove">✕</button>
          {:else}
            <span class="rm" aria-hidden="true"></span>
          {/if}
        </div>
      {/each}

      <p class="section-hdr">Engine flags</p>
      <div class="dlg-field">
        <p class="microlabel">VAE format</p>
        <select class="dlg-select" bind:value={vaeFormat}>
          <option value="">Default</option>
          {#each VAE_FORMATS.filter((v) => v) as v}<option value={v}>{v}</option>{/each}
        </select>
      </div>
      <div class="dlg-field last">
        <p class="microlabel">Prediction</p>
        <select class="dlg-select" bind:value={prediction}>
          <option value="">Default</option>
          {#each PREDICTIONS.filter((p) => p) as p}<option value={p}>{p}</option>{/each}
        </select>
      </div>

      <label class="check">
        <input type="checkbox" bind:checked={overrideOn} />
        <span class="check-box"></span>
        <span>Custom recommended settings</span>
      </label>
      {#if overrideOn}
        <div class="rec-grid">
          <div class="dlg-field"><p class="microlabel">Steps</p><input class="dlg-input" type="number" min="1" max="150" bind:value={rec.steps} /></div>
          <div class="dlg-field"><p class="microlabel">CFG</p><input class="dlg-input" type="number" min="1" max="30" step="0.5" bind:value={rec.cfg_scale} /></div>
          <div class="dlg-field"><p class="microlabel">Width</p><input class="dlg-input" type="number" min="64" max="2048" step="64" bind:value={rec.width} /></div>
          <div class="dlg-field"><p class="microlabel">Height</p><input class="dlg-input" type="number" min="64" max="2048" step="64" bind:value={rec.height} /></div>
          <div class="dlg-field wide last"><p class="microlabel">Sampler</p>
            <select class="dlg-select" bind:value={rec.sampler}>
              {#each SAMPLERS as s}<option value={s.value}>{s.label}</option>{/each}
            </select>
          </div>
        </div>
      {/if}
    </div>

    <div class="modal-foot">
      {#if confirmingDelete}
        <span class="warn">Delete “{entry.name}”? Files go to trash.</span>
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
  .dlg-field.last { margin-bottom: 0; }
  .comp { display: grid; grid-template-columns: 96px 1fr auto auto; align-items: center; gap: 12px;
    padding: 8px 0; border-bottom: 1px solid var(--border); }
  .comp:last-of-type { border-bottom: none; }
  .ck { font-size: 12.5px; color: var(--text-muted); font-weight: 550; }
  .cpath { font-size: 12px; color: var(--text); font-family: var(--mono);
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; direction: rtl; text-align: left; }
  .cpath.none { color: var(--text-muted); font-family: inherit; font-style: italic; direction: ltr; }
  .rm { width: 26px; height: 26px; border-radius: 6px; display: grid; place-items: center;
    color: var(--text-muted); background: transparent; border: none; cursor: pointer; font-size: 13px; }
  button.rm:hover:not(:disabled) { background: var(--card-hover); color: var(--danger); }
  .rec-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 0 12px; margin-top: 14px; }
  .rec-grid .wide { grid-column: 1 / -1; }
  .warn { font-size: 12px; color: var(--warn); }
  .err { color: var(--danger); font-size: 12px; margin: 0 0 10px; }
</style>
