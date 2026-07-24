<script lang="ts">
  import { onMount } from "svelte";
  import { get } from "svelte/store";
  import { settings, request, history, sysStats, gpuDevices, refreshLibrary, downloadProgress, downloadBusy, downloadError, library, selectedModelId, modelNotice } from "$lib/stores";
  import { getSettings, setSettings, listHistory, onSystemStats, listGpuDevices, onDownloadProgress } from "$lib/api";
  import ModelSelector from "$lib/components/ModelSelector.svelte";
  import NewModelDialog from "$lib/components/NewModelDialog.svelte";
  import ModelEditor from "$lib/components/ModelEditor.svelte";
  import DownloadStatus from "$lib/components/DownloadStatus.svelte";
  import type { LibraryEntry } from "$lib/types";
  import PromptPanel from "$lib/components/PromptPanel.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import GenerateBar from "$lib/components/GenerateBar.svelte";
  import ImagePreview from "$lib/components/ImagePreview.svelte";
  import ParamsPanel from "$lib/components/ParamsPanel.svelte";
  import HistoryStrip from "$lib/components/HistoryStrip.svelte";
  import ResourceMonitor from "$lib/components/ResourceMonitor.svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import WelcomeDialog from "$lib/components/WelcomeDialog.svelte";
  import PreferencesDialog from "$lib/components/PreferencesDialog.svelte";
  import AboutDialog from "$lib/components/AboutDialog.svelte";
  import { applyTheme } from "$lib/theme";

  let showWelcome = $state(false);
  let showPrefs = $state(false);
  let showAbout = $state(false);
  let showNew = $state(false);
  let editing = $state<LibraryEntry | null>(null);
  let vramTotalMb = $state<number | null>(null);
  let ramTotalMb = $state<number | null>(null);
  sysStats.subscribe((s) => {
    vramTotalMb = s?.gpu?.vram_total_mb ?? null;
    ramTotalMb = s?.ram_total_mb ?? null;
  });

  // Persist dismissal optimistically; the dialog stays closed this session even
  // if the write fails (onboarding is non-critical — worst case it shows once
  // more next launch).
  async function dismissWelcome() {
    showWelcome = false;
    const cur = $settings;
    if (!cur || cur.onboarded) return;
    const next = { ...cur, onboarded: true };
    settings.set(next);
    try {
      await setSettings(next);
    } catch {
      settings.set(cur);
    }
  }

  onMount(() => {
    (async () => {
      const cfg = await getSettings();
      settings.set(cfg);
      applyTheme(cfg.theme); // reconcile against the pre-paint localStorage cache
      showWelcome = !cfg.onboarded;
      // Build the active model: prefer last_request.model; if it's an empty
      // single-file and a legacy default_model_path exists, use that.
      let model = cfg.last_request.model;
      if (model.type === "single_file" && model.path === "" && cfg.default_model_path) {
        model = { type: "single_file", path: cfg.default_model_path };
      }
      request.set({ ...cfg.last_request, model });
      history.set(await listHistory());
      await refreshLibrary();
      // Single source of truth: if the persisted request names a managed model,
      // adopt it from the just-scanned library (re-reading model.json). If it's
      // gone (deleted/renamed), clear the selection and tell the user. Ad-hoc
      // requests (model_id null — includes pre-feature configs) are left as-is.
      const savedId = cfg.last_request.model_id ?? null;
      if (savedId) {
        const entry = get(library).find((e) => e.id === savedId) ?? null;
        if (entry) {
          selectedModelId.set(savedId);
          request.update((r) => ({ ...r, model: entry.model, model_id: savedId }));
        } else {
          selectedModelId.set(null);
          request.update((r) => ({ ...r, model: { type: "single_file", path: "" }, model_id: null }));
          modelNotice.set("Your last model is no longer in your library. Pick a model to continue.");
        }
      }
      gpuDevices.set(await listGpuDevices());
    })();
    const un = onSystemStats((s) => sysStats.set(s));
    // App-level download-progress feed: keeps the store live regardless of
    // whether the New-model dialog is open, so DownloadStatus can show it.
    const unDl = onDownloadProgress((p) => downloadProgress.set(p));
    return () => { un.then((f) => f()); unDl.then((f) => f()); };
  });
</script>

<main class="app">
  <aside class="controls">
    <header class="panel-head">
      <h1 class="brand">Much<span class="ai">AI</span></h1>
      <div class="hdr-actions">
        <button class="iconbtn" aria-label="Help" title="Help" onclick={() => (showWelcome = true)}>?</button>
        <button class="iconbtn" aria-label="Preferences" title="Preferences" onclick={() => (showPrefs = true)}>⚙</button>
        <ThemeToggle />
      </div>
    </header>

    <div class="panel-selector">
      <ModelSelector
        onNew={() => (showNew = true)}
        onEdit={(e) => (editing = e)}
        onDelete={(e) => (editing = e)}
      />
      {#if $modelNotice}
        <div class="model-notice" role="status">
          <span>{$modelNotice}</span>
          <button class="notice-x" aria-label="Dismiss" onclick={() => modelNotice.set(null)}>✕</button>
        </div>
      {/if}
    </div>

    <div class="panel-body">
      <PromptPanel />
      <div class="divider"></div>
      <p class="section">Parameters</p>
      <SettingsPanel />
    </div>

    <div class="panel-foot">
      <GenerateBar />
    </div>
  </aside>

  <section class="stage">
    <ImagePreview />
    <ParamsPanel />
    <HistoryStrip />
  </section>
</main>
<ResourceMonitor onAbout={() => (showAbout = true)} />

{#if showWelcome}
  <WelcomeDialog onclose={dismissWelcome} />
{/if}

{#if showPrefs}
  <PreferencesDialog onclose={() => (showPrefs = false)} />
{/if}

{#if showAbout}
  <AboutDialog onclose={() => (showAbout = false)} />
{/if}

{#if showNew}
  <NewModelDialog {vramTotalMb} {ramTotalMb} onClose={() => (showNew = false)} />
{/if}
{#if editing}
  <ModelEditor entry={editing} onClose={() => (editing = null)} />
{/if}

<!-- Persistent download status when the New-model dialog is closed but a
     download is still running (or just failed). While the dialog is open it
     shows its own inline bar, so suppress the toast to avoid duplication. -->
{#if !showNew && ($downloadBusy || $downloadError)}
  <DownloadStatus />
{/if}

<style>
  .app { display:flex; height:calc(100vh - 34px); }
  .controls { flex:0 0 340px; display:flex; flex-direction:column;
    border-right:1px solid var(--border); overflow:hidden; }

  .panel-head { flex:0 0 auto; display:flex; align-items:center; gap:8px;
    padding:14px 16px; border-bottom:1px solid var(--border); }
  .brand { margin:0; font-size:16px; font-weight:650; letter-spacing:-.01em; }
  .brand .ai { color:var(--accent); }
  .hdr-actions { margin-left:auto; display:flex; align-items:center; gap:6px; }
  .iconbtn { width:30px; height:30px; border-radius:8px; display:grid; place-items:center;
    color:var(--text-muted); cursor:pointer; font:inherit; font-size:14px;
    background:transparent; border:1px solid transparent; }
  .iconbtn:hover { background:var(--card-hover); color:var(--text); }

  .panel-selector { flex:0 0 auto; padding:12px 16px; border-bottom:1px solid var(--border); }
  .model-notice { display:flex; align-items:flex-start; gap:8px; margin-top:10px;
    padding:8px 10px; border-radius:var(--radius-sm); font-size:12px; line-height:1.4;
    background:var(--warn-tint); color:var(--warn); }
  .model-notice span { flex:1; }
  .notice-x { flex:0 0 auto; width:18px; height:18px; display:grid; place-items:center;
    border:none; background:transparent; color:var(--text-muted); cursor:pointer;
    font-size:11px; border-radius:4px; }
  .notice-x:hover { background:var(--card-hover); color:var(--text); }

  .panel-body { flex:1 1 auto; min-height:0; overflow-y:auto; padding:16px;
    display:flex; flex-direction:column;
    scrollbar-width:thin; scrollbar-color:var(--border-strong) transparent; }
  .divider { height:1px; background:var(--border); margin:20px -16px; }
  .section { font-size:10.5px; letter-spacing:.05em; text-transform:uppercase;
    font-weight:600; color:var(--text-muted); margin:0 0 12px; }

  .panel-foot { flex:0 0 auto; padding:12px 16px; border-top:1px solid var(--border);
    background:var(--surface); }

  .stage { flex:1; display:flex; flex-direction:column; padding:1rem; gap:.6rem; min-width:0; }
</style>
