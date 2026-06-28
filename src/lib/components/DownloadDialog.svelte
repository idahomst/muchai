<script lang="ts">
  import { sysStats } from "../stores";
  import { starterModels, downloadModel, cancelDownload, onDownloadProgress } from "../api";
  import type { RatedModel, Suitability } from "../types";
  import { onMount } from "svelte";

  let { onclose, ondownloaded }: { onclose: () => void; ondownloaded: (path: string) => void } = $props();

  let starters = $state<RatedModel[]>([]);
  let url = $state("");
  let token = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let cancelling = $state(false);
  let downloaded = $state(0);
  let total = $state<number | null>(null);
  let unlisten: (() => void) | null = null;

  const fmt = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
  const pct = $derived(total ? Math.round((downloaded / total) * 100) : 0);

  const badge: Record<Suitability, string> = {
    recommended: "✅ Recommended",
    tight: "⚠️ Tight for your GPU",
    too_big: "❌ Likely too big",
    unknown: "— GPU unknown",
  };

  onMount(() => {
    (async () => {
      starters = await starterModels($sysStats?.gpu?.vram_total_mb ?? null);
      unlisten = await onDownloadProgress((p) => { downloaded = p.downloaded; total = p.total; });
    })();
    return () => unlisten?.();
  });

  async function start(downloadUrl: string) {
    if (busy || !downloadUrl) return;
    busy = true; error = null; cancelling = false; downloaded = 0; total = null;
    try {
      const info = await downloadModel(downloadUrl, token.trim());
      ondownloaded(info.path);
    } catch (e) {
      if (!cancelling) error = String(e);   // silent on user-initiated cancel
    } finally {
      busy = false; cancelling = false;
    }
  }

  function cancel() { cancelling = true; void cancelDownload(); }
</script>

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Download model">
    <h2>Download a model</h2>

    {#if busy}
      <div class="progress"><div class="fill" style="width:{pct}%"></div></div>
      <p class="status">{fmt(downloaded)}{total ? ` / ${fmt(total)} (${pct}%)` : " downloaded…"}</p>
      <button class="btn-secondary" onclick={cancel}>Cancel</button>
    {:else}
      <section>
        <h3>Starter models</h3>
        {#each starters as s (s.id)}
          <div class="starter">
            <div class="meta">
              <span class="name">{s.name}</span>
              <span class="sub">{fmt(s.size_bytes)} · {badge[s.suitability]}</span>
            </div>
            <button class="btn-secondary" onclick={() => start(s.url)}>Get</button>
          </div>
        {/each}
      </section>

      <section>
        <h3>Or paste a URL</h3>
        <input class="in" type="text" placeholder="https://…/model.safetensors" bind:value={url} />
        <input class="in" type="password" placeholder="Access token (optional, for gated/civitai)" bind:value={token} />
        <div class="row">
          <button class="btn-primary" disabled={!url.trim()} onclick={() => start(url.trim())}>Download</button>
          <button class="btn-secondary" onclick={onclose}>Close</button>
        </div>
      </section>
    {/if}

    {#if error}<p class="err">{error}</p>{/if}
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:rgba(0,0,0,.5); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--bg, #1e1e1e); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(460px, 92vw); max-height:88vh; overflow-y:auto; display:flex; flex-direction:column; gap:.8rem; }
  h2 { margin:0; font-size:1.05rem; }
  h3 { margin:.2rem 0; font-size:.8rem; opacity:.7; }
  .starter { display:flex; align-items:center; justify-content:space-between; gap:.6rem; padding:.35rem 0; }
  .meta { display:flex; flex-direction:column; }
  .name { font-size:.9rem; }
  .sub { font-size:.72rem; opacity:.7; }
  .in { width:100%; font:inherit; padding:.4rem; box-sizing:border-box; margin-bottom:.4rem; }
  .row { display:flex; gap:.5rem; }
  button { font:inherit; font-size:.8rem; padding:.35rem .7rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .progress { height:12px; background:rgba(255,255,255,.1); border-radius:6px; overflow:hidden; }
  .fill { height:100%; background:var(--accent, #6ea8fe); transition:width .15s linear; }
  .status { font-size:.78rem; opacity:.85; margin:.2rem 0; }
  .err { font-size:.75rem; color:#ff6b6b; }
</style>
