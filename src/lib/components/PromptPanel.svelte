<script lang="ts">
  import { request } from "../stores";
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";
</script>

<div class="field">
  <div class="row">
    <span class="lbl-wrap"><label class="label" for="prompt">Prompt</label><InfoHint text={HELP.prompt} label="About the prompt" /></span>
    <button type="button" class="clear" title="Clear prompt"
      disabled={!$request.prompt}
      on:click={() => ($request.prompt = "")}>Clear</button>
  </div>
  <textarea id="prompt" rows="3" bind:value={$request.prompt} placeholder="a lovely cat, oil painting"></textarea>
</div>
<div class="field">
  <div class="row">
    <span class="lbl-wrap"><label class="label" for="neg">Negative prompt</label><InfoHint text={HELP.negativePrompt} label="About the negative prompt" /></span>
    <button type="button" class="clear" title="Clear negative prompt"
      disabled={!$request.negative_prompt}
      on:click={() => ($request.negative_prompt = "")}>Clear</button>
  </div>
  <textarea id="neg" rows="2" bind:value={$request.negative_prompt} placeholder="blurry, low quality"></textarea>
</div>

<style>
  /* Pinned to 16px on purpose: this size reads well and should not scale
     with the app-wide root font bump (see html font-size in app.css). */
  textarea { width:100%; resize:vertical; font:inherit; font-size:16px; padding:.4rem; box-sizing:border-box; }
  .row { display:flex; align-items:center; justify-content:space-between; margin-bottom:.2rem; }
  .clear { font:inherit; font-size:.7rem; padding:.1rem .45rem; cursor:pointer; opacity:.8; }
  .clear:disabled { opacity:.35; cursor:default; }
  .lbl-wrap { display:inline-flex; align-items:center; gap:.2rem; }
</style>
