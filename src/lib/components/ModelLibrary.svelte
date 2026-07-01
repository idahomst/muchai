<script lang="ts">
  import { request, models, downloadStatus, cancelActiveDownload } from "../stores";
  import { listModels, deleteModel } from "../api";
  import DownloadDialog from "./DownloadDialog.svelte";
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";

  let showDownload = $state(false);
  let confirming = $state(false);
  let busy = $state(false);
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
    if (!path || busy) return;
    busy = true;
    error = null;
    try {
      await deleteModel(path);
      request.update((r) => ({ ...r, model_path: "" }));
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      confirming = false; // close the prompt whether the delete succeeded or failed
    }
  }

  const dlPct = $derived(
    $downloadStatus.kind === "active" && $downloadStatus.total
      ? Math.round(($downloadStatus.downloaded / $downloadStatus.total) * 100)
      : 0,
  );

  // Refresh the model list once when a background download completes, so the new
  // model appears in the dropdown. The active selection (`model_path`) is left
  // untouched. `handledDone` guards against re-refreshing for the same notice;
  // the `error` state intentionally needs no reset since it's only ever reached
  // via `active` (which resets) and exits via `idle`/`active` (which reset too).
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
    <select value={$request.model_path} onchange={onSelect}>
      {#if !$request.model_path}<option value="" disabled selected>Select a model…</option>{/if}
      {#if orphanPath}<option value={orphanPath}>{basename(orphanPath)} — (not in library)</option>{/if}
      {#each $models as m (m.path)}
        <option value={m.path}>{m.name} — {fmtSize(m.size_bytes)}</option>
      {/each}
    </select>
  </div>
  <div class="row actions">
    <button class="btn-secondary" onclick={() => (showDownload = true)} disabled={confirming}>Download…</button>
    {#if confirming}
      <span class="ask">Move to trash?</span>
      <button class="btn-secondary del" onclick={removeSelected} disabled={busy}>Delete</button>
      <button class="btn-secondary" onclick={() => (confirming = false)} disabled={busy}>Cancel</button>
    {:else}
      <button class="btn-secondary" disabled={!$request.model_path} onclick={() => (confirming = true)}>Delete</button>
    {/if}
  </div>
  {#if $models.length === 0}
    <span class="hint">No models found. Click Download… to get one.</span>
  {/if}
  {#if error}<span class="err">{error}</span>{/if}

  {#if $downloadStatus.kind === "active"}
    <div class="dl">
      <div class="bar"><div class="fill" style="width:{dlPct}%"></div></div>
      <span class="dl-text">⬇ {$downloadStatus.name}… {fmtSize($downloadStatus.downloaded)}{$downloadStatus.total ? ` / ${fmtSize($downloadStatus.total)} (${dlPct}%)` : "…"}</span>
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
