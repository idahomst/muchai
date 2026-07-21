<script lang="ts">
  import { downloadProgress, downloadBusy, downloadError } from "../stores";
  import DownloadProgressBar from "./DownloadProgressBar.svelte";
</script>

<!-- App-level download toast: shown by +page when the New-model dialog is closed
     but a download is still running (or just failed). Progress mirrors the store
     fed by the model:download:progress event. -->
<div class="toast" class:err={!$downloadBusy && $downloadError}>
  {#if $downloadBusy}
    <div class="head">Downloading model…</div>
    <DownloadProgressBar progress={$downloadProgress} />
  {:else if $downloadError}
    <div class="head">Download failed</div>
    <p class="msg">{$downloadError}</p>
    <button onclick={() => downloadError.set(null)}>Dismiss</button>
  {/if}
</div>

<style>
  .toast {
    position: fixed; right: 16px; bottom: 46px; z-index: 40; width: min(360px, 90vw);
    background: var(--dialog-bg); border: 1px solid var(--border); border-radius: 8px;
    padding: 10px 12px; display: flex; flex-direction: column; gap: 4px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, .3); color: var(--text);
  }
  .toast.err { border-color: var(--danger); }
  .head { font-size: 12px; font-weight: 600; }
  .msg { font-size: 12px; margin: 0; color: var(--danger); word-break: break-word; }
  button {
    align-self: flex-end; font: inherit; font-size: 12px; cursor: pointer;
    border: 1px solid var(--border); border-radius: 6px; background: transparent;
    color: inherit; padding: 3px 10px;
  }
</style>
