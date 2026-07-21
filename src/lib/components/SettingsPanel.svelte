<script lang="ts">
  import { request } from "../stores";
  import { SAMPLERS, FORMATS } from "../types";
  import type { GenDefaults, ModelRef } from "../types";
  import { recommendedSettings } from "../api";
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";

  // Recommended settings for the current model (null → family has no preset,
  // so the button is hidden). Re-fetched only when the model reference changes,
  // not on every params edit, guarded by a serialized-key comparison.
  let recommended: GenDefaults | null = null;
  let lastModelKey = "";
  $: {
    const key = JSON.stringify($request.model);
    if (key !== lastModelKey) {
      lastModelKey = key;
      void loadRecommended($request.model, key);
    }
  }
  async function loadRecommended(model: ModelRef, key: string) {
    try {
      const res = await recommendedSettings(model);
      if (key === lastModelKey) recommended = res;
    } catch {
      if (key === lastModelKey) recommended = null;
    }
  }
  function applyRecommended() {
    const r = recommended;
    if (!r) return;
    request.update((req) => ({
      ...req,
      steps: r.steps,
      cfg_scale: r.cfg_scale,
      sampler: r.sampler,
      width: r.width,
      height: r.height,
    }));
  }
</script>

<div class="grid">
  <label class="label"><span class="lbl-row">Steps <InfoHint text={HELP.steps} label="About steps" /></span>
    <input type="number" min="1" max="150" bind:value={$request.steps} />
  </label>
  <label class="label"><span class="lbl-row">CFG <InfoHint text={HELP.cfg} label="About CFG" /></span>
    <input type="number" min="1" max="30" step="0.5" bind:value={$request.cfg_scale} />
  </label>
  <label class="label"><span class="lbl-row">Width <InfoHint text={HELP.width} label="About width" /></span>
    <input type="number" min="64" max="2048" step="64" bind:value={$request.width} />
  </label>
  <label class="label"><span class="lbl-row">Height <InfoHint text={HELP.height} label="About height" /></span>
    <input type="number" min="64" max="2048" step="64" bind:value={$request.height} />
  </label>
  <label class="label"><span class="lbl-row">Sampler <InfoHint text={HELP.sampler} label="About sampler" /></span>
    <select bind:value={$request.sampler}>
      {#each SAMPLERS as s}<option value={s.value}>{s.label}</option>{/each}
    </select>
  </label>
  <label class="label"><span class="lbl-row">Batch <InfoHint text={HELP.batch} label="About batch count" /></span>
    <input type="number" min="1" max="8" bind:value={$request.batch_count} />
  </label>
  <label class="label"><span class="lbl-row">Format <InfoHint text={HELP.format} label="About format" /></span>
    <select bind:value={$request.output_format}>
      {#each FORMATS as f}<option value={f.value}>{f.label}</option>{/each}
    </select>
  </label>
  <label class="label seed"><span class="lbl-row">Seed (-1 = random) <InfoHint text={HELP.seed} label="About seed" /></span>
    <input type="number" bind:value={$request.seed} />
  </label>
</div>

{#if recommended}
  <button type="button" class="recommend-btn" on:click={applyRecommended}>
    Use recommended settings
  </button>
{/if}

<style>
  .grid { display:grid; grid-template-columns:1fr 1fr; gap:.5rem; }
  .label { display:flex; flex-direction:column; font-size:.75rem; gap:.2rem; }
  .lbl-row { display:inline-flex; align-items:center; gap:.2rem; }
  .seed { grid-column:1 / -1; }
  input, select { font:inherit; padding:.3rem; }
  .recommend-btn { margin-top:.5rem; width:100%; padding:.4rem; font:inherit; cursor:pointer; }
</style>
