<script lang="ts">
  let { onclose }: { onclose: () => void } = $props();
  let gotItBtn = $state<HTMLButtonElement>();

  $effect(() => { gotItBtn?.focus(); });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
    <h2 id="welcome-title">Welcome to MuchAI 👋</h2>
    <p class="intro">Make images from text in three steps:</p>
    <ol class="steps">
      <li><strong>Download a model</strong> — the AI that creates images. Use the <em>Download…</em> button under "Model".</li>
      <li><strong>Describe your image</strong> in the Prompt box — the more specific, the better.</li>
      <li><strong>Press Generate</strong> and wait a few moments for your image.</li>
    </ol>
    <p class="tipnote">Hover the ⓘ icons anytime to learn what each setting does.</p>
    <div class="row">
      <button class="btn-primary" bind:this={gotItBtn} onclick={onclose}>Got it</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:var(--backdrop); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--dialog-bg); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(420px, 92vw); max-height:88vh; overflow-y:auto;
    display:flex; flex-direction:column; gap:.7rem; }
  h2 { margin:0; font-size:1.1rem; }
  .intro { margin:0; font-size:.85rem; opacity:.85; }
  .steps { margin:0; padding-left:1.2rem; display:flex; flex-direction:column; gap:.5rem;
    font-size:.82rem; line-height:1.4; }
  .tipnote { margin:0; font-size:.76rem; opacity:.7; padding:.4rem .5rem;
    background:var(--accent-tint); border:1px solid var(--border); border-radius:6px; }
  .row { display:flex; justify-content:flex-end; }
  button { font:inherit; font-size:.85rem; padding:.4rem .9rem; cursor:pointer; }
</style>
