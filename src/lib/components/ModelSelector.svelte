<script lang="ts">
  import { library, request, selectedModelId, modelNotice, sysStats, revealModelPicker } from "../stores";
  import { rateLibrary } from "../api";
  import { familyBadge, quantBadge } from "../modelFormat";
  import { HELP } from "../helpText";
  import InfoHint from "./InfoHint.svelte";
  import type { LibraryEntry, LibraryFit } from "../types";

  let { onNew, onEdit, onDelete }:
    { onNew: () => void; onEdit: (entry: LibraryEntry) => void; onDelete: (entry: LibraryEntry) => void } = $props();

  let entries = $state<LibraryEntry[]>([]);
  library.subscribe((v) => (entries = v));

  let selId = $state<string | null>(null);
  selectedModelId.subscribe((v) => (selId = v));

  let vramTotalMb = $state<number | null>(null);
  sysStats.subscribe((s) => (vramTotalMb = s?.gpu?.vram_total_mb ?? null));

  let open = $state(false);
  let filter = $state("");
  let fits = $state<Record<string, LibraryFit>>({});
  let rootEl: HTMLDivElement;

  const selected = $derived(entries.find((e) => e.id === selId) ?? null);
  const filtered = $derived(
    entries.filter((e) => e.name.toLowerCase().includes(filter.trim().toLowerCase())),
  );

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

  function toggle() { open = !open; if (open) filter = ""; }
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

  let filterEl: HTMLInputElement | undefined = $state();
  let highlight = $state(0);

  // Keep the highlight index in range as filtering shrinks the list.
  $effect(() => { if (highlight > filtered.length - 1) highlight = Math.max(0, filtered.length - 1); });

  // On open: focus the filter and start the highlight on the selected row.
  $effect(() => {
    if (!open) return;
    const i = filtered.findIndex((e) => e.id === selId);
    highlight = i >= 0 ? i : 0;
    queueMicrotask(() => filterEl?.focus());
  });

  // Scroll the keyboard-highlighted row into view.
  $effect(() => {
    if (!open) return;
    void highlight;
    (rootEl?.querySelector(".row.active") as HTMLElement | null)?.scrollIntoView({ block: "nearest" });
  });

  function onFilterKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") { e.preventDefault(); highlight = Math.min(highlight + 1, filtered.length - 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); highlight = Math.max(highlight - 1, 0); }
    else if (e.key === "Enter") { e.preventDefault(); const en = filtered[highlight]; if (en) select(en); }
  }

  // Global Ctrl/Cmd+M toggles the selector.
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && (e.key === "m" || e.key === "M")) { e.preventDefault(); toggle(); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // Non-modal popover: close on outside pointerdown / Escape while open.
  $effect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => { if (rootEl && !rootEl.contains(e.target as Node)) close(); };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") close(); };
    document.addEventListener("pointerdown", onDown, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onDown, true);
      document.removeEventListener("keydown", onKey);
    };
  });
</script>

<div class="selector-root" bind:this={rootEl}>
  <div class="label">Model<InfoHint text={HELP.model} label="About models" /></div>

  <button class="selector" class:open onclick={toggle} aria-haspopup="listbox" aria-expanded={open}>
    <span class="diamond" aria-hidden="true">◆</span>
    {#if selected}
      <span class="sel-name">{selected.broken ? "⚠ " : ""}{selected.name}</span>
      {#if quantBadge(selected)}<span class="chip">{quantBadge(selected)}</span>{/if}
      <span class="chip fam">{familyBadge(selected)}</span>
    {:else}
      <span class="sel-name placeholder">No model selected</span>
    {/if}
    <span class="caret" aria-hidden="true">⌄</span>
  </button>

  {#if open}
    <div class="popover">
      <div class="search">
        <span class="mag" aria-hidden="true">⌕</span>
        <input class="filter" aria-label="Filter models" placeholder="Filter models…" bind:value={filter} bind:this={filterEl} onkeydown={onFilterKey} />
      </div>

      <div class="list" role="listbox" tabindex="-1" aria-activedescendant={filtered[highlight] ? "ms-" + filtered[highlight].id : undefined}>
        {#if filtered.length === 0}
          <p class="empty">{entries.length === 0 ? "No models yet — add one below." : "No matches."}</p>
        {:else}
          {#each filtered as entry, idx (entry.id)}
            <button
              class="row"
              class:selected={entry.id === selId}
              class:bad-row={fitClass(entry.id) === "bad" || entry.broken}
              class:active={idx === highlight}
              id={"ms-" + entry.id}
              role="option"
              aria-selected={entry.id === selId}
              onclick={() => select(entry)}
              onmouseenter={() => (highlight = idx)}
            >
              <span class="rname">{entry.broken ? "⚠ " : ""}{entry.name}</span>
              <span class="rmeta">
                {#if quantBadge(entry)}<span class="chip">{quantBadge(entry)}</span>{/if}
                <span class="chip fam">{familyBadge(entry)}</span>
                {#if fitLabel(entry.id)}<span class="vram {fitClass(entry.id)}">{fitLabel(entry.id)}</span>{/if}
                <span class="check">{entry.id === selId ? "✓" : ""}</span>
              </span>
            </button>
          {/each}
        {/if}
      </div>

      <div class="foot">
        <button class="fbtn add" onclick={() => { close(); onNew(); }}>＋ Add…</button>
        <button class="fbtn" disabled={!selected} onclick={() => { if (selected) { close(); onEdit(selected); } }}>Edit…</button>
        <button class="fbtn del" disabled={!selected} onclick={() => { if (selected) { close(); onDelete(selected); } }}>Delete…</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .selector-root { position: relative; display: flex; flex-direction: column; gap: 8px; }
  .label { font-size: 12px; color: var(--text-muted); display: flex; align-items: center; gap: .1rem; }

  .selector { display: flex; align-items: center; gap: 10px; width: 100%; height: 44px;
    padding: 0 12px; border-radius: var(--radius); cursor: pointer;
    background: var(--card); border: 1px solid var(--border); color: var(--text); text-align: left; }
  .selector:hover { background: var(--card-hover); border-color: var(--border-strong); }
  .selector.open { border-color: var(--border-strong); }
  .diamond { color: var(--accent); font-size: 11px; line-height: 1; }
  .sel-name { font-size: 14px; font-weight: 550; flex: 0 1 auto; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sel-name.placeholder { color: var(--text-muted); font-weight: 400; }
  .chip { font-size: 10.5px; font-weight: 600; letter-spacing: .02em; padding: 2px 7px;
    border-radius: 5px; white-space: nowrap; background: var(--card-hover); color: var(--text-muted); }
  .chip.fam { color: var(--accent); background: var(--accent-soft); }
  .caret { margin-left: auto; font-size: 12px; color: var(--text-faint); }

  .popover { position: absolute; top: calc(100% + 6px); left: 0; right: 0; z-index: 30;
    border-radius: var(--radius); overflow: hidden; background: var(--surface);
    border: 1px solid var(--border-strong); box-shadow: 0 16px 40px var(--overlay), 0 2px 8px var(--overlay-soft); }
  .search { display: flex; align-items: center; gap: 8px; padding: 9px 11px; border-bottom: 1px solid var(--border); }
  .mag { font-size: 13px; color: var(--text-muted); }
  .filter { flex: 1; background: none; border: none; color: var(--text); font: inherit; font-size: 13px; padding: 0; outline: none; }
  .filter::placeholder { color: var(--text-muted); }

  .list { padding: 6px; max-height: 320px; overflow-y: auto; }
  .row { display: flex; align-items: center; gap: 10px; width: 100%; padding: 8px 10px;
    border-radius: var(--radius-sm); cursor: pointer; position: relative;
    background: transparent; border: none; color: var(--text); text-align: left; }
  .row + .row { margin-top: 2px; }
  .row:hover { background: var(--card-hover); }
  .row.active { background: var(--card-hover); box-shadow: inset 0 0 0 1px var(--border-strong); }
  .row.selected { background: var(--accent-soft); }
  .row.selected::before { content: ""; position: absolute; left: 0; top: 8px; bottom: 8px; width: 3px;
    border-radius: 0 3px 3px 0; background: var(--accent); }
  .rname { font-size: 13.5px; font-weight: 550; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .rmeta { display: flex; align-items: center; gap: 6px; margin-left: auto; }
  .vram { font-size: 11px; font-weight: 600; min-width: 56px; text-align: right; white-space: nowrap; }
  .vram.ok { color: var(--success); }
  .vram.warn { color: var(--warn); }
  .vram.bad { color: var(--danger); }
  .row.bad-row .rname { opacity: .55; }
  .check { color: var(--accent); font-size: 13px; width: 14px; text-align: center; }
  .empty { font-size: 13px; color: var(--text-muted); margin: 6px 8px; }

  .foot { display: flex; align-items: center; gap: 4px; padding: 7px; border-top: 1px solid var(--border); }
  .fbtn { flex: 1; text-align: center; font-size: 12px; font-weight: 550; padding: 7px 8px;
    border-radius: var(--radius-sm); cursor: pointer; background: transparent; border: none; color: var(--text-muted); }
  .fbtn:hover { background: var(--card-hover); color: var(--text); }
  .fbtn:disabled { opacity: .5; cursor: default; }
  .fbtn.add { color: var(--accent); }
  .fbtn.del:hover { color: var(--danger); }
</style>
