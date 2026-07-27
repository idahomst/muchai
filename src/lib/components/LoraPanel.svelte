<script lang="ts">
  import { untrack } from "svelte";
  import { get } from "svelte/store";
  import { loras, library, request, selectedModelId, refreshLoras } from "../stores";
  import { editLora, deleteLora, listFamilies } from "../api";
  import type { LoraInfo, LibraryEntry } from "../types";

  let { onAdd }: { onAdd: () => void } = $props();

  let entries = $state<LoraInfo[]>([]);
  loras.subscribe((v) => (entries = v));

  let lib = $state<LibraryEntry[]>([]);
  library.subscribe((v) => (lib = v));

  let selId = $state<string | null>(null);
  selectedModelId.subscribe((v) => (selId = v));

  let selected = $state<{ name: string; weight: number }[]>([]);
  request.subscribe((r) => (selected = r.loras));

  // Family strings come from the recipe list, so the dropdown always matches
  // what the rest of the app calls a family.
  let families = $state<string[]>([]);
  $effect(() => {
    listFamilies().then((fs) => (families = fs)).catch(() => (families = []));
  });

  const modelFamily = $derived(lib.find((e) => e.id === selId)?.family ?? "");

  // An ad-hoc model (added by file path, not through a recipe) carries no
  // family. Filtering on that would hide every LoRA and look like a bug, so an
  // unknown model family disables filtering entirely. A LoRA with no family is
  // always listed for the same reason.
  const visible = $derived(
    modelFamily === ""
      ? entries
      : entries.filter((l) => l.family === "" || l.family === modelFamily),
  );

  function isOn(name: string): boolean {
    return selected.some((s) => s.name === name);
  }
  function weightOf(name: string): number {
    return selected.find((s) => s.name === name)?.weight ?? 1.0;
  }
  function toggle(l: LoraInfo) {
    request.update((r) => ({
      ...r,
      loras: r.loras.some((s) => s.name === l.name)
        ? r.loras.filter((s) => s.name !== l.name)
        : [...r.loras, { name: l.name, weight: 1.0 }],
    }));
  }
  function setWeight(name: string, weight: number) {
    request.update((r) => ({
      ...r,
      loras: r.loras.map((s) => (s.name === name ? { ...s, weight } : s)),
    }));
  }

  // Appending, not inserting at the cursor — see the deviation note in the plan.
  function insertTrigger(word: string) {
    request.update((r) => {
      if (r.prompt.includes(word)) return r;
      const p = r.prompt.trimEnd().replace(/,$/, "");
      return { ...r, prompt: p === "" ? word : `${p}, ${word}` };
    });
  }

  // Auto-disable on model switch. A mismatched LoRA is never a hard error, but
  // it must not stay silently selected either — the user is told what was
  // turned off. `lastFamily` starts null and is only adopted once the pool has
  // loaded, so the restored selection isn't pruned against an empty list at
  // startup.
  let notice = $state<string | null>(null);
  let lastFamily: string | null = null;
  $effect(() => {
    const fam = modelFamily;
    const all = entries;
    if (all.length === 0) return;
    if (fam === lastFamily) return;
    const prev = lastFamily;
    lastFamily = fam;
    if (prev === null || fam === "") return;
    untrack(() => {
      const allowed = new Set(
        all.filter((l) => l.family === "" || l.family === fam).map((l) => l.name),
      );
      const dropped = get(request).loras.filter((s) => !allowed.has(s.name));
      if (dropped.length === 0) return;
      request.update((r) => ({ ...r, loras: r.loras.filter((s) => allowed.has(s.name)) }));
      const labels = dropped.map(
        (s) => all.find((l) => l.name === s.name)?.display_name ?? s.name,
      );
      notice = `Turned off ${labels.join(", ")} — ${labels.length === 1 ? "it doesn't" : "they don't"} match this model.`;
    });
  });

  // Row-level editor: rename, change family, delete. One inline form rather
  // than a popup menu, since rename and change-family are the same form.
  let editingId = $state<string | null>(null);
  let editName = $state("");
  let editFamily = $state("");
  let confirmingDelete = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  function openEditor(l: LoraInfo) {
    editingId = editingId === l.id ? null : l.id;
    editName = l.display_name;
    editFamily = l.family;
    confirmingDelete = false;
    error = null;
  }
  async function saveEdit(l: LoraInfo) {
    busy = true;
    error = null;
    try {
      await editLora(l.id, editName, editFamily);
      await refreshLoras();
      editingId = null;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function doDelete(l: LoraInfo) {
    busy = true;
    error = null;
    try {
      await deleteLora(l.id);
      // Drop it from the current run too, or the next generate would fail
      // pre-flight on a LoRA that no longer exists.
      request.update((r) => ({ ...r, loras: r.loras.filter((s) => s.name !== l.name) }));
      await refreshLoras();
      editingId = null;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="head">
  <p class="section">LoRAs</p>
  <button class="addbtn" type="button" onclick={onAdd}>+ Add</button>
</div>

{#if notice}
  <div class="notice" role="status">
    <span>{notice}</span>
    <button class="notice-x" aria-label="Dismiss" onclick={() => (notice = null)}>✕</button>
  </div>
{/if}

{#if entries.length === 0}
  <p class="empty">No LoRAs yet. Add one to nudge a model toward a style or subject.</p>
{:else if visible.length === 0}
  <p class="empty">None of your LoRAs match this model's family ({modelFamily}).</p>
{:else}
  <ul class="list">
    {#each visible as l (l.id)}
      <li class="row" class:broken={l.broken}>
        <div class="line">
          <label class="pick">
            <input
              type="checkbox"
              checked={isOn(l.name)}
              disabled={l.broken}
              onchange={() => toggle(l)}
            />
            <span class="label" title={l.display_name}>{l.display_name}</span>
          </label>
          <input
            class="slider"
            type="range"
            min="0"
            max="2"
            step="0.05"
            aria-label="{l.display_name} strength"
            disabled={!isOn(l.name) || l.broken}
            value={weightOf(l.name)}
            oninput={(e) => setWeight(l.name, Number(e.currentTarget.value))}
          />
          <span class="weight">{weightOf(l.name).toFixed(2)}</span>
          <button class="iconbtn" type="button" aria-label="Manage {l.display_name}" onclick={() => openEditor(l)}>⋯</button>
        </div>

        {#if l.broken}
          <p class="brokenmsg">File missing — re-add this LoRA or remove it.</p>
        {:else if l.trigger_words.length > 0}
          <div class="chips">
            <span class="chiplabel">triggers:</span>
            {#each l.trigger_words as w}
              <button class="chip" type="button" onclick={() => insertTrigger(w)}>{w}</button>
            {/each}
          </div>
        {/if}

        {#if editingId === l.id}
          <div class="editor">
            <input class="text" bind:value={editName} aria-label="LoRA name" />
            <select class="select" bind:value={editFamily} aria-label="Base family">
              <option value="">Unknown</option>
              {#each families as f}<option value={f}>{f}</option>{/each}
            </select>
            <!-- The pool filename is the engine's identity for this LoRA and is
                 immutable. Showing it here is how the user finds out an add was
                 auto-suffixed after a name collision. -->
            <p class="poolname">Stored as <code>{l.name}.safetensors</code></p>
            <div class="editrow">
              {#if confirmingDelete}
                <span class="warn">Remove “{l.display_name}”?</span>
                <button class="btn danger" disabled={busy} onclick={() => doDelete(l)}>Confirm</button>
                <button class="btn" disabled={busy} onclick={() => (confirmingDelete = false)}>Cancel</button>
              {:else}
                <button class="btn" disabled={busy} onclick={() => saveEdit(l)}>Save</button>
                <button class="btn danger" disabled={busy} onclick={() => (confirmingDelete = true)}>Remove…</button>
              {/if}
            </div>
            {#if error}<p class="err">{error}</p>{/if}
          </div>
        {/if}
      </li>
    {/each}
  </ul>
{/if}

<style>
  .head { display:flex; align-items:baseline; gap:8px; }
  .section { font-size:10.5px; letter-spacing:.05em; text-transform:uppercase;
    font-weight:600; color:var(--text-muted); margin:0 0 12px; }
  .addbtn { margin-left:auto; background:transparent; border:none; color:var(--accent);
    font:inherit; font-size:12px; font-weight:600; cursor:pointer; padding:0 0 12px; }
  .addbtn:hover { text-decoration:underline; }
  .empty { margin:0; font-size:12px; line-height:1.5; color:var(--text-muted); }
  .notice { display:flex; align-items:flex-start; gap:8px; margin:0 0 10px;
    padding:8px 10px; border-radius:var(--radius-sm); font-size:12px; line-height:1.4;
    background:var(--warn-tint); color:var(--warn); }
  .notice span { flex:1; }
  .notice-x { flex:0 0 auto; width:18px; height:18px; display:grid; place-items:center;
    border:none; background:transparent; color:inherit; cursor:pointer; font-size:11px; }
  .list { list-style:none; margin:0; padding:0; display:flex; flex-direction:column; gap:10px; }
  .row.broken { opacity:.6; }
  .line { display:flex; align-items:center; gap:8px; }
  .pick { display:flex; align-items:center; gap:6px; min-width:0; flex:1 1 auto; cursor:pointer; }
  .label { font-size:12.5px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .slider { flex:0 0 74px; accent-color:var(--accent); cursor:pointer; }
  .slider:disabled { cursor:default; opacity:.5; }
  .weight { flex:0 0 auto; font-size:11px; color:var(--text-muted);
    font-variant-numeric:tabular-nums; }
  .iconbtn { flex:0 0 auto; width:22px; height:22px; border-radius:6px; display:grid;
    place-items:center; color:var(--text-muted); cursor:pointer; font:inherit; font-size:13px;
    background:transparent; border:1px solid transparent; }
  .iconbtn:hover { background:var(--card-hover); color:var(--text); }
  .brokenmsg { margin:4px 0 0 22px; font-size:11px; color:var(--warn); }
  .chips { display:flex; flex-wrap:wrap; gap:5px; margin:5px 0 0 22px; align-items:center; }
  .chiplabel { font-size:10.5px; color:var(--text-muted); }
  .chip { background:var(--card); border:1px solid var(--border); border-radius:999px;
    color:var(--text-muted); font:inherit; font-size:11px; padding:2px 8px; cursor:pointer; }
  .chip:hover { background:var(--card-hover); color:var(--text); border-color:var(--border-strong); }
  .editor { display:flex; flex-direction:column; gap:6px; margin:8px 0 0 22px; }
  .text, .select { width:100%; background:var(--card); border:1px solid var(--border);
    border-radius:var(--radius-sm); color:var(--text); font:inherit; font-size:12.5px; padding:6px 9px; }
  .text:focus, .select:focus { outline:none; border-color:var(--accent); box-shadow:0 0 0 3px var(--accent-soft); }
  .poolname { margin:0; font-size:10.5px; color:var(--text-muted); overflow-wrap:anywhere; }
  .poolname code { font-family:var(--mono, ui-monospace, monospace); font-size:10.5px; }
  .editrow { display:flex; align-items:center; gap:6px; flex-wrap:wrap; }
  .warn { font-size:11.5px; color:var(--warn); flex:1 1 100%; }
  .btn { background:var(--card); border:1px solid var(--border); color:var(--text-muted);
    border-radius:var(--radius-sm); font:inherit; font-size:12px; padding:5px 10px; cursor:pointer; }
  .btn:hover:not(:disabled) { background:var(--card-hover); color:var(--text); border-color:var(--border-strong); }
  .btn:disabled { opacity:.5; cursor:default; }
  .btn.danger { color:var(--danger-soft); }
  .err { margin:0; font-size:11.5px; color:var(--danger-soft); overflow-wrap:anywhere; }
</style>
