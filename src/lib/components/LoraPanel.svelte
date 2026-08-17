<script lang="ts">
  import { untrack } from "svelte";
  import { get } from "svelte/store";
  import { loras, library, request, selectedModelId } from "../stores";
  import type { LoraInfo, LibraryEntry } from "../types";
  import { mismatched } from "../loraFit";
  import { HELP } from "../helpText";
  import InfoHint from "./InfoHint.svelte";
  import LoraSelector from "./LoraSelector.svelte";

  let { onAdd, onEdit }: { onAdd: () => void; onEdit: (lora: LoraInfo) => void } = $props();

  let entries = $state<LoraInfo[]>([]);
  loras.subscribe((v) => (entries = v));

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
  function weightOf(name: string): number {
    return selected.find((s) => s.name === name)?.weight ?? 1.0;
  }

  // The sidebar shows the run, not the library. Its length tracks what is
  // switched on rather than how many files the user owns, so a fiftieth LoRA
  // costs nothing here.
  const active = $derived(entries.filter((l) => isOn(l.name)));

  function turnOff(l: LoraInfo) {
    request.update((r) => ({ ...r, loras: r.loras.filter((s) => s.name !== l.name) }));
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
  // pre-flight, but the panel only renders rows for LoRAs that are on *and* in
  // the pool, so there is nothing left to clear it with. Reachable by changing
  // models_dir, by an index.json that recovered to empty, or by reusing a
  // gallery item's settings after the LoRA was deleted. Guarded on a non-empty
  // pool so a selection restored at startup isn't pruned against a list that
  // hasn't loaded yet.
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

  // Switching models never turns anything off. The old behaviour silently
  // un-picked a LoRA the user had deliberately chosen, on the strength of a
  // guessed family. Selections that look wrong are flagged here and in the
  // picker instead, and the run is left to the user.
  const selectedMismatches = $derived(
    active.filter((l) => mismatched(l, modelFamily)).map((l) => l.display_name),
  );
</script>

<div class="head">
  <span class="sec-wrap"><p class="section">LoRAs</p><InfoHint text={HELP.lora} label="About LoRAs" /></span>
  {#if active.length > 0}<span class="count">{active.length} on</span>{/if}
  <LoraSelector {onAdd} {onEdit} />
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
{:else if active.length === 0}
  <p class="empty">No LoRAs in this run. Choose… picks from the {entries.length} you have.</p>
{:else}
  <ul class="list">
    {#each active as l (l.id)}
      <li class="row" class:broken={l.broken}>
        <!-- Name on its own line. It used to share one with a 74px slider, a
             weight readout and a button, none of which can shrink. -->
        <div class="line">
          <span class="label" title={l.display_name}>{l.display_name}</span>
          {#if mismatched(l, modelFamily)}
            <span class="badge"
              title="Trained for {l.family}; this model is {modelFamily}. You can still try it.">!</span>
          {/if}
          <button class="offbtn" type="button" title="Turn off {l.display_name}"
            aria-label="Turn off {l.display_name}" onclick={() => turnOff(l)}>×</button>
        </div>

        {#if l.broken}
          <p class="brokenmsg">File missing — re-add this LoRA or turn it off.</p>
        {:else}
          <!-- What the source says it was trained for. Finer-grained than a
               family ("Flux.2 Klein" vs plain `flux2`) and often the only way to
               tell a 4B LoRA from a 9B one, so it is shown rather than acted on. -->
          {#if l.base_model}
            <p class="basemodel">for {l.base_model}</p>
          {/if}
          <div class="strength">
            <input
              class="slider"
              type="range"
              min="0"
              max="2"
              step="0.05"
              aria-label="{l.display_name} strength"
              value={weightOf(l.name)}
              oninput={(e) => setWeight(l.name, Number(e.currentTarget.value))}
            />
            <span class="weight">{weightOf(l.name).toFixed(2)}</span>
          </div>
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
      </li>
    {/each}
  </ul>
{/if}

<style>
  .head { display:flex; align-items:baseline; gap:8px; }
  /* Own wrapper so the head's 8px gap doesn't strand the ⓘ away from the word
     it belongs to. */
  .sec-wrap { display:flex; align-items:baseline; gap:.1rem; }
  .section { font-size:10.5px; letter-spacing:.05em; text-transform:uppercase;
    font-weight:600; color:var(--text-muted); margin:0 0 12px; }
  .count { margin-left:auto; font-size:11px; color:var(--text-muted); padding-bottom:12px; }
  .empty { margin:0; font-size:12px; line-height:1.5; color:var(--text-muted); }
  .notice { display:flex; align-items:flex-start; gap:8px; margin:0 0 10px;
    padding:8px 10px; border-radius:var(--radius-sm); font-size:12px; line-height:1.4;
    background:var(--warn-tint); color:var(--warn); }
  .notice span { flex:1; }
  .notice-x { flex:0 0 auto; width:18px; height:18px; display:grid; place-items:center;
    border:none; background:transparent; color:inherit; cursor:pointer; font-size:11px; }
  .list { list-style:none; margin:0; padding:0; display:flex; flex-direction:column; gap:14px; }
  .row.broken { opacity:.6; }
  .line { display:flex; align-items:center; gap:6px; }
  .label { flex:1 1 auto; min-width:0; font-size:12.5px; font-weight:550;
    overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .offbtn { flex:0 0 auto; width:20px; height:20px; border-radius:6px; display:grid;
    place-items:center; color:var(--text-faint); cursor:pointer; font:inherit; font-size:14px;
    background:transparent; border:1px solid transparent; }
  .offbtn:hover { background:var(--card-hover); color:var(--danger); }
  .strength { display:flex; align-items:center; gap:8px; margin-top:5px; }
  .slider { flex:1 1 auto; min-width:0; accent-color:var(--accent); cursor:pointer; }
  .weight { flex:0 0 auto; font-size:11px; color:var(--text-muted);
    font-variant-numeric:tabular-nums; }
  .brokenmsg { margin:4px 0 0; font-size:11px; color:var(--warn); }
  .basemodel { margin:3px 0 0; font-size:10.5px; color:var(--text-muted); overflow-wrap:anywhere; }
  .badge { flex:0 0 auto; width:14px; height:14px; border-radius:50%; display:grid;
    place-items:center; font-size:9.5px; font-weight:700; cursor:help;
    background:var(--warn-tint); color:var(--warn); }
  .chips { display:flex; flex-wrap:wrap; gap:5px; margin:6px 0 0; align-items:center; }
  .chiplabel { font-size:10.5px; color:var(--text-muted); }
  .chip { background:var(--card); border:1px solid var(--border); border-radius:999px;
    color:var(--text-muted); font:inherit; font-size:11px; padding:2px 8px; cursor:pointer; }
  .chip:hover { background:var(--card-hover); color:var(--text); border-color:var(--border-strong); }
  /* Already in the prompt — clicking is a deliberate no-op, so say so visually
     rather than letting the click look broken. */
  .chip.used { border-color:var(--accent); color:var(--accent); }
</style>
