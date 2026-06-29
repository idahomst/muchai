<script lang="ts">
  import { settings, gpuDevices } from "$lib/stores";
  import { setSettings } from "$lib/api";
  import type { GpuDevice } from "$lib/types";

  let busy = $state(false);
  let error = $state<string | null>(null);

  // The saved selection still maps to a real device only if some enumerated
  // device shares both its index and name. Otherwise the engine will fall back
  // to its default and we surface a one-line notice.
  const saved = $derived($settings?.gpu_device ?? null);
  const stale = $derived(
    !!saved && !$gpuDevices.some((d) => d.index === saved.index && d.name === saved.name),
  );
  // The dropdown reflects the saved choice only when it's still valid.
  const current = $derived(saved && !stale ? String(saved.index) : "default");

  function label(d: GpuDevice): string {
    return `GPU ${d.index} — ${d.name} (${d.kind})`;
  }

  async function onchange(e: Event) {
    if (!$settings || busy) return;
    const value = (e.currentTarget as HTMLSelectElement).value;
    const device = value === "default" ? null : $gpuDevices.find((d) => String(d.index) === value);
    const gpu_device = device ? { index: device.index, name: device.name } : null;
    busy = true;
    error = null;
    try {
      const next = { ...$settings, gpu_device };
      await setSettings(next);
      settings.set(next);
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="picker">
  <div class="hdr">
    <span class="lbl">GPU device</span>
    {#if $gpuDevices.length > 0}
      <select value={current} {onchange} disabled={!$settings || busy}>
        <option value="default">Default (engine picks)</option>
        {#each $gpuDevices as d (d.index)}
          <option value={String(d.index)}>{label(d)}</option>
        {/each}
      </select>
    {:else}
      <span class="none">No Vulkan devices detected</span>
    {/if}
  </div>
  {#if stale}
    <span class="note">Saved device unavailable — using engine default.</span>
  {/if}
  {#if error}<span class="err">{error}</span>{/if}
</div>

<style>
  .picker { font-size:.75rem; border-top:1px solid var(--border); padding:.45rem .2rem 0; display:flex; flex-direction:column; gap:.25rem; }
  .hdr { display:flex; align-items:center; justify-content:space-between; gap:.4rem; }
  .lbl { opacity:.6; }
  select { font:inherit; font-size:.72rem; padding:.2rem .4rem; cursor:pointer; flex:1; min-width:0; }
  select:disabled { opacity:.5; cursor:default; }
  .none { opacity:.5; }
  .note { opacity:.7; }
  .err { color:#ff6b6b; }
</style>
