<script lang="ts">
  import { listRecipes, detectFolder, pickFolder, pickModelFile, saveModelDefinition, multifileCatalog, downloadModel, listHfVariants } from "../api";
  import { startMultiFileDownload, downloadStatus, settings, sysStats } from "../stores";
  import { ROLE_LABELS, VAE_FORMATS, PREDICTIONS } from "../types";
  import type { RecipeInfo, ComponentRole, ModelComponents, ModelDefinition, RatedMultiFile, RatedHfVariant, FitVerdict } from "../types";
  import { onMount } from "svelte";
  import { get } from "svelte/store";

  let { onclose, onsaved, edit = null }: {
    onclose: () => void;
    onsaved: (def: ModelDefinition) => void;
    edit?: ModelDefinition | null;
  } = $props();

  type Mode = "choose" | "folder" | "manual" | "catalog" | "hf";
  let mode = $state<Mode>("choose");

  let recipes = $state<RecipeInfo[]>([]);
  let family = $state<string>("custom");
  let name = $state("");
  let slots = $state<Partial<Record<ComponentRole, string>>>({});
  let vaeFormat = $state("");
  let prediction = $state("");
  let error = $state<string | null>(null);
  let busy = $state(false);

  // Catalog flow state.
  let catalog = $state<RatedMultiFile[]>([]);
  let catalogLoading = $state(false);
  let selectedEntry = $state<RatedMultiFile | null>(null);

  // HuggingFace variant-picker flow state.
  let hfUrl = $state("");
  let hfVariants = $state<RatedHfVariant[]>([]);
  let hfLoading = $state(false);

  const fitBadge: Record<FitVerdict, string> = {
    fits: "✅ Fits (est.)",
    tight: "⚠️ Tight (est.)",
    wont_fit: "❌ Won't fit (est.) — try Low-VRAM mode",
    unknown: "— size only",
  };

  const recipe = $derived(recipes.find((r) => r.family === family));
  const basename = (p: string) => p.split(/[\\/]/).pop() || p;
  const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);

  onMount(() => {
    (async () => {
      recipes = await listRecipes();
      if (edit) {
        family = edit.family;
        name = edit.name;
        const c = edit.components;
        const seeded: Partial<Record<ComponentRole, string>> = { diffusion: c.diffusion_model };
        if (c.vae) seeded.vae = c.vae;
        if (c.clip_l) seeded.clip_l = c.clip_l;
        if (c.clip_g) seeded.clip_g = c.clip_g;
        if (c.t5xxl) seeded.t5xxl = c.t5xxl;
        if (c.llm) seeded.llm = c.llm;
        slots = seeded;
        vaeFormat = c.vae_format ?? "";
        prediction = c.prediction ?? "";
        mode = "manual";
      }
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
    applyFamily(family);
    slots = {};
    mode = "manual";
  }

  async function openCatalog() {
    error = null;
    mode = "catalog";
    catalogLoading = true;
    try {
      catalog = await multifileCatalog(null);
    } catch (e) {
      error = String(e);
    } finally {
      catalogLoading = false;
    }
  }

  function openHf() {
    error = null;
    hfVariants = [];
    hfUrl = "";
    mode = "hf";
  }

  async function findVariants() {
    if (hfLoading || !hfUrl.trim()) return;
    hfLoading = true;
    error = null;
    hfVariants = [];
    try {
      hfVariants = await listHfVariants(
        hfUrl.trim(),
        $settings?.hf_token ?? "",
        $sysStats?.gpu?.vram_total_mb ?? null,
      );
    } catch (e) {
      error = String(e);
    } finally {
      hfLoading = false;
    }
  }

  // Download the chosen diffusion file, then drop into manual assembly with the
  // slot filled + family applied so companions can be assigned.
  async function importVariant(v: RatedHfVariant) {
    if (busy) return;
    if (get(downloadStatus).kind === "active") {
      error = "Another download is already in progress.";
      return;
    }
    busy = true;
    error = null;
    try {
      const info = await downloadModel(v.url, $settings?.hf_token ?? "");
      applyFamily(v.family ?? "custom");
      slots = { diffusion: info.path };
      if (!name) name = info.name;
      mode = "manual";
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function downloadEntry() {
    if (!selectedEntry || busy) return;
    if (get(downloadStatus).kind === "active") {
      error = "Another download is already in progress.";
      return;
    }
    busy = true;
    error = null;
    try {
      // The download resolves shared encoders/VAE once and returns the saved
      // definition (already persisted by the backend).
      const def = await startMultiFileDownload(selectedEntry.id, $settings?.hf_token ?? "", selectedEntry.name);
      if (def) {
        onsaved(def);
        onclose();
      } else {
        error = "Download did not complete.";
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
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
      id: edit?.id ?? crypto.randomUUID(),
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
  <div class="dialog" role="dialog" aria-modal="true" aria-label={edit ? "Edit multi-file model" : "New multi-file model"}>
    <h2>{edit ? "Edit multi-file model" : "New multi-file model"}</h2>

    {#if mode === "choose"}
      <p class="lead">How would you like to set up this split model?</p>
      <button class="btn-primary" onclick={chooseFolder}>From a folder I have (auto-detect)</button>
      <button class="btn-secondary" onclick={startManual}>Assign files manually</button>
      <button class="btn-secondary" onclick={openCatalog}>Download from catalog</button>
      <button class="btn-secondary" onclick={openHf}>Import from a HuggingFace URL</button>
      <button class="btn-secondary" onclick={onclose}>Cancel</button>
    {:else if mode === "catalog"}
      {#if catalogLoading}
        <p class="lead">Loading catalog…</p>
      {:else}
        <p class="lead">Pick a model. Shared encoders/VAE are downloaded once and reused.</p>
        <div class="cat">
          {#each catalog as c (c.id)}
            <button
              class="cat-row"
              class:sel={selectedEntry?.id === c.id}
              onclick={() => (selectedEntry = c)}
            >
              <span class="cat-name">{c.name}</span>
              <span class="cat-meta">{c.family} · {fmtSize(c.diffusion_size_bytes)} · needs ~{Math.round(c.recommended_vram_mb / 1024)} GB VRAM · {c.suitability}</span>
            </button>
          {/each}
        </div>
        <p class="hint">Gated downloads use your HuggingFace token from Preferences (⚙).</p>
        {#if error}<p class="err">{error}</p>{/if}
        <div class="row">
          <button class="btn-primary" disabled={!selectedEntry || busy} onclick={downloadEntry}>Download</button>
          <button class="btn-secondary" onclick={onclose} disabled={busy}>Cancel</button>
        </div>
      {/if}
    {:else if mode === "hf"}
      <p class="lead">Paste a HuggingFace model page or file URL to see its variants.</p>
      <div class="row">
        <input class="in" type="text" placeholder="https://huggingface.co/org/repo" bind:value={hfUrl} />
        <button class="btn-secondary" disabled={hfLoading || !hfUrl.trim()} onclick={findVariants}>
          {hfLoading ? "Finding…" : "Find variants"}
        </button>
      </div>
      <p class="hint">Gated repos use your HuggingFace token from Preferences (⚙). Fit is an estimate.</p>
      {#if hfVariants.length > 0}
        <div class="cat">
          {#each hfVariants as v (v.url)}
            <button class="cat-row" disabled={busy} onclick={() => importVariant(v)}>
              <span class="cat-name">{v.label}</span>
              <span class="cat-meta">
                {v.size_bytes > 0 ? fmtSize(v.size_bytes) : "size unknown"} · {fitBadge[v.verdict]}
              </span>
            </button>
          {/each}
        </div>
      {/if}
      {#if error}<p class="err">{error}</p>{/if}
      <div class="row">
        <button class="btn-secondary" onclick={onclose} disabled={busy}>Cancel</button>
      </div>
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
  .cat { display:flex; flex-direction:column; gap:.35rem; max-height:44vh; overflow-y:auto; }
  .cat-row { display:flex; flex-direction:column; align-items:flex-start; gap:.15rem; text-align:left;
    padding:.5rem; border:1px solid var(--border-subtle); border-radius:6px; background:var(--dialog-bg); color:inherit; }
  .cat-row.sel { border-color:var(--accent); }
  .cat-name { font-size:.82rem; }
  .cat-meta { font-size:.7rem; opacity:.7; }
</style>
