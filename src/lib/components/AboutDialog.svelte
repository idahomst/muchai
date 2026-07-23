<script lang="ts">
  import { version } from "../../../package.json";
  import { APP_TAGLINE, CREDITS } from "../about";
  import { openExternal } from "../api";

  let { onclose }: { onclose: () => void } = $props();
  let closeBtn = $state<HTMLButtonElement>();

  $effect(() => { closeBtn?.focus(); });

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

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="about-title">
    <h2 id="about-title">About MuchAI <span class="ver">v{version}</span></h2>
    <p class="tagline">{APP_TAGLINE}</p>

    {#each CREDITS as section}
      <div class="section">
        <h3>{section.heading}</h3>
        <ul>
          {#each section.items as item}
            <li>
              {#if item.url}
                <button class="link" onclick={() => open(item.url)}>{item.label}</button>
              {:else}
                <span class="name">{item.label}</span>
              {/if}
              {#if item.note}<span class="note"> — {item.note}</span>{/if}
            </li>
          {/each}
        </ul>
      </div>
    {/each}

    <p class="footer">© 2026 Martin Stepanek · MIT License</p>

    <div class="row">
      <button class="btn-primary" bind:this={closeBtn} onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:var(--backdrop); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--dialog-bg); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(460px, 92vw); max-height:88vh; overflow-y:auto;
    display:flex; flex-direction:column; gap:.6rem; }
  h2 { margin:0; font-size:1.1rem; display:flex; align-items:baseline; gap:.5rem; }
  .ver { font-size:.8rem; opacity:.55; font-weight:normal; }
  .tagline { margin:0; font-size:.85rem; opacity:.85; }
  .section { display:flex; flex-direction:column; gap:.2rem; }
  .section h3 { margin:.3rem 0 0; font-size:.72rem; text-transform:uppercase;
    letter-spacing:.04em; opacity:.6; }
  ul { margin:0; padding-left:1.1rem; display:flex; flex-direction:column; gap:.15rem;
    font-size:.82rem; line-height:1.4; }
  .link { font:inherit; padding:0; background:none; border:none; cursor:pointer;
    color:var(--accent-bright); text-decoration:underline; }
  .note { opacity:.75; }
  .footer { margin:.4rem 0 0; font-size:.75rem; opacity:.6; }
  .row { display:flex; justify-content:flex-end; margin-top:.3rem; }
  .btn-primary { font:inherit; font-size:.85rem; padding:.4rem .9rem; cursor:pointer; }
</style>
