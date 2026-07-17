# Multi-file Variants & Low-VRAM Offload — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the four not-yet-built deltas from the 2026-07-17 spec amendment: a `flux2` recipe family, a pure fit-estimate heuristic, a single Low-VRAM offload toggle, and HuggingFace quantization-variant discovery with a picker.

**Architecture:** Three parts, ordered by dependency. **Part A** adds pure backend primitives (the `flux2` recipe in `recipes.rs`; a new `fit.rs` for VRAM estimation) reused later. **Part B** adds the Low-VRAM offload path — a persisted `AppConfig.low_vram` bool threaded through a new `EngineOptions` struct into the pure `build_args`, plus a Preferences toggle. **Part C** adds `hf.rs` (URL classification, tree-API parse, `precision_label`, variant grouping — all pure, with a thin `ureq` fetch behind the `list_hf_variants` command) and a variant-picker UI in the assembly dialog that reuses the existing single-file download to populate the diffusion slot.

**Tech Stack:** Rust (Tauri v2 backend, `serde`, `ureq` for HTTP, `cargo test` unit tests), Svelte 5 + TypeScript frontend (`npm run check` = svelte-check, no unit-test harness — consistent with the app). Image engine is the `stable-diffusion.cpp` CLI; flag spellings are confirmed against `src-tauri/fixtures/sd-help.txt`.

---

## File Structure

**Create:**
- `src-tauri/src/fit.rs` — pure VRAM fit estimate: `estimate_vram_mb()`, `FitVerdict`, `fit_verdict()`.
- `src-tauri/src/hf.rs` — HuggingFace URL classification, tree-API parse, `precision_label()`, variant grouping (all pure) + thin `ureq` `fetch_tree()`.
- `src-tauri/fixtures/hf-tree-flux1.json` — captured HF tree-API response for the `fetch_tree`/parse test.

**Modify (backend):**
- `src-tauri/src/recipes.rs` — add the `flux2` recipe to `recipes()`.
- `src-tauri/src/types.rs` — add `AppConfig.low_vram: bool` (`#[serde(default)]`).
- `src-tauri/src/config.rs` — add `low_vram: false` to `default_config()` + backward-compat test.
- `src-tauri/src/command_builder.rs` — `EngineOptions` struct; `build_args` gains an `opts` param and appends the three offload flags.
- `src-tauri/src/engine.rs` — `run_generation` gains an `opts: EngineOptions` param, forwarded to `build_args`.
- `src-tauri/src/commands.rs` — `generate` passes `EngineOptions { low_vram: cfg.low_vram }`; add the `list_hf_variants` command.
- `src-tauri/src/lib.rs` — `mod fit;`, `mod hf;`, register `commands::list_hf_variants` in the invoke handler.

**Modify (frontend):**
- `src/lib/types.ts` — `AppConfig.low_vram`; `FitVerdict` type; `RatedHfVariant` interface.
- `src/lib/api.ts` — `listHfVariants()` wrapper.
- `src/lib/components/PreferencesDialog.svelte` — Low-VRAM toggle in the Hardware section.
- `src/lib/components/ModelAssembly.svelte` — "Import from HuggingFace" variant picker that downloads the chosen diffusion file and fills the diffusion slot.

**Conventions to follow:**
- Every new `AppConfig` field needs `#[serde(default)]` in `types.rs` AND a value in `config.rs::default_config()`, or `save_then_load_round_trips` breaks.
- Never log or `{:?}`-format the whole `AppConfig` (it holds plaintext tokens).
- HTTP uses `ureq` (already a dep); auth is `Authorization: Bearer <token>` when the token is non-empty (mirror `downloader.rs:61-63`).
- Run backend tests with: `cargo test --manifest-path src-tauri/Cargo.toml --lib <filter>`.

---

## Part A — Recipe & estimation primitives (pure backend)

### Task 1: Add the `flux2` recipe family

**Files:**
- Modify: `src-tauri/src/recipes.rs` — insert a recipe into `recipes()` (before the `custom` entry, after `qwen-image`, around line 199).
- Test: `src-tauri/src/recipes.rs` (existing `#[cfg(test)] mod tests`).

FLUX.2 uses a diffusion transformer + a Qwen3-8B text encoder wired via `--llm` (not T5/CLIP) + a FLUX.2 VAE, with `vae_format = "flux2"` and `prediction = "flux2_flow"` (already whitelisted in `recipe_table_integrity`'s `VAE`/`PRED` arrays). `shared` stays empty — like `sd3` and `qwen-image` — so no companion URLs/sizes are fabricated; companions are assigned via the existing manual/detection flow.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/recipes.rs`:

```rust
    #[test]
    fn flux2_recipe_is_registered_with_expected_shape() {
        let r = recipe_for("flux2").expect("flux2 recipe must exist");
        assert_eq!(r.vae_format, Some("flux2"));
        assert_eq!(r.prediction, Some("flux2_flow"));
        // Required roles: diffusion + llm + vae; no t5xxl/clip.
        let required: Vec<ComponentRole> =
            r.roles.iter().filter(|s| s.required).map(|s| s.role).collect();
        assert!(required.contains(&ComponentRole::Diffusion));
        assert!(required.contains(&ComponentRole::Llm));
        assert!(required.contains(&ComponentRole::Vae));
        assert!(!required.contains(&ComponentRole::T5xxl));
        assert!(!required.contains(&ComponentRole::ClipL));
    }

    #[test]
    fn detect_best_picks_flux2_for_flux2_file_set() {
        let files = vec![
            "flux2-klein.safetensors".to_string(),
            "qwen3-8b.safetensors".to_string(),
            "flux2-vae.safetensors".to_string(),
        ];
        let (recipe, _d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "flux2");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib recipes::tests::flux2`
Expected: FAIL — `recipe_for("flux2")` returns `None` (panics on `.expect`).

- [ ] **Step 3: Add the recipe**

In `recipes()`, immediately before the `custom` `ModelRecipe { ... }` entry, insert:

```rust
        ModelRecipe {
            family: "flux2",
            name: "FLUX.2 (klein / dev)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["flux2", "flux-2", "flux.2"]),
                role(ComponentRole::Llm, true, &["qwen3", "qwen", "llm"]),
                role(ComponentRole::Vae, true, &["vae", "ae."]),
            ],
            vae_format: Some("flux2"),
            prediction: Some("flux2_flow"),
            shared: vec![],
        },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib recipes::`
Expected: PASS — the two new tests plus all existing `recipes` tests (incl. `recipe_table_integrity`, `detect_best_picks_flux_for_flux_files`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recipes.rs
git commit -m "feat(recipes): add flux2 family (Qwen3-8B via --llm, flux2 vae/prediction)"
```

### Task 2: Add the pure fit-estimate module

**Files:**
- Create: `src-tauri/src/fit.rs`
- Modify: `src-tauri/src/lib.rs` — add `mod fit;` next to the other `mod` declarations (line 6-7 area, keep alphabetical-ish: after `mod engine;`).
- Test: `src-tauri/src/fit.rs` (inline `#[cfg(test)] mod tests`).

Heuristic from spec §7.2: `estimate_vram_mb(bytes) ≈ (bytes / 1_048_576) * 1.15 + ACTIVATION_BUDGET_MB`, `ACTIVATION_BUDGET_MB = 1500`. Verdict thresholds: `est ≤ 0.9×VRAM` → Fits; `est ≤ VRAM` → Tight; `est > VRAM` → WontFit; size or VRAM unknown → Unknown (mirrors `catalog::rate`'s `None` path).

- [ ] **Step 1: Write the module with a failing test**

Create `src-tauri/src/fit.rs`:

```rust
//! Pure VRAM fit estimation for a model file. No I/O — fully unit-testable.
//! The estimate is deliberately rough and always surfaced to the user as an
//! *estimate*; `ACTIVATION_BUDGET_MB` is the single tunable constant.

use serde::Serialize;

/// Headroom (MB) for activations/working buffers on top of weight bytes.
pub const ACTIVATION_BUDGET_MB: u64 = 1500;

/// Rough peak VRAM (MB) needed to run a model whose on-GPU weights are
/// `file_size_bytes`. `weights_mb * 1.15 + activation budget`.
pub fn estimate_vram_mb(file_size_bytes: u64) -> u64 {
    let weights_mb = file_size_bytes as f64 / 1_048_576.0;
    (weights_mb * 1.15) as u64 + ACTIVATION_BUDGET_MB
}

/// Whether a model is expected to fit the selected device's VRAM.
/// Reuses the suitability vocabulary for UI consistency with `catalog::rate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FitVerdict {
    Fits,
    Tight,
    WontFit,
    Unknown,
}

/// Rate an estimated size against detected VRAM. `None` for either input →
/// `Unknown` (frontend shows size only, no verdict).
pub fn fit_verdict(file_size_bytes: Option<u64>, vram_total_mb: Option<u64>) -> FitVerdict {
    match (file_size_bytes, vram_total_mb) {
        (Some(bytes), Some(vram)) => {
            let est = estimate_vram_mb(bytes) as f64;
            if est <= 0.9 * vram as f64 {
                FitVerdict::Fits
            } else if est <= vram as f64 {
                FitVerdict::Tight
            } else {
                FitVerdict::WontFit
            }
        }
        _ => FitVerdict::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1_048_576;

    #[test]
    fn estimate_adds_overhead_and_budget() {
        // 1000 MB of weights → 1000*1.15 + 1500 = 2650.
        assert_eq!(estimate_vram_mb(1000 * MB), 2650);
    }

    #[test]
    fn verdict_fits_when_well_under_vram() {
        // est(1000MB) = 2650; 0.9 * 4096 = 3686.4 → Fits.
        assert_eq!(fit_verdict(Some(1000 * MB), Some(4096)), FitVerdict::Fits);
    }

    #[test]
    fn verdict_tight_between_ninety_percent_and_full() {
        // est(2500MB) = 2875+1500 = 4375; VRAM 4600: 0.9*4600=4140 < 4375 <= 4600 → Tight.
        assert_eq!(fit_verdict(Some(2500 * MB), Some(4600)), FitVerdict::Tight);
    }

    #[test]
    fn verdict_wont_fit_when_over_vram() {
        // est(6000MB) = 6900+1500 = 8400 > 8192 → WontFit.
        assert_eq!(fit_verdict(Some(6000 * MB), Some(8192)), FitVerdict::WontFit);
    }

    #[test]
    fn verdict_unknown_when_vram_or_size_missing() {
        assert_eq!(fit_verdict(Some(1000 * MB), None), FitVerdict::Unknown);
        assert_eq!(fit_verdict(None, Some(8192)), FitVerdict::Unknown);
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/lib.rs`, add after `mod engine;` (line 7):

```rust
mod fit;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib fit::`
Expected: PASS — all five `fit` tests.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/fit.rs src-tauri/src/lib.rs
git commit -m "feat(fit): pure VRAM fit-estimate heuristic + verdict"
```

---

## Part B — Low-VRAM offload mode

### Task 3: Persist `AppConfig.low_vram`

**Files:**
- Modify: `src-tauri/src/types.rs` — add the field to the `AppConfig` struct.
- Modify: `src-tauri/src/config.rs` — add `low_vram: false` to `default_config()`; add a backward-compat test.
- Test: `src-tauri/src/config.rs` (existing `#[cfg(test)] mod tests`).

Old config.json files predate this key, so it MUST be `#[serde(default)]` (→ `false`) and present in `default_config()` or `save_then_load_round_trips` fails.

- [ ] **Step 1: Write the failing backward-compat test**

Add to `mod tests` in `src-tauri/src/config.rs`:

```rust
    #[test]
    fn old_config_without_low_vram_defaults_to_false() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-lv-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no low_vram key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.low_vram, "missing low_vram must default to false");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn low_vram_round_trips() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-lv2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.low_vram = true;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert!(back.low_vram);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib config::tests::low_vram`
Expected: FAIL — compile error: no field `low_vram` on `AppConfig`.

- [ ] **Step 3: Add the field to `AppConfig`**

In `src-tauri/src/types.rs`, in the `AppConfig` struct, add the field alongside the other `#[serde(default)]` optional fields (next to `civitai_token`). Add:

```rust
    /// Low-VRAM offload mode: page weights from RAM + tiled VAE + flash attention
    /// so models larger than VRAM can run (slower). Old configs default to false.
    #[serde(default)]
    pub low_vram: bool,
```

- [ ] **Step 4: Add the default**

In `src-tauri/src/config.rs`, in `default_config()`, add before `last_request:` (after `civitai_token: None,`):

```rust
        low_vram: false,
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib config::`
Expected: PASS — the two new tests plus all existing round-trip/backward-compat tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/config.rs
git commit -m "feat(config): persist low_vram toggle (serde default false)"
```

### Task 4: Add `EngineOptions` and offload flags to `build_args`

**Files:**
- Modify: `src-tauri/src/command_builder.rs` — add `EngineOptions`; add an `opts` param to `build_args`; append offload flags; update existing test call sites.
- Test: `src-tauri/src/command_builder.rs` (existing `#[cfg(test)] mod tests`).

Spec §7.3: when Low-VRAM is on, append `--offload-to-cpu`, `--vae-tiling`, `--diffusion-fa` (all confirmed in `fixtures/sd-help.txt`) after the model/backend flags. Passing options via a struct (not a bare bool) keeps room for the deferred expert controls without another signature churn.

- [ ] **Step 1: Update existing tests + add the new ones (they will fail to compile first)**

In `src-tauri/src/command_builder.rs` `mod tests`, the `build_args` calls currently pass three args. Add `EngineOptions::default()` as the fourth arg to every existing call (there are calls in `includes_core_flags_and_values`, `uses_img_gen_mode`, `omits_negative_prompt_when_empty`, `appends_backend_when_some`, `omits_backend_when_none`, `single_file_emits_dash_m_and_no_diffusion_model`, `multi_file_maps_each_role_to_its_flag`, `multi_file_omits_absent_optional_roles`, `output_path_extension_passes_through_verbatim`). Example — change:

```rust
        let args = build_args(&sample(), "/out/x.png", None);
```
to:
```rust
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions::default());
```

Then add two new tests:

```rust
    #[test]
    fn low_vram_appends_offload_flags() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: true });
        assert!(args.iter().any(|x| x == "--offload-to-cpu"));
        assert!(args.iter().any(|x| x == "--vae-tiling"));
        assert!(args.iter().any(|x| x == "--diffusion-fa"));
    }

    #[test]
    fn low_vram_off_omits_offload_flags() {
        let args = build_args(&sample(), "/out/x.png", None, EngineOptions { low_vram: false });
        for flag in ["--offload-to-cpu", "--vae-tiling", "--diffusion-fa"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib command_builder::`
Expected: FAIL — compile error: `EngineOptions` not found / arity mismatch on `build_args`.

- [ ] **Step 3: Add `EngineOptions` and extend `build_args`**

In `src-tauri/src/command_builder.rs`, after the `use` line at the top add:

```rust
/// Engine knobs that aren't part of the generation request itself. A struct
/// (not a bare bool) leaves room for the deferred expert controls (--max-vram,
/// --stream-layers, per-component --backend) without another signature churn.
#[derive(Debug, Clone, Copy, Default)]
pub struct EngineOptions {
    pub low_vram: bool,
}
```

Change the signature to:

```rust
pub fn build_args(
    req: &GenerationRequest,
    output_path: &str,
    backend: Option<&str>,
    opts: EngineOptions,
) -> Vec<String> {
```

Then, immediately before the final `a.push("-v".into());` line, insert:

```rust
    if opts.low_vram {
        // Weights paged from RAM, tiled VAE decode, flash attention — the
        // low-UI/high-headroom bundle so models larger than VRAM can run.
        a.push("--offload-to-cpu".into());
        a.push("--vae-tiling".into());
        a.push("--diffusion-fa".into());
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib command_builder::`
Expected: PASS — all existing tests plus the two new offload tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/command_builder.rs
git commit -m "feat(engine): EngineOptions + low-VRAM offload flags in build_args"
```

### Task 5: Thread `low_vram` from config through generation

**Files:**
- Modify: `src-tauri/src/engine.rs` — `run_generation` gains `opts: EngineOptions`, forwarded to `build_args`.
- Modify: `src-tauri/src/commands.rs` — `generate` builds `EngineOptions` from `cfg.low_vram` and passes it.

`run_generation` has no unit test (it spawns a child process); correctness here is covered by `build_args` tests + `cargo build`. This task is a pure wiring change.

- [ ] **Step 1: Extend `run_generation`**

In `src-tauri/src/engine.rs`:

- Add to the top `use` for `command_builder`: change `use crate::command_builder::build_args;` to `use crate::command_builder::{build_args, EngineOptions};`
- Add a parameter to the signature (after `backend`):

```rust
pub fn run_generation<F: FnMut(ProgressUpdate)>(
    binary: &Path,
    req: &GenerationRequest,
    output_path: &Path,
    backend: Option<&str>,
    opts: EngineOptions,
    slot: &ChildSlot,
    mut on_progress: F,
) -> Result<Vec<i64>, GenError> {
```

- Update the `build_args` call (line ~47):

```rust
    let args = build_args(req, &output_path.to_string_lossy(), backend, opts);
```

- [ ] **Step 2: Pass options from `generate`**

In `src-tauri/src/commands.rs`, in `generate`, after `let backend_owned = backend;` (line ~175) add:

```rust
    let engine_opts = crate::command_builder::EngineOptions { low_vram: cfg.low_vram };
```

Then update the `run_generation` call inside `spawn_blocking` (line ~179) to pass it after `backend_owned.as_deref()`:

```rust
        engine::run_generation(&binary, &req, &img, backend_owned.as_deref(), engine_opts, &slot, |p| {
            let _ = app2.emit("generation:progress", p);
        })
```

- [ ] **Step 3: Verify it compiles and the suite passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS — whole backend suite; no `run_generation` arity errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/engine.rs src-tauri/src/commands.rs
git commit -m "feat(engine): thread low_vram config into run_generation"
```

### Task 6: Low-VRAM toggle in Preferences (frontend)

**Files:**
- Modify: `src/lib/types.ts` — add `low_vram: boolean` to `AppConfig`.
- Modify: `src/lib/components/PreferencesDialog.svelte` — a checkbox in the Hardware section, beside `DevicePicker`.

No frontend unit-test harness exists (spec §7.5); the gate is `npm run check` (0 errors) + manual. Settings persist through the existing `setSettings` optimistic pattern.

- [ ] **Step 1: Add the type field**

In `src/lib/types.ts`, in the `AppConfig` interface, after `civitai_token: string | null;` add:

```ts
  // Low-VRAM offload mode (mirrors Rust AppConfig.low_vram, #[serde(default)] →
  // false for old configs). When on, generation pages weights from RAM.
  low_vram: boolean;
```

- [ ] **Step 2: Add the toggle to the Hardware section**

In `src/lib/components/PreferencesDialog.svelte`, replace the Hardware `<section>` (lines 93-96) with:

```svelte
    <section class="grp">
      <div class="grp-hdr">Hardware</div>
      <DevicePicker />
      <label class="lowvram">
        <input
          type="checkbox"
          checked={$settings?.low_vram ?? false}
          disabled={!$settings}
          onchange={(e) => saveLowVram(e.currentTarget.checked)} />
        <span>Low-VRAM mode <em>(slower; fits bigger models)</em></span>
      </label>
    </section>
```

Then add the handler inside `<script>` (after `persistToken`, before the closing `</script>`):

```ts
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
```

Add the style rule in the `<style>` block:

```css
  .lowvram { display:flex; align-items:center; gap:.4rem; font-size:.75rem; padding:.35rem .2rem 0; }
  .lowvram em { opacity:.6; font-style:italic; }
```

- [ ] **Step 3: Verify the frontend type-checks**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 4: Manual verification**

Run `npm run tauri dev`, open Preferences (⚙) → Hardware. Toggle "Low-VRAM mode", close and reopen Preferences → the checkbox stays checked (persisted to config.json).

- [ ] **Step 5: Commit**

```bash
git add src/lib/types.ts src/lib/components/PreferencesDialog.svelte
git commit -m "feat(ui): Low-VRAM mode toggle in Preferences → Hardware"
```

---

## Part C — HuggingFace variant discovery & picker

### Task 7: `hf.rs` — URL classification (pure)

**Files:**
- Create: `src-tauri/src/hf.rs` (parse portion + helpers).
- Modify: `src-tauri/src/lib.rs` — add `mod hf;` after `mod gallery;` (line 8).
- Test: `src-tauri/src/hf.rs` (inline `#[cfg(test)] mod tests`).

Spec §7.1: classify the pasted string into a repo (enumerate) or a direct file (skip enumeration; that file is the diffusion component). Junk → `None`.

- [ ] **Step 1: Create the module with parsing + failing tests**

Create `src-tauri/src/hf.rs`:

```rust
//! HuggingFace model discovery: URL classification, tree-API parse, quant-label
//! extraction, and variant grouping. All grouping/parsing logic is pure and
//! unit-tested; only `fetch_tree` performs I/O (thin `ureq` wrapper).

/// A repo coordinate on huggingface.co.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfRepoRef {
    pub org: String,
    pub repo: String,
    pub revision: String,
}

/// What a pasted HuggingFace URL points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HfUrl {
    /// A repo (or tree) URL → enumerate its files.
    Repo(HfRepoRef),
    /// A direct file URL (blob/resolve) → skip enumeration; this IS the file.
    File { repo: HfRepoRef, path: String },
}

/// The last path segment (filename) of a repo-relative path.
pub fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// Filename without its final extension.
pub fn stem(path: &str) -> String {
    let b = basename(path);
    match b.rfind('.') {
        Some(i) => b[..i].to_string(),
        None => b,
    }
}

/// Classify a pasted string. Pure. `None` when it isn't a huggingface.co URL.
pub fn parse_hf_url(url: &str) -> Option<HfUrl> {
    let rest = url.trim();
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))
        .unwrap_or(rest);
    let rest = rest.strip_prefix("huggingface.co/")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    let org = segs[0].to_string();
    let repo = segs[1].to_string();
    let main = || "main".to_string();
    if segs.len() == 2 {
        return Some(HfUrl::Repo(HfRepoRef { org, repo, revision: main() }));
    }
    match segs[2] {
        "tree" => {
            let revision = segs.get(3).map(|s| s.to_string()).unwrap_or_else(main);
            Some(HfUrl::Repo(HfRepoRef { org, repo, revision }))
        }
        "blob" | "resolve" => {
            let revision = segs.get(3).map(|s| s.to_string()).unwrap_or_else(main);
            let path = segs.get(4..).map(|s| s.join("/")).unwrap_or_default();
            if path.is_empty() {
                Some(HfUrl::Repo(HfRepoRef { org, repo, revision }))
            } else {
                Some(HfUrl::File { repo: HfRepoRef { org, repo, revision }, path })
            }
        }
        _ => Some(HfUrl::Repo(HfRepoRef { org, repo, revision: main() })),
    }
}

/// Absolute download URL for a repo-relative file path.
pub fn resolve_url(repo: &HfRepoRef, path: &str) -> String {
    format!(
        "https://huggingface.co/{}/{}/resolve/{}/{}",
        repo.org, repo.repo, repo.revision, path
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_repo_url() {
        let u = parse_hf_url("https://huggingface.co/black-forest-labs/FLUX.1-dev").unwrap();
        assert_eq!(
            u,
            HfUrl::Repo(HfRepoRef {
                org: "black-forest-labs".into(),
                repo: "FLUX.1-dev".into(),
                revision: "main".into(),
            })
        );
    }

    #[test]
    fn parses_tree_url_with_revision() {
        let u = parse_hf_url("https://huggingface.co/org/repo/tree/refs%2Fpr%2F1/sub").unwrap();
        assert_eq!(u, HfUrl::Repo(HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "refs%2Fpr%2F1".into() }));
    }

    #[test]
    fn parses_resolve_file_url() {
        let u = parse_hf_url("https://huggingface.co/org/repo/resolve/main/flux1-dev.safetensors").unwrap();
        assert_eq!(
            u,
            HfUrl::File {
                repo: HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "main".into() },
                path: "flux1-dev.safetensors".into(),
            }
        );
    }

    #[test]
    fn rejects_non_hf_url() {
        assert!(parse_hf_url("https://example.com/foo/bar").is_none());
        assert!(parse_hf_url("not a url").is_none());
        assert!(parse_hf_url("https://huggingface.co/onlyorg").is_none());
    }

    #[test]
    fn resolve_url_builds_download_link() {
        let r = HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "main".into() };
        assert_eq!(resolve_url(&r, "a/b.safetensors"), "https://huggingface.co/org/repo/resolve/main/a/b.safetensors");
    }

    #[test]
    fn basename_and_stem() {
        assert_eq!(basename("a/b/c.safetensors"), "c.safetensors");
        assert_eq!(stem("a/b/c.safetensors"), "c");
        assert_eq!(stem("noext"), "noext");
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/lib.rs`, add after `mod gallery;` (line 8):

```rust
mod hf;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib hf::`
Expected: PASS — all six parse/helper tests. (A `dead_code` warning on `resolve_url`/`HfUrl::File` is fine until Task 10 consumes them.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/hf.rs src-tauri/src/lib.rs
git commit -m "feat(hf): HuggingFace URL classification (pure)"
```

### Task 8: `hf.rs` — `precision_label` (pure)

**Files:**
- Modify: `src-tauri/src/hf.rs` — add `precision_label`.
- Test: `src-tauri/src/hf.rs` (`mod tests`).

Spec §7.1: extract a quant label from a filename (`fp16`, `bf16`, `fp8_e4m3fn`, `fp8_e5m2`, `q8_0`, `q6_k`, `q4_k_m`, `q4_0`, …), `None` when nothing matches. Order matters: test the most specific tokens first so `q4_k_m` wins over `q4`.

- [ ] **Step 1: Add failing tests**

Add to `mod tests` in `src-tauri/src/hf.rs`:

```rust
    #[test]
    fn precision_label_extracts_known_tokens() {
        assert_eq!(precision_label("flux1-dev-fp8_e4m3fn.safetensors"), Some("fp8_e4m3fn".into()));
        assert_eq!(precision_label("model-fp16.safetensors"), Some("fp16".into()));
        assert_eq!(precision_label("model-bf16.safetensors"), Some("bf16".into()));
        assert_eq!(precision_label("t5-Q4_K_M.safetensors"), Some("q4_k_m".into()));
        assert_eq!(precision_label("x-q8_0.safetensors"), Some("q8_0".into()));
    }

    #[test]
    fn precision_label_none_when_absent() {
        assert_eq!(precision_label("flux1-dev.safetensors"), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib hf::tests::precision_label`
Expected: FAIL — `precision_label` not found.

- [ ] **Step 3: Implement `precision_label`**

Add to `src-tauri/src/hf.rs` (after `resolve_url`):

```rust
/// Canonical quant/precision tokens, most-specific first so `q4_k_m` matches
/// before `q4`. Returned lowercased for stable display.
const PRECISION_TOKENS: &[&str] = &[
    "fp8_e4m3fn", "fp8_e5m2", "fp32", "bf16", "fp16", "fp8",
    "q8_0", "q6_k", "q5_k_m", "q5_k", "q4_k_m", "q4_k", "q4_0", "q3_k", "q2_k",
    "q8", "q6", "q5", "q4", "q3", "q2", "int8",
];

/// Extract a quant/precision label from a filename, or `None` if none is found.
pub fn precision_label(filename: &str) -> Option<String> {
    let lower = filename.to_lowercase();
    PRECISION_TOKENS
        .iter()
        .find(|t| lower.contains(*t))
        .map(|t| t.to_string())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib hf::`
Expected: PASS — the two new tests plus the Task 7 tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/hf.rs
git commit -m "feat(hf): precision_label quant-token extraction (pure)"
```

### Task 9: `hf.rs` — tree parse + variant grouping (pure) with fixture

**Files:**
- Create: `src-tauri/fixtures/hf-tree-flux1.json` — captured tree-API response.
- Modify: `src-tauri/src/hf.rs` — `HfTreeEntry`, `parse_tree_json`, `HfVariant`, `classify_variants`.
- Test: `src-tauri/src/hf.rs` (`mod tests`).

Spec §7.1/§7.5: parse the tree JSON (`lfs.size ?? size`, files only, keep `.safetensors`); tag each file's role via the recipe patterns; the **diffusion-role files are the variant list** (companion files like a VAE are excluded). A file is a diffusion variant iff it matches the family's Diffusion patterns AND matches no companion-role pattern (so `flux2-vae` is never a "variant" of the transformer).

- [ ] **Step 1: Create the fixture**

Create `src-tauri/fixtures/hf-tree-flux1.json`:

```json
[
  {"type": "directory", "path": "vae", "size": 0},
  {"type": "file", "path": "flux1-dev.safetensors", "size": 52, "lfs": {"size": 23802932552}},
  {"type": "file", "path": "flux1-dev-fp8.safetensors", "size": 52, "lfs": {"size": 11901466276}},
  {"type": "file", "path": "ae.safetensors", "size": 52, "lfs": {"size": 335304388}},
  {"type": "file", "path": "model_index.json", "size": 320}
]
```

- [ ] **Step 2: Add failing tests**

Add to `mod tests` in `src-tauri/src/hf.rs`:

```rust
    #[test]
    fn parse_tree_json_normalizes_lfs_size_and_drops_dirs() {
        let body = include_str!("../fixtures/hf-tree-flux1.json");
        let entries = parse_tree_json(body).unwrap();
        // Directory dropped; 4 files kept.
        assert_eq!(entries.len(), 4);
        let diff = entries.iter().find(|e| e.path == "flux1-dev.safetensors").unwrap();
        assert_eq!(diff.size_bytes, 23802932552); // from lfs.size, not the 52-byte pointer
        let idx = entries.iter().find(|e| e.path == "model_index.json").unwrap();
        assert_eq!(idx.size_bytes, 320); // no lfs → plain size
    }

    #[test]
    fn classify_variants_lists_diffusion_files_only() {
        let body = include_str!("../fixtures/hf-tree-flux1.json");
        let entries = parse_tree_json(body).unwrap();
        let variants = classify_variants(&entries);
        // ae.safetensors is a VAE companion → excluded; model_index.json isn't
        // safetensors → excluded. Two diffusion variants remain.
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().all(|v| v.family.as_deref() == Some("flux1")));
        let labels: Vec<&str> = variants.iter().map(|v| v.label.as_str()).collect();
        assert!(labels.contains(&"flux1-dev")); // no precision token → stem
        assert!(labels.contains(&"fp8"));
        let fp8 = variants.iter().find(|v| v.label == "fp8").unwrap();
        assert_eq!(fp8.path, "flux1-dev-fp8.safetensors");
        assert_eq!(fp8.size_bytes, 11901466276);
    }

    #[test]
    fn classify_variants_falls_back_to_all_safetensors_when_family_unknown() {
        let entries = vec![
            HfTreeEntry { path: "mystery-model.safetensors".into(), size_bytes: 100 },
            HfTreeEntry { path: "readme.md".into(), size_bytes: 5 },
        ];
        let variants = classify_variants(&entries);
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].family, None);
        assert_eq!(variants[0].path, "mystery-model.safetensors");
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib hf::tests::classify`
Expected: FAIL — `parse_tree_json` / `classify_variants` / `HfTreeEntry` not found.

- [ ] **Step 4: Implement parse + grouping**

Add to the top `use` section of `src-tauri/src/hf.rs`:

```rust
use crate::recipes::{self, ComponentRole};
use serde::{Deserialize, Serialize};
```

Add the types + functions (after `precision_label`):

```rust
/// One file from the HF tree API, size already normalized (lfs.size ?? size).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfTreeEntry {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Deserialize)]
struct RawTreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<RawLfs>,
}

#[derive(Deserialize)]
struct RawLfs {
    #[serde(default)]
    size: u64,
}

/// Parse the tree-API JSON array into normalized file entries (dirs dropped).
pub fn parse_tree_json(body: &str) -> Result<Vec<HfTreeEntry>, String> {
    let raw: Vec<RawTreeEntry> = serde_json::from_str(body).map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .filter(|e| e.kind == "file")
        .map(|e| {
            let size_bytes = match &e.lfs {
                Some(l) if l.size > 0 => l.size,
                _ => e.size,
            };
            HfTreeEntry { path: e.path, size_bytes }
        })
        .collect())
}

/// One selectable diffusion variant within a repo.
#[derive(Debug, Clone, Serialize)]
pub struct HfVariant {
    /// Quant label (e.g. "fp8") or, if none, the filename stem.
    pub label: String,
    /// Detected family (None when no recipe matched the file set).
    pub family: Option<String>,
    /// Repo-relative path of the diffusion file.
    pub path: String,
    pub size_bytes: u64,
}

/// True if `lower` (a lowercased filename) contains any of `patterns`.
fn matches_any(patterns: &[&str], lower: &str) -> bool {
    patterns.iter().any(|p| lower.contains(&p.to_lowercase()))
}

/// Group the tree's `.safetensors` files into the diffusion-variant list.
/// The detected family (via `recipes::detect_best`) determines which files are
/// diffusion (kept) vs companion encoders/VAE (excluded). With no family match,
/// every `.safetensors` is offered as a variant.
pub fn classify_variants(entries: &[HfTreeEntry]) -> Vec<HfVariant> {
    let files: Vec<&HfTreeEntry> = entries
        .iter()
        .filter(|e| e.path.to_lowercase().ends_with(".safetensors"))
        .collect();
    if files.is_empty() {
        return Vec::new();
    }
    let names: Vec<String> = files.iter().map(|e| basename(&e.path)).collect();
    let detected = recipes::detect_best(&names);
    let family = detected.as_ref().map(|(r, _)| r.family.to_string());

    // Split the recipe's role patterns into diffusion vs companion sets.
    let (diffusion_patterns, companion_patterns): (Vec<&str>, Vec<&str>) = match &detected {
        Some((recipe, _)) => {
            let mut diff = Vec::new();
            let mut comp = Vec::new();
            for spec in &recipe.roles {
                if spec.role == ComponentRole::Diffusion {
                    diff.extend(spec.patterns.iter().copied());
                } else {
                    comp.extend(spec.patterns.iter().copied());
                }
            }
            (diff, comp)
        }
        None => (Vec::new(), Vec::new()),
    };

    files
        .iter()
        .filter(|e| {
            let lower = basename(&e.path).to_lowercase();
            match &detected {
                // Family known: diffusion file = matches a diffusion pattern and
                // no companion pattern (so a VAE/encoder is never a "variant").
                Some(_) => matches_any(&diffusion_patterns, &lower) && !matches_any(&companion_patterns, &lower),
                // No family: offer every safetensors.
                None => true,
            }
        })
        .map(|e| HfVariant {
            label: precision_label(&basename(&e.path)).unwrap_or_else(|| stem(&e.path)),
            family: family.clone(),
            path: e.path.clone(),
            size_bytes: e.size_bytes,
        })
        .collect()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib hf::`
Expected: PASS — the three new tests plus all earlier `hf` tests.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/hf.rs src-tauri/fixtures/hf-tree-flux1.json
git commit -m "feat(hf): tree-API parse + diffusion-variant grouping (pure)"
```

### Task 10: `fetch_tree` + `list_hf_variants` command + frontend bindings

**Files:**
- Modify: `src-tauri/src/hf.rs` — `fetch_tree` (ureq) + `RatedHfVariant` + `rate_variant`.
- Modify: `src-tauri/src/commands.rs` — `list_hf_variants` command.
- Modify: `src-tauri/src/lib.rs` — register `commands::list_hf_variants`.
- Modify: `src/lib/types.ts` — `FitVerdict`, `RatedHfVariant`.
- Modify: `src/lib/api.ts` — `listHfVariants` wrapper.

`fetch_tree` does network I/O (no unit test; covered by `cargo build` + manual). Rating reuses `fit::fit_verdict` on the diffusion file size; a direct-file URL has unknown size (0) → `Unknown` verdict (size-only), matching spec §7.2's `None` path.

- [ ] **Step 1: Add `fetch_tree`, `RatedHfVariant`, `rate_variant`**

Append to `src-tauri/src/hf.rs`:

```rust
/// A variant enriched with a download URL + fit verdict, ready for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct RatedHfVariant {
    pub label: String,
    pub family: Option<String>,
    pub url: String,
    pub size_bytes: u64,
    pub verdict: crate::fit::FitVerdict,
}

/// Attach a resolve URL + fit verdict to a variant. Size 0 (direct-file, size
/// unknown until download) → `Unknown` verdict (size-only in the UI).
pub fn rate_variant(repo: &HfRepoRef, v: &HfVariant, vram_total_mb: Option<u64>) -> RatedHfVariant {
    let size = if v.size_bytes > 0 { Some(v.size_bytes) } else { None };
    RatedHfVariant {
        label: v.label.clone(),
        family: v.family.clone(),
        url: resolve_url(repo, &v.path),
        size_bytes: v.size_bytes,
        verdict: crate::fit::fit_verdict(size, vram_total_mb),
    }
}

/// Fetch and parse a repo's file tree. Reuses the stored HF token as a bearer
/// header for gated repos (mirrors `downloader.rs`). Returns a user-facing
/// error string on any failure.
pub fn fetch_tree(repo: &HfRepoRef, token: &str) -> Result<Vec<HfTreeEntry>, String> {
    let url = format!(
        "https://huggingface.co/api/models/{}/{}/tree/{}?recursive=true",
        repo.org, repo.repo, repo.revision
    );
    let mut req = ureq::get(&url);
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    match req.call() {
        Ok(resp) => {
            let body = resp.into_string().map_err(|e| e.to_string())?;
            parse_tree_json(&body)
        }
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err("This repo is gated — add a HuggingFace token in Preferences (⚙).".into())
        }
        Err(ureq::Error::Status(404, _)) => Err("Repo not found on HuggingFace.".into()),
        Err(ureq::Error::Status(code, _)) => Err(format!("HuggingFace returned HTTP {code}.")),
        Err(ureq::Error::Transport(t)) => Err(format!("Network error reaching HuggingFace: {t}")),
    }
}
```

- [ ] **Step 2: Add the command**

In `src-tauri/src/commands.rs`, add (near `multifile_catalog`, and ensure `use crate::hf;` — add it to the module's imports if not present):

```rust
/// Discover a HuggingFace model's downloadable variants, each with a size + fit
/// verdict against the given VRAM. Repo URLs enumerate the tree; a direct file
/// URL yields a single-row picker (size unknown until download).
#[tauri::command]
pub fn list_hf_variants(
    url: String,
    token: String,
    vram_total_mb: Option<u64>,
) -> Result<Vec<hf::RatedHfVariant>, String> {
    let parsed = hf::parse_hf_url(&url)
        .ok_or_else(|| "Not a HuggingFace URL. Paste a huggingface.co repo or file link.".to_string())?;
    match parsed {
        hf::HfUrl::File { repo, path } => {
            let name = hf::basename(&path);
            let family = crate::recipes::detect_best(&[name.clone()]).map(|(r, _)| r.family.to_string());
            let variant = hf::HfVariant {
                label: hf::precision_label(&name).unwrap_or_else(|| hf::stem(&path)),
                family,
                path,
                size_bytes: 0, // unknown until download; verdict → size-only
            };
            Ok(vec![hf::rate_variant(&repo, &variant, vram_total_mb)])
        }
        hf::HfUrl::Repo(repo) => {
            let entries = hf::fetch_tree(&repo, &token)?;
            let variants = hf::classify_variants(&entries);
            if variants.is_empty() {
                return Err("No .safetensors models found in that repo. Paste a direct file URL instead.".into());
            }
            Ok(variants.iter().map(|v| hf::rate_variant(&repo, v, vram_total_mb)).collect())
        }
    }
}
```

If `commands.rs` doesn't already `use crate::hf;`, add it with the other `use crate::...;` imports at the top.

- [ ] **Step 3: Register the command**

In `src-tauri/src/lib.rs`, add to the `tauri::generate_handler![...]` list (after `commands::broken_definitions,` on line 93):

```rust
            commands::list_hf_variants,
```

- [ ] **Step 4: Verify the backend compiles + full suite passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS — whole suite; no unused-code warnings for `hf` now that the command consumes it.

- [ ] **Step 5: Add the frontend type + API wrapper**

In `src/lib/types.ts`, add near the other wire-enum types (after `Suitability`):

```ts
// Wire values MUST match the Rust `FitVerdict` enum's serde snake_case form
// (src-tauri/src/fit.rs).
export type FitVerdict = "fits" | "tight" | "wont_fit" | "unknown";

// Mirrors Rust `hf::RatedHfVariant`. One selectable diffusion variant + fit.
export interface RatedHfVariant {
  label: string;
  family: string | null;
  url: string;
  size_bytes: number;
  verdict: FitVerdict;
}
```

In `src/lib/api.ts`, add `RatedHfVariant` to the type import on line 3, then add the wrapper after `brokenDefinitions`:

```ts
export const listHfVariants = (url: string, token: string, vramTotalMb: number | null) =>
  invoke<RatedHfVariant[]>("list_hf_variants", { url, token, vramTotalMb });
```

- [ ] **Step 6: Verify the frontend type-checks**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/hf.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src/lib/types.ts src/lib/api.ts
git commit -m "feat(hf): list_hf_variants command + frontend bindings"
```

### Task 11: Variant picker UI in the assembly dialog

**Files:**
- Modify: `src/lib/components/ModelAssembly.svelte` — add an `"hf"` mode: paste an HF URL, list variants with size + fit badge, pick one to download the diffusion file and pre-fill the manual assembly form.

Flow (spec §7.1): the diffusion file the user picks is downloaded via the existing `download_model` command (returns a local path), then the dialog drops into the existing `"manual"` mode with the diffusion slot filled and the detected family applied — companions are assigned via the recipe/manual flow already built. No frontend test harness (§7.5); gate is `npm run check` + manual.

- [ ] **Step 1: Extend imports, `Mode`, and state**

In `src/lib/components/ModelAssembly.svelte`:

- Line 2 — add `downloadModel` and `listHfVariants` to the api import:
```ts
  import { listRecipes, detectFolder, pickFolder, pickModelFile, saveModelDefinition, multifileCatalog, downloadModel, listHfVariants } from "../api";
```
- Line 3 — add `sysStats` to the stores import:
```ts
  import { startMultiFileDownload, downloadStatus, settings, sysStats } from "../stores";
```
- Line 5 — add `RatedHfVariant`, `FitVerdict` to the type import:
```ts
  import type { RecipeInfo, ComponentRole, ModelComponents, ModelDefinition, RatedMultiFile, RatedHfVariant, FitVerdict } from "../types";
```
- Line 15 — add `"hf"` to the `Mode` union:
```ts
  type Mode = "choose" | "folder" | "manual" | "catalog" | "hf";
```
- After the catalog flow state (line 30), add:
```ts
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
```

- [ ] **Step 2: Add the picker functions**

After `openCatalog` (line 94), add:

```ts
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
```

- [ ] **Step 3: Add the entry button + the `hf` view**

In the template, add a button to the `mode === "choose"` block (after the catalog button, line 172):

```svelte
      <button class="btn-secondary" onclick={openHf}>Import from a HuggingFace URL</button>
```

Then add a new branch immediately after the `{:else if mode === "catalog"}` block closes (after its `{/if}` on line 197, before the final `{:else}`):

```svelte
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
```

- [ ] **Step 4: Verify the frontend type-checks**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 5: Manual verification**

Run `npm run tauri dev`, open Model Library → "＋ New multi-file model…" → "Import from a HuggingFace URL". Paste `https://huggingface.co/black-forest-labs/FLUX.1-dev` → "Find variants" lists the diffusion variants (not the VAE) with sizes + a fit badge. (Downloading a full variant is a large real download; verify the list + badges render and the flow advances to manual assembly on selection.)

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/ModelAssembly.svelte
git commit -m "feat(ui): HuggingFace variant picker in the assembly dialog"
```

---

## Self-Review

**Spec coverage:**
- §7.1 URL classification → Task 7 (`parse_hf_url`, repo vs file vs junk). ✓
- §7.1 enumeration (tree API, `lfs.size ?? size`, safetensors-only, bearer token) → Tasks 9 (`parse_tree_json`) + 10 (`fetch_tree`). ✓
- §7.1 grouping via `recipes::detect` + `precision_label`; diffusion files = variant list; companions excluded; per-companion override deferred → Tasks 8 + 9. ✓
- §7.1 direct-file path (single-row picker) → Task 10 command's `HfUrl::File` arm. ✓
- §7.1 GGUF note (safetensors-only) → enforced by the `.safetensors` filter in Task 9; `.gguf` never offered. ✓
- §7.2 `estimate_vram_mb` + verdict table incl. `None` → size-only → Task 2. ✓
- §7.3 `AppConfig.low_vram` (serde default) + Preferences→Hardware toggle + three offload flags via engine-options struct + threading → Tasks 3, 4, 5, 6. ✓
- §7.3 auto-suggest on Won't-fit → surfaced in the variant picker via the `wont_fit` badge text ("try Low-VRAM mode"); the standalone on-generate auto-suggest prompt is **not** built here — see note below.
- §7.4 error handling (gated 401/403, 404, network, no safetensors, not-an-HF-URL) → Tasks 10 (`fetch_tree` + command errors). ✓
- §7.5 testing (URL classification, variant grouping from fixture JSON, `precision_label`, estimate+verdict, `build_args` low_vram on/off, serde round-trip + old-config default, `npm run check`) → Tasks 1–11 tests. ✓
- flux2 recipe family → Task 1. ✓

**Scope note (intentional deferral, flagged for the executor):** Spec §7.3's *auto-suggest* is specified as a non-blocking prompt when a selected/about-to-generate model estimates *Won't fit*. This plan delivers the estimate, the verdict, and a `wont_fit` cue inside the variant picker, but does **not** add a generate-time suggestion banner (that touches the main generate bar and model-selection state, a separate UI surface). If the reviewer wants the full auto-suggest in this pass, add a follow-up task against `GenerateBar.svelte`/model-selection; otherwise it ships as a fast follow. All other §7 requirements are covered.

**Placeholder scan:** No TBD/TODO/"handle errors appropriately"; every code step shows complete code. ✓

**Type consistency:**
- `FitVerdict` serde `snake_case` (`fits`/`tight`/`wont_fit`/`unknown`) matches the TS union in Task 10. ✓
- `EngineOptions { low_vram: bool }` — defined Task 4, consumed Tasks 4/5 with the same field name. ✓
- `build_args(req, output_path, backend, opts)` arity — changed in Task 4, all call sites (tests + `engine.rs`) updated in Tasks 4/5. ✓
- `RatedHfVariant` fields (`label`, `family`, `url`, `size_bytes`, `verdict`) identical in Rust (Task 10) and TS (Task 10). ✓
- `run_generation(binary, req, output_path, backend, opts, slot, on_progress)` — new param order fixed in Task 5, matching the single caller in `commands.rs`. ✓

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-17-multifile-variants-and-low-vram.md`. Two execution options:

**1. Subagent-Driven (recommended)** — I dispatch a fresh subagent per task, review between tasks (spec compliance, then code quality), fast iteration.

**2. Inline Execution** — Execute tasks in this session using executing-plans, batch execution with checkpoints for review.

Which approach?
