<script lang="ts">
  import { currentItem, settings, definitions } from "$lib/stores";
  import { setSettings } from "$lib/api";
  import { SAMPLERS, modelLabel } from "$lib/types";

  let busy = $state(false);
  let error = $state<string | null>(null);

  const samplerLabel = (v: string) =>
    SAMPLERS.find((s) => s.value === v)?.label ?? v;

  // Collapsed by default; a not-yet-loaded config is treated as collapsed.
  const expanded = $derived($settings?.params_expanded ?? false);

  async function toggle() {
    if (!$settings || busy) return;
    const prev = $settings;
    const next = { ...prev, params_expanded: !prev.params_expanded };
    settings.set(next); // optimistic — UI + image size respond immediately
    busy = true;
    error = null;
    try {
      await setSettings(next);
    } catch (e) {
      settings.set(prev); // revert to what's actually persisted
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if $currentItem}
  {@const r = $currentItem.request}
  <div class="params">
    <button class="hdr" onclick={toggle} disabled={busy}
            aria-expanded={expanded}
            title={expanded ? "Collapse parameters" : "Expand parameters"}>
      <span class="chev">{expanded ? "▾" : "▸"}</span>
      <span class="title">Parameters</span>
      {#if !expanded}
        <span class="summary">Seed {r.seed} · {r.steps} steps · CFG {r.cfg_scale} · {r.width}×{r.height}</span>
      {/if}
    </button>
    {#if expanded}
      <div class="grid">
        <span class="k">Model</span><span class="v" title={modelLabel(r.model, $definitions)}>{modelLabel(r.model, $definitions)}</span>
        <span class="k">Seed</span><span class="v">{r.seed}</span>
        <span class="k">Steps</span><span class="v">{r.steps}</span>
        <span class="k">CFG</span><span class="v">{r.cfg_scale}</span>
        <span class="k">Sampler</span><span class="v">{samplerLabel(r.sampler)}</span>
        <span class="k">Size</span><span class="v">{r.width}×{r.height}</span>
        <span class="k">Format</span><span class="v">{r.output_format.toUpperCase()}</span>
      </div>
      <div class="prompts">
        <div class="pl">Prompt</div>
        <div class="pt">{r.prompt}</div>
        {#if r.negative_prompt}
          <div class="pl">Negative</div>
          <div class="pt neg">{r.negative_prompt}</div>
        {/if}
      </div>
    {/if}
    {#if error}<span class="err">{error}</span>{/if}
  </div>
{/if}

<style>
  .params { font-size:.78rem; border:1px solid var(--border); border-radius:8px;
    padding:.5rem .7rem; display:flex; flex-direction:column; gap:.5rem; flex:0 0 auto; }
  .hdr { display:flex; align-items:center; gap:.45rem; width:100%; padding:0;
    background:none; border:none; color:inherit; font:inherit; font-size:.78rem;
    text-align:left; cursor:pointer; }
  .hdr:disabled { cursor:default; }
  .chev { opacity:.6; width:.8rem; flex:0 0 auto; }
  .title { opacity:.7; font-weight:600; flex:0 0 auto; }
  .summary { opacity:.55; font-variant-numeric:tabular-nums; overflow:hidden;
    text-overflow:ellipsis; white-space:nowrap; min-width:0; }
  .grid { display:grid; grid-template-columns:auto 1fr auto 1fr auto 1fr; gap:.2rem .5rem;
    align-items:baseline; }
  .k { opacity:.55; }
  .v { font-variant-numeric:tabular-nums; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .prompts { display:flex; flex-direction:column; gap:.15rem; }
  .pl { opacity:.55; }
  .pt { white-space:pre-wrap; word-break:break-word; }
  .pt.neg { opacity:.8; }
  .err { color:var(--danger); }
</style>
