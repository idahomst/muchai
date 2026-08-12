<script lang="ts">
  import { APP_TAGLINE, CREDITS } from "../about";
  import { openExternal, engineStatus } from "../api";
  import type { EngineStatus } from "../types";
  const version = __APP_VERSION__;

  let { onclose }: { onclose: () => void } = $props();
  let closeBtn = $state<HTMLButtonElement>();
  // The whole status, not just the commit: a bare hash means nothing to a user
  // comparing against the releases page, and the tag is what they can compare.
  let engine = $state<EngineStatus | null>(null);

  $effect(() => { closeBtn?.focus(); });

  // Best-effort engine probe; a failure just leaves the line hidden.
  $effect(() => {
    engineStatus().then((s) => { engine = s; }).catch(() => {});
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  // Best-effort: a dead/unreachable link must never crash or alert (mirrors
  // openFolder's fire-and-forget spirit).
  function open(url: string | undefined) {
    if (url) openExternal(url).catch(() => {});
  }
</script>

<svelte:window {onkeydown} />

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="modal about" role="dialog" aria-modal="true" aria-labelledby="about-title">
    <div class="modal-head">
      <span class="modal-title" id="about-title">About MuchAI <span class="ver">v{version}</span></span>
      <button class="modal-x" onclick={onclose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      <p class="tagline">{APP_TAGLINE}</p>

      {#each CREDITS as section}
        <div class="section">
          <p class="section-hdr">{section.heading}</p>
          <ul>
            {#each section.items as item}
              <li>
                {#if item.url}
                  <button class="link" onclick={() => open(item.url)}>{item.label}</button>
                {:else}
                  <span class="name">{item.label}</span>
                {/if}
                {#if item.note}<span class="note"> — {item.note}</span>{/if}
                {#if section.heading === "Image engine" && engine?.commit}
                  <span class="commit" title="stable-diffusion.cpp build in use">({engine.tag ? `${engine.tag}, ` : ""}commit {engine.commit})</span>
                {/if}
              </li>
            {/each}
          </ul>
        </div>
      {/each}

      <p class="copyright">© 2026 Martin Stepanek · MIT License</p>
    </div>

    <div class="modal-foot">
      <button class="btn btn-primary spacer" bind:this={closeBtn} onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .modal.about { width: min(460px, 92vw); }
  .ver { font-size:.8rem; color:var(--text-muted); font-weight:normal; }
  .tagline { margin:0 0 6px; font-size:13px; color:var(--text-muted); }
  .section { display:flex; flex-direction:column; }
  .section .section-hdr { margin:16px 0 6px; }
  ul { margin:0; padding-left:1.1rem; display:flex; flex-direction:column; gap:.15rem;
    font-size:13px; line-height:1.4; }
  .link { font:inherit; padding:0; background:none; border:none; cursor:pointer;
    color:var(--accent-bright); text-decoration:underline; }
  .note { color:var(--text-muted); }
  .commit { color:var(--text-faint); font-variant-numeric:tabular-nums; margin-left:.3rem; }
  .copyright { margin:16px 0 0; font-size:12px; color:var(--text-faint); }
</style>
