<script lang="ts">
  import { request, models } from "../stores";
  import { listModels, deleteModel } from "../api";
  import DownloadDialog from "./DownloadDialog.svelte";

  let showDownload = $state(false);
  let error = $state<string | null>(null);

  const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
  const basename = (p: string) => p.split("/").pop() ?? p;

  // A model selected before the library scanned it (e.g. carried over from an
  // older config, or a path outside any watched folder). Surface it as a
  // selectable option so the dropdown isn't blank and generation still works.
  const orphanPath = $derived(
    $request.model_path && !$models.some((m) => m.path === $request.model_path)
      ? $request.model_path
      : null,
  );

  async function refresh() {
    models.set(await listModels());
  }

  function onSelect(e: Event) {
    const path = (e.currentTarget as HTMLSelectElement).value;
    request.update((r) => ({ ...r, model_path: path }));
  }

  async function removeSelected() {
    const path = $request.model_path;
    if (!path) return;
    const name = $models.find((m) => m.path === path)?.name ?? path;
    if (!confirm(`Permanently delete "${name}"? This cannot be undone.`)) return;
    error = null;
    try {
      await deleteModel(path);
      request.update((r) => ({ ...r, model_path: "" }));
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function onDownloaded(path: string) {
    await refresh();
    request.update((r) => ({ ...r, model_path: path }));
    showDownload = false;
  }
</script>

<div class="field">
  <span class="label">Model</span>
  <div class="row">
    <select value={$request.model_path} onchange={onSelect}>
      {#if !$request.model_path}<option value="" disabled selected>Select a model…</option>{/if}
      {#if orphanPath}<option value={orphanPath}>{basename(orphanPath)} — (not in library)</option>{/if}
      {#each $models as m (m.path)}
        <option value={m.path}>{m.name} — {fmtSize(m.size_bytes)}</option>
      {/each}
    </select>
  </div>
  <div class="row actions">
    <button class="btn-secondary" onclick={() => (showDownload = true)}>Download…</button>
    <button class="btn-secondary" disabled={!$request.model_path} onclick={removeSelected}>Delete</button>
  </div>
  {#if $models.length === 0}
    <span class="hint">No models found. Click Download… to get one.</span>
  {/if}
  {#if error}<span class="err">{error}</span>{/if}
</div>

{#if showDownload}
  <DownloadDialog onclose={() => (showDownload = false)} ondownloaded={onDownloaded} />
{/if}

<style>
  .row { display:flex; gap:.5rem; align-items:center; }
  .actions { margin-top:.4rem; }
  select { flex:1; font:inherit; padding:.3rem; min-width:0; }
  button { font:inherit; font-size:.78rem; padding:.3rem .6rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .hint { font-size:.72rem; opacity:.6; margin-top:.3rem; display:block; }
  .err { font-size:.72rem; color:#ff6b6b; margin-top:.3rem; display:block; }
</style>
