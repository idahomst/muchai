<script lang="ts">
  import { library, request, selectedModelId } from "../stores";
  import { familyBadge } from "../modelFormat";
  import type { LibraryEntry } from "../types";

  // NOTE: Delete routes through `onDelete`; Edit routes through `onEdit`.
  let { onNew, onEdit, onDelete }:
    { onNew: () => void; onEdit: (entry: LibraryEntry) => void; onDelete: (entry: LibraryEntry) => void } = $props();

  let entries = $state<LibraryEntry[]>([]);
  library.subscribe((v) => (entries = v));

  let selId = $state<string | null>(null);
  selectedModelId.subscribe((v) => (selId = v));

  $effect(() => {
    // Keep a valid selection as the library changes.
    if (entries.length && !entries.some((e) => e.id === selId)) {
      selectedModelId.set(entries[0].id);
    }
  });

  const selected = $derived(entries.find((e) => e.id === selId) ?? null);

  function select(entry: LibraryEntry) {
    selectedModelId.set(entry.id);
    if (entry.broken) return;
    request.update((r) => ({ ...r, model: entry.model }));
  }
</script>

<div class="model-library">
  <div class="label">Model</div>
  {#if entries.length === 0}
    <p class="empty">No models yet. Click ＋ New to add one.</p>
  {:else}
    <ul class="rows">
      {#each entries as entry (entry.id)}
        <li>
          <button
            class="row"
            class:selected={entry.id === selId}
            class:broken={entry.broken}
            onclick={() => select(entry)}
          >
            {#if entry.broken}⚠ {/if}{entry.name}
            <span class="badge">{familyBadge(entry)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <div class="actions">
    <button class="btn" onclick={onNew}>＋ New</button>
    <button class="btn" disabled={!selected} onclick={() => selected && onEdit(selected)}>Edit</button>
    <button class="btn" disabled={!selected} onclick={() => selected && onDelete(selected)}>Delete</button>
  </div>
</div>

<style>
  .model-library { display: flex; flex-direction: column; gap: 8px; }
  .label { font-size: 12px; opacity: 0.7; }
  .rows { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 5px; }
  .row {
    width: 100%; text-align: left; padding: 6px 8px; border-radius: 6px;
    border: 1px solid var(--border); background: transparent; cursor: pointer; color: inherit;
    display: flex; justify-content: space-between; align-items: center; gap: 8px;
  }
  .row.selected { background: var(--accent-tint); border-color: var(--accent); }
  .row.broken { opacity: 0.7; }
  .badge { font-size: 11px; opacity: 0.6; }
  .empty { font-size: 13px; opacity: 0.7; margin: 0; }
  /* Equal-width action buttons (user requirement: New == Edit == Delete). */
  .actions { display: flex; gap: 6px; }
  .actions .btn {
    flex: 1; font-size: 12px; padding: 5px 0; cursor: pointer;
    border: 1px solid var(--border); border-radius: 6px; background: transparent; color: inherit;
  }
  .actions .btn:disabled { opacity: 0.5; cursor: default; }
</style>
