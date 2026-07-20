# Multi-File Model Automation & Data-Loss Fix — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the config-clobber data-loss hole and add three automation conveniences (gguf folder auto-detect, auto Low-VRAM per run, one-click per-family recommended settings) to MuchAI's multi-file model support.

**Architecture:** Decision logic lands as pure, unit-tested Rust functions (`resolve_low_vram`, `family_defaults`, weight summing, path extraction); Tauri commands only wire them in. `set_settings` treats `model_definitions` + `last_request` as backend-owned and refuses to let a stale settings payload overwrite them. The frontend gets a belt-and-suspenders store sync, a device-VRAM argument threaded into `generate`, a transient "Low-VRAM auto-enabled" notice, and a "Use recommended settings" button.

**Tech Stack:** Rust (Tauri v2 commands, `serde`, `cargo test`), Svelte 5 + TypeScript (svelte-check via `npm run check`). Working dir for all Rust paths is `src-tauri/`; frontend paths are repo-root-relative.

**Spec:** `docs/superpowers/specs/2026-07-20-multi-file-automation-design.md`

**Testing conventions:**
- Rust tasks are full TDD: write the failing test, run it red, implement, run it green, commit. Run a single test with `cargo test <name>` and the module suite with `cargo test`. All Rust commands run from `src-tauri/`.
- The frontend has **no** JS test runner (package.json has only `check`). Introducing one is out of scope (YAGNI). Svelte/TS tasks are gated by `npm run check` (svelte-check, must stay **0 errors / 0 warnings**) plus the explicit manual-reasoning checks written into each task. Run `npm run check` from the repo root.

**Correction vs. spec (units):** the spec's `resolve_low_vram` sketch passes `weights_mb` into `estimate_vram_mb`, but `fit::estimate_vram_mb` takes **bytes** (`file_size_bytes`). This plan passes **bytes** end-to-end (`weights_bytes` / `sum_file_sizes` returns bytes) so the estimate is correct. Same behavior the spec intended, right units.

---

## File Structure

**Rust (`src-tauri/src/`):**
- `commands.rs` — `set_settings` preserve fix (Task 1); `detect_folder` gguf (Task 3); `generate` Low-VRAM wiring + notice emit + new `device_vram_mb` arg (Task 6); new `recommended_settings` command + `single_file_family` + `basename` helpers (Task 8).
- `models.rs` — make `MODEL_EXTS` / `is_model_file` `pub(crate)` (Task 3); new `sum_file_sizes` (Task 5).
- `types.rs` — new `ModelRef::component_paths` (Task 5); new `GenDefaults` struct (Task 7).
- `fit.rs` — new `resolve_low_vram` (Task 4).
- `recipes.rs` — new `family_defaults` (Task 7).
- `lib.rs` — register `recommended_settings` in the invoke handler (Task 8).

**Frontend (`src/`):**
- `lib/stores.ts` — `definitions` → `settings.model_definitions` sync (Task 2).
- `lib/api.ts` — `generate` gains `deviceVramMb`; new `onGenNotice`, `recommendedSettings` (Tasks 6, 8).
- `lib/types.ts` — new `GenDefaults` interface (Task 8).
- `lib/components/GenerateBar.svelte` — pass device VRAM into `generate`; show transient Low-VRAM notice (Task 6).
- `lib/components/SettingsPanel.svelte` — "Use recommended settings" button (Task 9).

**Task dependency order:** 1, 2, 3, 4, 5 are independent. 6 depends on 4 + 5. 7 is independent. 8 depends on 7. 9 depends on 8.

---

## Task 1: Preserve backend-owned config on `set_settings` (Item 2)

**Problem:** `set_settings` overwrites the entire in-memory `AppConfig` with the payload the UI sends, then saves it. The UI's `settings` store can carry a stale `model_definitions` / `last_request` (e.g. a definition was added by `download_multifile` after the store was last loaded). Saving the stale payload erases those fields on disk → data loss (old bugs #6/#7/#8, one root cause). Fix: on `set_settings`, keep the current backend-owned `model_definitions` and `last_request`, ignoring whatever the payload carried for them. Those fields have their own dedicated commands (`save_model_definition`, `delete_model_definition`, the `generate` last-request persist); `set_settings` is only for preference fields.

**Files:**
- Modify: `src-tauri/src/commands.rs:90-113` (`set_settings`)
- Test: `src-tauri/src/commands.rs` (append to the existing `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src-tauri/src/commands.rs` (after `safe_model_dir_guards_shared_and_separators`). This test drives the preserve logic through a pure helper `merged_settings` (introduced in Step 3) so it needs no Tauri `State`:

```rust
    #[test]
    fn set_settings_preserves_definitions_and_last_request() {
        // Current backend state: has a saved definition and a meaningful last_request.
        let mut current = crate::config::default_config();
        current.model_definitions = vec![flux_def("keep-me")];
        current.last_request.prompt = "backend-owned prompt".into();

        // Incoming payload from the UI: preference fields changed, but it carries a
        // STALE (empty) definitions list and a default last_request.
        let mut incoming = crate::config::default_config();
        incoming.theme = crate::types::Theme::Light;
        incoming.low_vram = true;
        incoming.model_definitions = Vec::new(); // stale — must NOT clobber
        incoming.last_request = crate::types::GenerationRequest::default(); // stale

        let merged = merged_settings(&current, incoming);

        // Preference fields adopt the incoming values…
        assert_eq!(merged.theme, crate::types::Theme::Light);
        assert!(merged.low_vram);
        // …but backend-owned fields are preserved from `current`.
        assert_eq!(merged.model_definitions, current.model_definitions);
        assert_eq!(merged.last_request.prompt, "backend-owned prompt");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test set_settings_preserves_definitions_and_last_request`
Expected: FAIL to compile — `cannot find function 'merged_settings' in this scope`.

- [ ] **Step 3: Implement the preserve helper and rewire `set_settings`**

In `src-tauri/src/commands.rs`, add this pure helper immediately above `pub fn set_settings` (before line 90):

```rust
/// Merge an incoming settings payload with the current backend state, keeping the
/// backend-owned fields (`model_definitions`, `last_request`) from `current`. The
/// UI's copy of those can be stale; they have their own dedicated commands, so a
/// preference save must never clobber them. Pure so it is unit-testable.
fn merged_settings(current: &AppConfig, incoming: AppConfig) -> AppConfig {
    AppConfig {
        model_definitions: current.model_definitions.clone(),
        last_request: current.last_request.clone(),
        ..incoming
    }
}
```

Then replace the body of `set_settings` (lines 90-113) with a version that merges before saving:

```rust
#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<AppState>,
    config: AppConfig,
) -> Result<(), String> {
    // Merge under the lock so the preserved backend-owned fields reflect the
    // latest state (a concurrent download_multifile may have just added one).
    let merged = {
        let mut cfg = state.config.lock().unwrap();
        // A changed engine path means a different binary that may enumerate devices
        // in a different order — drop the cached list so the next probe re-reads it,
        // preserving index parity with `--backend vulkanN`.
        if cfg.sd_binary_path != config.sd_binary_path {
            *state.gpu_devices.lock().unwrap() = None;
        }
        let merged = merged_settings(&cfg, config);
        *cfg = merged.clone();
        merged
    };
    config::save_config_to(&config::config_file_path(), &merged).map_err(|e| e.to_string())?;
    // Keep the asset-protocol scope in sync so images in a newly chosen gallery
    // dir can be displayed without restarting the app.
    let _ = app
        .asset_protocol_scope()
        .allow_directory(&merged.gallery_dir, true);
    Ok(())
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test set_settings_preserves_definitions_and_last_request`
Expected: PASS.

- [ ] **Step 5: Run the whole Rust suite (no regressions)**

Run: `cargo test`
Expected: all tests pass (previously-passing count + 1).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "fix(config): preserve model_definitions + last_request on set_settings"
```

---

## Task 2: Belt-and-suspenders — keep `settings.model_definitions` synced (Item 2)

**Problem:** Task 1 fixes the backend so a stale payload can't clobber definitions. This task removes the *source* of the staleness on the frontend: the `settings` store's `model_definitions` field can drift from the authoritative `definitions` store (which `startMultiFileDownload` updates). Mirror `definitions` into `settings.model_definitions` whenever it changes, so any future `setSettings(get(settings))` call already carries the current list. Defensive redundancy with Task 1 — cheap, and it keeps the two stores coherent for any other reader.

**Files:**
- Modify: `src/lib/stores.ts` (add a subscription after the store declarations, ~line 14)

**No JS test runner** — this task is gated by `npm run check` plus the manual reasoning in Step 3.

- [ ] **Step 1: Add the sync subscription**

In `src/lib/stores.ts`, immediately after the `sysStats` store declaration (line 14) and before the `GenStatus` type (line 16), add:

```ts
// Belt-and-suspenders: mirror the authoritative `definitions` list into the
// `settings` snapshot so any `setSettings(get(settings))` already carries the
// current definitions. The backend also preserves them (see set_settings), so
// this only has to keep the in-memory copy coherent for other readers.
definitions.subscribe((defs) => {
  settings.update((s) => (s ? { ...s, model_definitions: defs } : s));
});
```

- [ ] **Step 2: Run svelte-check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Manual verification (reason through it)**

Confirm each holds by reading the code:
- `definitions.subscribe` fires immediately with the current value on registration; when `settings` is still `null` (before `+page.svelte` loads config) the callback returns `s` unchanged — no crash, no spurious write.
- After `startMultiFileDownload` upserts into `definitions`, the subscription copies the new list into `settings.model_definitions`.
- No infinite loop: the callback writes to `settings`, not `definitions`, so it can't re-trigger itself.

- [ ] **Step 4: Commit**

```bash
git add src/lib/stores.ts
git commit -m "fix(ui): mirror definitions into settings.model_definitions"
```

---

## Task 3: Auto-detect `.gguf` (and `.ckpt`) in folder scan (Item 1)

**Problem:** `detect_folder` filters folder entries to `.safetensors` only (commands.rs:596), so a user pointing at a folder with a `.gguf` diffusion model gets no detection and has to hand-pick it. The recipe matcher (`recipes::detect`) is already extension-agnostic (it matches filename substrings), so widening the accepted extensions is all that's needed. Reuse the model-file predicate that already backs the model scanner (`models::is_model_file`, which covers `safetensors`/`ckpt`/`gguf`) instead of a second inline extension check — one source of truth for "what counts as a model file."

**Files:**
- Modify: `src-tauri/src/models.rs:6-13` (make `MODEL_EXTS` + `is_model_file` `pub(crate)`)
- Modify: `src-tauri/src/commands.rs:592-600` (`detect_folder` file filter)
- Test: `src-tauri/src/commands.rs` (append to `mod tests`)

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src-tauri/src/commands.rs`:

```rust
    #[test]
    fn detect_folder_picks_up_gguf_diffusion() {
        // A FLUX folder whose diffusion model is a .gguf; encoders + VAE are
        // .safetensors. Detection must find the family and fill the diffusion slot
        // with the .gguf file.
        let root = std::env::temp_dir().join(format!("muchai-detect-gguf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for f in ["flux1-schnell-Q4_0.gguf", "t5xxl_fp16.safetensors", "clip_l.safetensors", "ae.safetensors"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }

        let result = detect_folder(root.to_string_lossy().into_owned());
        assert_eq!(result.family, "flux1");
        let diffusion = result
            .slots
            .iter()
            .find(|s| s.role == ComponentRole::Diffusion)
            .expect("diffusion slot must be filled");
        assert!(diffusion.path.ends_with("flux1-schnell-Q4_0.gguf"), "got {}", diffusion.path);

        let _ = std::fs::remove_dir_all(&root);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test detect_folder_picks_up_gguf_diffusion`
Expected: FAIL — the `.gguf` file is filtered out before detection, so `family` is `"custom"` and the diffusion slot is absent (`expect` panics).

- [ ] **Step 3: Make the model-file predicate crate-visible**

In `src-tauri/src/models.rs`, change lines 6 and 8 from private to `pub(crate)`:

```rust
pub(crate) const MODEL_EXTS: [&str; 3] = ["safetensors", "ckpt", "gguf"];

pub(crate) fn is_model_file(path: &Path) -> bool {
```

(The body of `is_model_file` is unchanged.)

- [ ] **Step 4: Use it in `detect_folder`**

In `src-tauri/src/commands.rs`, replace the file-filter block inside `detect_folder` (lines 592-601):

```rust
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_file()
                && p.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("safetensors")).unwrap_or(false)
            {
                entries.push(p);
            }
        }
    }
```

with:

```rust
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.flatten() {
            let p = e.path();
            // Accept every recognized model extension (safetensors/ckpt/gguf) so a
            // .gguf diffusion model is auto-detected too. `recipes::detect` matches
            // on filename substrings, so it is already extension-agnostic.
            if p.is_file() && models::is_model_file(&p) {
                entries.push(p);
            }
        }
    }
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test detect_folder_picks_up_gguf_diffusion`
Expected: PASS.

- [ ] **Step 6: Run the whole Rust suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/commands.rs
git commit -m "feat(models): auto-detect gguf/ckpt diffusion files in folder scan"
```

---

## Task 4: `resolve_low_vram` decision function (Item 4a)

**Problem:** Low-VRAM offload is a manual toggle today. We want it to auto-engage per run when the model's weights won't fit the selected GPU, while keeping the manual toggle as a force-always-on. This task adds the pure decision function; Task 6 wires it into `generate`.

**Units correction vs. spec:** the spec sketch named the parameter `weights_mb` and fed it to `estimate_vram_mb`, but `fit::estimate_vram_mb` takes **bytes** (its param is `file_size_bytes`). Passing MB would make the estimate ~1500 MB for any model (wrong). This function takes `weights_bytes` and passes bytes straight through — correct units, same intended behavior.

**Return value:** `(low_vram_enabled, auto_engaged)`. `auto_engaged` is `true` only when the fit estimate (not the manual toggle) turned it on, so the caller knows to emit the "auto-enabled" notice.

**Rules (in order):**
1. Manual toggle on → `(true, false)` — forced on, not an auto decision, no notice.
2. CPU device → `(false, false)` — offload flags are a GPU-VRAM measure; irrelevant on CPU.
3. Have both weight bytes and device VRAM, and `estimate_vram_mb(weights) > vram` → `(true, true)` — auto-engage + notice.
4. Otherwise (fits, or either value unknown) → `(false, false)`.

**Files:**
- Modify: `src-tauri/src/fit.rs` (add `resolve_low_vram` after `fit_verdict`, ~line 44)
- Test: `src-tauri/src/fit.rs` (append to `mod tests`)

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/src/fit.rs` (after `verdict_unknown_when_vram_or_size_missing`):

```rust
    #[test]
    fn low_vram_manual_toggle_forces_on_without_auto_flag() {
        // Manual on wins regardless of fit, and is never reported as "auto".
        assert_eq!(resolve_low_vram(true, Some(500 * MB), Some(24000), false), (true, false));
        assert_eq!(resolve_low_vram(true, None, None, false), (true, false));
    }

    #[test]
    fn low_vram_auto_engages_when_weights_exceed_vram() {
        // est(20 GB) ≈ 20480*1.15 + 1500 ≈ 25052 MB > 12000 MB VRAM → auto on.
        let twenty_gb = 20u64 * 1024 * MB;
        assert_eq!(resolve_low_vram(false, Some(twenty_gb), Some(12000), false), (true, true));
    }

    #[test]
    fn low_vram_stays_off_when_model_fits() {
        // est(1 GB) = 1024*1.15 + 1500 ≈ 2677 MB < 12000 MB → off.
        let one_gb = 1024 * MB;
        assert_eq!(resolve_low_vram(false, Some(one_gb), Some(12000), false), (false, false));
    }

    #[test]
    fn low_vram_off_on_cpu_device_even_if_weights_huge() {
        // CPU has no GPU VRAM to overflow; offload flags don't apply.
        let twenty_gb = 20u64 * 1024 * MB;
        assert_eq!(resolve_low_vram(false, Some(twenty_gb), Some(12000), true), (false, false));
    }

    #[test]
    fn low_vram_off_when_vram_or_weights_unknown() {
        // Can't decide a fit → don't auto-engage (manual toggle still available).
        assert_eq!(resolve_low_vram(false, None, Some(12000), false), (false, false));
        assert_eq!(resolve_low_vram(false, Some(1024 * MB), None, false), (false, false));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test resolve_low_vram`
Expected: FAIL to compile — `cannot find function 'resolve_low_vram'`.

- [ ] **Step 3: Implement `resolve_low_vram`**

In `src-tauri/src/fit.rs`, add after `fit_verdict` (after line 44, before `#[cfg(test)]`):

```rust
/// Decide whether to run the engine in Low-VRAM offload mode for one generation.
/// Returns `(low_vram_enabled, auto_engaged)`. `auto_engaged` is true only when
/// the fit estimate turned it on (so the caller can surface a one-time notice);
/// a manual toggle forces it on but is never reported as "auto".
///
/// `weights_bytes` is the summed size of the model's weight files in BYTES (fed
/// straight to `estimate_vram_mb`, which expects bytes). `device_vram_mb` is the
/// selected GPU's total VRAM in MB. `is_cpu_device` short-circuits to off since
/// the offload flags only relieve GPU-VRAM pressure.
pub fn resolve_low_vram(
    manual_toggle: bool,
    weights_bytes: Option<u64>,
    device_vram_mb: Option<u64>,
    is_cpu_device: bool,
) -> (bool, bool) {
    if manual_toggle {
        return (true, false);
    }
    if is_cpu_device {
        return (false, false);
    }
    match (weights_bytes, device_vram_mb) {
        (Some(w), Some(v)) if estimate_vram_mb(w) > v => (true, true),
        _ => (false, false),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test resolve_low_vram`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/fit.rs
git commit -m "feat(fit): add resolve_low_vram auto-engage decision"
```

---

## Task 5: Weight-size helpers — `component_paths` + `sum_file_sizes` (Item 4b)

**Problem:** To decide auto Low-VRAM (Task 4), `generate` needs the total on-disk size of the model's weight files. A single-file model is one path; a multi-file model is the diffusion file plus its set optional components. This task adds (a) `ModelRef::component_paths` (the file paths that make up a model) and (b) `models::sum_file_sizes` (their summed byte size, `None` if any file can't be stat'd). Task 6 chains them: `sum_file_sizes(model.component_paths())`.

**Files:**
- Modify: `src-tauri/src/types.rs` (add `impl ModelRef` after the `Default for ModelRef` impl, ~line 114)
- Modify: `src-tauri/src/models.rs` (add `sum_file_sizes` after `scan_models_excluding`, ~line 62)
- Test: `src-tauri/src/types.rs` and `src-tauri/src/models.rs` (append to each `mod tests`)

- [ ] **Step 1: Write the failing test for `component_paths`**

Append inside `mod tests` in `src-tauri/src/types.rs`:

```rust
    #[test]
    fn component_paths_single_file_is_just_the_path() {
        let m = ModelRef::SingleFile { path: "/m/model.safetensors".into() };
        assert_eq!(m.component_paths(), vec!["/m/model.safetensors".to_string()]);
    }

    #[test]
    fn component_paths_multi_file_lists_diffusion_plus_set_components() {
        let m = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/flux1-dev.safetensors".into(),
            t5xxl: Some("/m/t5xxl.safetensors".into()),
            clip_l: Some("/m/clip_l.safetensors".into()),
            vae: Some("/m/ae.safetensors".into()),
            clip_g: None,                 // unset → excluded
            llm: Some("   ".into()),      // blank → excluded
            vae_format: Some("flux".into()), // NOT a file path → excluded
            prediction: Some("flux_flow".into()), // NOT a file path → excluded
        });
        assert_eq!(
            m.component_paths(),
            vec![
                "/m/flux1-dev.safetensors".to_string(),
                "/m/ae.safetensors".to_string(),
                "/m/clip_l.safetensors".to_string(),
                "/m/t5xxl.safetensors".to_string(),
            ]
        );
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test component_paths`
Expected: FAIL to compile — `no method named 'component_paths'`.

- [ ] **Step 3: Implement `component_paths`**

In `src-tauri/src/types.rs`, add after the `impl Default for ModelRef { … }` block (after line 114):

```rust
impl ModelRef {
    /// The weight files that make up this model, for size/estimation purposes.
    /// Single-file → just its path. Multi-file → diffusion model plus every SET,
    /// non-blank optional component (`vae`/`clip_l`/`clip_g`/`t5xxl`/`llm`).
    /// The `vae_format` / `prediction` fields are engine flags, not files, and
    /// are excluded. Order: diffusion, vae, clip_l, clip_g, t5xxl, llm.
    pub fn component_paths(&self) -> Vec<String> {
        match self {
            ModelRef::SingleFile { path } => vec![path.clone()],
            ModelRef::MultiFile(c) => {
                let mut paths = vec![c.diffusion_model.clone()];
                for opt in [&c.vae, &c.clip_l, &c.clip_g, &c.t5xxl, &c.llm] {
                    if let Some(p) = opt {
                        if !p.trim().is_empty() {
                            paths.push(p.clone());
                        }
                    }
                }
                paths
            }
        }
    }
}
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test component_paths`
Expected: PASS (2 tests).

- [ ] **Step 5: Write the failing test for `sum_file_sizes`**

Append inside `mod tests` in `src-tauri/src/models.rs`:

```rust
    #[test]
    fn sum_file_sizes_totals_existing_files() {
        let root = std::env::temp_dir().join(format!("muchai-sumsz-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("a.safetensors"), 100);
        touch(&root.join("b.safetensors"), 250);
        let paths = vec![
            root.join("a.safetensors").to_string_lossy().into_owned(),
            root.join("b.safetensors").to_string_lossy().into_owned(),
        ];
        assert_eq!(sum_file_sizes(&paths), Some(350));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sum_file_sizes_is_none_if_any_file_missing() {
        let root = std::env::temp_dir().join(format!("muchai-sumsz2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("a.safetensors"), 100);
        let paths = vec![
            root.join("a.safetensors").to_string_lossy().into_owned(),
            root.join("gone.safetensors").to_string_lossy().into_owned(),
        ];
        assert_eq!(sum_file_sizes(&paths), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sum_file_sizes_empty_is_zero() {
        assert_eq!(sum_file_sizes(&[]), Some(0));
    }
```

- [ ] **Step 6: Run to verify it fails**

Run: `cargo test sum_file_sizes`
Expected: FAIL to compile — `cannot find function 'sum_file_sizes'`.

- [ ] **Step 7: Implement `sum_file_sizes`**

In `src-tauri/src/models.rs`, add after `scan_models_excluding` (after line 62):

```rust
/// Total size in BYTES of the given files. Returns `None` if ANY path can't be
/// stat'd (missing/unreadable), so callers treat a broken model as "size unknown"
/// rather than silently undercounting. An empty slice sums to `Some(0)`.
pub fn sum_file_sizes(paths: &[String]) -> Option<u64> {
    let mut total: u64 = 0;
    for p in paths {
        let meta = fs::metadata(p).ok()?;
        total = total.saturating_add(meta.len());
    }
    Some(total)
}
```

- [ ] **Step 8: Run to verify it passes**

Run: `cargo test sum_file_sizes`
Expected: PASS (3 tests).

- [ ] **Step 9: Run the whole Rust suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/models.rs
git commit -m "feat(models): add ModelRef::component_paths and sum_file_sizes"
```

---

## Task 6: Wire auto Low-VRAM into `generate` + surface the notice (Item 4c)

**Problem:** With Tasks 4 and 5 in place, `generate` must (a) accept the selected GPU's VRAM from the frontend, (b) compute the summed weight bytes, (c) call `resolve_low_vram`, (d) pass the resulting `low_vram` to the engine, and (e) emit a one-time event when it was auto-engaged so the UI can tell the user. The frontend passes VRAM from the existing `sysStats.gpu.vram_total_mb` (no NVML rebuild in the command path) and shows a transient note, mirroring the existing `willRunOnCpu` `.cpu-note` pattern.

**Depends on:** Task 4 (`resolve_low_vram`), Task 5 (`component_paths`, `sum_file_sizes`).

**Files:**
- Modify: `src-tauri/src/commands.rs` (`generate` signature + body, lines 133-176)
- Modify: `src/lib/api.ts` (`generate` gains `deviceVramMb`; add `onGenNotice`)
- Modify: `src/lib/components/GenerateBar.svelte` (pass VRAM, show note)

**Testing:** the Rust decision logic is already unit-tested (Task 4). `generate` is an I/O command (spawns the engine) with no unit harness; its wiring is verified by `cargo test` (compiles + no regressions) and the manual check in Step 6. Frontend gated by `npm run check`.

- [ ] **Step 1: Add the `device_vram_mb` parameter to `generate`**

In `src-tauri/src/commands.rs`, change the `generate` signature (lines 133-138) to add the new argument:

```rust
#[tauri::command]
pub async fn generate(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GenerationRequest,
    device_vram_mb: Option<u64>,
) -> Result<Vec<GalleryItem>, String> {
```

- [ ] **Step 2: Compute Low-VRAM and emit the notice**

In `src-tauri/src/commands.rs`, replace the single line that builds `engine_opts` (line 176):

```rust
    let engine_opts = crate::command_builder::EngineOptions { low_vram: cfg.low_vram };
```

with the resolve-and-notify block:

```rust
    // Decide Low-VRAM for THIS run: manual toggle forces it on; otherwise
    // auto-engage when the summed weight bytes won't fit the selected GPU's VRAM.
    // Weights are summed in bytes (what estimate_vram_mb expects). A broken model
    // (some file un-stat'able) yields None → treated as "unknown", no auto-engage.
    // `backend` was just moved into `backend_owned` (line above), so read that.
    let is_cpu = backend_owned.as_deref() == Some("cpu");
    let weights_bytes = models::sum_file_sizes(&request.model.component_paths());
    let (low_vram, auto_engaged) =
        crate::fit::resolve_low_vram(cfg.low_vram, weights_bytes, device_vram_mb, is_cpu);
    if auto_engaged {
        // One-time, payload-free signal; the note text lives in the frontend.
        let _ = app.emit("generation:low_vram_auto", ());
    }
    let engine_opts = crate::command_builder::EngineOptions { low_vram };
```

- [ ] **Step 3: Run the Rust suite (compiles + no regressions)**

Run: `cargo test`
Expected: all tests pass. (The signature change and new wiring compile; `models`, `crate::fit`, and `app.emit` are already in scope via the existing imports.)

- [ ] **Step 4: Update the `generate` API wrapper + add the notice listener**

In `src/lib/api.ts`, replace the `generate` export (line 11):

```ts
/** Returns one item per produced image (batch_count may yield several).
 *  `deviceVramMb` is the selected GPU's total VRAM (from sysStats) so the
 *  backend can auto-engage Low-VRAM; null when unknown / running on CPU. */
export const generate = (request: GenerationRequest, deviceVramMb: number | null = null) =>
  invoke<GalleryItem[]>("generate", { request, deviceVramMb });
```

And add, next to `onProgress` (after line 24):

```ts
/** Fires once per run when the backend auto-engaged Low-VRAM mode for it. */
export const onGenNotice = (cb: () => void): Promise<UnlistenFn> =>
  listen("generation:low_vram_auto", () => cb());
```

- [ ] **Step 5: Pass VRAM and show the note in GenerateBar**

In `src/lib/components/GenerateBar.svelte`, update the imports (lines 4-5) to add `sysStats` and `onGenNotice`:

```ts
  import { request, genStatus, history, currentImage, currentItem, settings, gpuDevices, sysStats } from "../stores";
  import { generate, cancelGeneration, imageSrc, listHistory, onProgress, onGenNotice } from "../api";
```

Add a reactive flag declaration and reset it at the top of `run()`. Replace the `run()` function (lines 8-24) with:

```ts
  let lowVramAuto = false;

  async function run() {
    const req = get(request);
    if (!modelIsSet(req.model)) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
    if (!req.prompt.trim()) { genStatus.set({ kind: "error", message: "Enter a prompt." }); return; }
    lowVramAuto = false;
    // Pass the selected GPU's VRAM so the backend can auto-engage Low-VRAM.
    // sysStats only reports it for a real GPU; 0/absent → null (CPU or unknown).
    const vram = get(sysStats)?.gpu?.vram_total_mb ?? 0;
    const deviceVramMb = vram > 0 ? vram : null;
    genStatus.set({ kind: "running", progress: null });
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
    }
  }
```

Register the notice listener in the existing `onMount` (replace lines 37-40):

```ts
  onMount(() => {
    const un = onProgress((p) => genStatus.update((s) => s.kind === "running" ? { kind: "running", progress: p } : s));
    const unNotice = onGenNotice(() => { lowVramAuto = true; });
    return () => { un.then((f) => f()); unNotice.then((f) => f()); };
  });
```

Add the note to the markup, right after the `.cpu-note` block (after line 54):

```svelte
{#if $genStatus.kind === "running" && lowVramAuto}
  <div class="cpu-note" role="status">Low-VRAM mode auto-enabled — this model is larger than your GPU's memory, so generation will be slower.</div>
{/if}
```

(Reuses the existing `.cpu-note` style — no new CSS.)

- [ ] **Step 6: Run svelte-check + manual reasoning**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

Reason through the flow: on Generate, `lowVramAuto` resets to false; `deviceVramMb` is the GPU's VRAM (or null on CPU / when sysStats has no GPU). If the backend auto-engages, it emits `generation:low_vram_auto`, the listener sets `lowVramAuto = true`, and the note renders while `genStatus.kind === "running"`. On the next run it resets.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src/lib/api.ts src/lib/components/GenerateBar.svelte
git commit -m "feat(generate): auto-engage Low-VRAM per run with a UI notice"
```

---

## Task 7: `GenDefaults` type + `family_defaults` table (Item 5a)

**Problem:** `GenerationRequest::default` is SD-1.5-appropriate (20 steps, CFG 7, Euler a, 512²) and wrong for FLUX/SD3/Qwen. This task adds a per-family recommended-settings table as a pure function. Task 8 exposes it as a command; Task 9 adds the manual "Use recommended settings" button (never auto-applied).

**`GenDefaults` fields:** `steps: u32`, `cfg_scale: f32`, `sampler: Sampler`, `width: u32`, `height: u32`. Derives `Serialize`/`Deserialize` (command return), `Copy`/`Clone`, `PartialEq` (NOT `Eq` — `f32`), `Debug`.

**Family table** (from the approved spec):

| family key | schnell? | steps | cfg_scale | sampler | width×height |
|---|---|---|---|---|---|
| `flux1` | yes (diffusion filename contains "schnell") | 4 | 1.0 | Euler | 1024² |
| `flux1` | no (dev / krea) | 20 | 1.0 | Euler | 1024² |
| `flux2` | — | 4 | 1.0 | Euler | 1024² |
| `sd3` | — | 28 | 4.5 | Euler | 1024² |
| `qwen-image` | — | 20 | 2.5 | Euler | 1024² |
| `sdxl` | — | 28 | 7.0 | Euler a | 1024² |
| `sd15` | — | 20 | 7.0 | Euler a | 512² |
| anything else (incl. `custom`) | — | → `None` (button hidden) | | | |

**Files:**
- Modify: `src-tauri/src/types.rs` (add `GenDefaults` after `GenerationRequest`'s `Default` impl, ~line 181)
- Modify: `src-tauri/src/recipes.rs` (add `family_defaults` after `recipe_for`, ~line 232; add `GenDefaults` to the `use crate::types::…` import on line 1)
- Test: `src-tauri/src/recipes.rs` (append to `mod tests`)

- [ ] **Step 1: Add the `GenDefaults` struct**

In `src-tauri/src/types.rs`, add after the `impl Default for GenerationRequest { … }` block (after line 181):

```rust
/// Recommended generation settings for a model family. Applied only on explicit
/// user action (the "Use recommended settings" button) — never auto-applied.
/// `PartialEq` (not `Eq`) because `cfg_scale` is `f32`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GenDefaults {
    pub steps: u32,
    pub cfg_scale: f32,
    pub sampler: Sampler,
    pub width: u32,
    pub height: u32,
}
```

- [ ] **Step 2: Write the failing tests for `family_defaults`**

Append inside `mod tests` in `src-tauri/src/recipes.rs`:

```rust
    #[test]
    fn family_defaults_flux1_schnell_uses_four_steps() {
        let d = family_defaults("flux1", Some("flux1-schnell-Q4_0.gguf")).unwrap();
        assert_eq!(d.steps, 4);
        assert_eq!(d.cfg_scale, 1.0);
        assert_eq!(d.sampler, crate::types::Sampler::Euler);
        assert_eq!((d.width, d.height), (1024, 1024));
    }

    #[test]
    fn family_defaults_flux1_dev_uses_twenty_steps() {
        let d = family_defaults("flux1", Some("flux1-dev.safetensors")).unwrap();
        assert_eq!(d.steps, 20);
        assert_eq!(d.cfg_scale, 1.0);
    }

    #[test]
    fn family_defaults_flux1_without_filename_defaults_to_dev() {
        // No filename to check for "schnell" → assume the dev/krea profile.
        let d = family_defaults("flux1", None).unwrap();
        assert_eq!(d.steps, 20);
    }

    #[test]
    fn family_defaults_cover_each_family() {
        assert_eq!(family_defaults("flux2", None).unwrap().steps, 4);
        let sd3 = family_defaults("sd3", None).unwrap();
        assert_eq!((sd3.steps, sd3.cfg_scale), (28, 4.5));
        let qwen = family_defaults("qwen-image", None).unwrap();
        assert_eq!((qwen.steps, qwen.cfg_scale), (20, 2.5));
        let sdxl = family_defaults("sdxl", None).unwrap();
        assert_eq!((sdxl.steps, sdxl.sampler, (sdxl.width, sdxl.height)),
                   (28, crate::types::Sampler::EulerA, (1024, 1024)));
        let sd15 = family_defaults("sd15", None).unwrap();
        assert_eq!((sd15.steps, sd15.sampler, (sd15.width, sd15.height)),
                   (20, crate::types::Sampler::EulerA, (512, 512)));
    }

    #[test]
    fn family_defaults_unknown_and_custom_are_none() {
        assert!(family_defaults("custom", None).is_none());
        assert!(family_defaults("totally-unknown", None).is_none());
    }
```

- [ ] **Step 3: Run to verify they fail**

Run: `cargo test family_defaults`
Expected: FAIL to compile — `cannot find function 'family_defaults'`.

- [ ] **Step 4: Implement `family_defaults`**

In `src-tauri/src/recipes.rs`, change the import on line 1 to also bring in `GenDefaults` and `Sampler`:

```rust
use crate::types::{GenDefaults, ModelComponents, Sampler};
```

Then add after `recipe_for` (after line 232):

```rust
/// Recommended generation settings for a model family, or `None` for families
/// without a meaningful preset (`custom`, single-file "none", or any unknown id
/// → the UI hides the "Use recommended settings" button). For `flux1`, the
/// diffusion filename selects the schnell (4-step) vs. dev/krea (20-step)
/// profile; a missing filename assumes dev. Family keys match `detect_best`
/// family ids plus the single-file heuristics `"sdxl"` / `"sd15"`.
pub fn family_defaults(family: &str, diffusion_filename: Option<&str>) -> Option<GenDefaults> {
    let d = |steps, cfg_scale, sampler, width, height| GenDefaults {
        steps,
        cfg_scale,
        sampler,
        width,
        height,
    };
    match family {
        "flux1" => {
            let is_schnell = diffusion_filename
                .map(|f| f.to_lowercase().contains("schnell"))
                .unwrap_or(false);
            let steps = if is_schnell { 4 } else { 20 };
            Some(d(steps, 1.0, Sampler::Euler, 1024, 1024))
        }
        "flux2" => Some(d(4, 1.0, Sampler::Euler, 1024, 1024)),
        "sd3" => Some(d(28, 4.5, Sampler::Euler, 1024, 1024)),
        "qwen-image" => Some(d(20, 2.5, Sampler::Euler, 1024, 1024)),
        "sdxl" => Some(d(28, 7.0, Sampler::EulerA, 1024, 1024)),
        "sd15" => Some(d(20, 7.0, Sampler::EulerA, 512, 512)),
        _ => None,
    }
}
```

- [ ] **Step 5: Run to verify they pass**

Run: `cargo test family_defaults`
Expected: PASS (5 tests).

- [ ] **Step 6: Run the whole Rust suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/recipes.rs
git commit -m "feat(recipes): add GenDefaults and per-family family_defaults table"
```

---

## Task 8: `recommended_settings` command + registration + API (Item 5b)

**Problem:** Task 7 built the table. This task exposes it as a Tauri command that derives the family from a `ModelRef` and returns `Option<GenDefaults>`. A multi-file model's family comes from `recipes::detect_best` over its component filenames; a single-file model uses a filename heuristic (`"xl"` in the name → SDXL, else SD-1.5) since a single file carries no family metadata. The button (Task 9) shows only when this returns `Some`.

**Placement correction vs. spec:** the spec placed the button in ParamsPanel, but ParamsPanel is a read-only display of `$currentItem`. The editable store is `request`, bound in SettingsPanel — so the button lands there (Task 9). This task only adds the backend command + API wrapper + TS type.

**Depends on:** Task 7 (`GenDefaults`, `family_defaults`).

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `basename`, `single_file_family`, `recommended_settings`; extend the `use crate::types::…` import on line 4)
- Modify: `src-tauri/src/lib.rs` (register the command, ~line 97)
- Modify: `src/lib/api.ts` (add `recommendedSettings`; import `ModelRef`, `GenDefaults`)
- Modify: `src/lib/types.ts` (add the `GenDefaults` interface)
- Test: `src-tauri/src/commands.rs` (append to `mod tests`)

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/src/commands.rs`:

```rust
    #[test]
    fn recommended_settings_multi_file_flux_schnell() {
        // A multi-file FLUX schnell model → flux1 schnell profile (4 steps).
        let model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/flux1-schnell-Q4_0.gguf".into(),
            t5xxl: Some("/m/t5xxl_fp16.safetensors".into()),
            clip_l: Some("/m/clip_l.safetensors".into()),
            vae: Some("/m/ae.safetensors".into()),
            ..Default::default()
        });
        let d = recommended_settings(model).expect("flux1 has defaults");
        assert_eq!(d.steps, 4);
        assert_eq!(d.cfg_scale, 1.0);
    }

    #[test]
    fn recommended_settings_single_file_sdxl_by_name() {
        let model = ModelRef::SingleFile { path: "/m/sd_xl_base_1.0.safetensors".into() };
        let d = recommended_settings(model).expect("sdxl has defaults");
        assert_eq!(d.steps, 28);
        assert_eq!((d.width, d.height), (1024, 1024));
    }

    #[test]
    fn recommended_settings_single_file_defaults_to_sd15() {
        let model = ModelRef::SingleFile { path: "/m/dreamshaper_8.safetensors".into() };
        let d = recommended_settings(model).expect("sd15 has defaults");
        assert_eq!(d.steps, 20);
        assert_eq!((d.width, d.height), (512, 512));
    }

    #[test]
    fn recommended_settings_empty_single_file_is_none() {
        // No model selected yet → no recommendation (button hidden).
        assert!(recommended_settings(ModelRef::SingleFile { path: "".into() }).is_none());
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test recommended_settings`
Expected: FAIL to compile — `cannot find function 'recommended_settings'`.

- [ ] **Step 3: Extend the types import**

In `src-tauri/src/commands.rs`, change line 4 from:

```rust
use crate::types::{ModelDefinition, ModelRef};
```

to:

```rust
use crate::types::{GenDefaults, ModelDefinition, ModelRef};
```

- [ ] **Step 4: Implement the helpers + command**

In `src-tauri/src/commands.rs`, add near the other free helpers (e.g. directly after `engine_binary_name`, ~line 40):

```rust
/// Last path segment (handles both `/` and `\` separators), for family
/// heuristics. Returns the whole string when there is no separator.
fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

/// Family heuristic for a single-file model, which carries no explicit family:
/// an "xl" in the filename means SDXL, otherwise assume SD-1.5. Both have a
/// recommended-settings preset, so a single-file model always gets the button.
fn single_file_family(filename: &str) -> &'static str {
    if filename.to_lowercase().contains("xl") {
        "sdxl"
    } else {
        "sd15"
    }
}
```

Then add the command near the other model-definition commands (e.g. after `list_recipes`, ~line 583):

```rust
/// Recommended generation settings for the given model, or `None` when the
/// family has no preset (unknown / custom / no model selected) so the UI can
/// hide the button. Multi-file family comes from filename detection; single-file
/// falls back to the SDXL/SD-1.5 name heuristic.
#[tauri::command]
pub fn recommended_settings(model: ModelRef) -> Option<GenDefaults> {
    let (family, diffusion_filename): (Option<String>, Option<String>) = match &model {
        ModelRef::MultiFile(c) => {
            let names: Vec<String> = model.component_paths().iter().map(|p| basename(p)).collect();
            let fam = recipes::detect_best(&names).map(|(r, _)| r.family.to_string());
            (fam, Some(basename(&c.diffusion_model)))
        }
        ModelRef::SingleFile { path } => {
            if path.trim().is_empty() {
                (None, None)
            } else {
                let name = basename(path);
                (Some(single_file_family(&name).to_string()), Some(name))
            }
        }
    };
    family.and_then(|f| recipes::family_defaults(&f, diffusion_filename.as_deref()))
}
```

- [ ] **Step 5: Register the command**

In `src-tauri/src/lib.rs`, add to the `invoke_handler` list after `commands::list_hf_variants,` (line 97):

```rust
            commands::recommended_settings,
```

- [ ] **Step 6: Run the tests + whole Rust suite**

Run: `cargo test recommended_settings` then `cargo test`
Expected: both PASS.

- [ ] **Step 7: Add the `GenDefaults` TS interface**

In `src/lib/types.ts`, add after the `GenerationRequest` interface (after line 80):

```ts
// Mirrors Rust `GenDefaults` (src-tauri/src/types.rs). Recommended per-family
// generation settings, applied only via the "Use recommended settings" button.
export interface GenDefaults {
  steps: number;
  cfg_scale: number;
  sampler: Sampler;
  width: number;
  height: number;
}
```

- [ ] **Step 8: Add the API wrapper**

In `src/lib/api.ts`, add `ModelRef` and `GenDefaults` to the type import on line 3 (append them to the existing `import type { … } from "./types";` list), then add after `listHfVariants` (line 53):

```ts
export const recommendedSettings = (model: ModelRef) =>
  invoke<GenDefaults | null>("recommended_settings", { model });
```

- [ ] **Step 9: Run svelte-check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/lib/api.ts src/lib/types.ts
git commit -m "feat(commands): add recommended_settings command + API"
```

---

## Task 9: "Use recommended settings" button in SettingsPanel (Item 5c)

**Problem:** The final piece of Item 5: a manual button that applies the family's recommended settings to the editable `request`. It appears only when the current model has a preset (`recommendedSettings` returns non-null), and never fires automatically. It lives in `SettingsPanel` (the editable params surface), not ParamsPanel (read-only) — see the Task 8 placement note.

**Depends on:** Task 8 (`recommendedSettings`, `GenDefaults`).

**Files:**
- Modify: `src/lib/components/SettingsPanel.svelte` (script + markup + one style rule)

**Testing:** no JS test runner; gated by `npm run check` + the manual reasoning in Step 3.

- [ ] **Step 1: Add the script logic**

In `src/lib/components/SettingsPanel.svelte`, replace the `<script>` block (lines 1-6) with:

```svelte
<script lang="ts">
  import { request } from "../stores";
  import { SAMPLERS, FORMATS } from "../types";
  import type { GenDefaults, ModelRef } from "../types";
  import { recommendedSettings } from "../api";
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";

  // Recommended settings for the current model (null → family has no preset,
  // so the button is hidden). Re-fetched only when the model reference changes,
  // not on every params edit, guarded by a serialized-key comparison.
  let recommended: GenDefaults | null = null;
  let lastModelKey = "";
  $: {
    const key = JSON.stringify($request.model);
    if (key !== lastModelKey) {
      lastModelKey = key;
      void loadRecommended($request.model);
    }
  }
  async function loadRecommended(model: ModelRef) {
    try {
      recommended = await recommendedSettings(model);
    } catch {
      recommended = null;
    }
  }
  function applyRecommended() {
    const r = recommended;
    if (!r) return;
    request.update((req) => ({
      ...req,
      steps: r.steps,
      cfg_scale: r.cfg_scale,
      sampler: r.sampler,
      width: r.width,
      height: r.height,
    }));
  }
</script>
```

- [ ] **Step 2: Add the button to the markup**

In `src/lib/components/SettingsPanel.svelte`, add the button immediately after the closing `</div>` of `.grid` (after line 37, before the `<style>` block):

```svelte
{#if recommended}
  <button type="button" class="recommend-btn" on:click={applyRecommended}>
    Use recommended settings
  </button>
{/if}
```

Add its style inside the `<style>` block (after the `input, select` rule, before the closing `</style>`):

```css
  .recommend-btn { margin-top:.5rem; width:100%; padding:.4rem; font:inherit; cursor:pointer; }
```

- [ ] **Step 3: Run svelte-check + manual reasoning**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

Reason through it:
- On mount and whenever `$request.model` changes, the guarded reactive block fetches `recommended`; editing steps/CFG/etc. does not re-fetch (the serialized model key is unchanged).
- The button renders only when `recommended` is non-null (a family with a preset). Clicking it applies the five fields to `request`; the bound inputs update live. It is never called automatically.
- `applyRecommended` copies `recommended` to a local `r` before the null check so TypeScript narrows it (no `!` needed) and a concurrent re-fetch can't null it mid-update.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/SettingsPanel.svelte
git commit -m "feat(ui): add Use recommended settings button to SettingsPanel"
```

---

## Final verification (after all tasks)

- [ ] Run the full Rust suite: `cargo test` (from `src-tauri/`) — all green.
- [ ] Run svelte-check: `npm run check` (from repo root) — 0 errors / 0 warnings.
- [ ] Manual smoke (optional, needs the engine + a model): point at a folder holding a `.gguf` FLUX diffusion + encoders → detection fills the diffusion slot (Task 3); select the assembled FLUX model on a 12 GB GPU and Generate → Low-VRAM note appears (Task 6); click "Use recommended settings" → steps become 4/20, CFG 1.0, 1024² (Task 9); change a preference in Settings and confirm saved model definitions survive a restart (Tasks 1–2).
- [ ] Hand off to `superpowers:finishing-a-development-branch`.

## Security constraints (in force throughout)

- Plaintext token storage in `config.json` is an approved decision. Never `{:?}`/log the whole `AppConfig` or any token (`hf_token` / `civitai_token`). None of these tasks logs config; keep it that way — the new `merged_settings` (Task 1) clones token fields via `..incoming` but never logs them.
- Do not remove or weaken the read-only-token recommendation notice in `PreferencesDialog.svelte`.
