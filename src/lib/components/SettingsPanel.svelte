<script lang="ts">
  import { request } from "../stores";
  import { SAMPLERS } from "../types";
</script>

<div class="grid">
  <label class="label">Steps
    <input type="number" min="1" max="150" bind:value={$request.steps} />
  </label>
  <label class="label">CFG
    <input type="number" min="1" max="30" step="0.5" bind:value={$request.cfg_scale} />
  </label>
  <label class="label">Sampler
    <select bind:value={$request.sampler}>
      {#each SAMPLERS as s}<option value={s.value}>{s.label}</option>{/each}
    </select>
  </label>
  <label class="label">Width
    <input type="number" min="64" max="2048" step="64" bind:value={$request.width} />
  </label>
  <label class="label">Height
    <input type="number" min="64" max="2048" step="64" bind:value={$request.height} />
  </label>
  <label class="label">Batch
    <input type="number" min="1" max="8" bind:value={$request.batch_count} />
  </label>
  <label class="label seed">Seed (-1 = random)
    <input type="number" bind:value={$request.seed} />
  </label>
</div>

<style>
  .grid { display:grid; grid-template-columns:1fr 1fr; gap:.5rem; }
  .label { display:flex; flex-direction:column; font-size:.75rem; gap:.2rem; }
  .seed { grid-column:1 / -1; }
  input, select { font:inherit; padding:.3rem; }
</style>
