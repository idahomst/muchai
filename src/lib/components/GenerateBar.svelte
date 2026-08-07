<script lang="ts">
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import { request, genStatus, history, currentImage, currentItem, settings, gpuDevices, sysStats, livePreview, loras } from "../stores";
  import { generate, cancelGeneration, imageSrc, listHistory, onProgress, onGenNotice, onPreview, onLoraMissing, engineSelect, engineStatus } from "../api";
  import { modelIsSet } from "../types";

  let lowVramAuto = false;

  // Offer a way back only when the running engine is one we installed. A
  // downloaded build that runs happily and emits garbage is the one failure the
  // pre-install validation provably cannot catch, and a generation failing is
  // the only moment the user will connect the two. Never offered for the
  // built-in (nothing to revert to) or a custom build (their own choice).
  //
  // Resolved when a run fails rather than at mount: an engine installed from
  // Preferences during this session is exactly the case this exists for, and a
  // value read at startup would say "built-in" and hide the button.
  let revertable = false;
  let reverting = false;
  // Shown *under* the generation error rather than replacing it: the failure
  // the user was acting on is the context for this one.
  let revertError = "";

  async function revertEngine() {
    reverting = true;
    revertError = "";
    try {
      await engineSelect({ type: "builtin" });
      revertable = false;
      errorDismissed = true;
    } catch (e) {
      revertError = String(e);
    } finally {
      reverting = false;
    }
  }

  // Hides the error banner without touching `genStatus`, which stays "error"
  // until the next run and is read elsewhere. Reset when a run starts, so a new
  // failure is never swallowed by an earlier dismissal.
  let errorDismissed = false;

  // LoRAs the engine reported as missing during the last run. Deliberately NOT
  // cleared when the run ends: the image lands looking normal, so the note has
  // to outlive the run. Cleared when the next one starts.
  let missingLoras: string[] = [];

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
    // Cleared before the guards below, not after: their banners ("Enter a
    // prompt.") have nothing to do with the engine, and a `true` left over from
    // an earlier failure would offer to switch engines beside one of them.
    revertable = false;
    revertError = "";
    const req = get(request);
    if (!modelIsSet(req.model)) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
    if (!req.prompt.trim()) { genStatus.set({ kind: "error", message: "Enter a prompt." }); return; }
    genStatus.set({ kind: "running", progress: null });
    lowVramAuto = false;
    errorDismissed = false;
    missingLoras = [];
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
      // Deliberately not awaited: `engine_status` is a synchronous command that
      // may spend ten seconds probing an engine that has stopped answering —
      // exactly the case this button exists for — and awaiting it would hold
      // the banner and the live-draft cleanup below behind that stall.
      // Best-effort: a status we can't read just leaves the banner as it was.
      // `fell_back` excluded — the built-in engine already ran, so switching to
      // it cannot change this outcome and the button would promise a fix it
      // can't deliver. Preferences is where that stale selection is reported.
      engineStatus()
        .then((s) => { revertable = s.selection.type === "downloaded" && !s.fell_back; })
        .catch(() => {});
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
    const unLora = onLoraMissing((name) => {
      // The engine only knows the pool filename; show the label the user gave
      // it. Falls back to the stem if the row has since been removed.
      const label = get(loras).find((l) => l.name === name)?.display_name ?? name;
      if (!missingLoras.includes(label)) missingLoras = [...missingLoras, label];
    });
    window.addEventListener("keydown", onKey);
    return () => { un.then((f) => f()); unNotice.then((f) => f()); unPreview.then((f) => f()); unLora.then((f) => f()); window.removeEventListener("keydown", onKey); };
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
      <span class="kbd" aria-hidden="true">Ctrl ↵</span>
    </button>
  {/if}
</div>

{#if $genStatus.kind === "running" && willRunOnCpu}
  <div class="cpu-note" role="status">Running on CPU — this will be much slower.</div>
{/if}

{#if $genStatus.kind === "running" && lowVramAuto}
  <div class="cpu-note" role="status">Low-VRAM mode auto-enabled — this model needs more memory than your GPU has, so generation will be slower.</div>
{/if}

{#if missingLoras.length > 0}
  <div class="cpu-note" role="status">
    {missingLoras.length === 1 ? "This LoRA was not found" : "These LoRAs were not found"}:
    {missingLoras.join(", ")}. The image was generated without
    {missingLoras.length === 1 ? "it" : "them"}.
  </div>
{/if}

{#if $genStatus.kind === "error" && !errorDismissed}
  <div class="error" role="alert">
    <div class="errortext">
      <span>{$genStatus.message}</span>
      {#if revertError}<span class="reverterr">Could not switch to the built-in engine: {revertError}</span>{/if}
    </div>
    {#if revertable}
      <button class="revert" on:click={revertEngine} disabled={reverting}>Use built-in engine</button>
    {/if}
    <button class="error-x" aria-label="Dismiss error" on:click={() => (errorDismissed = true)}>✕</button>
  </div>
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
  /* Engine errors can be many lines long. Cap the banner and let it scroll
     internally so it never grows the sticky footer enough to shove Generate
     up over the prompt/settings. overflow-wrap stops long tokens (paths,
     tensor names) forcing horizontal overflow. */
  .error { margin-top:.5rem; padding:.5rem; border-radius:6px; background:var(--danger-tint);
    color:var(--danger-soft); font-size:.8rem; white-space:pre-wrap; overflow-wrap:anywhere;
    max-height:7.5rem; overflow-y:auto; scrollbar-width:thin; scrollbar-color:var(--danger) transparent;
    display:flex; align-items:flex-start; gap:.5rem; }
  .errortext { flex:1; min-width:0; display:flex; flex-direction:column; gap:.35rem; }
  .reverterr { opacity:.85; }
  /* Sticks to the top of a scrolled banner so it stays reachable on a long
     engine dump. */
  .error-x { position:sticky; top:0; flex:0 0 auto; width:18px; height:18px; display:grid;
    place-items:center; border:none; background:transparent; color:inherit; cursor:pointer;
    font-size:11px; line-height:1; padding:0; }
  .error-x:hover { opacity:.7; }
  /* Sticky for the same reason as the dismiss button: on a long engine dump the
     way out must not scroll off the top of the banner. */
  .revert { position:sticky; top:0; flex:0 0 auto; font:inherit; font-size:11px;
    padding:2px 8px; white-space:nowrap; cursor:pointer; }
  .revert:disabled { opacity:.5; cursor:default; }
  .cpu-note { margin-top:.5rem; padding:.4rem .5rem; border-radius:6px; background:var(--warn-tint);
    color:var(--warn); font-size:.75rem; }
</style>
