<script lang="ts">
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import { request, genStatus, history, currentImage, currentItem, settings, gpuDevices, sysStats, livePreview } from "../stores";
  import { generate, cancelGeneration, imageSrc, listHistory, onProgress, onGenNotice, onPreview } from "../api";
  import { modelIsSet } from "../types";

  let lowVramAuto = false;

  // Absolute path the engine writes the live draft to for the current run;
  // null when preview is off or no run is active. Set by the onPreview event.
  let previewPath: string | null = null;
  let previewTick = 0;

  async function run() {
    const req = get(request);
    if (!modelIsSet(req.model)) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
    if (!req.prompt.trim()) { genStatus.set({ kind: "error", message: "Enter a prompt." }); return; }
    genStatus.set({ kind: "running", progress: null });
    lowVramAuto = false;
    previewPath = null;
    previewTick = 0;
    livePreview.set(null);
    const vram = get(sysStats)?.gpu?.vram_total_mb ?? 0;
    const deviceVramMb = vram > 0 ? vram : null;
    try {
      const items = await generate(req, deviceVramMb);
      if (items.length > 0) {
        currentImage.set(imageSrc(items[0].image_path));
        currentItem.set(items[0]);
      }
      history.set(await listHistory());
      genStatus.set({ kind: "idle" });
    } catch (e) {
      genStatus.set({ kind: "error", message: String(e) });
    } finally {
      // Drop the live draft on every outcome (success, error, cancel-as-empty)
      // so the final image / prior view shows and no stale frame lingers.
      previewPath = null;
      previewTick = 0;
      livePreview.set(null);
    }
  }
  $: pct = $genStatus.kind === "running" && $genStatus.progress
    ? Math.round(($genStatus.progress.current_step / $genStatus.progress.total_steps) * 100) : 0;
  // Text label beside the bar: "Step N/M" once the engine reports a step, or
  // "Starting…" for the pre-first-step window (progress is null until then).
  $: stepLabel = $genStatus.kind === "running"
    ? ($genStatus.progress
        ? `Step ${$genStatus.progress.current_step}/${$genStatus.progress.total_steps}`
        : "Starting…")
    : "";
  // Mirrors the Rust `resolve_backend` rule: CPU when the saved selection is a
  // cpu device, or when there's no valid selection and no real GPU is present.
  $: willRunOnCpu = (() => {
    const sel = $settings?.gpu_device ?? null;
    const devices = $gpuDevices;
    const match = sel ? devices.find((d) => d.index === sel.index && d.name === sel.name) : undefined;
    if (match) return match.kind === "cpu";
    return !devices.some((d) => d.kind !== "cpu");
  })();

  onMount(() => {
    const un = onProgress((p) => {
      genStatus.update((s) => s.kind === "running" ? { kind: "running", progress: p } : s);
      // Refresh the live draft on each step; ?t busts the webview image cache so
      // the same fixed file path reloads. Steps before the first write 404 →
      // ImagePreview's onerror keeps the prior view.
      // Monotonic per-run counter (NOT p.current_step, which restarts at 1 for
      // each image in a batch and would collide on the fixed preview path).
      if (previewPath) livePreview.set(imageSrc(previewPath) + "?t=" + (++previewTick));
    });
    const unNotice = onGenNotice(() => { lowVramAuto = true; });
    const unPreview = onPreview((path) => { previewPath = path; });
    return () => { un.then((f) => f()); unNotice.then((f) => f()); unPreview.then((f) => f()); };
  });
</script>

<div class="bar">
  {#if $genStatus.kind === "running"}
    <div class="progress"><div class="fill" style="width:{pct}%"></div></div>
    <span class="step" aria-live="polite">{stepLabel}</span>
    <button class="btn-secondary" on:click={cancelGeneration}>Cancel</button>
  {:else}
    <button class="btn-primary" on:click={run}>Generate</button>
  {/if}
</div>

{#if $genStatus.kind === "running" && willRunOnCpu}
  <div class="cpu-note" role="status">Running on CPU — this will be much slower.</div>
{/if}

{#if $genStatus.kind === "running" && lowVramAuto}
  <div class="cpu-note" role="status">Low-VRAM mode auto-enabled — this model needs more memory than your GPU has, so generation will be slower.</div>
{/if}

{#if $genStatus.kind === "error"}
  <div class="error" role="alert">{$genStatus.message}</div>
{/if}

<style>
  .bar { display:flex; gap:.5rem; align-items:center; }
  .btn-primary { flex:1; padding:.6rem; font-weight:600; }
  .progress { flex:1; height:12px; background:var(--border-subtle); border-radius:6px; overflow:hidden; }
  .fill { height:100%; background:var(--accent); transition:width .15s linear; }
  .step { font-size:.72rem; opacity:.75; font-variant-numeric:tabular-nums; white-space:nowrap; }
  .error { margin-top:.5rem; padding:.5rem; border-radius:6px; background:var(--danger-tint);
    color:var(--danger-soft); font-size:.8rem; white-space:pre-wrap; }
  .cpu-note { margin-top:.5rem; padding:.4rem .5rem; border-radius:6px; background:var(--warn-tint);
    color:var(--warn); font-size:.75rem; }
</style>
