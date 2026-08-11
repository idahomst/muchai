<script lang="ts">
  import { request, isEditingModel } from "../stores";
  import { HELP } from "../helpText";

  // Legacy-mode component: `$:` and store auto-subscription, not runes.
  // The label must agree with the panel above it, so both read the one store
  // that mirrors the backend's gate rather than each deriving their own.
  $: editing = $isEditingModel;
</script>

<div class="field">
  <div class="flabel">
    <label for="prompt" title={editing ? HELP.instruction : HELP.prompt}>
      {editing ? "Instruction" : "Prompt"}
    </label>
    <button type="button" class="clear"
      title={editing ? "Clear instruction" : "Clear prompt"}
      disabled={!$request.prompt}
      on:click={() => ($request.prompt = "")}>Clear</button>
  </div>
  <textarea id="prompt" rows="3" bind:value={$request.prompt}
    placeholder={editing
      ? 'make the cat blue · remove the person on the left · change the sign to say "OPEN"'
      : "a lovely cat, oil painting"}></textarea>
</div>
<div class="field">
  <div class="flabel">
    <label for="neg" title={HELP.negativePrompt}>Negative prompt</label>
    <button type="button" class="clear" title="Clear negative prompt"
      disabled={!$request.negative_prompt}
      on:click={() => ($request.negative_prompt = "")}>Clear</button>
  </div>
  <textarea id="neg" rows="2" bind:value={$request.negative_prompt} placeholder="blurry, low quality"></textarea>
</div>

<style>
  .field { margin-bottom:16px; }
  .field:last-child { margin-bottom:0; }
  .flabel { display:flex; align-items:center; margin-bottom:6px; }
  .flabel label { font-size:11px; letter-spacing:.03em; text-transform:uppercase;
    font-weight:600; color:var(--text-muted); cursor:default; }
  .clear { margin-left:auto; font:inherit; font-size:10.5px; font-weight:550;
    color:var(--text-muted); cursor:pointer; padding:2px 6px; border-radius:5px;
    background:transparent; border:none; }
  .clear:hover:not(:disabled) { background:var(--card-hover); color:var(--text); }
  .clear:disabled { opacity:.35; cursor:default; }
  /* Pinned to 16px on purpose: this size reads well and should not scale
     with the app-wide root font bump (see html font-size in app.css). */
  textarea { width:100%; resize:vertical; box-sizing:border-box;
    background:var(--card); border:1px solid var(--border); border-radius:var(--radius-sm);
    color:var(--text); font:inherit; font-size:16px; line-height:1.45; padding:9px 11px; }
  textarea:focus { outline:none; border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
  textarea::placeholder { color:var(--text-muted); }
</style>
