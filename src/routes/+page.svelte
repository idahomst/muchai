<script lang="ts">
  import { onMount } from "svelte";
  import { settings, request, history, sysStats, models, gpuDevices, definitions } from "$lib/stores";
  import { getSettings, setSettings, listHistory, onSystemStats, listModels, listGpuDevices } from "$lib/api";
  import ModelLibrary from "$lib/components/ModelLibrary.svelte";
  import PromptPanel from "$lib/components/PromptPanel.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import GenerateBar from "$lib/components/GenerateBar.svelte";
  import ImagePreview from "$lib/components/ImagePreview.svelte";
  import ParamsPanel from "$lib/components/ParamsPanel.svelte";
  import HistoryStrip from "$lib/components/HistoryStrip.svelte";
  import GalleryLocation from "$lib/components/GalleryLocation.svelte";
  import ModelFolders from "$lib/components/ModelFolders.svelte";
  import DevicePicker from "$lib/components/DevicePicker.svelte";
  import ResourceMonitor from "$lib/components/ResourceMonitor.svelte";
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import WelcomeDialog from "$lib/components/WelcomeDialog.svelte";
  import { applyTheme } from "$lib/theme";

  let showWelcome = $state(false);

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
      // Seed the saved multi-file library.
      definitions.set(cfg.model_definitions ?? []);
      // Build the active model: prefer last_request.model; if it's an empty
      // single-file and a legacy default_model_path exists, use that.
      let model = cfg.last_request.model;
      if (model.type === "single_file" && model.path === "" && cfg.default_model_path) {
        model = { type: "single_file", path: cfg.default_model_path };
      }
      request.set({ ...cfg.last_request, model });
      history.set(await listHistory());
      models.set(await listModels());
      gpuDevices.set(await listGpuDevices());
    })();
    const un = onSystemStats((s) => sysStats.set(s));
    return () => { un.then((f) => f()); };
  });
</script>

<main class="app">
  <aside class="controls">
    <header class="brandbar">
      <h1 class="brand">FridAI</h1>
      <div class="hdr-actions">
        <button class="help-btn" aria-label="Help" title="Help" onclick={() => (showWelcome = true)}>?</button>
        <ThemeToggle />
      </div>
    </header>
    <ModelLibrary />
    <ModelFolders />
    <DevicePicker />
    <PromptPanel />
    <SettingsPanel />
    <div class="spacer"></div>
    <GenerateBar />
  </aside>

  <section class="stage">
    <ImagePreview />
    <ParamsPanel />
    <HistoryStrip />
    <GalleryLocation />
  </section>
</main>
<ResourceMonitor />

{#if showWelcome}
  <WelcomeDialog onclose={dismissWelcome} />
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
