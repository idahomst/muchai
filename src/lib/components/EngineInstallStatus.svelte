<script lang="ts">
  import { engineInstalling, enginePct, engineError } from "../stores";
  import { cancelDownload } from "../api";
</script>

<!-- App-level engine toast, shown by +page when Preferences is closed but an
     install is still running (or has just failed). The install outlives the
     dialog and holds the generation slot while it runs, so closing Preferences
     must not be what makes it invisible and uncancellable. -->
<div class="toast" class:err={!$engineInstalling && $engineError}>
  {#if $engineInstalling}
    <div class="head">{$enginePct === null ? "Installing engine…" : `Downloading engine… ${$enginePct}%`}</div>
    <div class="bar"><div class="fill" style="width:{$enginePct ?? 0}%"></div></div>
    <button onclick={() => cancelDownload().catch(() => {})}>Cancel</button>
  {:else if $engineError}
    <div class="head">Engine update failed</div>
    <p class="msg">{$engineError}</p>
    <button onclick={() => engineError.set(null)}>Dismiss</button>
  {/if}
</div>

<style>
  /* Sits above the model toast's slot: both can be on screen at once only if a
     download was already running when the install started, which the Engine
     panel refuses — but the offset costs nothing and stacking would hide one. */
  .toast {
    position: fixed; right: 16px; bottom: 46px; z-index: 41; width: min(360px, 90vw);
    background: var(--dialog-bg); border: 1px solid var(--border); border-radius: 8px;
    padding: 10px 12px; display: flex; flex-direction: column; gap: 6px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, .3); color: var(--text);
  }
  .toast.err { border-color: var(--danger); }
  .head { font-size: 12px; font-weight: 600; }
  .msg { font-size: 12px; margin: 0; color: var(--danger); word-break: break-word; }
  .bar { height: 8px; background: var(--border-subtle); border-radius: 4px; overflow: hidden; }
  .fill { height: 100%; background: var(--accent); transition: width .15s linear; }
  button {
    align-self: flex-end; font: inherit; font-size: 12px; cursor: pointer;
    border: 1px solid var(--border); border-radius: 6px; background: transparent;
    color: inherit; padding: 3px 10px;
  }
</style>
