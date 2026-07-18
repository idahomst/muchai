# Preferences & Secrets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Preferences dialog that stores HuggingFace + Civitai tokens (plaintext in config.json) and consolidates the scattered model-folder / GPU / theme controls, so tokens are entered once and reused for downloads.

**Architecture:** Two new `Option<String>` fields on `AppConfig` (Rust + TS mirror), persisted through the existing `get_settings`/`set_settings` round-trip (no new commands). A new `PreferencesDialog.svelte` opened from a ⚙ header button hosts a Secrets section plus the existing self-persisting `ModelFolders` / `GalleryLocation` / `DevicePicker` / theme controls, which are removed from the sidebar. Download dialogs read the stored HF token instead of prompting.

**Tech Stack:** Rust (Tauri v2, serde), Svelte 5 runes, TypeScript.

**Spec:** `docs/superpowers/specs/2026-07-16-preferences-and-secrets-design.md`

---

## File Structure

- `src-tauri/src/types.rs` — add `hf_token` / `civitai_token` to `AppConfig` (+ tests). *(modify)*
- `src/lib/types.ts` — mirror the two fields on the `AppConfig` interface. *(modify)*
- `src/lib/components/PreferencesDialog.svelte` — new modal: Secrets + relocated controls. *(create)*
- `src/routes/+page.svelte` — ⚙ button, mount dialog, remove relocated controls from sidebar. *(modify)*
- `src/lib/components/DownloadDialog.svelte` — read stored HF token, drop token input. *(modify)*
- `src/lib/components/ModelAssembly.svelte` — read stored HF token, drop inline token field. *(modify)*

The relocated components (`ModelFolders`, `GalleryLocation`, `DevicePicker`, `ThemeToggle`) already self-persist via `setSettings` + `settings.set`, so moving them only changes where they render — no logic changes inside them.

---

### Task 1: Add token fields to the Rust `AppConfig`

**Files:**
- Modify: `src-tauri/src/types.rs` (struct near line 272, tests module near line 303)

- [ ] **Step 1: Write the failing tests**

Add these two tests inside the existing `#[cfg(test)] mod tests { ... }` block in `src-tauri/src/types.rs` (after the last existing test, before the closing `}`):

```rust
    #[test]
    fn app_config_round_trips_tokens() {
        // A config JSON that includes both tokens deserializes with them set,
        // and re-serializing preserves them.
        let json = r#"{
            "sd_binary_path": null,
            "default_model_path": null,
            "gallery_dir": "/g",
            "models_dir": "/m",
            "extra_model_dirs": [],
            "gpu_device": null,
            "params_expanded": false,
            "theme": "dark",
            "onboarded": false,
            "model_definitions": [],
            "hf_token": "hf_abc123",
            "civitai_token": "civ_xyz",
            "last_request": {
                "model": { "type": "single_file", "path": "" },
                "prompt": "",
                "negative_prompt": "",
                "steps": 20,
                "cfg_scale": 7.0,
                "sampler": "euler_a",
                "width": 512,
                "height": 512,
                "seed": -1,
                "batch_count": 1
            }
        }"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.hf_token.as_deref(), Some("hf_abc123"));
        assert_eq!(cfg.civitai_token.as_deref(), Some("civ_xyz"));

        let round: AppConfig =
            serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(round.hf_token.as_deref(), Some("hf_abc123"));
        assert_eq!(round.civitai_token.as_deref(), Some("civ_xyz"));
    }

    #[test]
    fn app_config_defaults_tokens_to_none_for_old_config() {
        // A pre-feature config JSON lacking the token keys must deserialize with
        // both tokens as None (serde default), not error.
        let json = r#"{
            "sd_binary_path": null,
            "default_model_path": null,
            "gallery_dir": "/g",
            "models_dir": "/m",
            "extra_model_dirs": [],
            "gpu_device": null,
            "params_expanded": false,
            "theme": "dark",
            "onboarded": false,
            "model_definitions": [],
            "last_request": {
                "model": { "type": "single_file", "path": "" },
                "prompt": "",
                "negative_prompt": "",
                "steps": 20,
                "cfg_scale": 7.0,
                "sampler": "euler_a",
                "width": 512,
                "height": 512,
                "seed": -1,
                "batch_count": 1
            }
        }"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.hf_token.is_none());
        assert!(cfg.civitai_token.is_none());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib app_config`
Expected: FAIL — compile error (`no field hf_token on type AppConfig`) or unknown field, because the fields don't exist yet.

- [ ] **Step 3: Add the fields to `AppConfig`**

In `src-tauri/src/types.rs`, inside `struct AppConfig`, add the two fields immediately after the `model_definitions` field (before `last_request`):

```rust
    /// HuggingFace access token for gated/large downloads. Plaintext; pre-feature
    /// configs lack this key and deserialize as None.
    #[serde(default)]
    pub hf_token: Option<String>,
    /// Civitai access token. Plaintext; stored now, consumed by the multi-file
    /// download rework. Pre-feature configs deserialize as None.
    #[serde(default)]
    pub civitai_token: Option<String>,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib app_config`
Expected: PASS (both `app_config_round_trips_tokens` and `app_config_defaults_tokens_to_none_for_old_config`).

- [ ] **Step 5: Confirm the whole lib still builds and tests pass**

Run: `cd src-tauri && cargo test --lib`
Expected: all tests pass (previously 111 + 2 new).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/types.rs
git commit -m "feat(config): add hf_token and civitai_token to AppConfig"
```

---

### Task 2: Mirror the token fields in the TypeScript `AppConfig`

**Files:**
- Modify: `src/lib/types.ts` (the `AppConfig` interface, ends around line 131)

- [ ] **Step 1: Add the fields**

In `src/lib/types.ts`, inside `interface AppConfig`, add the two fields immediately after `onboarded: boolean;` (keep them next to the other config fields):

```ts
  // HuggingFace / Civitai access tokens. Plaintext in config.json. Mirrors the
  // Rust AppConfig fields (#[serde(default)] → null for old configs). null = unset.
  hf_token: string | null;
  civitai_token: string | null;
```

- [ ] **Step 2: Run the type/lint check**

Run: `npm run check`
Expected: 0 errors, 0 warnings. (No consumer sets these yet; adding required fields to the interface is safe because `AppConfig` is only ever produced by `getSettings()` from the backend, which now always includes them.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/types.ts
git commit -m "feat(config): mirror hf_token/civitai_token in TS AppConfig"
```

---

### Task 3: Create the `PreferencesDialog` component

**Files:**
- Create: `src/lib/components/PreferencesDialog.svelte`

This modal hosts the Secrets section (token fields) plus the relocated
`ModelFolders`, `GalleryLocation`, `DevicePicker`, and `ThemeToggle` components.
Those four already read/write the `settings` store themselves, so we just render
them here.

- [ ] **Step 1: Create the component**

Create `src/lib/components/PreferencesDialog.svelte` with exactly this content:

```svelte
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
      <p class="tip">Tip: a <strong>read-only</strong> token is all MuchAI needs — create your tokens with read permissions only.</p>

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
```

- [ ] **Step 2: Verify it type-checks**

Run: `npm run check`
Expected: 0 errors, 0 warnings. (The component isn't mounted yet — Task 4 mounts it — but it must compile.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/PreferencesDialog.svelte
git commit -m "feat(ui): add PreferencesDialog (secrets + consolidated settings)"
```

---

### Task 4: Add the ⚙ button, mount the dialog, declutter the sidebar

**Files:**
- Modify: `src/routes/+page.svelte`

- [ ] **Step 1: Swap the imports**

`ModelFolders`, `DevicePicker`, and `GalleryLocation` now render only inside
`PreferencesDialog`, so remove their imports from `+page.svelte` and add the dialog
import. Replace these three lines:

```svelte
  import GalleryLocation from "$lib/components/GalleryLocation.svelte";
  import ModelFolders from "$lib/components/ModelFolders.svelte";
  import DevicePicker from "$lib/components/DevicePicker.svelte";
```

with:

```svelte
  import PreferencesDialog from "$lib/components/PreferencesDialog.svelte";
```

(Keep the `ThemeToggle` import — it stays in the header.)

- [ ] **Step 2: Add the dialog's open-state**

Immediately after the existing `let showWelcome = $state(false);` line, add:

```svelte
  let showPrefs = $state(false);
```

- [ ] **Step 3: Add the ⚙ button to the header**

Replace the header actions block:

```svelte
      <div class="hdr-actions">
        <button class="help-btn" aria-label="Help" title="Help" onclick={() => (showWelcome = true)}>?</button>
        <ThemeToggle />
      </div>
```

with:

```svelte
      <div class="hdr-actions">
        <button class="help-btn" aria-label="Help" title="Help" onclick={() => (showWelcome = true)}>?</button>
        <button class="help-btn" aria-label="Preferences" title="Preferences" onclick={() => (showPrefs = true)}>⚙</button>
        <ThemeToggle />
      </div>
```

- [ ] **Step 4: Remove the relocated controls from the sidebar**

Replace the sidebar body:

```svelte
    <ModelLibrary />
    <ModelFolders />
    <DevicePicker />
    <PromptPanel />
    <SettingsPanel />
    <div class="spacer"></div>
    <GenerateBar />
```

with:

```svelte
    <ModelLibrary />
    <PromptPanel />
    <SettingsPanel />
    <div class="spacer"></div>
    <GenerateBar />
```

- [ ] **Step 5: Remove `GalleryLocation` from the stage**

Replace the stage body:

```svelte
  <section class="stage">
    <ImagePreview />
    <ParamsPanel />
    <HistoryStrip />
    <GalleryLocation />
  </section>
```

with:

```svelte
  <section class="stage">
    <ImagePreview />
    <ParamsPanel />
    <HistoryStrip />
  </section>
```

- [ ] **Step 6: Render the dialog**

After the existing welcome-dialog block:

```svelte
{#if showWelcome}
  <WelcomeDialog onclose={dismissWelcome} />
{/if}
```

add:

```svelte
{#if showPrefs}
  <PreferencesDialog onclose={() => (showPrefs = false)} />
{/if}
```

- [ ] **Step 7: Verify**

Run: `npm run check`
Expected: 0 errors, 0 warnings (no unused-import complaints — the three removed imports are gone, and `PreferencesDialog` is used).

- [ ] **Step 8: Commit**

```bash
git add src/routes/+page.svelte
git commit -m "feat(ui): open Preferences from a header gear; move settings out of the sidebar"
```

---

### Task 5: Use the stored HF token for downloads; remove the per-download token inputs

**Files:**
- Modify: `src/lib/components/DownloadDialog.svelte`
- Modify: `src/lib/components/ModelAssembly.svelte`

Both dialogs currently take a typed-in token. They now read `hf_token` from the
`settings` store, so the user is never re-prompted.

#### DownloadDialog.svelte

- [ ] **Step 1: Import `settings`**

Replace:

```svelte
  import { sysStats, downloadStatus, startDownload } from "../stores";
```

with:

```svelte
  import { sysStats, downloadStatus, startDownload, settings } from "../stores";
```

- [ ] **Step 2: Remove the local token state**

Delete this line:

```svelte
  let token = $state("");
```

- [ ] **Step 3: Pass the stored token in `start()`**

Replace:

```svelte
    void startDownload(downloadUrl, token.trim(), name);
```

with:

```svelte
    void startDownload(downloadUrl, $settings?.hf_token ?? "", name);
```

- [ ] **Step 4: Replace the token input with a hint**

Replace:

```svelte
      <input class="in" type="password" placeholder="Access token (optional, for gated/civitai)" bind:value={token} />
```

with:

```svelte
      <p class="tokhint">Gated downloads use your HuggingFace token from Preferences (⚙).</p>
```

Then add this rule to the component's `<style>` block (e.g. after the `.in` rule):

```css
  .tokhint { font-size:.72rem; opacity:.6; margin:0 0 .2rem; }
```

#### ModelAssembly.svelte

- [ ] **Step 5: Import `settings`**

Replace:

```svelte
  import { startMultiFileDownload, downloadStatus } from "../stores";
```

with:

```svelte
  import { startMultiFileDownload, downloadStatus, settings } from "../stores";
```

- [ ] **Step 6: Remove the local token state**

Delete this line (in the "Catalog flow state" group):

```svelte
  let token = $state("");
```

- [ ] **Step 7: Pass the stored token in `downloadEntry()`**

Replace:

```svelte
      const def = await startMultiFileDownload(selectedEntry.id, token.trim(), selectedEntry.name);
```

with:

```svelte
      const def = await startMultiFileDownload(selectedEntry.id, $settings?.hf_token ?? "", selectedEntry.name);
```

- [ ] **Step 8: Replace the token input with a hint**

Replace:

```svelte
        <label class="fld"><span>HF access token (optional, for gated models)</span>
          <input class="in" type="password" bind:value={token} placeholder="hf_…" />
        </label>
```

with (reusing the existing `.hint` class in this component):

```svelte
        <p class="hint">Gated downloads use your HuggingFace token from Preferences (⚙).</p>
```

- [ ] **Step 9: Verify**

Run: `npm run check`
Expected: 0 errors, 0 warnings (no leftover references to `token`).

- [ ] **Step 10: Commit**

```bash
git add src/lib/components/DownloadDialog.svelte src/lib/components/ModelAssembly.svelte
git commit -m "feat(downloads): use stored HuggingFace token; drop per-download token inputs"
```

---

## Final verification

After all tasks:

- [ ] `cd src-tauri && cargo test --lib` — all pass (113 tests).
- [ ] `npm run check` — 0 errors, 0 warnings.
- [ ] Manual smoke (optional, requires a build): open ⚙, set an HF token, close/reopen → persists masked; model folders / GPU / theme work from the dialog and are gone from the sidebar; starting a download does not prompt for a token.

