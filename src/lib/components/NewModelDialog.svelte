<script lang="ts">
  import { catalogEntries, addCatalogModel, addUrlModel, addLocalModel, pickModelFile } from "../api";
  import { refreshLibrary } from "../stores";
  import { suitabilityBadge } from "../modelFormat";
  import type { RatedCatalogEntry } from "../types";

  let { vramTotalMb, onClose }: { vramTotalMb: number | null; onClose: () => void } = $props();

  type Tab = "catalog" | "url" | "local";
  let tab = $state<Tab>("catalog");
  let busy = $state(false);
  let error = $state<string | null>(null);

  let catalog = $state<RatedCatalogEntry[]>([]);
  $effect(() => {
    catalogEntries(vramTotalMb).then((c) => (catalog = c)).catch((e) => (error = String(e)));
  });

  let url = $state("");
  let urlName = $state("");
  let localPath = $state("");
  let localName = $state("");

  async function run(fn: () => Promise<unknown>) {
    busy = true; error = null;
    try {
      await fn();
      await refreshLibrary();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function pickLocal() {
    const picked = await pickModelFile();
    if (picked) localPath = picked;
  }
</script>

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onClose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Add a model">
    <header>
      <b>Add a model</b>
      <button class="x" onclick={onClose} aria-label="Close">✕</button>
    </header>

    <nav class="tabs">
      <button class:active={tab === "catalog"} onclick={() => (tab = "catalog")}>Catalog</button>
      <button class:active={tab === "url"} onclick={() => (tab = "url")}>URL</button>
      <button class:active={tab === "local"} onclick={() => (tab = "local")}>Local file</button>
    </nav>

    {#if error}<p class="error">{error}</p>{/if}

    {#if tab === "catalog"}
      <ul class="catalog">
        {#each catalog as e (e.id)}
          {@const b = suitabilityBadge(e.suitability)}
          <li>
            <div class="ci">
              <b>{e.name}</b>
              <span class="fam">{e.family}</span>
              <span class="fit {b.tone}">{b.text}</span>
              <span class="lic">{e.license}</span>
            </div>
            <button disabled={busy} onclick={() => run(() => addCatalogModel(e.id))}>Add</button>
          </li>
        {/each}
      </ul>
    {:else if tab === "url"}
      <div class="form">
        <label>URL (https)<input bind:value={url} placeholder="https://…" /></label>
        <label>Name<input bind:value={urlName} placeholder="My model" /></label>
        <button disabled={busy || !url.startsWith("https://")} onclick={() => run(() => addUrlModel(url, urlName))}>
          Download & add
        </button>
      </div>
    {:else}
      <div class="form">
        <label>File
          <div class="pick">
            <input readonly value={localPath} placeholder="Choose a .safetensors/.gguf…" />
            <button onclick={pickLocal}>Browse…</button>
          </div>
        </label>
        <label>Name<input bind:value={localName} placeholder="My model" /></label>
        <button disabled={busy || !localPath} onclick={() => run(() => addLocalModel(localPath, localName, null))}>
          Add (reference in place)
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: var(--backdrop); display: flex; align-items: center; justify-content: center; z-index: 50; }
  .dialog { background: var(--dialog-bg); border: 1px solid var(--border); border-radius: 10px; width: min(560px, 92vw); max-height: 82vh; overflow: auto; padding: 14px; color: var(--text); }
  header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
  .x { background: none; border: none; cursor: pointer; font-size: 15px; color: inherit; }
  .tabs { display: flex; gap: 4px; margin-bottom: 12px; }
  .tabs button { flex: 1; padding: 6px; border: 1px solid var(--border); background: transparent; border-radius: 6px; cursor: pointer; color: inherit; }
  .tabs button.active { background: var(--accent-tint); border-color: var(--accent); }
  .catalog { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .catalog li { display: flex; justify-content: space-between; align-items: center; gap: 8px; padding: 8px; border: 1px solid var(--border); border-radius: 6px; }
  .ci { display: flex; flex-direction: column; gap: 2px; }
  .fam, .lic { font-size: 11px; opacity: .6; }
  .fit { font-size: 11px; }
  .fit.good { color: var(--success); } .fit.warn { color: var(--warn); } .fit.bad { color: var(--danger); } .fit.muted { opacity: .6; }
  .form { display: flex; flex-direction: column; gap: 10px; }
  .form label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
  .pick { display: flex; gap: 6px; }
  .pick input { flex: 1; }
  .error { color: var(--danger); font-size: 12px; }
</style>
