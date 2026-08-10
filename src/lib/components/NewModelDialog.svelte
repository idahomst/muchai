<script lang="ts">
  import { get } from "svelte/store";
  import { catalogEntries, addCatalogModel, addUrlModel, addLocalModel, pickModelFile, openExternal,
           diskSpace, checkCatalogSpace, listReclaimable, trashDir, deleteModelEntry, openFolder } from "../api";
  import { settings, runDownload, downloadBusy, downloadProgress, downloadError, refreshLibrary, engineInstalling } from "../stores";
  import { formatBytes, catalogTotalBytes } from "../modelFormat";
  import DownloadProgressBar from "./DownloadProgressBar.svelte";
  import { INSUFFICIENT_SPACE_PREFIX } from "../types";
  import type { RatedCatalogEntry, ReclaimableModel } from "../types";

  let { vramTotalMb, ramTotalMb, onClose }: { vramTotalMb: number | null; ramTotalMb: number | null; onClose: () => void } = $props();

  type Tab = "catalog" | "url" | "local";
  let tab = $state<Tab>("catalog");
  let catalogError = $state<string | null>(null);

  let catalog = $state<RatedCatalogEntry[]>([]);
  $effect(() => {
    catalogEntries(vramTotalMb, ramTotalMb).then((c) => (catalog = c)).catch((e) => (catalogError = String(e)));
  });

  // Free space where models land. Null = probe failed; we then show nothing
  // rather than a scary blank number.
  let freeBytes = $state<number | null>(null);
  async function refreshFree() {
    freeBytes = await diskSpace().catch(() => null);
  }
  $effect(() => {
    refreshFree();
  });

  // Blocked state. `required` is known for catalog pre-flights and null when we
  // only learned about it from a download error (the error text carries the
  // numbers in that case).
  let blocked = $state<{ required: number | null } | null>(null);
  let reclaimable = $state<ReclaimableModel[]>([]);
  let confirmId = $state<string | null>(null);
  let trashNote = $state(false);
  let trashPath = $state<string | null>(null);
  // Separate from `catalogError`, which only renders inside the tab body — an
  // error raised while the blocked panel is open must be visible in the panel.
  let reclaimError = $state<string | null>(null);

  async function openBlocked(required: number | null) {
    blocked = { required };
    confirmId = null;
    reclaimError = null;
    trashNote = false;
    reclaimable = await listReclaimable().catch(() => []);
    trashPath = await trashDir().catch(() => null);
    await refreshFree();
  }

  /** Delete a model to reclaim its bytes, then re-measure. If free space did
   *  not actually grow, the trash is on the same filesystem and still holds
   *  the data — say so instead of leaving the user confused. */
  async function reclaim(m: ReclaimableModel) {
    const before = freeBytes;
    reclaimError = null;
    try {
      await deleteModelEntry(m.id);
    } catch (e) {
      reclaimError = String(e);
      confirmId = null;
      return;
    }
    confirmId = null;
    await refreshLibrary();
    reclaimable = await listReclaimable().catch(() => []);
    await refreshFree();
    if (before !== null && freeBytes !== null) {
      trashNote = freeBytes - before < m.size_bytes / 2;
    }
  }

  // Show what fit is rated against: VRAM if a GPU is present, else RAM (CPU path).
  const fitLabel = $derived(
    vramTotalMb
      ? `Your VRAM: ${+(vramTotalMb / 1024).toFixed(1)} GB`
      : ramTotalMb
        ? `No GPU detected — fit vs. RAM: ${+(ramTotalMb / 1024).toFixed(1)} GB (CPU generation is slow)`
        : "Hardware unknown — fit not rated",
  );

  // Sort best→worst by fit so the most usable models surface first. Array.sort is
  // stable, so catalog (curated) order is preserved within each suitability tier.
  const RANK: Record<string, number> = { recommended: 0, tight: 1, too_big: 2, unknown: 3 };
  const sorted = $derived([...catalog].sort((a, b) => RANK[a.suitability] - RANK[b.suitability]));

  // "Best fit" is scarce — only the top 1–2 recommended picks earn it, and only
  // when we actually have a hardware budget to rate against.
  const bestFitIds = $derived(
    new Set(
      vramTotalMb || ramTotalMb
        ? sorted.filter((e) => e.suitability === "recommended").slice(0, 2).map((e) => e.id)
        : [],
    ),
  );

  // Compact per-row fit chip. Over-budget shows the download size so the reason
  // ("won't fit — it's 14 GB") is obvious at a glance.
  function compactFit(e: RatedCatalogEntry): { cls: string; text: string } {
    switch (e.suitability) {
      case "recommended": return { cls: "ok", text: "✓ fits" };
      case "tight": return { cls: "warn", text: "⚠ tight" };
      case "too_big": return { cls: "bad", text: `✗ ${formatBytes(catalogTotalBytes(e))}` };
      default: return { cls: "muted", text: "—" };
    }
  }

  let url = $state("");
  let urlName = $state("");
  let localPath = $state("");
  let localName = $state("");

  // Downloads run at the app level (stores.runDownload) so busy/progress/error
  // survive this dialog closing mid-download. Close only on success.
  async function run(fn: () => Promise<unknown>) {
    if ((await runDownload(fn)) !== null) {
      onClose();
      return;
    }
    const err = get(downloadError);
    if (err?.includes(INSUFFICIENT_SPACE_PREFIX)) await openBlocked(null);
  }

  // Which catalog entry is currently downloading, so its progress bar renders
  // inline under that row (not at the bottom of the whole list). Cleared when
  // the download settles (success closes the dialog; failure returns here).
  let downloadingId = $state<string | null>(null);
  async function runCatalog(id: string) {
    const check = await checkCatalogSpace(id).catch(() => null);
    if (check && !check.ok) {
      await openBlocked(check.required_bytes);
      return;
    }
    downloadingId = id;
    try {
      await run(() => addCatalogModel(id));
    } finally {
      downloadingId = null;
    }
  }

  async function pickLocal() {
    const picked = await pickModelFile(get(settings)?.models_dir ?? undefined);
    if (picked) localPath = picked;
  }
</script>

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onClose(); }} role="presentation">
  <div class="modal" role="dialog" aria-modal="true" aria-label="Add a model">
    <div class="modal-head">
      <span class="modal-title">Add a model</span>
      <button class="modal-x" onclick={onClose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      {#if blocked}
        <div class="blocked">
          <p class="bhead">Not enough disk space</p>
          {#if blocked.required !== null}
            <p class="bsub">
              Needs {formatBytes(blocked.required)}{#if freeBytes !== null} · {formatBytes(freeBytes)} free{/if}
            </p>
          {:else if $downloadError}
            <p class="bsub">{$downloadError}</p>
          {/if}

          {#if reclaimError}<p class="err">{reclaimError}</p>{/if}

          <p class="microlabel">Delete a model to make room</p>
          {#if reclaimable.length === 0}
            <p class="bsub">No installed models to remove.</p>
          {/if}
          {#each reclaimable as m (m.id)}
            <div class="rrow">
              <span class="rname">{m.name}</span>
              <span class="rsize">{formatBytes(m.size_bytes)}</span>
              {#if confirmId === m.id}
                <span class="rconfirm">Move to trash?</span>
                <button class="btn btn-danger btn-sm" onclick={() => reclaim(m)}>Delete</button>
                <button class="btn btn-ghost btn-sm" onclick={() => (confirmId = null)}>Cancel</button>
              {:else}
                <button class="btn btn-ghost btn-sm" onclick={() => (confirmId = m.id)}>Delete</button>
              {/if}
            </div>
          {/each}

          {#if trashNote}
            <p class="tnote">
              Deleted models are in the Trash and still use disk space.
              {#if trashPath}
                <button type="button" class="src" onclick={() => openFolder(trashPath!)}>Open Trash ↗</button>
              {/if}
            </p>
          {/if}

          <div class="bfoot">
            <button class="btn btn-ghost" onclick={() => (blocked = null)}>Back</button>
          </div>
        </div>
      {:else}
        <div class="seg" role="group" aria-label="Model source">
          <button class="seg-item" class:on={tab === "catalog"} aria-pressed={tab === "catalog"} onclick={() => (tab = "catalog")}>Catalog</button>
          <button class="seg-item" class:on={tab === "url"} aria-pressed={tab === "url"} onclick={() => (tab = "url")}>URL</button>
          <button class="seg-item" class:on={tab === "local"} aria-pressed={tab === "local"} onclick={() => (tab = "local")}>Local file</button>
        </div>

        {#if catalogError}<p class="err">{catalogError}</p>{/if}
        {#if $downloadError}<p class="err">{$downloadError}</p>{/if}

        {#if tab === "catalog"}
          <p class="vramnote">
            {fitLabel}{#if freeBytes !== null} · Disk free: {formatBytes(freeBytes)}{/if}
          </p>
          <div class="catlist">
            {#each sorted as e (e.id)}
              {@const f = compactFit(e)}
              <div class="catrow" class:dim={e.suitability === "too_big"}>
                <div class="catmain">
                  <div class="catname">
                    {e.name}
                    {#if bestFitIds.has(e.id)}<span class="best">Best fit</span>{/if}
                  </div>
                  <div class="catmeta">
                    <span class="fam">{e.family}</span> · {formatBytes(catalogTotalBytes(e))} · {e.license}
                    {#if e.source_url}
                      · <button type="button" class="src" title={e.source_url} onclick={() => openExternal(e.source_url)}>Source ↗</button>
                    {/if}
                  </div>
                  {#if downloadingId === e.id && $downloadBusy}
                    <div class="progress"><DownloadProgressBar progress={$downloadProgress} /></div>
                  {/if}
                </div>
                <div class="catadd">
                  <span class="vfit {f.cls}">{f.text}</span>
                  <button class="btn btn-ghost btn-sm" disabled={$downloadBusy || $engineInstalling} onclick={() => runCatalog(e.id)}>Add</button>
                </div>
              </div>
            {/each}
          </div>
        {:else if tab === "url"}
          <div class="dlg-field">
            <p class="microlabel">URL (https)</p>
            <input class="dlg-input" bind:value={url} placeholder="https://…" />
          </div>
          <div class="dlg-field">
            <p class="microlabel">Name</p>
            <input class="dlg-input" bind:value={urlName} placeholder="My model" />
          </div>
          <button class="btn btn-primary" disabled={$downloadBusy || $engineInstalling || !url.startsWith("https://")} onclick={() => run(() => addUrlModel(url, urlName))}>
            Download &amp; add
          </button>
          {#if $downloadBusy}<div class="progress"><DownloadProgressBar progress={$downloadProgress} /></div>{/if}
        {:else}
          <div class="dlg-field">
            <p class="microlabel">File</p>
            <div class="pick">
              <input class="dlg-input" readonly value={localPath} placeholder="Choose a .safetensors/.gguf…" />
              <button class="btn btn-ghost" onclick={pickLocal}>Browse…</button>
            </div>
          </div>
          <div class="dlg-field">
            <p class="microlabel">Name</p>
            <input class="dlg-input" bind:value={localName} placeholder="My model" />
          </div>
          <button class="btn btn-primary" disabled={$downloadBusy || $engineInstalling || !localPath} onclick={() => run(() => addLocalModel(localPath, localName, null))}>
            Add (reference in place)
          </button>
          {#if $downloadBusy}<div class="progress"><DownloadProgressBar progress={$downloadProgress} /></div>{/if}
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  .vramnote { font-size: 12px; color: var(--text-muted); margin: 0 0 14px; }
  .catlist { margin: 0 -6px; padding: 0 6px; }
  .catrow { display: flex; align-items: center; gap: 12px; padding: 11px 12px; border-radius: var(--radius-sm); }
  .catrow:hover { background: var(--card); }
  .catrow + .catrow { border-top: 1px solid var(--border); }
  .catrow:hover + .catrow { border-top-color: transparent; }
  .catrow.dim { opacity: .6; }
  .catmain { min-width: 0; flex: 1; }
  .catname { font-size: 13.5px; font-weight: 600; display: flex; align-items: center; gap: 8px; }
  .best { font-size: 9.5px; font-weight: 700; letter-spacing: .04em; color: var(--accent);
    background: var(--accent-soft); padding: 2px 6px; border-radius: 5px; text-transform: uppercase; }
  .catmeta { font-size: 11.5px; color: var(--text-muted); margin-top: 3px; }
  .catmeta .fam { color: var(--text); }
  .src { background: none; border: none; padding: 0; font: inherit; font-size: 11.5px; color: var(--accent); cursor: pointer; }
  .src:hover { text-decoration: underline; }
  .catadd { margin-left: auto; display: flex; align-items: center; gap: 10px; flex: 0 0 auto; }
  .vfit { font-size: 11px; font-weight: 600; white-space: nowrap; }
  .vfit.ok { color: var(--success); } .vfit.warn { color: var(--warn); }
  .vfit.bad { color: var(--danger); } .vfit.muted { color: var(--text-muted); }
  .pick { display: flex; gap: 8px; }
  .pick .dlg-input { flex: 1; }
  .err { color: var(--danger); font-size: 12px; margin: 0 0 10px; }
  .progress { margin-top: 12px; }

  .blocked { display: flex; flex-direction: column; gap: 10px; }
  .bhead { font-size: 15px; font-weight: 700; color: var(--danger); margin: 0; }
  .bsub { font-size: 12.5px; color: var(--text-muted); margin: 0; }
  .rrow { display: flex; align-items: center; gap: 10px; padding: 8px 0; border-top: 1px solid var(--border); }
  .rname { font-size: 13px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .rsize { font-family: var(--mono); font-size: 11.5px; color: var(--text-muted); margin-left: auto; }
  .rconfirm { font-size: 11.5px; color: var(--text-muted); }
  .tnote { font-size: 11.5px; color: var(--warn); margin: 0; }
  .bfoot { display: flex; justify-content: flex-end; margin-top: 4px; }
</style>
