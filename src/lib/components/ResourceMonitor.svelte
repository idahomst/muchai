<script lang="ts">
  import { sysStats } from "../stores";
  import { version } from "../../../package.json";
  let { onAbout }: { onAbout: () => void } = $props();
  const gb = (v: number) => (v / 1024).toFixed(1);
  const frac = (used: number, total: number) => (total > 0 ? used / total : 0);
  // Meter fill hue: green while there's headroom, amber as it fills, red when
  // nearly full. Thresholds match the "green→amber" intent of the mockup.
  const meterClass = (f: number) => (f < 0.7 ? "lo" : f < 0.9 ? "mid" : "hi");
  // Computed once each so the meter's class and width stay provably in sync.
  const vramFrac = $derived(
    $sysStats?.gpu ? frac($sysStats.gpu.vram_used_mb ?? 0, $sysStats.gpu.vram_total_mb ?? 0) : 0,
  );
  const ramFrac = $derived($sysStats ? frac($sysStats.ram_used_mb, $sysStats.ram_total_mb) : 0);
</script>

<div class="monitor">
  {#if $sysStats}
    {#if $sysStats.gpu}
      <span class="stat" title="GPU">
        <span class="glyph" aria-hidden="true">🖥</span>{$sysStats.gpu.name}
      </span>
      <span class="stat" title={$sysStats.gpu.shared ? "Memory used (shared with the system)" : "VRAM used"}>
        <span class="lbl">VRAM</span>
        <span class="meter"><i class={meterClass(vramFrac)} style="width:{vramFrac * 100}%"></i></span>
        <span class="v">
          <span class="num mem">{$sysStats.gpu.vram_used_mb === null ? "N/A" : gb($sysStats.gpu.vram_used_mb)}</span>
          {$sysStats.gpu.shared ? "of" : "/"}
          {$sysStats.gpu.vram_total_mb === null ? "N/A" : gb($sysStats.gpu.vram_total_mb)} GB{$sysStats.gpu.shared ? " shared" : ""}
        </span>
      </span>
      <span class="stat" title="GPU utilization">
        <span class="lbl">GPU</span>
        <!-- No trailing % on N/A: "N/A%" reads as a number that failed to load. -->
        {#if $sysStats.gpu.utilization_pct === null}
          <span class="v"><span class="num pct">N/A</span></span>
        {:else}
          <span class="v"><span class="num pct">{$sysStats.gpu.utilization_pct}</span>%</span>
        {/if}
      </span>
    {:else}
      <span class="stat">No GPU stats</span>
    {/if}
    <span class="stat" title="RAM used">
      <span class="lbl">RAM</span>
      <span class="meter"><i class={meterClass(ramFrac)} style="width:{ramFrac * 100}%"></i></span>
      <span class="v"><span class="num mem">{gb($sysStats.ram_used_mb)}</span> / {gb($sysStats.ram_total_mb)} GB</span>
    </span>
    <span class="stat" title="CPU utilization">
      <span class="lbl">CPU</span>
      <span class="v"><span class="num pct">{$sysStats.cpu_pct.toFixed(0)}</span>%</span>
    </span>
  {:else}
    <span class="stat">reading system…</span>
  {/if}
  <button class="ver" title="About MuchAI" onclick={onAbout}>v{version}</button>
</div>

<style>
  .monitor { display:flex; align-items:center; gap:16px; font-size:11.5px; color:var(--text-muted);
    padding:.4rem .8rem; border-top:1px solid var(--border); background:var(--bg);
    white-space:nowrap; overflow-x:auto; }
  .stat { display:inline-flex; gap:7px; align-items:center; }
  .glyph { color:var(--text-faint); }
  .lbl { color:var(--text-faint); font-weight:600; letter-spacing:.02em; }
  /* Inline usage meter: fills green→amber→red as the resource fills. */
  .meter { width:52px; height:5px; border-radius:3px; background:var(--card); overflow:hidden; }
  .meter i { display:block; height:100%; border-radius:3px; }
  .meter i.lo { background:var(--success); }
  .meter i.mid { background:var(--warn); }
  .meter i.hi { background:var(--danger); }
  /* Only the changing number gets a fixed-width, right-aligned box so the unit
     (% / GB) and everything after it never reflow when the digit count changes
     (e.g. 96%→100%, 9.8→10.1 GB). */
  .num { display:inline-block; text-align:right; font-variant-numeric:tabular-nums; }
  .num.pct { min-width:3ch; }  /* fits "100" */
  .num.mem { min-width:5ch; }  /* fits "100.0" */
  .ver { margin-left:auto; padding-left:1rem; color:var(--text-faint);
    font-family:var(--mono); font-size:11px; background:none; border:none; cursor:pointer; }
  .ver:hover { color:var(--accent-bright); }
</style>
