<script lang="ts">
  import { get } from "svelte/store";
  import { onMount } from "svelte";
  import { request, genStatus, history, currentImage } from "../stores";
  import { generate, cancelGeneration, imageSrc, listHistory, onProgress } from "../api";

  async function run() {
    const req = get(request);
    if (!req.model_path) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
    if (!req.prompt.trim()) { genStatus.set({ kind: "error", message: "Enter a prompt." }); return; }
    genStatus.set({ kind: "running", progress: null });
    try {
      const item = await generate(req);
      currentImage.set(imageSrc(item.image_path));
      history.set(await listHistory());
      genStatus.set({ kind: "idle" });
    } catch (e) {
      genStatus.set({ kind: "error", message: String(e) });
    }
  }
  $: pct = $genStatus.kind === "running" && $genStatus.progress
    ? Math.round(($genStatus.progress.current_step / $genStatus.progress.total_steps) * 100) : 0;

  onMount(() => {
    const un = onProgress((p) => genStatus.update((s) => s.kind === "running" ? { kind: "running", progress: p } : s));
    return () => { un.then((f) => f()); };
  });
</script>

<div class="bar">
  {#if $genStatus.kind === "running"}
    <div class="progress"><div class="fill" style="width:{pct}%"></div></div>
    <button class="btn-secondary" on:click={cancelGeneration}>Cancel</button>
  {:else}
    <button class="btn-primary" on:click={run}>Generate</button>
  {/if}
</div>

{#if $genStatus.kind === "error"}
  <div class="error" role="alert">{$genStatus.message}</div>
{/if}

<style>
  .bar { display:flex; gap:.5rem; align-items:center; }
  .btn-primary { flex:1; padding:.6rem; font-weight:600; }
  .progress { flex:1; height:12px; background:rgba(255,255,255,.1); border-radius:6px; overflow:hidden; }
  .fill { height:100%; background:var(--accent); transition:width .15s linear; }
  .error { margin-top:.5rem; padding:.5rem; border-radius:6px; background:rgba(255,80,80,.15);
    color:#ffb4b4; font-size:.8rem; white-space:pre-wrap; }
</style>
