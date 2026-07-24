<script lang="ts">
  import { get } from "svelte/store";
  import { catalogEntries, addCatalogModel, addUrlModel, addLocalModel, pickModelFile, openExternal } from "../api";
  import { settings, runDownload, downloadBusy, downloadProgress, downloadError } from "../stores";
  import { formatBytes, catalogTotalBytes } from "../modelFormat";
  import DownloadProgressBar from "./DownloadProgressBar.svelte";
  import type { RatedCatalogEntry } from "../types";

  let { vramTotalMb, ramTotalMb, onClose }: { vramTotalMb: number | null; ramTotalMb: number | null; onClose: () => void } = $props();

  type Tab = "catalog" | "url" | "local";
  let tab = $state<Tab>("catalog");
  let catalogError = $state<string | null>(null);

  let catalog = $state<RatedCatalogEntry[]>([]);
  $effect(() => {
    catalogEntries(vramTotalMb, ramTotalMb).then((c) => (catalog = c)).catch((e) => (catalogError = String(e)));
  });

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
    if (await runDownload(fn)) onClose();
  }

  // Which catalog entry is currently downloading, so its progress bar renders
  // inline under that row (not at the bottom of the whole list). Cleared when
  // the download settles (success closes the dialog; failure returns here).
  let downloadingId = $state<string | null>(null);
  async function runCatalog(id: string) {
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
      <div class="seg" role="group" aria-label="Model source">
        <button class="seg-item" class:on={tab === "catalog"} aria-pressed={tab === "catalog"} onclick={() => (tab = "catalog")}>Catalog</button>
        <button class="seg-item" class:on={tab === "url"} aria-pressed={tab === "url"} onclick={() => (tab = "url")}>URL</button>
        <button class="seg-item" class:on={tab === "local"} aria-pressed={tab === "local"} onclick={() => (tab = "local")}>Local file</button>
      </div>

      {#if catalogError}<p class="err">{catalogError}</p>{/if}
      {#if $downloadError}<p class="err">{$downloadError}</p>{/if}

      {#if tab === "catalog"}
        <p class="vramnote">{fitLabel}</p>
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
                <button class="btn btn-ghost btn-sm" disabled={$downloadBusy} onclick={() => runCatalog(e.id)}>Add</button>
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
        <button class="btn btn-primary" disabled={$downloadBusy || !url.startsWith("https://")} onclick={() => run(() => addUrlModel(url, urlName))}>
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
        <button class="btn btn-primary" disabled={$downloadBusy || !localPath} onclick={() => run(() => addLocalModel(localPath, localName, null))}>
          Add (reference in place)
        </button>
        {#if $downloadBusy}<div class="progress"><DownloadProgressBar progress={$downloadProgress} /></div>{/if}
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
</style>
