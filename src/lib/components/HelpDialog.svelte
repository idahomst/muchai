<script lang="ts">
  import { GETTING_STARTED, SHORTCUTS } from "../helpText";

  let { onclose }: { onclose: () => void } = $props();
  let closeBtn = $state<HTMLButtonElement>();

  $effect(() => { closeBtn?.focus(); });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }
</script>

<svelte:window {onkeydown} />

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="modal help" role="dialog" aria-modal="true" aria-labelledby="help-title">
    <div class="modal-head">
      <span class="modal-title" id="help-title">Help</span>
      <button class="modal-x" onclick={onclose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      <p class="section-hdr">Getting started</p>
      <ol class="steps">
        {#each GETTING_STARTED as step}
          <li><strong>{step.title}</strong> — {step.body}</li>
        {/each}
      </ol>

      <p class="section-hdr">Keyboard shortcuts</p>
      <dl class="keys">
        {#each SHORTCUTS as s}
          <dt><kbd>{s.keys}</kbd></dt>
          <dd>{s.what}</dd>
        {/each}
      </dl>

      <p class="tipnote">Click the <span aria-hidden="true">ⓘ</span> next to any setting to learn what it does.</p>
    </div>

    <div class="modal-foot">
      <button class="btn btn-primary spacer" bind:this={closeBtn} onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .modal.help { width: min(440px, 92vw); }
  .section-hdr { margin:0 0 8px; }
  .section-hdr ~ .section-hdr { margin-top:18px; }
  .steps { margin:0; padding-left:1.2rem; display:flex; flex-direction:column; gap:.5rem;
    font-size:13px; line-height:1.4; }
  /* Two columns: the key badges size to their content and stay aligned, the
     description column takes the rest. */
  .keys { margin:0; display:grid; grid-template-columns:max-content 1fr;
    gap:.4rem .7rem; align-items:center; font-size:13px; }
  .keys dt { margin:0; }
  .keys dd { margin:0; color:var(--text-muted); }
  kbd { font-family:var(--mono); font-size:11px; padding:2px 6px; border-radius:4px;
    background:var(--card); border:1px solid var(--border); color:var(--text-muted);
    white-space:nowrap; }
  .tipnote { margin:18px 0 0; font-size:12px; color:var(--text-muted); padding:9px 12px;
    background:var(--accent-soft); border:1px solid var(--border); border-radius:var(--radius-sm); }
</style>
