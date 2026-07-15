<script lang="ts">
  import { request, models, definitions, downloadStatus, cancelActiveDownload } from "../stores";
  import { listModels, deleteModel, deleteModelDefinition } from "../api";
  import DownloadDialog from "./DownloadDialog.svelte";
  import ModelAssembly from "./ModelAssembly.svelte";
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";
  import type { ModelDefinition } from "../types";

  let showDownload = $state(false);
  let showAssembly = $state(false);
  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  // The definition the user explicitly picked. Lets us resolve a multi-file
  // request back to its exact definition even when two definitions share a
  // diffusion checkpoint but differ in encoders. Null after a reload (we then
  // fall back to matching on diffusion path for display only).
  let selectedDefId = $state<string | null>(null);

  const NEW = "__new_multifile__";
  const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
  const basename = (p: string) => p.split(/[\\/]/).pop() || p;

  // The definition currently selected (multi_file model whose diffusion matches).
  const selectedDef = $derived.by(() => {
    if ($request.model.type !== "multi_file") return null;
    const diff = ($request.model as any).diffusion_model;
    return (
      (selectedDefId ? $definitions.find((d) => d.id === selectedDefId) : null) ??
      $definitions.find((d) => d.components.diffusion_model === diff) ??
      null
    );
  });

  // A single-file path selected but not in the scanned library (orphan).
  const orphanPath = $derived(
    $request.model.type === "single_file" && $request.model.path && !$models.some((m) => m.path === ($request.model as any).path)
      ? $request.model.path
      : null,
  );

  // Synthetic <select> value: sf:<path> | mf:<id> | "" .
  const selectValue = $derived(
    $request.model.type === "single_file"
      ? $request.model.path ? `sf:${$request.model.path}` : ""
      : selectedDef ? `mf:${selectedDef.id}` : "",
  );

  async function refresh() {
    models.set(await listModels());
  }

  function selectDefinition(def: ModelDefinition) {
    selectedDefId = def.id;
    // Snapshot the resolved components so the request stays reproducible.
    request.update((r) => ({ ...r, model: { type: "multi_file", ...def.components } }));
  }

  function onSelect(e: Event) {
    const el = e.currentTarget as HTMLSelectElement;
    const v = el.value;
    if (v === NEW) {
      el.value = selectValue; // opening the dialog must not commit NEW as a selection
      showAssembly = true;
      return;
    }
    if (v.startsWith("sf:")) {
      selectedDefId = null;
      request.update((r) => ({ ...r, model: { type: "single_file", path: v.slice(3) } }));
    } else if (v.startsWith("mf:")) {
      const def = $definitions.find((d) => d.id === v.slice(3));
      if (def) selectDefinition(def);
    }
  }

  function onAssembled(def: ModelDefinition) {
    definitions.update((d) => {
      const i = d.findIndex((x) => x.id === def.id);
      if (i === -1) return [...d, def];
      const next = d.slice();
      next[i] = def;
      return next;
    });
    selectDefinition(def);
  }

  async function removeSelected() {
    if (busy) return;
    busy = true;
    error = null;
    const def = selectedDef; // snapshot: don't re-read the derived across await
    try {
      if (def) {
        await deleteModelDefinition(def.id);
        definitions.update((d) => d.filter((x) => x.id !== def.id));
        selectedDefId = null;
        request.update((r) => ({ ...r, model: { type: "single_file", path: "" } }));
      } else if ($request.model.type === "single_file" && $request.model.path) {
        await deleteModel(($request.model as any).path);
        request.update((r) => ({ ...r, model: { type: "single_file", path: "" } }));
        await refresh();
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      confirming = false;
    }
  }

  const hasSelection = $derived(selectValue !== "");

  const dlPct = $derived(
    $downloadStatus.kind === "active" && $downloadStatus.total
      ? Math.round(($downloadStatus.downloaded / $downloadStatus.total) * 100)
      : 0,
  );
  const dlFileSuffix = $derived(
    $downloadStatus.kind === "active" && $downloadStatus.fileCount && $downloadStatus.fileCount > 1
      ? ` (${($downloadStatus.fileIndex ?? 0) + 1}/${$downloadStatus.fileCount})`
      : "",
  );

  let handledDone = $state<string | null>(null);
  $effect(() => {
    const s = $downloadStatus;
    if (s.kind === "done" && handledDone !== s.name) {
      handledDone = s.name;
      refresh().catch((e) => (error = String(e)));
    } else if (s.kind === "idle" || s.kind === "active") {
      handledDone = null;
    }
  });
</script>

<div class="field">
  <span class="label lbl-wrap">Model <InfoHint text={HELP.model} label="About models" /></span>
  <div class="row">
    <select value={selectValue} onchange={onSelect}>
      {#if !hasSelection}<option value="" disabled selected>Select a model…</option>{/if}
      {#if orphanPath}<option value={`sf:${orphanPath}`}>{basename(orphanPath)} — (not in library)</option>{/if}
      {#if $definitions.length > 0}
        <optgroup label="Multi-file models">
          {#each $definitions as d (d.id)}
            <option value={`mf:${d.id}`}>{d.name} — multi-file</option>
          {/each}
        </optgroup>
      {/if}
      <optgroup label="Single-file models">
        {#each $models as m (m.path)}
          <option value={`sf:${m.path}`}>{m.name} — {fmtSize(m.size_bytes)}</option>
        {/each}
      </optgroup>
      <option value={NEW}>＋ New multi-file model…</option>
    </select>
  </div>
  <div class="row actions">
    <button class="btn-secondary" onclick={() => (showDownload = true)} disabled={confirming}>Download…</button>
    {#if confirming}
      <span class="ask">Move to trash?</span>
      <button class="btn-secondary del" onclick={removeSelected} disabled={busy}>Delete</button>
      <button class="btn-secondary" onclick={() => (confirming = false)} disabled={busy}>Cancel</button>
    {:else}
      <button class="btn-secondary" disabled={!hasSelection} onclick={() => (confirming = true)}>Delete</button>
    {/if}
  </div>
  {#if $models.length === 0 && $definitions.length === 0}
    <span class="hint">No models found. Click Download… or add a multi-file model.</span>
  {/if}
  {#if error}<span class="err">{error}</span>{/if}

  {#if $downloadStatus.kind === "active"}
    <div class="dl">
      <div class="bar"><div class="fill" style="width:{dlPct}%"></div></div>
      <span class="dl-text">⬇ {$downloadStatus.name}{dlFileSuffix}… {fmtSize($downloadStatus.downloaded)}{$downloadStatus.total ? ` / ${fmtSize($downloadStatus.total)} (${dlPct}%)` : "…"}</span>
      <button class="btn-secondary" onclick={cancelActiveDownload}>Cancel</button>
    </div>
  {:else if $downloadStatus.kind === "done"}
    <div class="dl">
      <span class="dl-text ok">✓ {$downloadStatus.name} ready</span>
      <button class="x" aria-label="Dismiss" onclick={() => downloadStatus.set({ kind: "idle" })}>✕</button>
    </div>
  {:else if $downloadStatus.kind === "error"}
    <div class="dl">
      <span class="dl-text err">⚠ {$downloadStatus.name}: {$downloadStatus.message}</span>
      <button class="x" aria-label="Dismiss" onclick={() => downloadStatus.set({ kind: "idle" })}>✕</button>
    </div>
  {/if}
</div>

{#if showDownload}
  <DownloadDialog onclose={() => (showDownload = false)} />
{/if}

{#if showAssembly}
  <ModelAssembly onclose={() => (showAssembly = false)} onsaved={onAssembled} />
{/if}

<style>
  .lbl-wrap { display:inline-flex; align-items:center; gap:.2rem; }
  .row { display:flex; gap:.5rem; align-items:center; }
  .actions { margin-top:.4rem; }
  select { flex:1; font:inherit; font-size:.72rem; padding:.3rem; min-width:0; }
  button { font:inherit; font-size:.78rem; padding:.3rem .6rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .ask { font-size:.72rem; opacity:.85; }
  .del { background:var(--danger-bg); color:var(--on-accent); border-color:transparent; }
  .hint { font-size:.72rem; opacity:.6; margin-top:.3rem; display:block; }
  .err { font-size:.72rem; color:var(--danger); margin-top:.3rem; display:block; }
  .dl { display:flex; align-items:center; gap:.5rem; margin-top:.4rem; flex-wrap:wrap; }
  .bar { flex:1 1 100%; height:8px; background:var(--border-subtle); border-radius:4px; overflow:hidden; }
  .fill { height:100%; background:var(--accent); transition:width .15s linear; }
  .dl-text { font-size:.72rem; opacity:.85; }
  .dl-text.ok { color:var(--success); opacity:1; }
  .dl-text.err { color:var(--danger); opacity:1; }
  .x { background:none; border:none; color:inherit; cursor:pointer; font-size:.75rem; opacity:.7; padding:0 .2rem; }
</style>
