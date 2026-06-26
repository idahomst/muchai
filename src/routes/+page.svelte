<script lang="ts">
  import { onMount } from "svelte";
  import { settings, request, history, sysStats, models } from "$lib/stores";
  import { getSettings, listHistory, onSystemStats, listModels } from "$lib/api";
  import ModelLibrary from "$lib/components/ModelLibrary.svelte";
  import PromptPanel from "$lib/components/PromptPanel.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import GenerateBar from "$lib/components/GenerateBar.svelte";
  import ImagePreview from "$lib/components/ImagePreview.svelte";
  import ParamsPanel from "$lib/components/ParamsPanel.svelte";
  import HistoryStrip from "$lib/components/HistoryStrip.svelte";
  import GalleryLocation from "$lib/components/GalleryLocation.svelte";
  import ModelFolders from "$lib/components/ModelFolders.svelte";
  import ResourceMonitor from "$lib/components/ResourceMonitor.svelte";

  onMount(() => {
    (async () => {
      const cfg = await getSettings();
      settings.set(cfg);
      // seed the form with last-used params + default model if present
      request.set({ ...cfg.last_request, model_path: cfg.default_model_path ?? cfg.last_request.model_path });
      history.set(await listHistory());
      models.set(await listModels());
    })();
    const un = onSystemStats((s) => sysStats.set(s));
    return () => { un.then((f) => f()); };
  });
</script>

<main class="app">
  <aside class="controls">
    <h1 class="brand">fridAI</h1>
    <ModelLibrary />
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
    <ModelFolders />
  </section>
</main>
<ResourceMonitor />

<style>
  .app { display:flex; height:calc(100vh - 34px); }
  .controls { flex:0 0 340px; display:flex; flex-direction:column; gap:.8rem;
    padding:1rem; border-right:1px solid var(--border); overflow-y:auto; }
  .brand { margin:0 0 .5rem; font-size:1.2rem; }
  .spacer { flex:1; }
  .stage { flex:1; display:flex; flex-direction:column; padding:1rem; gap:.6rem; min-width:0; }
</style>
