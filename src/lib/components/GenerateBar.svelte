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
  // Cache-buster for the fixed preview path's `?t=`. MUST be strictly
  // increasing for the whole session (and unique across sessions) — the
  // preview file path never changes, so any repeated `?t=` value serves a
  // stale frame the webview cached on an earlier run. Seed from the clock so
  // a persisted webview cache from a prior app launch can't collide either;
  // never reset it per-run.
  let previewTick = Date.now();

  async function run() {
    const req = get(request);
    if (!modelIsSet(req.model)) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
    if (!req.prompt.trim()) { genStatus.set({ kind: "error", message: "Enter a prompt." }); return; }
    genStatus.set({ kind: "running", progress: null });
    lowVramAuto = false;
    previewPath = null;
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
      // previewTick is deliberately NOT reset — it must keep increasing across
      // runs or the next run's `?t=` values collide with this run's cache.
      previewPath = null;
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

  // Ctrl/Cmd+Enter triggers Generate from anywhere (ignored mid-run).
  function onKey(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === "Enter") {
      e.preventDefault();
      if (get(genStatus).kind !== "running") run();
    }
  }

  onMount(() => {
    const un = onProgress((p) => {
      genStatus.update((s) => s.kind === "running" ? { kind: "running", progress: p } : s);
      // Refresh the live draft on each step; ?t busts the webview image cache so
      // the same fixed file path reloads. Steps before the first write 404 →
      // ImagePreview's onerror keeps the prior view.
      // Session-monotonic counter (NOT p.current_step, which restarts at 1 per
      // batch image, and NOT reset per-run — either would repeat a `?t=` value
      // on the fixed preview path and the webview would serve a stale frame).
      if (previewPath) livePreview.set(imageSrc(previewPath) + "?t=" + (++previewTick));
    });
    const unNotice = onGenNotice(() => { lowVramAuto = true; });
    const unPreview = onPreview((path) => { previewPath = path; });
    window.addEventListener("keydown", onKey);
    return () => { un.then((f) => f()); unNotice.then((f) => f()); unPreview.then((f) => f()); window.removeEventListener("keydown", onKey); };
  });
</script>

<div class="bar">
  {#if $genStatus.kind === "running"}
    <div class="progress"><div class="fill" style="width:{pct}%"></div></div>
    <span class="step" aria-live="polite">{stepLabel}</span>
    <button class="cancel" on:click={cancelGeneration}>Cancel</button>
  {:else}
    <button class="generate" on:click={run}>
      <span class="bolt" aria-hidden="true">⚡</span>
      Generate
      <span class="kbd">Ctrl ↵</span>
    </button>
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
  .generate { flex:1; height:44px; border:none; border-radius:var(--radius); cursor:pointer;
    background:var(--accent); color:var(--on-accent); font:inherit; font-size:14px; font-weight:600;
    display:flex; align-items:center; justify-content:center; gap:9px; }
  .generate:hover { background:var(--accent-bright); }
  .bolt { font-size:14px; }
  /* Translucent white pill on the violet button — theme-independent overlay. */
  .kbd { font-size:11px; font-weight:600; opacity:.75; background:rgba(255,255,255,.16);
    border-radius:5px; padding:2px 6px; }
  .cancel { height:40px; padding:0 .9rem; font:inherit; }
  .progress { flex:1; height:12px; background:var(--border-subtle); border-radius:6px; overflow:hidden; }
  .fill { height:100%; background:var(--accent); transition:width .15s linear; }
  .step { font-size:.72rem; opacity:.75; font-variant-numeric:tabular-nums; white-space:nowrap; }
  .error { margin-top:.5rem; padding:.5rem; border-radius:6px; background:var(--danger-tint);
    color:var(--danger-soft); font-size:.8rem; white-space:pre-wrap; }
  .cpu-note { margin-top:.5rem; padding:.4rem .5rem; border-radius:6px; background:var(--warn-tint);
    color:var(--warn); font-size:.75rem; }
</style>
