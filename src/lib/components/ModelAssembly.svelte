<script lang="ts">
  import { listRecipes, detectFolder, pickFolder, pickModelFile, saveModelDefinition } from "../api";
  import { ROLE_LABELS, VAE_FORMATS, PREDICTIONS } from "../types";
  import type { RecipeInfo, ComponentRole, ModelComponents, ModelDefinition } from "../types";
  import { onMount } from "svelte";

  let { onclose, onsaved }: { onclose: () => void; onsaved: (def: ModelDefinition) => void } = $props();

  type Mode = "choose" | "folder" | "manual";
  let mode = $state<Mode>("choose");

  let recipes = $state<RecipeInfo[]>([]);
  let family = $state<string>("custom");
  let name = $state("");
  let slots = $state<Partial<Record<ComponentRole, string>>>({});
  let vaeFormat = $state("");
  let prediction = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  const recipe = $derived(recipes.find((r) => r.family === family));
  const basename = (p: string) => p.split(/[\\/]/).pop() || p;

  onMount(() => {
    (async () => {
      recipes = await listRecipes();
    })();
  });

  // When the family changes (manual flow), seed the format defaults from it.
  function applyFamily(fam: string) {
    family = fam;
    const r = recipes.find((x) => x.family === fam);
    vaeFormat = r?.vae_format ?? "";
    prediction = r?.prediction ?? "";
  }

  async function chooseFolder() {
    error = null;
    const dir = await pickFolder();
    if (!dir) return;
    const result = await detectFolder(dir);
    applyFamily(result.family);
    slots = {};
    for (const s of result.slots) slots[s.role] = s.path;
    if (!name) name = result.name;
    mode = "folder";
  }

  function startManual() {
    applyFamily(family === "custom" ? "custom" : family);
    slots = {};
    mode = "manual";
  }

  async function assignSlot(role: ComponentRole) {
    const path = await pickModelFile();
    if (path) slots = { ...slots, [role]: path };
  }

  const requiredRoles = $derived(recipe?.roles.filter((r) => r.required).map((r) => r.role) ?? []);
  const missing = $derived(requiredRoles.filter((r) => !(slots[r] && slots[r]!.trim() !== "")));
  const canSave = $derived(!busy && name.trim() !== "" && missing.length === 0);

  function buildComponents(): ModelComponents {
    const c: ModelComponents = { diffusion_model: slots.diffusion ?? "" };
    if (slots.vae) c.vae = slots.vae;
    if (slots.clip_l) c.clip_l = slots.clip_l;
    if (slots.clip_g) c.clip_g = slots.clip_g;
    if (slots.t5xxl) c.t5xxl = slots.t5xxl;
    if (slots.llm) c.llm = slots.llm;
    c.vae_format = vaeFormat.trim() === "" ? null : vaeFormat;
    c.prediction = prediction.trim() === "" ? null : prediction;
    return c;
  }

  async function save() {
    if (!canSave) return;
    busy = true;
    error = null;
    const def: ModelDefinition = {
      id: crypto.randomUUID(),
      name: name.trim(),
      family,
      components: buildComponents(),
    };
    try {
      await saveModelDefinition(def);
      onsaved(def);
      onclose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="New multi-file model">
    <h2>New multi-file model</h2>

    {#if mode === "choose"}
      <p class="lead">How would you like to set up this split model?</p>
      <button class="btn-primary" onclick={chooseFolder}>From a folder I have (auto-detect)</button>
      <button class="btn-secondary" onclick={startManual}>Assign files manually</button>
      <button class="btn-secondary" onclick={onclose}>Cancel</button>
    {:else}
      <label class="fld"><span>Name</span>
        <input class="in" type="text" bind:value={name} placeholder="My FLUX model" />
      </label>

      <label class="fld"><span>Family</span>
        <select value={family} onchange={(e) => applyFamily((e.currentTarget as HTMLSelectElement).value)} disabled={mode === "folder"}>
          {#each recipes as r (r.family)}<option value={r.family}>{r.name}</option>{/each}
        </select>
      </label>

      <div class="slots">
        {#each recipe?.roles ?? [] as rs (rs.role)}
          <div class="slot" class:missing={rs.required && !(slots[rs.role] && slots[rs.role]!.trim() !== "")}>
            <span class="role">{ROLE_LABELS[rs.role]}{rs.required ? " *" : ""}</span>
            <span class="path">{slots[rs.role] ? basename(slots[rs.role]!) : "— not set"}</span>
            <button class="btn-secondary" onclick={() => assignSlot(rs.role)}>Choose…</button>
          </div>
        {/each}
      </div>

      <div class="fmt">
        <label class="fld"><span>VAE format</span>
          <select bind:value={vaeFormat}>
            {#each VAE_FORMATS as v}<option value={v}>{v === "" ? "auto (omit)" : v}</option>{/each}
          </select>
        </label>
        <label class="fld"><span>Prediction</span>
          <select bind:value={prediction}>
            {#each PREDICTIONS as p}<option value={p}>{p === "" ? "auto (omit)" : p}</option>{/each}
          </select>
        </label>
      </div>

      {#if missing.length > 0}
        <p class="hint">Fill the required (*) components to save.</p>
      {/if}
      {#if error}<p class="err">{error}</p>{/if}

      <div class="row">
        <button class="btn-primary" disabled={!canSave} onclick={save}>Save model</button>
        <button class="btn-secondary" onclick={onclose}>Cancel</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:var(--backdrop); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--dialog-bg); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(520px, 94vw); max-height:90vh; overflow-y:auto; display:flex; flex-direction:column; gap:.7rem; }
  h2 { margin:0; font-size:1.05rem; }
  .lead { font-size:.85rem; opacity:.8; margin:0; }
  .fld { display:flex; flex-direction:column; gap:.2rem; font-size:.75rem; }
  .in, select { font:inherit; padding:.35rem; box-sizing:border-box; width:100%; }
  .slots { display:flex; flex-direction:column; gap:.4rem; margin:.3rem 0; }
  .slot { display:grid; grid-template-columns:1fr 1fr auto; gap:.5rem; align-items:center;
    padding:.35rem; border:1px solid var(--border-subtle); border-radius:6px; }
  .slot.missing { border-color:var(--danger); }
  .role { font-size:.75rem; }
  .path { font-size:.72rem; opacity:.7; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .fmt { display:flex; gap:.6rem; }
  .fmt .fld { flex:1; }
  .row { display:flex; gap:.5rem; margin-top:.3rem; }
  .hint { font-size:.72rem; opacity:.7; margin:0; }
  .err { font-size:.72rem; color:var(--danger); margin:0; }
  button { font:inherit; font-size:.8rem; padding:.4rem .7rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
</style>
