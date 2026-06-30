<script lang="ts">
  import { onMount } from "svelte";
  import { settings, request, history, sysStats, models, gpuDevices } from "$lib/stores";
  import { getSettings, listHistory, onSystemStats, listModels, listGpuDevices } from "$lib/api";
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
  import { applyTheme } from "$lib/theme";

  onMount(() => {
    (async () => {
      const cfg = await getSettings();
      settings.set(cfg);
      applyTheme(cfg.theme); // reconcile against the pre-paint localStorage cache
      // seed the form with last-used params + default model if present
      request.set({ ...cfg.last_request, model_path: cfg.default_model_path ?? cfg.last_request.model_path });
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
      <h1 class="brand">fridAI</h1>
      <ThemeToggle />
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

<style>
  .app { display:flex; height:calc(100vh - 34px); }
  .controls { flex:0 0 340px; display:flex; flex-direction:column; gap:.8rem;
    padding:1rem; border-right:1px solid var(--border); overflow:hidden auto; }
  .brandbar { display:flex; align-items:center; justify-content:space-between; margin:0 0 .5rem; }
  .brand { margin:0; font-size:1.2rem; }
  .spacer { flex:1; }
  .stage { flex:1; display:flex; flex-direction:column; padding:1rem; gap:.6rem; min-width:0; }
</style>
