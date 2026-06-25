<script lang="ts">
  import { sysStats } from "../stores";
  const mb = (v: number) => `${(v / 1024).toFixed(1)} GB`;
</script>

<div class="monitor">
  {#if $sysStats}
    {#if $sysStats.gpu}
      <span title="GPU">{$sysStats.gpu.name} · {$sysStats.gpu.utilization_pct}%</span>
      <span title="VRAM">VRAM {mb($sysStats.gpu.vram_used_mb)}/{mb($sysStats.gpu.vram_total_mb)}</span>
    {:else}
      <span>No NVIDIA GPU detected</span>
    {/if}
    <span title="CPU">CPU {$sysStats.cpu_pct.toFixed(0)}%</span>
    <span title="RAM">RAM {mb($sysStats.ram_used_mb)}/{mb($sysStats.ram_total_mb)}</span>
  {:else}
    <span>reading system…</span>
  {/if}
</div>

<style>
  .monitor { display:flex; gap:1rem; font-size:.75rem; opacity:.8; padding:.4rem .6rem;
    border-top:1px solid var(--border); white-space:nowrap; overflow-x:auto; }
</style>
