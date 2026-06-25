<script lang="ts">
  import { request } from "../stores";
</script>

<div class="field">
  <label class="label" for="prompt">Prompt</label>
  <textarea id="prompt" rows="3" bind:value={$request.prompt} placeholder="a lovely cat, oil painting"></textarea>
</div>
<div class="field">
  <label class="label" for="neg">Negative prompt</label>
  <textarea id="neg" rows="2" bind:value={$request.negative_prompt} placeholder="blurry, low quality"></textarea>
</div>

<style>
  textarea { width:100%; resize:vertical; font:inherit; padding:.4rem; box-sizing:border-box; }
</style>
