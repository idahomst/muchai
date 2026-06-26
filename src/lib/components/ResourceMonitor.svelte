<script lang="ts">
  import { sysStats } from "../stores";
  const mb = (v: number) => `${(v / 1024).toFixed(1)} GB`;
</script>

<div class="monitor">
  {#if $sysStats}
    {#if $sysStats.gpu}
      <span class="stat" title="GPU">
        <span class="k">{$sysStats.gpu.name}</span>
        <span class="v pct">{$sysStats.gpu.utilization_pct}%</span>
      </span>
      <span class="stat" title="VRAM">
        <span class="k">VRAM</span>
        <span class="v mem">{mb($sysStats.gpu.vram_used_mb)} / {mb($sysStats.gpu.vram_total_mb)}</span>
      </span>
    {:else}
      <span class="stat">No NVIDIA GPU detected</span>
    {/if}
    <span class="stat" title="CPU">
      <span class="k">CPU</span>
      <span class="v pct">{$sysStats.cpu_pct.toFixed(0)}%</span>
    </span>
    <span class="stat" title="RAM">
      <span class="k">RAM</span>
      <span class="v mem">{mb($sysStats.ram_used_mb)} / {mb($sysStats.ram_total_mb)}</span>
    </span>
  {:else}
    <span class="stat">reading system…</span>
  {/if}
</div>

<style>
  .monitor { display:flex; gap:1rem; font-size:.75rem; opacity:.8; padding:.4rem .6rem;
    border-top:1px solid var(--border); white-space:nowrap; overflow-x:auto; }
  .stat { display:inline-flex; gap:.4rem; align-items:baseline; }
  .k { opacity:.7; }
  /* Tabular numerals + reserved width so a changing value (e.g. 9%→10%)
     never reflows the neighbouring readouts. */
  .v { font-variant-numeric: tabular-nums; text-align:right; display:inline-block; }
  .v.pct { min-width:3ch; }
  .v.mem { min-width:12ch; }
</style>
