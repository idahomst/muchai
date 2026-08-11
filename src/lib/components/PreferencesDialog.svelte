<script lang="ts">
  import { settings, engineUpdateTag, sysStats } from "$lib/stores";
  import { setSettings, autoUmaBudgetMb } from "$lib/api";
  import { LOAD_PRECISION_OPTIONS, type LoadPrecision } from "$lib/types";
  import ModelFolders from "./ModelFolders.svelte";
  import GalleryLocation from "./GalleryLocation.svelte";
  import DevicePicker from "./DevicePicker.svelte";
  import EnginePanel from "./EnginePanel.svelte";
  import ThemeToggle from "./ThemeToggle.svelte";

  let { onclose }: { onclose: () => void } = $props();

  // Local editable copies of the tokens, seeded from settings (null → "").
  let hf = $state($settings?.hf_token ?? "");
  let civitai = $state($settings?.civitai_token ?? "");
  let showHf = $state(false);
  let showCivitai = $state(false);
  let error = $state<string | null>(null);

  // The config stores MB; this field talks GB, the unit model sizes and the
  // monitor row are quoted in. Seeded once on mount, like the token fields.
  const mbToGb = (mb: number | null) => (mb === null ? "" : (mb / 1024).toFixed(1));
  let umaBudget = $state(mbToGb($settings?.uma_budget_mb ?? null));
  // The auto figure for this machine, for the placeholder. Fetched rather than
  // recomputed here so the 70%-of-RAM rule lives in exactly one place.
  let autoBudget = $state<string | null>(null);
  $effect(() => {
    autoUmaBudgetMb()
      .then((mb) => (autoBudget = mbToGb(mb)))
      .catch(() => (autoBudget = null));
  });

  // Writes are serialized through this promise chain so two token fields saved in
  // quick succession (the first-run case: entering HF + Civitai together) can't
  // race the config file or clobber each other, and no save is silently dropped.
  let saveChain: Promise<void> = Promise.resolve();

  // Queue a save for one token field. Empty string is stored as null so "is it
  // set?" is a simple null check.
  function saveToken(field: "hf_token" | "civitai_token", value: string) {
    saveChain = saveChain.then(() => persistToken(field, value));
  }

  // Persist one token field. Optimistic with per-field rollback: on failure only
  // the affected field reverts (preserving any other field edited meanwhile) and
  // its local input is re-seeded so it never shows a value that wasn't persisted.
  async function persistToken(field: "hf_token" | "civitai_token", value: string) {
    const cur = $settings;
    if (!cur) return;
    const normalized = value.trim() === "" ? null : value.trim();
    if (cur[field] === normalized) return;
    const next = { ...cur, [field]: normalized };
    settings.set(next);
    error = null;
    try {
      await setSettings(next);
    } catch (e) {
      // revert only this field, preserving any other field's concurrent edit
      settings.set({ ...($settings ?? cur), [field]: cur[field] });
      if (field === "hf_token") hf = cur.hf_token ?? "";
      else civitai = cur.civitai_token ?? "";
      error = String(e);
    }
  }

  // Optimistic save of the low-VRAM toggle, reusing the same serialized chain so
  // it can't race a token save. Reverts on failure.
  function saveLowVram(value: boolean) {
    saveChain = saveChain.then(async () => {
      const cur = $settings;
      if (!cur || cur.low_vram === value) return;
      const next = { ...cur, low_vram: value };
      settings.set(next);
      error = null;
      try {
        await setSettings(next);
      } catch (e) {
        settings.set({ ...($settings ?? cur), low_vram: cur.low_vram });
        error = String(e);
      }
    });
  }

  // Optimistic save of the live-preview toggle, on the same serialized chain as
  // low-VRAM / tokens so no save races or is dropped. Reverts on failure.
  function saveLivePreview(value: boolean) {
    saveChain = saveChain.then(async () => {
      const cur = $settings;
      if (!cur || cur.live_preview === value) return;
      const next = { ...cur, live_preview: value };
      settings.set(next);
      error = null;
      try {
        await setSettings(next);
      } catch (e) {
        settings.set({ ...($settings ?? cur), live_preview: cur.live_preview });
        error = String(e);
      }
    });
  }

  // Optimistic save of the load-precision select, on the same serialized chain
  // as the toggles above so no save races or is dropped. Reverts on failure.
  function saveLoadPrecision(value: LoadPrecision) {
    saveChain = saveChain.then(async () => {
      const cur = $settings;
      if (!cur || cur.load_precision === value) return;
      const next = { ...cur, load_precision: value };
      settings.set(next);
      error = null;
      try {
        await setSettings(next);
      } catch (e) {
        settings.set({ ...($settings ?? cur), load_precision: cur.load_precision });
        error = String(e);
      }
    });
  }

  const precisionHint = $derived(
    LOAD_PRECISION_OPTIONS.find((o) => o.value === ($settings?.load_precision ?? "auto"))?.hint ?? ""
  );

  // Optimistic save of the shared-memory budget, on the same serialized chain as
  // the tokens and toggles. Blank — or anything that isn't a positive number —
  // means auto, stored as null rather than a silent 0. `target` is the input
  // element itself: the `value={umaBudget}` binding is one-way and only touches
  // the DOM when the expression changes, so when normalizing lands back on the
  // already-stored value (e.g. rejected text, or two typings that round to the
  // same GB) there is no state change to clear what's on screen — set the DOM
  // directly so a rejected entry never lingers as if it were in force.
  function saveUmaBudget(raw: string, target: HTMLInputElement) {
    saveChain = saveChain.then(async () => {
      const cur = $settings;
      if (!cur) return;
      const gb = Number(raw.trim().replace(",", "."));
      const value = raw.trim() === "" || !Number.isFinite(gb) || gb <= 0 ? null : Math.round(gb * 1024);
      if (cur.uma_budget_mb === value) {
        umaBudget = mbToGb(value);
        target.value = umaBudget;
        return;
      }
      const next = { ...cur, uma_budget_mb: value };
      settings.set(next);
      error = null;
      try {
        await setSettings(next);
        umaBudget = mbToGb(value);
        target.value = umaBudget;
      } catch (e) {
        settings.set({ ...($settings ?? cur), uma_budget_mb: cur.uma_budget_mb });
        umaBudget = mbToGb(cur.uma_budget_mb ?? null);
        target.value = umaBudget;
        error = String(e);
      }
    });
  }

  // Three states, not two: no GPU row at all (CPU selected, or stats not in yet)
  // is different from a discrete card being selected.
  const umaHint = $derived.by(() => {
    const gpu = $sysStats?.gpu ?? null;
    if (gpu?.shared) return "In force: the selected device shares system memory.";
    if (gpu) return "Ignored right now — the selected device has memory of its own.";
    return "Used only while a device that shares system memory is selected.";
  });
</script>

<div class="modal-backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="modal" role="dialog" aria-modal="true" aria-label="Preferences">
    <div class="modal-head">
      <span class="modal-title">Preferences</span>
      <button class="modal-x" onclick={onclose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      <p class="section-hdr first">Secrets</p>
      <p class="tip">Tip: a <strong>read-only</strong> token is all MuchAI needs — create your tokens with read permissions only.</p>

      <div class="dlg-field">
        <p class="microlabel">HuggingFace token</p>
        <div class="tokenfield">
          <input class="dlg-input mono" type={showHf ? "text" : "password"} value={hf}
            oninput={(e) => (hf = e.currentTarget.value)}
            onchange={() => saveToken("hf_token", hf)}
            disabled={!$settings}
            placeholder="hf_…" autocomplete="off" spellcheck="false" />
          <button class="btn btn-ghost btn-sm" type="button" onclick={() => (showHf = !showHf)}>{showHf ? "Hide" : "Show"}</button>
        </div>
        <p class="hint">For gated / large models · huggingface.co/settings/tokens</p>
      </div>

      <div class="dlg-field last">
        <p class="microlabel">Civitai token</p>
        <div class="tokenfield">
          <input class="dlg-input mono" type={showCivitai ? "text" : "password"} value={civitai}
            oninput={(e) => (civitai = e.currentTarget.value)}
            onchange={() => saveToken("civitai_token", civitai)}
            disabled={!$settings}
            placeholder="not set" autocomplete="off" spellcheck="false" />
          <button class="btn btn-ghost btn-sm" type="button" onclick={() => (showCivitai = !showCivitai)}>{showCivitai ? "Hide" : "Show"}</button>
        </div>
        <p class="hint">Used for Civitai downloads · civitai.com/user/account (API Keys)</p>
      </div>

      {#if error}<p class="err">{error}</p>{/if}

      <p class="section-hdr">Folders</p>
      <ModelFolders />
      <GalleryLocation />

      <p class="section-hdr">Hardware</p>
      <DevicePicker />
      <label class="check">
        <input type="checkbox"
          checked={$settings?.low_vram ?? false}
          disabled={!$settings}
          onchange={(e) => saveLowVram(e.currentTarget.checked)} />
        <span class="check-box"></span>
        <span>Low-VRAM mode <em>— slower; fits bigger models</em></span>
      </label>
      <label class="check">
        <input type="checkbox"
          checked={$settings?.live_preview ?? true}
          disabled={!$settings}
          onchange={(e) => saveLivePreview(e.currentTarget.checked)} />
        <span class="check-box"></span>
        <span>Live preview <em>— rough draft while generating, cancel early</em></span>
      </label>
      <div class="dlg-field uma">
        <p class="microlabel">Shared-memory budget (GB)</p>
        <input class="dlg-input" inputmode="decimal"
          value={umaBudget}
          disabled={!$settings}
          placeholder={autoBudget ? `Auto — ${autoBudget} GB` : "Auto"}
          onchange={(e) => saveUmaBudget(e.currentTarget.value, e.currentTarget)} />
        <p class="hint">{umaHint}</p>
      </div>
      <div class="dlg-field precision">
        <p class="microlabel">Load precision</p>
        <select class="dlg-select"
          value={$settings?.load_precision ?? "auto"}
          disabled={!$settings}
          onchange={(e) => saveLoadPrecision(e.currentTarget.value as LoadPrecision)}>
          {#each LOAD_PRECISION_OPTIONS as o (o.value)}
            <option value={o.value}>{o.label}</option>
          {/each}
        </select>
        <p class="hint">{precisionHint}</p>
      </div>

      <!-- The dot mirrors the one on the gear so a user who opened Preferences
           for an unrelated reason still finds the section. It disappears as
           soon as EnginePanel mounts and clears the store. -->
      <p class="section-hdr">
        Engine{#if $engineUpdateTag}<span class="hdr-dot" role="img" aria-label="update available"></span>{/if}
      </p>
      <EnginePanel />

      <p class="section-hdr">Appearance</p>
      <div class="prefrow theme-row">
        <span class="pk">Theme</span>
        <span class="pv"><ThemeToggle segmented /></span>
      </div>
    </div>

    <div class="modal-foot">
      <button class="btn btn-primary spacer" onclick={onclose}>Done</button>
    </div>
  </div>
</div>

<style>
  /* The dialog's only visual break used to be whatever border the first widget
     in a section happened to draw, so Folders and Hardware looked divided and
     Secrets, Engine and Appearance did not. The rule belongs to the header, so
     every section is separated the same way. Scoped here: the same class is
     used in ModelEditor and About, which are not sectioned lists. */
  .section-hdr { border-top: 1px solid var(--border); padding-top: 18px; }
  .section-hdr.first { margin-top: 8px; border-top: none; padding-top: 0; }
  .dlg-field.last { margin-bottom: 0; }
  .tip { font-size:12.5px; color:var(--text-muted); background:var(--card);
    border:1px solid var(--border); border-radius:var(--radius-sm);
    padding:9px 12px; margin:0 0 14px; }
  .tip strong { color:var(--text); font-weight:600; }
  .tokenfield { display:flex; gap:8px; }
  .tokenfield .dlg-input { flex:1; }
  .dlg-input.mono { font-family:var(--mono); font-size:12px; }
  .hint { font-size:11.5px; color:var(--text-muted); margin:5px 0 0; }
  .check { margin-top:14px; }
  .dlg-field.precision { margin-top:14px; margin-bottom:0; }
  .dlg-field.uma { margin-top:14px; margin-bottom:0; }
  .check em { color:var(--text-muted); font-style:normal; }
  .prefrow { display:flex; align-items:center; gap:12px; padding:10px 0; }
  .prefrow.theme-row { padding-top:0; }
  .prefrow .pk { font-size:13px; color:var(--text-muted); min-width:120px; }
  .prefrow .pv { flex:1; min-width:0; }
  .err { font-size:12px; color:var(--danger); margin:8px 0 0; }
  .hdr-dot { display:inline-block; width:5px; height:5px; margin-left:5px; vertical-align:middle;
    border-radius:50%; background:var(--accent); }
</style>
