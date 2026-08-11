<script lang="ts">
  import { untrack } from "svelte";
  import { get } from "svelte/store";
  import { loras, library, request, selectedModelId, refreshLoras } from "../stores";
  import { editLora, deleteLora, listFamilies } from "../api";
  import type { LoraInfo, LibraryEntry } from "../types";
  import { HELP } from "../helpText";

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

  // Every LoRA is always listed. Family is a hint, never a filter: it is too
  // coarse to decide compatibility (a klein-4B and a klein-9B LoRA are both
  // `flux2` yet only one will load), and it is itself guessed — a model
  // mislabelled by the filename heuristic used to hide every LoRA the user had,
  // with no way to override it. So the picker shows what it knows and the user
  // makes the call.
  function mismatched(l: LoraInfo): boolean {
    return modelFamily !== "" && l.family !== "" && l.family !== modelFamily;
  }

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

  // Whole-word test, not `includes`: a plain substring check would treat the
  // trigger "grain" as already present in "grainy texture" and the chip would
  // silently do nothing. Trigger words routinely contain spaces and regex
  // metacharacters ("35mm photo", "a photo of (x)"), so the needle is escaped.
  function promptHas(prompt: string, word: string): boolean {
    const esc = word.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    return new RegExp(`(^|[^\\w-])${esc}($|[^\\w-])`, "i").test(prompt);
  }

  // Appending, not inserting at the cursor — see the deviation note in the plan.
  function insertTrigger(word: string) {
    request.update((r) => {
      if (promptHas(r.prompt, word)) return r;
      const p = r.prompt.trimEnd().replace(/,$/, "");
      return { ...r, prompt: p === "" ? word : `${p}, ${word}` };
    });
  }

  let prompt = $state("");
  request.subscribe((r) => (prompt = r.prompt));

  let notice = $state<string | null>(null);

  /** Drop `names` from the run and say so. Returns true if anything changed. */
  function dropSelections(names: Set<string>, all: LoraInfo[], why: string): boolean {
    const dropped = get(request).loras.filter((s) => names.has(s.name));
    if (dropped.length === 0) return false;
    request.update((r) => ({ ...r, loras: r.loras.filter((s) => !names.has(s.name)) }));
    const labels = dropped.map((s) => all.find((l) => l.name === s.name)?.display_name ?? s.name);
    notice = `Turned off ${labels.join(", ")} — ${why}`;
    return true;
  }

  // Prune selections naming a LoRA that isn't in the pool at all. Without this
  // the run is blocked forever: `resolve_selection` rejects the unknown name
  // pre-flight, but the panel only renders rows for LoRAs that exist, so there
  // is no checkbox to clear. Reachable by changing models_dir, by an index.json
  // that recovered to empty, or by reusing a gallery item's settings after the
  // LoRA was deleted. Guarded on a non-empty pool so a selection restored at
  // startup isn't pruned against a list that hasn't loaded yet.
  $effect(() => {
    const all = entries;
    if (all.length === 0) return;
    untrack(() => {
      const known = new Set(all.map((l) => l.name));
      const gone = new Set(
        get(request).loras.map((s) => s.name).filter((n) => !known.has(n)),
      );
      if (gone.size === 0) return;
      dropSelections(
        gone,
        all,
        `${gone.size === 1 ? "it is" : "they are"} no longer in your library.`,
      );
    });
  });

  // Switching models no longer turns anything off. The old behaviour silently
  // un-picked a LoRA the user had deliberately chosen, on the strength of a
  // guessed family. Selections that look wrong are flagged in the list and in
  // the strip below instead, and the run is left to the user.
  const selectedMismatches = $derived(
    entries.filter((l) => isOn(l.name) && mismatched(l)).map((l) => l.display_name),
  );

  // Row-level editor: rename, change family, delete. One inline form rather
  // than a popup menu, since rename and change-family are the same form.
  let editingId = $state<string | null>(null);
  let editName = $state("");
  let editFamily = $state("");
  let editBaseModel = $state("");
  let confirmingDelete = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  function openEditor(l: LoraInfo) {
    editingId = editingId === l.id ? null : l.id;
    editName = l.display_name;
    editFamily = l.family;
    editBaseModel = l.base_model;
    confirmingDelete = false;
    error = null;
  }
  async function saveEdit(l: LoraInfo) {
    busy = true;
    error = null;
    try {
      await editLora(l.id, editName, editFamily, editBaseModel);
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
  <p class="section" title={HELP.lora}>LoRAs</p>
  <button class="addbtn" type="button" onclick={onAdd}>+ Add</button>
</div>

{#if notice}
  <div class="notice" role="status">
    <span>{notice}</span>
    <button class="notice-x" aria-label="Dismiss" onclick={() => (notice = null)}>✕</button>
  </div>
{/if}

{#if selectedMismatches.length > 0}
  <div class="notice" role="status">
    <span>
      {selectedMismatches.join(", ")} {selectedMismatches.length === 1 ? "was" : "were"}
      trained for a different model family than the one selected. It may be ignored,
      or stop the run — turn it off if generation fails.
    </span>
  </div>
{/if}

{#if entries.length === 0}
  <p class="empty">No LoRAs yet. Add one to nudge a model toward a style or subject.</p>
{:else}
  <ul class="list">
    {#each entries as l (l.id)}
      <li class="row" class:broken={l.broken}>
        <div class="line">
          <label class="pick">
            <input
              type="checkbox"
              checked={isOn(l.name)}
              disabled={l.broken && !isOn(l.name)}
              onchange={() => toggle(l)}
            />
            <span class="label" title={l.display_name}>{l.display_name}</span>
            {#if mismatched(l)}
              <!-- A hint, not a veto: the checkbox stays live. -->
              <span
                class="badge"
                title="Trained for {l.family}; this model is {modelFamily}. You can still try it.">!</span>
            {/if}
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
        {:else}
          <!-- What the source says it was trained for. Finer-grained than a
               family ("Flux.2 Klein" vs plain `flux2`) and often the only way to
               tell a 4B LoRA from a 9B one, so it is shown rather than acted on. -->
          {#if l.base_model}
            <p class="basemodel">for {l.base_model}</p>
          {/if}
          {#if l.trigger_words.length > 0}
          <div class="chips">
            <span class="chiplabel">triggers:</span>
            {#each l.trigger_words as w}
              <button
                class="chip"
                class:used={promptHas(prompt, w)}
                type="button"
                title={promptHas(prompt, w) ? "Already in your prompt" : "Add to prompt"}
                onclick={() => insertTrigger(w)}>{w}</button>
            {/each}
          </div>
          {/if}
        {/if}

        {#if editingId === l.id}
          <div class="editor">
            <input class="text" bind:value={editName} aria-label="LoRA name" />
            <select class="select" bind:value={editFamily} aria-label="Base family">
              <option value="">Unknown</option>
              {#each families as f}<option value={f}>{f}</option>{/each}
            </select>
            <!-- Free text on purpose. It is a note to yourself, shown verbatim
                 and never matched against anything, so anything that helps you
                 recognise the right base model is a valid value. Civitai fills
                 it at add-time; a local file or an older entry starts empty. -->
            <input
              class="text"
              bind:value={editBaseModel}
              aria-label="Base model note"
              placeholder="Base model — e.g. Flux.2 Klein 4B" />
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
  .basemodel { margin:3px 0 0 22px; font-size:10.5px; color:var(--text-muted); overflow-wrap:anywhere; }
  .badge { flex:0 0 auto; width:14px; height:14px; border-radius:50%; display:grid;
    place-items:center; font-size:9.5px; font-weight:700; cursor:help;
    background:var(--warn-tint); color:var(--warn); }
  .chips { display:flex; flex-wrap:wrap; gap:5px; margin:5px 0 0 22px; align-items:center; }
  .chiplabel { font-size:10.5px; color:var(--text-muted); }
  .chip { background:var(--card); border:1px solid var(--border); border-radius:999px;
    color:var(--text-muted); font:inherit; font-size:11px; padding:2px 8px; cursor:pointer; }
  .chip:hover { background:var(--card-hover); color:var(--text); border-color:var(--border-strong); }
  /* Already in the prompt — clicking is a deliberate no-op, so say so visually
     rather than letting the click look broken. */
  .chip.used { border-color:var(--accent); color:var(--accent); }
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
