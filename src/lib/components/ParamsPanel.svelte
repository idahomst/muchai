<script lang="ts">
  import { currentItem } from "../stores";
  import { SAMPLERS } from "../types";

  const basename = (p: string) => p.split(/[\\/]/).pop() || p;
  const samplerLabel = (v: string) =>
    SAMPLERS.find((s) => s.value === v)?.label ?? v;
</script>

{#if $currentItem}
  {@const r = $currentItem.request}
  <div class="params">
    <div class="grid">
      <span class="k">Model</span><span class="v" title={r.model_path}>{basename(r.model_path)}</span>
      <span class="k">Seed</span><span class="v">{r.seed}</span>
      <span class="k">Steps</span><span class="v">{r.steps}</span>
      <span class="k">CFG</span><span class="v">{r.cfg_scale}</span>
      <span class="k">Sampler</span><span class="v">{samplerLabel(r.sampler)}</span>
      <span class="k">Size</span><span class="v">{r.width}×{r.height}</span>
    </div>
    <div class="prompts">
      <div class="pl">Prompt</div>
      <div class="pt">{r.prompt}</div>
      {#if r.negative_prompt}
        <div class="pl">Negative</div>
        <div class="pt neg">{r.negative_prompt}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .params { font-size:.78rem; border:1px solid var(--border); border-radius:8px;
    padding:.6rem .7rem; display:flex; flex-direction:column; gap:.5rem; }
  .grid { display:grid; grid-template-columns:auto 1fr auto 1fr auto 1fr; gap:.2rem .5rem;
    align-items:baseline; }
  .k { opacity:.55; }
  .v { font-variant-numeric:tabular-nums; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .prompts { display:flex; flex-direction:column; gap:.15rem; }
  .pl { opacity:.55; }
  .pt { white-space:pre-wrap; word-break:break-word; }
  .pt.neg { opacity:.8; }
</style>
