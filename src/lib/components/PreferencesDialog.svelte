<script lang="ts">
  import { settings } from "$lib/stores";
  import { setSettings } from "$lib/api";
  import ModelFolders from "./ModelFolders.svelte";
  import GalleryLocation from "./GalleryLocation.svelte";
  import DevicePicker from "./DevicePicker.svelte";
  import ThemeToggle from "./ThemeToggle.svelte";

  let { onclose }: { onclose: () => void } = $props();

  // Local editable copies of the tokens, seeded from settings (null → "").
  let hf = $state($settings?.hf_token ?? "");
  let civitai = $state($settings?.civitai_token ?? "");
  let showHf = $state(false);
  let showCivitai = $state(false);
  let error = $state<string | null>(null);

  // Persist one token field. Empty string is stored as null so "is it set?" is a
  // simple null check. Optimistic with rollback, matching the other controls.
  async function saveToken(field: "hf_token" | "civitai_token", value: string) {
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
      settings.set(cur); // roll back the optimistic write
      error = String(e);
    }
  }
</script>

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Preferences">
    <h2>Preferences</h2>

    <section class="grp">
      <div class="grp-hdr">Secrets</div>
      <p class="tip">Tip: a <strong>read-only</strong> token is all FridAI needs — create your tokens with read permissions only.</p>

      <label class="fld"><span>HuggingFace token</span>
        <div class="tok">
          <input class="in" type={showHf ? "text" : "password"} value={hf}
            oninput={(e) => (hf = e.currentTarget.value)}
            onchange={() => saveToken("hf_token", hf)}
            placeholder="hf_…" autocomplete="off" spellcheck="false" />
          <button class="reveal" type="button" onclick={() => (showHf = !showHf)}>{showHf ? "hide" : "show"}</button>
        </div>
        <span class="hint">For gated / large models. Create at huggingface.co/settings/tokens</span>
      </label>

      <label class="fld"><span>Civitai token</span>
        <div class="tok">
          <input class="in" type={showCivitai ? "text" : "password"} value={civitai}
            oninput={(e) => (civitai = e.currentTarget.value)}
            onchange={() => saveToken("civitai_token", civitai)}
            placeholder="not set" autocomplete="off" spellcheck="false" />
          <button class="reveal" type="button" onclick={() => (showCivitai = !showCivitai)}>{showCivitai ? "hide" : "show"}</button>
        </div>
        <span class="hint">Used for Civitai downloads. Create at civitai.com/user/account (API Keys)</span>
      </label>

      {#if error}<p class="err">{error}</p>{/if}
    </section>

    <section class="grp">
      <div class="grp-hdr">Folders</div>
      <ModelFolders />
      <GalleryLocation />
    </section>

    <section class="grp">
      <div class="grp-hdr">Hardware</div>
      <DevicePicker />
    </section>

    <section class="grp">
      <div class="grp-hdr">Appearance</div>
      <div class="appearance"><span class="lbl">Theme</span><ThemeToggle /></div>
    </section>

    <div class="row">
      <button class="btn-primary" onclick={onclose}>Done</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:var(--backdrop); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--dialog-bg); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(520px, 94vw); max-height:90vh; overflow-y:auto;
    display:flex; flex-direction:column; gap:.8rem; }
  h2 { margin:0; font-size:1.05rem; }
  .grp { display:flex; flex-direction:column; gap:.4rem; }
  .grp-hdr { font-size:.7rem; text-transform:uppercase; letter-spacing:.05em; opacity:.6; }
  .tip { font-size:.72rem; opacity:.8; margin:0; }
  .fld { display:flex; flex-direction:column; gap:.2rem; font-size:.75rem; }
  .tok { display:flex; gap:.4rem; }
  .in { flex:1; font:inherit; padding:.35rem; box-sizing:border-box; }
  .reveal { font:inherit; font-size:.72rem; padding:.2rem .5rem; cursor:pointer; }
  .hint { font-size:.68rem; opacity:.6; }
  .appearance { display:flex; align-items:center; gap:.5rem; font-size:.75rem; }
  .appearance .lbl { opacity:.6; }
  .err { font-size:.72rem; color:var(--danger); margin:0; }
  .row { display:flex; justify-content:flex-end; margin-top:.3rem; }
  button.btn-primary { font:inherit; font-size:.8rem; padding:.4rem .8rem; cursor:pointer; }
</style>
