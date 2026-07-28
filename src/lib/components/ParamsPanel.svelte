<script lang="ts">
  import { currentItem, settings, request, selectedModelId, loras } from "$lib/stores";
  import { setSettings } from "$lib/api";
  import { SAMPLERS, modelLabel } from "$lib/types";
  import type { LoraSelection } from "$lib/types";

  let busy = $state(false);
  let error = $state<string | null>(null);

  // Brief confirmation on the Load button. Without it the click looks inert:
  // the panel that changed is on the other side of the window.
  let justLoaded = $state(false);
  let loadedTimer: ReturnType<typeof setTimeout> | undefined;

  /** Copy this image's settings into the left panel. Explicit, never automatic —
   *  selecting an image is for looking at it, and silently overwriting a
   *  half-composed prompt to do that was the old behaviour. */
  function load() {
    const item = $currentItem;
    if (!item) return;
    // A replay is ad-hoc: keep the frozen ModelRef verbatim rather than
    // re-resolving it against a manifest that may have changed since.
    request.set({ ...item.request, model_id: null });
    selectedModelId.set(null);
    justLoaded = true;
    clearTimeout(loadedTimer);
    loadedTimer = setTimeout(() => (justLoaded = false), 1600);
  }

  // The engine tag is the pool filename; show the label the user knows it by.
  function loraLabel(s: LoraSelection): string {
    const name = $loras.find((l) => l.name === s.name)?.display_name ?? s.name;
    return `${name} @ ${s.weight.toFixed(2)}`;
  }

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
    <div class="top">
      <button class="hdr" onclick={toggle} disabled={busy}
              aria-expanded={expanded}
              title={expanded ? "Collapse parameters" : "Expand parameters"}>
        <span class="chev">{expanded ? "▾" : "▸"}</span>
        <span class="ttl">Parameters</span>
        {#if expanded}
          <span class="src">from this image</span>
        {:else}
          <span class="summary">Seed {r.seed} · {r.steps} steps · CFG {r.cfg_scale} · {r.width}×{r.height}</span>
        {/if}
      </button>
      <!-- Outside .hdr, not inside: .hdr is itself a button and nesting one
           inside another is invalid. -->
      <button class="load" onclick={load}
              title="Copy these settings into the panel on the left">
        {justLoaded ? "Loaded ✓" : "Load"}
      </button>
    </div>
    {#if expanded}
      <div class="grid">
        <div class="kv"><span class="k">Model</span><span class="v mono" title={modelLabel(r.model)}>{modelLabel(r.model)}</span></div>
        <div class="kv"><span class="k">Seed</span><span class="v mono">{r.seed}</span></div>
        <div class="kv"><span class="k">Steps</span><span class="v">{r.steps}</span></div>
        <div class="kv"><span class="k">CFG</span><span class="v">{r.cfg_scale}</span></div>
        <div class="kv"><span class="k">Sampler</span><span class="v">{samplerLabel(r.sampler)}</span></div>
        <div class="kv"><span class="k">Size</span><span class="v">{r.width}×{r.height}</span></div>
        <div class="kv"><span class="k">Format</span><span class="v">{r.output_format.toUpperCase()}</span></div>
        {#if r.loras.length > 0}
          <div class="kv wide"><span class="k">LoRAs</span><span class="v">{r.loras.map(loraLabel).join(", ")}</span></div>
        {/if}
        <div class="kv wide"><span class="k">Prompt</span><span class="v">{r.prompt}</span></div>
        {#if r.negative_prompt}
          <div class="kv wide"><span class="k">Negative</span><span class="v">{r.negative_prompt}</span></div>
        {/if}
      </div>
    {/if}
    {#if error}<span class="err">{error}</span>{/if}
  </div>
{/if}

<style>
  .params { border:1px solid var(--border); border-radius:8px;
    padding:.55rem .7rem; display:flex; flex-direction:column; gap:.55rem; flex:0 0 auto; }
  .top { display:flex; align-items:center; gap:.6rem; }
  .load { flex:0 0 auto; background:var(--card); border:1px solid var(--border);
    color:var(--text-muted); border-radius:var(--radius-sm); font:inherit;
    font-size:11.5px; font-weight:600; padding:3px 10px; cursor:pointer; white-space:nowrap; }
  .load:hover { background:var(--card-hover); color:var(--text); border-color:var(--border-strong); }
  .hdr { display:flex; align-items:center; gap:.45rem; flex:1; min-width:0; padding:0;
    background:none; border:none; color:var(--text-muted); font:inherit;
    text-align:left; cursor:pointer; }
  .hdr:hover:not(:disabled) { color:var(--text); }
  .hdr:disabled { cursor:default; }
  .chev { font-size:.7rem; color:var(--text-faint); width:.8rem; flex:0 0 auto; }
  .ttl { font-size:11px; letter-spacing:.03em; text-transform:uppercase; font-weight:600; flex:0 0 auto; }
  .src { margin-left:auto; font-size:11px; color:var(--text-faint); }
  .summary { margin-left:auto; font-size:.75rem; color:var(--text-faint);
    font-variant-numeric:tabular-nums; overflow:hidden;
    text-overflow:ellipsis; white-space:nowrap; min-width:0; }
  .grid { display:grid; grid-template-columns:repeat(3,1fr); gap:4px 24px; }
  .kv { display:flex; gap:10px; font-size:12.5px; padding:2px 0; min-width:0; }
  .kv .k { color:var(--text-faint); min-width:54px; flex:0 0 auto; }
  .kv .v { color:var(--text-muted); font-variant-numeric:tabular-nums;
    overflow:hidden; text-overflow:ellipsis; white-space:nowrap; min-width:0; }
  .kv .v.mono { font-family:var(--mono); font-size:11.5px; }
  .kv.wide { grid-column:1 / -1; }
  .kv.wide .v { color:var(--text); white-space:normal; word-break:break-word; }
  .err { color:var(--danger); font-size:.78rem; }
</style>
