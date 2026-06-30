<script lang="ts">
  import { sysStats, downloadStatus, startDownload } from "../stores";
  import { starterModels } from "../api";
  import type { RatedModel, Suitability } from "../types";
  import { onMount } from "svelte";

  let { onclose }: { onclose: () => void } = $props();

  let starters = $state<RatedModel[]>([]);
  let url = $state("");
  let token = $state("");

  const fmt = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
  const active = $derived($downloadStatus.kind === "active");

  const badge: Record<Suitability, string> = {
    recommended: "✅ Recommended",
    tight: "⚠️ Tight for your GPU",
    too_big: "❌ Likely too big",
    unknown: "— GPU unknown",
  };

  onMount(() => {
    (async () => {
      starters = await starterModels($sysStats?.gpu?.vram_total_mb ?? null);
    })();
  });

  // Derive a display name from a URL (filename without extension) for paste-URL
  // downloads, where we have no catalog name.
  function nameFromUrl(u: string): string {
    const base = u.split("/").pop()?.split("?")[0] ?? u;
    return base.replace(/\.[^.]+$/, "") || u;
  }

  // Start the download in the background and close the dialog; progress shows
  // inline in the Model panel. No-op while a download is already active.
  function start(downloadUrl: string, name: string) {
    if (active || !downloadUrl) return;
    void startDownload(downloadUrl, token.trim(), name);
    onclose();
  }
</script>

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Download model">
    <h2>Download a model</h2>

    {#if active}
      <p class="note">A download is already running — see the Model panel for progress.</p>
    {/if}

    <section>
      <h3>Starter models</h3>
      {#each starters as s (s.id)}
        <div class="starter">
          <div class="meta">
            <span class="name">{s.name}</span>
            <span class="sub">{fmt(s.size_bytes)} · {badge[s.suitability]}</span>
          </div>
          <button class="btn-secondary" disabled={active} onclick={() => start(s.url, s.name)}>Get</button>
        </div>
      {/each}
    </section>

    <section>
      <h3>Or paste a URL</h3>
      <input class="in" type="text" placeholder="https://…/model.safetensors" bind:value={url} />
      <input class="in" type="password" placeholder="Access token (optional, for gated/civitai)" bind:value={token} />
      <div class="row">
        <button class="btn-primary" disabled={active || !url.trim()} onclick={() => start(url.trim(), nameFromUrl(url.trim()))}>Download</button>
        <button class="btn-secondary" onclick={onclose}>Close</button>
      </div>
    </section>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:rgba(0,0,0,.5); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--bg, #1e1e1e); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(460px, 92vw); max-height:88vh; overflow-y:auto; display:flex; flex-direction:column; gap:.8rem; }
  h2 { margin:0; font-size:1.05rem; }
  h3 { margin:.2rem 0; font-size:.8rem; opacity:.7; }
  .note { font-size:.78rem; opacity:.85; margin:0; padding:.4rem .5rem;
    background:rgba(110,168,254,.12); border:1px solid var(--border); border-radius:6px; }
  .starter { display:flex; align-items:center; justify-content:space-between; gap:.6rem; padding:.35rem 0; }
  .meta { display:flex; flex-direction:column; }
  .name { font-size:.9rem; }
  .sub { font-size:.72rem; opacity:.7; }
  .in { width:100%; font:inherit; padding:.4rem; box-sizing:border-box; margin-bottom:.4rem; }
  .row { display:flex; gap:.5rem; }
  button { font:inherit; font-size:.8rem; padding:.35rem .7rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
</style>
