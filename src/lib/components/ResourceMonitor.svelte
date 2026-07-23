<script lang="ts">
  import { sysStats } from "../stores";
  import { version } from "../../../package.json";
  let { onAbout }: { onAbout: () => void } = $props();
  const gb = (v: number) => (v / 1024).toFixed(1);
</script>

<div class="monitor">
  {#if $sysStats}
    {#if $sysStats.gpu}
      <span class="stat" title="GPU">
        <span class="k">{$sysStats.gpu.name}</span>
        <span class="v"><span class="num pct">{$sysStats.gpu.utilization_pct}</span>%</span>
      </span>
      <span class="stat" title="VRAM">
        <span class="k">VRAM</span>
        <span class="v"><span class="num mem">{gb($sysStats.gpu.vram_used_mb)}</span> / {gb($sysStats.gpu.vram_total_mb)} GB</span>
      </span>
    {:else}
      <span class="stat">No GPU stats</span>
    {/if}
    <span class="stat" title="CPU">
      <span class="k">CPU</span>
      <span class="v"><span class="num pct">{$sysStats.cpu_pct.toFixed(0)}</span>%</span>
    </span>
    <span class="stat" title="RAM">
      <span class="k">RAM</span>
      <span class="v"><span class="num mem">{gb($sysStats.ram_used_mb)}</span> / {gb($sysStats.ram_total_mb)} GB</span>
    </span>
  {:else}
    <span class="stat">reading system…</span>
  {/if}
  <button class="ver" title="About MuchAI" onclick={onAbout}>v{version}</button>
</div>

<style>
  .monitor { display:flex; gap:1rem; font-size:.75rem; opacity:.8; padding:.4rem .6rem;
    border-top:1px solid var(--border); white-space:nowrap; overflow-x:auto; }
  .stat { display:inline-flex; gap:.4rem; align-items:baseline; }
  .k { opacity:.7; }
  /* Pushed to the far right of the bar; unobtrusive. */
  .ver { margin-left:auto; opacity:.7; padding-left:1rem; font:inherit;
    background:none; border:none; cursor:pointer; color:inherit;
    text-decoration:underline; text-underline-offset:2px; }
  .ver:hover { opacity:1; color:var(--accent-bright); }
  /* Only the changing number gets a fixed-width, right-aligned box so the unit
     (% / GB) and everything after it never reflow when the digit count changes
     (e.g. 96%→100%, 9.8→10.1 GB). The unit stays glued to the number's right
     edge — no inter-element gap — because it lives in the same .v span. */
  .num { display:inline-block; text-align:right; font-variant-numeric:tabular-nums; }
  .num.pct { min-width:3ch; }  /* fits "100" */
  .num.mem { min-width:5ch; }  /* fits "999.9" */
</style>
