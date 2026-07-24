<script lang="ts">
  import { request, selectedModelId } from "../stores";
  import { SAMPLERS, FORMATS } from "../types";
  import type { GenDefaults } from "../types";
  import { recommendedSettings } from "../api";
  import { HELP } from "../helpText";
  import NumberStepper from "./NumberStepper.svelte";

  // Recommended settings for the current model (null → family has no preset,
  // so the button is hidden). Re-fetched only when the selected model id
  // changes, not on every params edit.
  let recommended: GenDefaults | null = null;
  let lastId: string | null = null;
  $: {
    const id = $selectedModelId;
    if (id !== lastId) {
      lastId = id;
      void loadRecommended(id);
    }
  }
  async function loadRecommended(id: string | null) {
    if (!id) { recommended = null; return; }
    try {
      const res = await recommendedSettings(id);
      if (id === lastId) recommended = res;   // discard stale fetch (a501753)
    } catch {
      if (id === lastId) recommended = null;
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
  // ⟳ pins a fresh concrete seed (so the result is reproducible), unlike the
  // -1 "random each run" sentinel.
  function randomizeSeed() { $request.seed = Math.floor(Math.random() * 1_000_000_000); }
</script>

<div class="rowpair">
  <div class="field">
    <label class="flabel" for="steps" title={HELP.steps}>Steps</label>
    <NumberStepper id="steps" ariaLabel="Steps" min={1} max={150} step={1} bind:value={$request.steps} />
  </div>
  <div class="field">
    <label class="flabel" for="cfg" title={HELP.cfg}>CFG</label>
    <NumberStepper id="cfg" ariaLabel="CFG" min={1} max={30} step={0.5} bind:value={$request.cfg_scale} />
  </div>
</div>
<div class="rowpair">
  <div class="field">
    <label class="flabel" for="width" title={HELP.width}>Width</label>
    <NumberStepper id="width" ariaLabel="Width" min={64} max={2048} step={64} bind:value={$request.width} />
  </div>
  <div class="field">
    <label class="flabel" for="height" title={HELP.height}>Height</label>
    <NumberStepper id="height" ariaLabel="Height" min={64} max={2048} step={64} bind:value={$request.height} />
  </div>
</div>
<div class="rowpair">
  <div class="field">
    <label class="flabel" for="sampler" title={HELP.sampler}>Sampler</label>
    <select id="sampler" class="select" bind:value={$request.sampler}>
      {#each SAMPLERS as s}<option value={s.value}>{s.label}</option>{/each}
    </select>
  </div>
  <div class="field">
    <label class="flabel" for="batch" title={HELP.batch}>Batch</label>
    <NumberStepper id="batch" ariaLabel="Batch" min={1} max={8} step={1} bind:value={$request.batch_count} />
  </div>
</div>
<div class="rowpair">
  <div class="field">
    <label class="flabel" for="format" title={HELP.format}>Format</label>
    <select id="format" class="select" bind:value={$request.output_format}>
      {#each FORMATS as f}<option value={f.value}>{f.label}</option>{/each}
    </select>
  </div>
  <div class="field">
    <label class="flabel" for="seed" title={HELP.seed}>Seed (-1 = random)</label>
    <div class="num">
      <input class="val" id="seed" type="number" aria-label="Seed" bind:value={$request.seed} />
      <button type="button" class="stp" title="Randomize seed" aria-label="Randomize seed" on:click={randomizeSeed}>⟳</button>
    </div>
  </div>
</div>

{#if recommended}
  <button type="button" class="ghostbtn" on:click={applyRecommended}>Use recommended settings</button>
{/if}

<style>
  .rowpair { display:grid; grid-template-columns:1fr 1fr; gap:12px; margin-bottom:16px; }
  .field { display:flex; flex-direction:column; min-width:0; }
  .flabel { font-size:11px; letter-spacing:.03em; text-transform:uppercase; font-weight:600;
    color:var(--text-muted); margin-bottom:6px; cursor:default; }
  .select { width:100%; background:var(--card); border:1px solid var(--border);
    border-radius:var(--radius-sm); color:var(--text); font:inherit; font-size:13px;
    padding:9px 11px; cursor:pointer; }
  .select:focus { outline:none; border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
  /* Seed field: number input + ⟳ randomize, same shell as NumberStepper. */
  .num { display:flex; align-items:center; background:var(--card); border:1px solid var(--border);
    border-radius:var(--radius-sm); overflow:hidden; }
  .num:focus-within { border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
  .val { flex:1; min-width:0; background:none; border:none; color:var(--text); font:inherit;
    font-size:13px; padding:9px 11px; outline:none; font-variant-numeric:tabular-nums; }
  .val::-webkit-inner-spin-button, .val::-webkit-outer-spin-button { -webkit-appearance:none; margin:0; }
  .val { -moz-appearance:textfield; appearance:textfield; }
  .stp { width:30px; align-self:stretch; display:grid; place-items:center; color:var(--text-muted);
    cursor:pointer; font-size:14px; background:transparent; border:none;
    border-left:1px solid var(--border); }
  .stp:hover { background:var(--card-hover); color:var(--text); }
  .ghostbtn { width:100%; margin-top:4px; background:var(--card); border:1px solid var(--border);
    color:var(--text-muted); border-radius:var(--radius-sm); font:inherit; font-size:12.5px;
    font-weight:550; padding:9px; cursor:pointer; }
  .ghostbtn:hover { background:var(--card-hover); color:var(--text); border-color:var(--border-strong); }
</style>
