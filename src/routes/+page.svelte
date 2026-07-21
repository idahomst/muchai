<script lang="ts">
  import { onMount } from "svelte";
  import { settings, request, history, sysStats, gpuDevices, refreshLibrary } from "$lib/stores";
  import { getSettings, setSettings, listHistory, onSystemStats, listGpuDevices } from "$lib/api";
  import ModelLibrary from "$lib/components/ModelLibrary.svelte";
  import NewModelDialog from "$lib/components/NewModelDialog.svelte";
  import ModelEditor from "$lib/components/ModelEditor.svelte";
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
  import { applyTheme } from "$lib/theme";

  let showWelcome = $state(false);
  let showPrefs = $state(false);
  let showNew = $state(false);
  let editing = $state<LibraryEntry | null>(null);
  let vramTotalMb = $state<number | null>(null);
  sysStats.subscribe((s) => { vramTotalMb = s?.gpu?.vram_total_mb ?? null; });

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
      gpuDevices.set(await listGpuDevices());
    })();
    const un = onSystemStats((s) => sysStats.set(s));
    return () => { un.then((f) => f()); };
  });
</script>

<main class="app">
  <aside class="controls">
    <header class="brandbar">
      <h1 class="brand">MuchAI</h1>
      <div class="hdr-actions">
        <button class="help-btn" aria-label="Help" title="Help" onclick={() => (showWelcome = true)}>?</button>
        <button class="help-btn" aria-label="Preferences" title="Preferences" onclick={() => (showPrefs = true)}>⚙</button>
        <ThemeToggle />
      </div>
    </header>
    <ModelLibrary
      onNew={() => (showNew = true)}
      onEdit={(e) => (editing = e)}
      onDelete={(e) => (editing = e)}
    />
    <PromptPanel />
    <SettingsPanel />
    <div class="spacer"></div>
    <GenerateBar />
  </aside>

  <section class="stage">
    <ImagePreview />
    <ParamsPanel />
    <HistoryStrip />
  </section>
</main>
<ResourceMonitor />

{#if showWelcome}
  <WelcomeDialog onclose={dismissWelcome} />
{/if}

{#if showPrefs}
  <PreferencesDialog onclose={() => (showPrefs = false)} />
{/if}

{#if showNew}
  <NewModelDialog {vramTotalMb} onClose={() => (showNew = false)} />
{/if}
{#if editing}
  <ModelEditor entry={editing} onClose={() => (editing = null)} />
{/if}

<style>
  .app { display:flex; height:calc(100vh - 34px); }
  .controls { flex:0 0 340px; display:flex; flex-direction:column; gap:.8rem;
    padding:1rem; border-right:1px solid var(--border); overflow:hidden auto; }
  .brandbar { display:flex; align-items:center; justify-content:space-between; margin:0 0 .5rem; }
  .brand { margin:0; font-size:1.2rem; }
  .hdr-actions { display:flex; align-items:center; gap:.4rem; }
  .help-btn { font:inherit; font-size:.85rem; line-height:1; width:1.5rem; height:1.5rem;
    display:flex; align-items:center; justify-content:center; cursor:pointer;
    border:1px solid var(--border); border-radius:50%;
    background:var(--surface); color:var(--text); }
  .help-btn:hover { color:var(--accent-bright); border-color:var(--accent-bright); }
  .spacer { flex:1; }
  .stage { flex:1; display:flex; flex-direction:column; padding:1rem; gap:.6rem; min-width:0; }
</style>
