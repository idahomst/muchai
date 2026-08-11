<script lang="ts">
  let { onclose }: { onclose: () => void } = $props();
  let gotItBtn = $state<HTMLButtonElement>();

  $effect(() => { gotItBtn?.focus(); });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="modal welcome" role="dialog" aria-modal="true" aria-labelledby="welcome-title">
    <div class="modal-head">
      <span class="modal-title" id="welcome-title">Welcome to MuchAI 👋</span>
      <button class="modal-x" onclick={onclose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      <p class="intro">Make images from text in three steps:</p>
      <ol class="steps">
        <li><strong>Download a model</strong> — the AI that creates images. Use the <em>Download…</em> button under "Model".</li>
        <li><strong>Describe your image</strong> in the Prompt box — the more specific, the better.</li>
        <li><strong>Press Generate</strong> and wait a few moments for your image.</li>
      </ol>
      <p class="editnote">Already have a picture? Pick an editing model, drop the image in, and describe the change you want.</p>
      <p class="tipnote">Hover any setting's label anytime to learn what it does.</p>
    </div>

    <div class="modal-foot">
      <button class="btn btn-primary spacer" bind:this={gotItBtn} onclick={onclose}>Got it</button>
    </div>
  </div>
</div>

<style>
  .modal.welcome { width: min(420px, 92vw); }
  .intro { margin:0 0 10px; font-size:13px; color:var(--text-muted); }
  .steps { margin:0; padding-left:1.2rem; display:flex; flex-direction:column; gap:.5rem;
    font-size:13px; line-height:1.4; }
  .editnote { margin:12px 0 0; font-size:13px; line-height:1.4; color:var(--text-muted); }
  .tipnote { margin:12px 0 0; font-size:12px; color:var(--text-muted); padding:9px 12px;
    background:var(--accent-soft); border:1px solid var(--border); border-radius:var(--radius-sm); }
</style>
