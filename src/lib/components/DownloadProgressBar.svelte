<script lang="ts">
  import { formatBytes } from "../modelFormat";
  import type { DownloadProgress } from "../types";

  let { progress }: { progress: DownloadProgress | null } = $props();

  const pct = $derived(progress?.total ? Math.round((progress.downloaded / progress.total) * 100) : null);
</script>

<div class="track"><div class="fill" class:indet={pct === null} style:width={pct !== null ? pct + "%" : undefined}></div></div>
<div class="ptext">
  {#if progress}
    {#if progress.file_count && progress.file_count > 1}File {(progress.file_index ?? 0) + 1}/{progress.file_count} · {/if}
    {#if progress.file_name}{progress.file_name} · {/if}
    {formatBytes(progress.downloaded)}{#if progress.total}/{formatBytes(progress.total)}{/if}{#if pct !== null} ({pct}%){/if}
  {:else}
    Preparing…
  {/if}
</div>

<style>
  .track { height: 6px; border-radius: 3px; background: var(--border); overflow: hidden; }
  .fill { height: 100%; background: var(--accent); transition: width .15s linear; }
  /* Indeterminate: total unknown — a slim pulsing sliver so the user still sees life. */
  .fill.indet { width: 35%; animation: slide 1.1s ease-in-out infinite; }
  @keyframes slide { 0% { margin-left: -35%; } 100% { margin-left: 100%; } }
  .ptext { font-size: 11px; opacity: .7; font-variant-numeric: tabular-nums; margin-top: 5px; }
</style>
