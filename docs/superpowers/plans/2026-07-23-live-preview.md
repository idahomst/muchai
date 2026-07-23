# Live Preview During Generation — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show a rough live draft of the image as it forms in the main preview area (via stable-diffusion.cpp `--preview proj`), so the user can cancel early when the composition is wrong.

**Architecture:** The engine writes a fixed preview PNG every 2 steps. The backend enables this when `AppConfig.live_preview` is on, adds the preview dir to the Tauri asset-protocol scope, emits a `generation:preview { path }` event once per run, and deletes the file when the run ends (any outcome) and on cancel. The frontend reloads the `<img>` with a cache-busting `?t=<step>` query on each existing `generation:progress` tick (approach A — no file-watcher). A settings toggle (default ON) lives in `PreferencesDialog` (mirroring the existing `low_vram` optimistic save-chain), NOT `SettingsPanel` (which is bound only to the per-image request).

**Tech Stack:** Rust (Tauri v2), Svelte 5 (runes), stable-diffusion.cpp Vulkan engine (pinned `b290693`).

**Spec:** `docs/superpowers/specs/2026-07-23-live-preview-design.md`

---

## File Structure

**Backend (Rust):**
- `src-tauri/src/types.rs` — add `AppConfig.live_preview: bool` (`#[serde(default = "default_true")]`) + `default_true()` helper.
- `src-tauri/src/config.rs` — `default_config()` sets `live_preview: true`.
- `src-tauri/src/command_builder.rs` — `EngineOptions.preview_path: Option<String>`; append `--preview proj --preview-path <p> --preview-interval 2` when `Some`.
- `src-tauri/src/commands.rs` — `preview_path()` helper; `generate` wires preview + emits event + deletes file on end; `cancel_generation` deletes file.
- `src-tauri/src/lib.rs` — `setup()` creates + `allow_directory`s the preview dir.

**Frontend (Svelte/TS):**
- `src/lib/types.ts` — add `live_preview: boolean` to `AppConfig`.
- `src/lib/stores.ts` — add `livePreview` store.
- `src/lib/api.ts` — add `onPreview()`.
- `src/lib/components/GenerateBar.svelte` — subscribe to preview event, update `livePreview` on progress, clear on completion.
- `src/lib/components/ImagePreview.svelte` — show `$livePreview ?? $currentImage` + "Preview" badge + `onerror`.
- `src/lib/components/PreferencesDialog.svelte` — "Live preview" checkbox.
- `src/lib/helpText.ts` — `livePreview` tooltip copy.

## Task Index
1. Backend: `live_preview` config field (default ON)
2. Backend: `command_builder` preview flags
3. Backend: `commands` + `lib` wiring (path, scope, event, cleanup, cancel)
4. Frontend: types + store + api
5. Frontend: `GenerateBar` preview wiring
6. Frontend: `ImagePreview` live display
7. Frontend: `PreferencesDialog` toggle + help copy
8. Full-suite verification

---

### Task 1: Backend — `live_preview` config field (default ON)

**Files:**
- Modify: `src-tauri/src/types.rs` (AppConfig struct, ~line 298-337; tests module)
- Modify: `src-tauri/src/config.rs` (`default_config()`, ~line 30-46)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/types.rs`:

```rust
#[test]
fn app_config_live_preview_defaults_to_true_and_round_trips() {
    // A config JSON written before this feature lacks the key: it must
    // deserialize with live_preview = true (feature default ON).
    let legacy = r#"{
        "sd_binary_path": null,
        "default_model_path": null,
        "gallery_dir": "/g",
        "last_request": {
            "model": {"type": "single_file", "path": ""},
            "prompt": "", "negative_prompt": "",
            "steps": 20, "cfg_scale": 7.0, "sampler": "euler_a",
            "width": 512, "height": 512, "seed": -1, "batch_count": 1
        }
    }"#;
    let cfg: AppConfig = serde_json::from_str(legacy).unwrap();
    assert!(cfg.live_preview, "missing key must default to true");

    // And an explicit false survives a serialize/deserialize round-trip.
    let mut off = cfg.clone();
    off.live_preview = false;
    let json = serde_json::to_string(&off).unwrap();
    let back: AppConfig = serde_json::from_str(&json).unwrap();
    assert!(!back.live_preview);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test app_config_live_preview_defaults_to_true_and_round_trips`
Expected: FAIL — compile error (`no field live_preview on AppConfig`).

- [ ] **Step 3: Add the field + default helper**

In `src-tauri/src/types.rs`, add this free function just above the `AppConfig`
struct (after the `DownloadProgress` struct, before `pub struct AppConfig`):

```rust
/// serde default for `AppConfig.live_preview`: ON. Pre-feature config files
/// lack the key and must default to true (the feature is enabled by default).
fn default_true() -> bool {
    true
}
```

Then add the field to `AppConfig`, immediately after the `low_vram` field
(keeping `last_request` last, as it is now):

```rust
    /// Show a rough live draft of the image as it generates (engine
    /// `--preview proj`). Default ON; pre-feature configs lack the key and
    /// default to true via `default_true`.
    #[serde(default = "default_true")]
    pub live_preview: bool,
```

- [ ] **Step 4: Set the explicit default in config.rs**

In `src-tauri/src/config.rs`, add to the `AppConfig { .. }` literal in
`default_config()`, right after `low_vram: false,`:

```rust
        live_preview: true,
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test`
Expected: PASS — 208 passed (the new test plus the existing 207).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/config.rs
git commit -m "feat(config): add live_preview flag (default ON)"
```

### Task 2: Backend — `command_builder` preview flags

**Files:**
- Modify: `src-tauri/src/command_builder.rs` (`EngineOptions` struct ~line 6-9; `build_args` ~line 70-78; tests)

Note: `EngineOptions` currently derives `Copy`. Adding an `Option<String>` field
makes it non-`Copy`; the `Copy` derive must be removed. `EngineOptions` is only
ever constructed once and moved by value (into the spawn closure → `run_generation`
→ `build_args`), so removing `Copy` compiles fine — but the existing tests
construct it with a struct literal missing the new field and must be updated.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/src/command_builder.rs`:

```rust
#[test]
fn preview_flags_present_when_preview_path_some() {
    let opts = EngineOptions { preview_path: Some("/tmp/p/preview.png".into()), ..Default::default() };
    let args = build_args(&sample(), "/out/x.png", None, opts);
    assert_eq!(val_after(&args, "--preview"), Some("proj"));
    assert_eq!(val_after(&args, "--preview-path"), Some("/tmp/p/preview.png"));
    assert_eq!(val_after(&args, "--preview-interval"), Some("2"));
}

#[test]
fn preview_flags_absent_when_preview_path_none() {
    let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
    for flag in ["--preview", "--preview-path", "--preview-interval"] {
        assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test command_builder`
Expected: FAIL — compile error (`no field preview_path on EngineOptions`).

- [ ] **Step 3: Update `EngineOptions` and `build_args`**

Replace the `EngineOptions` definition (remove `Copy`, add the field):

```rust
/// Engine knobs that aren't part of the generation request itself. A struct
/// (not a bare bool) leaves room for the deferred expert controls (--max-vram,
/// --stream-layers, per-component --backend) without another signature churn.
#[derive(Debug, Clone, Default)]
pub struct EngineOptions {
    pub low_vram: bool,
    /// When `Some(path)`, enable a live preview written to `path`
    /// (`--preview proj --preview-interval 2`); `None` disables it.
    pub preview_path: Option<String>,
}
```

In `build_args`, add this block just before the final `a.push("-v".into());`:

```rust
    if let Some(p) = &opts.preview_path {
        // Cheap linear latent→RGB projection written every 2 steps; the app
        // watches the file to show a live draft so the user can cancel early.
        a.push("--preview".into());
        a.push("proj".into());
        a.push("--preview-path".into());
        a.push(p.clone());
        a.push("--preview-interval".into());
        a.push("2".into());
    }
```

- [ ] **Step 4: Fix the two existing tests that use a struct literal**

In `src-tauri/src/command_builder.rs`, the `low_vram_appends_offload_flags` and
`low_vram_off_omits_offload_flags` tests construct `EngineOptions { low_vram: .. }`.
Add `..Default::default()` to each:

```rust
    // in low_vram_appends_offload_flags:
    let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: true, ..Default::default() });
```

```rust
    // in low_vram_off_omits_offload_flags:
    let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: false, ..Default::default() });
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test command_builder`
Expected: PASS — all `command_builder` tests green (the 2 new + updated existing).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/command_builder.rs
git commit -m "feat(engine): add --preview proj flags to command builder"
```

### Task 3: Backend — `commands` + `lib` wiring

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `preview_path()`; `generate` ~line 217-243; `cancel_generation` ~line 171-176; tests module ~line 889)
- Modify: `src-tauri/src/lib.rs` (`setup()` ~line 39-43)

Context (already imported in commands.rs): `use std::path::PathBuf;`,
`use tauri::{AppHandle, Emitter, Manager, State};`. lib.rs already imports
`tauri::Manager` and the `commands` module.

- [ ] **Step 1: Write the failing test**

Add to the existing `tests` module in `src-tauri/src/commands.rs` (at line ~889):

```rust
#[test]
fn preview_path_is_under_temp_muchai_preview() {
    let p = super::preview_path();
    assert!(p.ends_with("muchai-preview/preview.png"), "got {p:?}");
    assert!(p.starts_with(std::env::temp_dir()), "must live under the OS temp dir");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test preview_path_is_under_temp_muchai_preview`
Expected: FAIL — compile error (`cannot find function preview_path`).

- [ ] **Step 3: Add the `preview_path()` helper**

In `src-tauri/src/commands.rs`, add just above `pub fn cancel_generation` (line ~171):

```rust
/// Fixed path for the live-preview draft the engine overwrites during a run.
/// In the OS temp dir (tmpfs on Linux) so the tiny, constantly-rewritten file
/// is RAM-backed and cleared on reboot. Safe as a fixed (non-unique) path
/// because generation is single-flight (one `child` slot in AppState).
pub fn preview_path() -> PathBuf {
    std::env::temp_dir().join("muchai-preview").join("preview.png")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test preview_path_is_under_temp_muchai_preview`
Expected: PASS.

- [ ] **Step 5: Delete the preview file on cancel**

Replace the body of `cancel_generation` in `src-tauri/src/commands.rs`:

```rust
#[tauri::command]
pub fn cancel_generation(state: State<AppState>) {
    if let Some(mut child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
    }
    // Remove the live-preview draft immediately so a cancelled run leaves
    // nothing behind. The run's own cleanup (in `generate`) also deletes it,
    // but cancel may win the race. Best-effort.
    let _ = std::fs::remove_file(preview_path());
}
```

- [ ] **Step 6: Wire preview into `generate`**

In `src-tauri/src/commands.rs`, find the low-VRAM block that ends with:

```rust
    let engine_opts = crate::command_builder::EngineOptions { low_vram };
```

Replace that single line with:

```rust
    // Live preview: when enabled, the engine writes a rough draft to a fixed
    // file every 2 steps; the frontend reloads it on each progress tick.
    let preview = if cfg.live_preview { Some(preview_path()) } else { None };
    if let Some(p) = &preview {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Tell the frontend where the draft will appear (file arrives ~step 2).
        let _ = app.emit("generation:preview", p.to_string_lossy().to_string());
    }
    let engine_opts = crate::command_builder::EngineOptions {
        low_vram,
        preview_path: preview.as_ref().map(|p| p.to_string_lossy().into_owned()),
    };
```

- [ ] **Step 7: Delete the preview file when the run ends (any outcome)**

Still in `generate`, find the spawn/await:

```rust
    .await
    .map_err(|e| e.to_string())?;

    match result {
```

Insert the cleanup between the `?;` and `match result {`:

```rust
    .await
    .map_err(|e| e.to_string())?;

    // The engine has exited, so no more preview writes: remove the draft file
    // regardless of outcome (success, error, or cancel). Best-effort.
    if let Some(p) = &preview {
        let _ = std::fs::remove_file(p);
    }

    match result {
```

- [ ] **Step 8: Allow the preview dir in the asset scope (lib.rs)**

In `src-tauri/src/lib.rs`, inside `.setup(move |app| {`, right after the existing
`let _ = app.asset_protocol_scope().allow_directory(&gallery_dir, true);`:

```rust
            // Allow the live-preview directory so convertFileSrc can load the
            // draft file the engine writes during generation.
            let preview_file = commands::preview_path();
            if let Some(dir) = preview_file.parent() {
                let _ = std::fs::create_dir_all(dir);
                let _ = app.asset_protocol_scope().allow_directory(dir, true);
            }
```

- [ ] **Step 9: Build + run the full backend suite**

Run: `cd src-tauri && cargo test`
Expected: PASS — 211 passed (207 original + Task 1 test + Task 2's 2 tests + Task 3 test); no warnings about unused `preview`.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(generate): emit preview path, enable preview, clean up on end/cancel"
```

### Task 4: Frontend — types + store + api

**Files:**
- Modify: `src/lib/types.ts` (`AppConfig` interface, ends at line 171 with `low_vram: boolean;`)
- Modify: `src/lib/stores.ts` (after `currentImage`, ~line 10)
- Modify: `src/lib/api.ts` (after `onProgress`, ~line 31)

No unit test — this is type/store/api plumbing exercised by `npm run check` and the
components in Tasks 5-6. TDD does not apply to plain type declarations.

- [ ] **Step 1: Add `live_preview` to the `AppConfig` interface**

In `src/lib/types.ts`, replace the closing of the interface (line 168-171):

```ts
  // Low-VRAM offload mode (mirrors Rust AppConfig.low_vram, #[serde(default)] →
  // false for old configs). When on, generation pages weights from RAM.
  low_vram: boolean;
}
```

with:

```ts
  // Low-VRAM offload mode (mirrors Rust AppConfig.low_vram, #[serde(default)] →
  // false for old configs). When on, generation pages weights from RAM.
  low_vram: boolean;
  // Show a rough live draft as the image generates (mirrors Rust
  // AppConfig.live_preview, #[serde(default = "default_true")] → true for old
  // configs).
  live_preview: boolean;
}
```

- [ ] **Step 2: Add the `livePreview` store**

In `src/lib/stores.ts`, right after the `currentImage` line (line 10):

```ts
export const currentImage = writable<string | null>(null); // converted asset src
```

add:

```ts
// Live-preview frame during a run: a convertFileSrc()'d asset URL with a
// cache-busting ?t=<step> query, or null when no run is showing a draft.
// Takes visual precedence over currentImage while set (see ImagePreview).
export const livePreview = writable<string | null>(null);
```

- [ ] **Step 3: Add `onPreview` to the api**

In `src/lib/api.ts`, right after the `onProgress` export (ends line 31):

```ts
/** Fires once per run (only when live preview is enabled) with the absolute
 *  path the engine will overwrite with the draft image. */
export const onPreview = (cb: (path: string) => void): Promise<UnlistenFn> =>
  listen<string>("generation:preview", (e) => cb(e.payload));
```

- [ ] **Step 4: Type-check**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/types.ts src/lib/stores.ts src/lib/api.ts
git commit -m "feat(ui): add live_preview config type, livePreview store, onPreview listener"
```

### Task 5: Frontend — `GenerateBar` preview wiring

**Files:**
- Modify: `src/lib/components/GenerateBar.svelte` (imports line 4-5; `run()` line 10-29; `onMount` line 49-53)

This is UI event wiring with no isolated unit under test; correctness is confirmed
by `npm run check` and the user's manual pass. No new automated test.

The mechanism: `onPreview` stores the absolute path the engine will write in a
local `previewPath`. On each `generation:progress` tick, if `previewPath` is set,
`livePreview` is refreshed to `imageSrc(previewPath) + "?t=<step>"` (cache-bust).
`livePreview` and `previewPath` are cleared at the start and end of every run.

- [ ] **Step 1: Add store + api imports**

In `src/lib/components/GenerateBar.svelte`, replace the two import lines (4-5):

```ts
  import { request, genStatus, history, currentImage, currentItem, settings, gpuDevices, sysStats } from "../stores";
  import { generate, cancelGeneration, imageSrc, listHistory, onProgress, onGenNotice } from "../api";
```

with:

```ts
  import { request, genStatus, history, currentImage, currentItem, settings, gpuDevices, sysStats, livePreview } from "../stores";
  import { generate, cancelGeneration, imageSrc, listHistory, onProgress, onGenNotice, onPreview } from "../api";
```

- [ ] **Step 2: Add the `previewPath` local + reset/clear in `run()`**

Replace the `run()` function (line 10-29) with:

```ts
  // Absolute path the engine writes the live draft to for the current run;
  // null when preview is off or no run is active. Set by the onPreview event.
  let previewPath: string | null = null;

  async function run() {
    const req = get(request);
    if (!modelIsSet(req.model)) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
    if (!req.prompt.trim()) { genStatus.set({ kind: "error", message: "Enter a prompt." }); return; }
    genStatus.set({ kind: "running", progress: null });
    lowVramAuto = false;
    previewPath = null;
    livePreview.set(null);
    const vram = get(sysStats)?.gpu?.vram_total_mb ?? 0;
    const deviceVramMb = vram > 0 ? vram : null;
    try {
      const items = await generate(req, deviceVramMb);
      if (items.length > 0) {
        currentImage.set(imageSrc(items[0].image_path));
        currentItem.set(items[0]);
      }
      history.set(await listHistory());
      genStatus.set({ kind: "idle" });
    } catch (e) {
      genStatus.set({ kind: "error", message: String(e) });
    } finally {
      // Drop the live draft on every outcome (success, error, cancel-as-empty)
      // so the final image / prior view shows and no stale frame lingers.
      previewPath = null;
      livePreview.set(null);
    }
  }
```

- [ ] **Step 3: Wire onPreview + the per-tick refresh in `onMount`**

Replace the `onMount` block (line 49-53) with:

```ts
  onMount(() => {
    const un = onProgress((p) => {
      genStatus.update((s) => s.kind === "running" ? { kind: "running", progress: p } : s);
      // Refresh the live draft on each step; ?t busts the webview image cache so
      // the same fixed file path reloads. Steps before the first write 404 →
      // ImagePreview's onerror keeps the prior view.
      if (previewPath) livePreview.set(imageSrc(previewPath) + "?t=" + p.current_step);
    });
    const unNotice = onGenNotice(() => { lowVramAuto = true; });
    const unPreview = onPreview((path) => { previewPath = path; });
    return () => { un.then((f) => f()); unNotice.then((f) => f()); unPreview.then((f) => f()); };
  });
```

- [ ] **Step 4: Type-check**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/GenerateBar.svelte
git commit -m "feat(ui): drive livePreview from progress ticks in GenerateBar"
```

### Task 6: Frontend — `ImagePreview` live display

**Files:**
- Modify: `src/lib/components/ImagePreview.svelte` (imports line 2-4; markup line 44-63; styles)

While a live frame is set it takes visual precedence over `$currentImage`. During
preview the Delete controls are hidden (there is no gallery item yet) and a small
"Preview" badge shows. A transient 404 (draft not written yet) is swallowed by
`onerror`, which clears the failed `livePreview` so the fallback view shows.

- [ ] **Step 1: Import the `livePreview` store**

In `src/lib/components/ImagePreview.svelte`, replace the import block (line 2-4):

```ts
  import { get } from "svelte/store";
  import { currentImage, currentItem, history, request } from "../stores";
  import { deleteImage, listHistory, imageSrc } from "../api";
```

with:

```ts
  import { get } from "svelte/store";
  import { currentImage, currentItem, history, request, livePreview } from "../stores";
  import { deleteImage, listHistory, imageSrc } from "../api";

  // The live draft (if any) wins over the settled image. When a draft 404s
  // (engine hasn't written the first frame yet) we clear it so the fallback
  // shows and never render a broken-image icon.
  const shown = $derived($livePreview ?? $currentImage);
  const isPreview = $derived($livePreview !== null);
```

- [ ] **Step 2: Update the markup to show the live frame + badge, hide delete during preview**

Replace the markup block (line 44-63):

```svelte
<div class="preview">
  {#if $currentImage}
    <img src={$currentImage} alt="generated result" />
    <div class="actions">
      {#if confirming}
        <span class="ask">Move to trash?</span>
        <button class="del" onclick={doDelete} disabled={busy}>Delete</button>
        <button onclick={cancel} disabled={busy}>Cancel</button>
      {:else}
        <button class="del" onclick={() => (confirming = true)} disabled={!$currentItem}>Delete</button>
      {/if}
    </div>
    {#if error}<span class="err">{error}</span>{/if}
  {:else}
    <div class="empty">
      <p class="empty-title">Your image will appear here.</p>
      <p class="empty-sub">Pick a model, write a prompt, then press Generate.</p>
    </div>
  {/if}
</div>
```

with:

```svelte
<div class="preview">
  {#if shown}
    <img src={shown} alt={isPreview ? "generation preview" : "generated result"}
      onerror={() => { if (isPreview) livePreview.set(null); }} />
    {#if isPreview}
      <span class="badge">Preview</span>
    {:else}
      <div class="actions">
        {#if confirming}
          <span class="ask">Move to trash?</span>
          <button class="del" onclick={doDelete} disabled={busy}>Delete</button>
          <button onclick={cancel} disabled={busy}>Cancel</button>
        {:else}
          <button class="del" onclick={() => (confirming = true)} disabled={!$currentItem}>Delete</button>
        {/if}
      </div>
      {#if error}<span class="err">{error}</span>{/if}
    {/if}
  {:else}
    <div class="empty">
      <p class="empty-title">Your image will appear here.</p>
      <p class="empty-sub">Pick a model, write a prompt, then press Generate.</p>
    </div>
  {/if}
</div>
```

- [ ] **Step 3: Add the `.badge` style**

In the `<style>` block, right after the `.ask` rule (line 73), add:

```css
  .badge { position:absolute; top:.5rem; right:.5rem; font-size:.7rem; letter-spacing:.03em;
    background:var(--overlay); color:var(--on-accent); padding:.25rem .55rem; border-radius:5px; }
```

- [ ] **Step 4: Type-check**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/ImagePreview.svelte
git commit -m "feat(ui): show live draft with Preview badge in ImagePreview"
```

### Task 7: Frontend — `PreferencesDialog` "Live preview" toggle

**Files:**
- Modify: `src/lib/components/PreferencesDialog.svelte` (add `saveLivePreview` after `saveLowVram`, line 53-67; add checkbox in the Hardware section, after line 121)

**Deviation from spec (items 11-12):** The spec placed the toggle in
`SettingsPanel.svelte` with an `InfoHint`/`helpText.ts` entry. It goes in
`PreferencesDialog.svelte` instead — `SettingsPanel` binds only `$request`
(per-image params) while `live_preview` is app config, and `PreferencesDialog`
already owns the other app-config toggle (`low_vram`). Following that section's
established pattern, the explanation is an inline `<em>` note (as `low_vram` has),
not an `InfoHint`. No `helpText.ts` entry is added — it would have no consumer
(YAGNI). The plain-language description lives in the inline note.

- [ ] **Step 1: Add the `saveLivePreview` save-chain function**

In `src/lib/components/PreferencesDialog.svelte`, right after the `saveLowVram`
function closes (line 67, the `}` before `</script>`), add:

```ts
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
```

- [ ] **Step 2: Add the checkbox to the Hardware section**

Replace the low-VRAM label block (line 114-121):

```svelte
      <label class="lowvram">
        <input
          type="checkbox"
          checked={$settings?.low_vram ?? false}
          disabled={!$settings}
          onchange={(e) => saveLowVram(e.currentTarget.checked)} />
        <span>Low-VRAM mode <em>(slower; fits bigger models)</em></span>
      </label>
```

with (append the second label after it):

```svelte
      <label class="lowvram">
        <input
          type="checkbox"
          checked={$settings?.low_vram ?? false}
          disabled={!$settings}
          onchange={(e) => saveLowVram(e.currentTarget.checked)} />
        <span>Low-VRAM mode <em>(slower; fits bigger models)</em></span>
      </label>
      <label class="lowvram">
        <input
          type="checkbox"
          checked={$settings?.live_preview ?? true}
          disabled={!$settings}
          onchange={(e) => saveLivePreview(e.currentTarget.checked)} />
        <span>Live preview <em>(shows a rough draft as it generates so you can cancel early)</em></span>
      </label>
```

- [ ] **Step 3: Type-check**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/PreferencesDialog.svelte
git commit -m "feat(ui): add Live preview toggle to Preferences (default on)"
```

### Task 8: Full-suite verification

**Files:** none (verification only).

Per the spec, the per-family `proj` render sweep is intentionally dropped — the
user verifies live behavior manually during other testing. This task confirms the
automated suites are green end-to-end.

- [ ] **Step 1: Rust suite**

Run: `cd src-tauri && cargo test`
Expected: PASS — 211 tests (207 original + Task 1 config default + Task 2's 2
command_builder tests + Task 3 `preview_path`). No warnings.

- [ ] **Step 2: Frontend type-check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Confirm no clippy regressions (matches project baseline)**

Run: `cd src-tauri && cargo clippy --all-targets -- -D warnings`
Expected: clean (no new warnings from the preview code).

- [ ] **Step 4: Hand off for manual verification**

Report to the user that the feature is built and the automated suites pass, then
list the manual checks (which the user runs in their own GPU session):
- Live draft appears and sharpens during a run.
- Pressing Cancel leaves no `preview.png` in `<temp>/muchai-preview/`.
- Toggling "Live preview" off in Preferences disables the draft.

No commit — this task produces no code changes.

