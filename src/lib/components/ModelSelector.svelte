<script lang="ts">
  import { library, request, selectedModelId, modelNotice, sysStats, revealModelPicker } from "../stores";
  import { rateLibrary } from "../api";
  import { familyBadge, quantBadge } from "../modelFormat";
  import { HELP } from "../helpText";
  import InfoHint from "./InfoHint.svelte";
  import PickerPopover from "./PickerPopover.svelte";
  import type { LibraryEntry, LibraryFit } from "../types";

  let { onNew, onEdit }:
    { onNew: () => void; onEdit: (entry: LibraryEntry) => void } = $props();

  let entries = $state<LibraryEntry[]>([]);
  library.subscribe((v) => (entries = v));

  let selId = $state<string | null>(null);
  selectedModelId.subscribe((v) => (selId = v));

  let vramTotalMb = $state<number | null>(null);
  sysStats.subscribe((s) => (vramTotalMb = s?.gpu?.vram_total_mb ?? null));

  let open = $state(false);
  let anchorEl = $state<HTMLButtonElement>();
  let fits = $state<Record<string, LibraryFit>>({});

  const selected = $derived(entries.find((e) => e.id === selId) ?? null);

  // Fetch per-model VRAM fit whenever the library or detected VRAM changes.
  // (vram_total_mb is stable after detection, so this does not spin on stats.)
  $effect(() => {
    const vram = vramTotalMb;
    void entries.length; // track library changes
    if (!entries.length) { fits = {}; return; }
    rateLibrary(vram)
      .then((list) => {
        const map: Record<string, LibraryFit> = {};
        for (const f of list) map[f.id] = f;
        fits = map;
      })
      .catch(() => {});
  });

  function fitClass(id: string): "ok" | "warn" | "bad" | "muted" {
    switch (fits[id]?.verdict) {
      case "fits": return "ok";
      case "tight": return "warn";
      case "wont_fit": return "bad";
      default: return "muted";
    }
  }
  function fitLabel(id: string): string {
    const f = fits[id];
    if (!f || f.verdict === "unknown" || f.estimate_mb == null) return "";
    const gb = Math.round(f.estimate_mb / 1024);
    const glyph = f.verdict === "fits" ? "✓" : f.verdict === "tight" ? "⚠" : "✗";
    return `${glyph} ${gb} GB`;
  }

  function toggle() { open = !open; }
  function close() { open = false; }

  // "Edit this image" picked a model on the user's behalf when several were
  // possible; opening the picker is how that choice is made visible.
  $effect(() => {
    if (!$revealModelPicker) return;
    open = true;
    revealModelPicker.set(false);
  });

  function select(entry: LibraryEntry) {
    selectedModelId.set(entry.id);
    modelNotice.set(null);
    if (!entry.broken) request.update((r) => ({ ...r, model: entry.model, model_id: entry.id }));
    close();
  }

  // ✎ never touches selectedModelId. Editing a model used to mean selecting it
  // first — changing the active model was a side effect of wanting to rename
  // one. The popover closes first so the modal is never stacked on it.
  function edit(entry: LibraryEntry) {
    close();
    onEdit(entry);
  }

  // Global Ctrl/Cmd+M toggles the selector.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "m" || e.key === "M")) { e.preventDefault(); toggle(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });
</script>

<div class="selector-root">
  <div class="label">Model<InfoHint text={HELP.model} label="About models" /></div>

  <button class="selector" class:open bind:this={anchorEl} onclick={toggle}
    aria-haspopup="listbox" aria-expanded={open}>
    <span class="diamond" aria-hidden="true">◆</span>
    <!-- Name on its own line, badges beneath it. On one line the badge group is
         nowrap and cannot shrink, so the name absorbed the whole deficit. -->
    <span class="trigger-text">
      {#if selected}
        <span class="sel-name" title={selected.name}>{selected.broken ? "⚠ " : ""}{selected.name}</span>
        <span class="sel-meta">
          {#if quantBadge(selected)}<span class="chip">{quantBadge(selected)}</span>{/if}
          <span class="chip fam">{familyBadge(selected)}</span>
        </span>
      {:else}
        <span class="sel-name placeholder">No model selected</span>
      {/if}
    </span>
    <span class="caret" aria-hidden="true">⌄</span>
  </button>

  {#if open}
    <PickerPopover
      anchor={anchorEl}
      placeholder="Filter models…"
      items={entries}
      key={(e) => e.id}
      match={(e, q) => e.name.toLowerCase().includes(q)}
      selected={(e) => e.id === selId}
      onclose={close}
      onchoose={select}
    >
      {#snippet row(entry: LibraryEntry)}
        <span class="rname" class:dim={entry.broken || fitClass(entry.id) === "bad"}
          >{entry.broken ? "⚠ " : ""}{entry.name}</span>
        <span class="rmeta">
          {#if quantBadge(entry)}<span class="chip">{quantBadge(entry)}</span>{/if}
          <span class="chip fam">{familyBadge(entry)}</span>
          {#if fitLabel(entry.id)}<span class="vram {fitClass(entry.id)}">{fitLabel(entry.id)}</span>{/if}
          <span class="check">{entry.id === selId ? "✓" : ""}</span>
        </span>
      {/snippet}

      {#snippet action(entry: LibraryEntry)}
        <button class="rowedit" type="button" title="Edit {entry.name}"
          aria-label="Edit {entry.name}" onclick={() => edit(entry)}>✎</button>
      {/snippet}

      {#snippet empty()}
        {entries.length === 0 ? "No models yet — add one below." : "No matches."}
      {/snippet}

      {#snippet footer()}
        <button class="fbtn add" onclick={() => { close(); onNew(); }}>＋ Add…</button>
      {/snippet}
    </PickerPopover>
  {/if}
</div>

<style>
  .selector-root { display: flex; flex-direction: column; gap: 8px; }
  .label { font-size: 12px; color: var(--text-muted); display: flex; align-items: center; gap: .1rem; }

  .selector { display: flex; align-items: center; gap: 10px; width: 100%; min-height: 52px;
    padding: 8px 12px; border-radius: var(--radius); cursor: pointer;
    background: var(--card); border: 1px solid var(--border); color: var(--text); text-align: left; }
  .selector:hover { background: var(--card-hover); border-color: var(--border-strong); }
  .selector.open { border-color: var(--border-strong); }
  .diamond { flex: 0 0 auto; color: var(--accent); font-size: 11px; line-height: 1; }
  .trigger-text { flex: 1 1 auto; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .sel-name { font-size: 14px; font-weight: 550; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sel-name.placeholder { color: var(--text-muted); font-weight: 400; }
  .sel-meta { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .caret { flex: 0 0 auto; font-size: 12px; color: var(--text-faint); }

  .chip { font-size: 10.5px; font-weight: 600; letter-spacing: .02em; padding: 2px 7px;
    border-radius: 5px; white-space: nowrap; background: var(--card-hover); color: var(--text-muted); }
  .chip.fam { color: var(--accent); background: var(--accent-soft); }

  /* Rows live inside PickerPopover's DOM but are authored here, so these
     scoped rules still apply. flex:1 1 auto + min-width:0 is the fix: without
     it the nowrap .rmeta group wins and the name truncates to one character. */
  .rname { flex: 1 1 auto; min-width: 0; font-size: 13.5px; font-weight: 550;
    white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .rname.dim { opacity: .55; }
  .rmeta { flex: 0 0 auto; display: flex; align-items: center; gap: 6px; }
  .vram { font-size: 11px; font-weight: 600; min-width: 56px; text-align: right; white-space: nowrap; }
  .vram.ok { color: var(--success); }
  .vram.warn { color: var(--warn); }
  .vram.bad { color: var(--danger); }
  .check { color: var(--accent); font-size: 13px; width: 14px; text-align: center; }

  .rowedit { flex: 0 0 auto; width: 26px; height: 26px; margin-right: 6px; border-radius: 6px;
    display: grid; place-items: center; color: var(--text-faint); cursor: pointer;
    font: inherit; font-size: 12px; background: transparent; border: 1px solid transparent; }
  .rowedit:hover { background: var(--card); color: var(--text); border-color: var(--border-strong); }

  .fbtn { flex: 1; text-align: center; font-size: 12px; font-weight: 550; padding: 7px 8px;
    border-radius: var(--radius-sm); cursor: pointer; background: transparent; border: none; color: var(--text-muted); }
  .fbtn:hover { background: var(--card-hover); color: var(--text); }
  .fbtn.add { color: var(--accent); }
</style>
