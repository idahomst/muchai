<script lang="ts">
  import { loras, library, request, selectedModelId } from "../stores";
  import { mismatched } from "../loraFit";
  import PickerPopover from "./PickerPopover.svelte";
  import type { LoraInfo, LibraryEntry } from "../types";

  let { onAdd, onEdit }: { onAdd: () => void; onEdit: (lora: LoraInfo) => void } = $props();

  let open = $state(false);
  let anchorEl = $state<HTMLButtonElement>();

  let pool = $state<LoraInfo[]>([]);
  loras.subscribe((v) => (pool = v));

  let lib = $state<LibraryEntry[]>([]);
  library.subscribe((v) => (lib = v));

  let selId = $state<string | null>(null);
  selectedModelId.subscribe((v) => (selId = v));

  let selected = $state<{ name: string; weight: number }[]>([]);
  request.subscribe((r) => (selected = r.loras));

  const modelFamily = $derived(lib.find((e) => e.id === selId)?.family ?? "");

  function isOn(name: string): boolean {
    return selected.some((s) => s.name === name);
  }

  // Ticking does not close the popover — that is the whole reason the primitive
  // leaves closing to the caller. Several LoRAs get picked in one visit.
  // A broken LoRA can be turned off but never on.
  function toggle(l: LoraInfo) {
    if (l.broken && !isOn(l.name)) return;
    request.update((r) => ({
      ...r,
      loras: r.loras.some((s) => s.name === l.name)
        ? r.loras.filter((s) => s.name !== l.name)
        : [...r.loras, { name: l.name, weight: 1.0 }],
    }));
  }

  // Same gesture as a model row: close the picker, open the modal, change
  // nothing about what is switched on.
  function edit(l: LoraInfo) {
    open = false;
    onEdit(l);
  }
</script>

<button class="choose" bind:this={anchorEl} onclick={() => (open = !open)}
  aria-haspopup="listbox" aria-expanded={open}>Choose…</button>

{#if open}
  <PickerPopover
    anchor={anchorEl}
    placeholder="Filter LoRAs…"
    items={pool}
    key={(l) => l.id}
    match={(l, q) => l.display_name.toLowerCase().includes(q) || l.base_model.toLowerCase().includes(q)}
    selected={(l) => isOn(l.name)}
    onclose={() => (open = false)}
    onchoose={toggle}
  >
    {#snippet row(l: LoraInfo)}
      <span class="mark" aria-hidden="true">{isOn(l.name) ? "☑" : "☐"}</span>
      <span class="lname" class:dim={l.broken}>{l.display_name}</span>
      <span class="lmeta">
        {#if l.base_model}<span class="base">for {l.base_model}</span>{/if}
        {#if l.broken}<span class="badge bad" title="File missing — re-add this LoRA or delete it">✕</span>{/if}
        {#if mismatched(l, modelFamily)}
          <!-- A hint, not a veto: the row stays live. -->
          <span class="badge"
            title="Trained for {l.family}; this model is {modelFamily}. You can still try it.">!</span>
        {/if}
      </span>
    {/snippet}

    {#snippet action(l: LoraInfo)}
      <button class="rowedit" type="button" title="Edit {l.display_name}"
        aria-label="Edit {l.display_name}" onclick={() => edit(l)}>✎</button>
    {/snippet}

    {#snippet empty()}
      {pool.length === 0 ? "No LoRAs yet — add one below." : "No matches."}
    {/snippet}

    {#snippet footer()}
      <button class="fbtn add" onclick={() => { open = false; onAdd(); }}>＋ Add…</button>
    {/snippet}
  </PickerPopover>
{/if}

<style>
  .choose { margin-left: auto; background: transparent; border: none; color: var(--accent);
    font: inherit; font-size: 12px; font-weight: 600; cursor: pointer; padding: 0 0 12px; }
  .choose:hover { text-decoration: underline; }

  /* Rows are authored here and rendered inside PickerPopover, so these scoped
     rules still apply. Same anatomy as a model row: marker, full-width name,
     trailing metadata, ✎ at the end. */
  .mark { flex: 0 0 auto; font-size: 13px; color: var(--text-muted); }
  .lname { flex: 1 1 auto; min-width: 0; font-size: 13.5px; font-weight: 550;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .lname.dim { opacity: .55; }
  .lmeta { flex: 0 0 auto; display: flex; align-items: center; gap: 8px; }
  .base { font-size: 11px; color: var(--text-muted); white-space: nowrap; }
  .badge { flex: 0 0 auto; width: 15px; height: 15px; border-radius: 50%; display: grid;
    place-items: center; font-size: 10px; font-weight: 700; cursor: help;
    background: var(--warn-tint); color: var(--warn); }
  .badge.bad { background: var(--danger-tint); color: var(--danger); }

  .rowedit { flex: 0 0 auto; width: 26px; height: 26px; margin-right: 6px; border-radius: 6px;
    display: grid; place-items: center; color: var(--text-faint); cursor: pointer;
    font: inherit; font-size: 12px; background: transparent; border: 1px solid transparent; }
  .rowedit:hover { background: var(--card); color: var(--text); border-color: var(--border-strong); }

  .fbtn { flex: 1; text-align: center; font-size: 12px; font-weight: 550; padding: 7px 8px;
    border-radius: var(--radius-sm); cursor: pointer; background: transparent; border: none;
    color: var(--text-muted); }
  .fbtn:hover { background: var(--card-hover); color: var(--text); }
  .fbtn.add { color: var(--accent); }
</style>
