<script lang="ts">
  import { request } from "../stores";
  import { pickModelFile } from "../api";
  async function pick() {
    const p = await pickModelFile();
    if (p) request.update((r) => ({ ...r, model_path: p }));
  }
  const basename = (p: string) => p.split("/").pop() ?? p;
</script>

<div class="field">
  <span class="label">Model</span>
  <div class="row">
    <button class="btn-secondary" on:click={pick}>Choose…</button>
    <span class="path" title={$request.model_path}>
      {$request.model_path ? basename($request.model_path) : "no model selected"}
    </span>
  </div>
</div>

<style>
  .row { display:flex; gap:.5rem; align-items:center; }
  .path { font-size:.8rem; opacity:.8; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
</style>
