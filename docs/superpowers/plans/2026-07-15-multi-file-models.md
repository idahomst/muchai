# Multi-file (split) Model Support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let MuchAI load split models (FLUX, SD3, Qwen-Image) whose transformer, text encoders, and VAE are separate `.safetensors` files, plus a curated catalog and filename auto-assembly so the common case is "point at it, done."

**Architecture:** A model becomes a sum type `ModelRef` — either one all-in-one file (`-m`) or typed component files (`--diffusion-model` + friends). Illegal states are unrepresentable. Recipes describe model families and recognize their component files by filename. Three entry flows (catalog download, point-at-folder auto-detect, manual assign) all converge on a saved `ModelDefinition` that appears in the Model dropdown. All decision logic lives in pure Rust functions covered by `cargo test --lib`; the UI is thin.

**Tech Stack:** Rust (Tauri v2, serde, ureq), Svelte 5 (runes), TypeScript. Engine flags confirmed against `src-tauri/fixtures/sd-help.txt`.

**Source spec:** `docs/superpowers/specs/2026-07-13-multi-file-models-design.md`

**Branch:** `feat/multi-file-models` (already checked out). We are in beta — **no config/gallery backward-compat is required**; take the cleaner data model.

---

## Execution notes

- **Phase A (Tasks 1–6, 9–12): core split-model loading.** After Phase A you can load a split model you already have (folder auto-detect + manual assembly) and generate with it. Shippable on its own.
- **Phase B (Tasks 7–8, 13): curated catalog + shared component pool.** Adds "download a Flux model, encoders fetched once, reused by the next Flux model."
- Backend tasks are TDD: write the failing test, watch it fail, implement, watch it pass, commit. Run `cargo test --lib` from `src-tauri/`.
- Frontend has **no unit-test framework** (consistent with the app). The verification gate for every frontend task is `npm run check` staying at **0 errors / 0 warnings**. Run it from the repo root.
- Commit after every task.

## File structure map

**Rust (`src-tauri/src/`):**
- `recipes.rs` — **NEW**. `ComponentRole`, `RoleSpec`, `ModelRecipe`, `SharedComponent`, `recipes()`, `detect()`, `detect_best()`, `ModelRecipe::missing_required_roles()`, plus serializable `RecipeInfo` DTO for the frontend.
- `types.rs` — add `ModelComponents`, `ModelRef`, `ModelDefinition`, `missing_components()`; change `GenerationRequest.model_path` → `model: ModelRef`; add `AppConfig.model_definitions`.
- `command_builder.rs` — `build_args` becomes a `match` on `req.model`.
- `catalog.rs` — add `MultiFileCatalogEntry`, `multi_file_catalog()`, `PlannedDownload`, `plan_downloads()`, multi-file rating.
- `models.rs` — exclude paths referenced by a saved `ModelDefinition` from the single-file scan.
- `downloader.rs` — add `download_to()` (stream to an explicit dest path); refactor `download_model()` on top of it.
- `commands.rs` + `lib.rs` — new commands (`download_multifile`, definition CRUD, `list_recipes`, `multifile_catalog`, `detect_folder`, `pick_model_files`), richer download progress; register them.

**Frontend (`src/lib/`):**
- `types.ts` — `ModelRef` discriminated union, `ModelComponents`, `ModelDefinition`, `GenerationRequest.model`, `AppConfig.model_definitions`, recipe/catalog types, richer `DownloadProgress`, `ROLE_LABELS`, `VAE_FORMATS`, `PREDICTIONS`, `modelIsSet()`, `modelLabel()`.
- `api.ts` — invoke wrappers for the new commands.
- `stores.ts` — `definitions` store; richer `DownloadStatus.active`; `startMultiFileDownload()`.
- `components/ModelLibrary.svelte` — dropdown lists single-file models + definitions (badged), broken flag, opens the assembly dialog.
- `components/ModelAssembly.svelte` — **NEW**. The three-flow assembly dialog + definition edit.
- `components/GenerateBar.svelte`, `components/ParamsPanel.svelte` — adapt to `req.model`.

---

## Task 1: Recipes module — roles, recipe table, filename detection

Pure, additive, no behavior change. `ComponentRole` lands here (per spec) and is reused everywhere else.

**Files:**
- Create: `src-tauri/src/recipes.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod recipes;`)

- [ ] **Step 1: Register the module**

In `src-tauri/src/lib.rs`, add `mod recipes;` to the module list (alongside `mod catalog;` etc., keep alphabetical-ish grouping):

```rust
mod progress_parser;
mod recipes;
mod sysmon;
```

- [ ] **Step 2: Write `recipes.rs` with the failing tests**

Create `src-tauri/src/recipes.rs`:

```rust
use crate::types::ModelComponents;
use serde::{Deserialize, Serialize};

/// A typed slot in a split model, each wired to one engine flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentRole {
    Diffusion,
    Vae,
    ClipL,
    ClipG,
    T5xxl,
    Llm,
}

/// One role's recognition rule within a recipe.
#[derive(Debug, Clone)]
pub struct RoleSpec {
    pub role: ComponentRole,
    pub required: bool,
    /// Case-insensitive substring matches on the filename, e.g. ["t5xxl", "t5-xxl"].
    pub patterns: Vec<&'static str>,
}

/// A family-common downloadable part (VAE / encoder) reused across the family.
#[derive(Debug, Clone, Serialize)]
pub struct SharedComponent {
    pub role: ComponentRole,
    pub url: &'static str,
    pub size_bytes: u64,
    /// Stable filename in the shared pool (never unique-suffixed).
    pub filename: &'static str,
}

/// One model family and how to recognize/assemble its parts.
#[derive(Debug, Clone)]
pub struct ModelRecipe {
    pub family: &'static str,
    pub name: &'static str,
    pub roles: Vec<RoleSpec>,
    pub vae_format: Option<&'static str>,
    pub prediction: Option<&'static str>,
    pub shared: Vec<SharedComponent>,
}

impl ModelRecipe {
    /// Required roles whose slot is empty (None / blank). Gates Save + generation.
    pub fn missing_required_roles(&self, c: &ModelComponents) -> Vec<ComponentRole> {
        self.roles
            .iter()
            .filter(|r| r.required)
            .filter(|r| slot(c, r.role).map(|s| s.trim().is_empty()).unwrap_or(true))
            .map(|r| r.role)
            .collect()
    }
}

/// Read the component slot for a role. Diffusion is always present (String).
fn slot(c: &ModelComponents, role: ComponentRole) -> Option<&str> {
    match role {
        ComponentRole::Diffusion => Some(c.diffusion_model.as_str()),
        ComponentRole::Vae => c.vae.as_deref(),
        ComponentRole::ClipL => c.clip_l.as_deref(),
        ComponentRole::ClipG => c.clip_g.as_deref(),
        ComponentRole::T5xxl => c.t5xxl.as_deref(),
        ComponentRole::Llm => c.llm.as_deref(),
    }
}

/// Result of running detection: at most one matched filename per role.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedComponents {
    pub assignments: Vec<(ComponentRole, String)>,
}

impl DetectedComponents {
    pub fn get(&self, role: ComponentRole) -> Option<&str> {
        self.assignments.iter().find(|(r, _)| *r == role).map(|(_, f)| f.as_str())
    }
    /// How many of the recipe's REQUIRED roles were matched (detection confidence).
    pub fn required_matched(&self, recipe: &ModelRecipe) -> usize {
        recipe
            .roles
            .iter()
            .filter(|r| r.required)
            .filter(|r| self.get(r.role).is_some())
            .count()
    }
}

/// Match filenames to this recipe's roles. Pure: no filesystem access.
/// For each role, among files matching any of its patterns (case-insensitive),
/// pick the file whose longest-matching pattern is longest (most specific).
pub fn detect(recipe: &ModelRecipe, filenames: &[String]) -> DetectedComponents {
    let mut assignments = Vec::new();
    for spec in &recipe.roles {
        let mut best: Option<(usize, &str)> = None; // (pattern length, filename)
        for name in filenames {
            let lower = name.to_lowercase();
            let score = spec
                .patterns
                .iter()
                .filter(|p| lower.contains(&p.to_lowercase()))
                .map(|p| p.len())
                .max();
            if let Some(s) = score {
                if best.map(|(bs, _)| s > bs).unwrap_or(true) {
                    best = Some((s, name.as_str()));
                }
            }
        }
        if let Some((_, name)) = best {
            assignments.push((spec.role, name.to_string()));
        }
    }
    DetectedComponents { assignments }
}

/// Pick the family that best explains this file set: most required roles matched,
/// then most total roles matched. None if no recipe matches any required role.
pub fn detect_best(filenames: &[String]) -> Option<(ModelRecipe, DetectedComponents)> {
    recipes()
        .into_iter()
        .filter(|r| r.family != "custom")
        .map(|r| {
            let d = detect(&r, filenames);
            (r, d)
        })
        .filter(|(r, d)| d.required_matched(r) > 0)
        .max_by_key(|(r, d)| (d.required_matched(r), d.assignments.len()))
}

fn role(role: ComponentRole, required: bool, patterns: &[&'static str]) -> RoleSpec {
    RoleSpec { role, required, patterns: patterns.to_vec() }
}

/// Built-in family recipes. `custom` is the manual-flow pseudo-family:
/// diffusion required, everything else optional, no patterns, no defaults.
pub fn recipes() -> Vec<ModelRecipe> {
    vec![
        ModelRecipe {
            family: "flux1",
            name: "FLUX.1 (dev / schnell / krea)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["flux1", "flux-1", "flux"]),
                role(ComponentRole::T5xxl, true, &["t5xxl", "t5-xxl", "t5"]),
                role(ComponentRole::ClipL, true, &["clip_l", "clip-l"]),
                role(ComponentRole::Vae, true, &["ae.", "vae"]),
            ],
            vae_format: Some("flux"),
            prediction: Some("flux_flow"),
            shared: vec![],
        },
        ModelRecipe {
            family: "sd3",
            name: "Stable Diffusion 3 / 3.5",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["sd3", "sd_3", "stable-diffusion-3"]),
                role(ComponentRole::ClipL, true, &["clip_l", "clip-l"]),
                role(ComponentRole::ClipG, true, &["clip_g", "clip-g"]),
                role(ComponentRole::T5xxl, false, &["t5xxl", "t5-xxl", "t5"]),
                role(ComponentRole::Vae, false, &["vae", "ae."]),
            ],
            vae_format: Some("sd3"),
            prediction: Some("sd3_flow"),
            shared: vec![],
        },
        ModelRecipe {
            family: "qwen-image",
            name: "Qwen-Image",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["qwen-image", "qwen_image", "qwen"]),
                role(ComponentRole::Llm, true, &["qwenvl", "qwen2.5", "qwen2_5", "llm"]),
                role(ComponentRole::Vae, true, &["vae", "ae."]),
            ],
            vae_format: Some("auto"),
            prediction: None,
            shared: vec![],
        },
        ModelRecipe {
            family: "custom",
            name: "Custom (assign files manually)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &[]),
                role(ComponentRole::Vae, false, &[]),
                role(ComponentRole::ClipL, false, &[]),
                role(ComponentRole::ClipG, false, &[]),
                role(ComponentRole::T5xxl, false, &[]),
                role(ComponentRole::Llm, false, &[]),
            ],
            vae_format: None,
            prediction: None,
            shared: vec![],
        },
    ]
}

/// Look up a recipe by family id.
pub fn recipe_for(family: &str) -> Option<ModelRecipe> {
    recipes().into_iter().find(|r| r.family == family)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flux() -> ModelRecipe {
        recipe_for("flux1").unwrap()
    }

    #[test]
    fn detect_matches_canonical_flux_set() {
        let files = vec![
            "flux1-schnell.safetensors".to_string(),
            "t5xxl_fp16.safetensors".to_string(),
            "clip_l.safetensors".to_string(),
            "ae.safetensors".to_string(),
        ];
        let d = detect(&flux(), &files);
        assert_eq!(d.get(ComponentRole::Diffusion), Some("flux1-schnell.safetensors"));
        assert_eq!(d.get(ComponentRole::T5xxl), Some("t5xxl_fp16.safetensors"));
        assert_eq!(d.get(ComponentRole::ClipL), Some("clip_l.safetensors"));
        assert_eq!(d.get(ComponentRole::Vae), Some("ae.safetensors"));
        assert_eq!(d.required_matched(&flux()), 4);
    }

    #[test]
    fn detect_leaves_missing_required_unmatched() {
        let files = vec!["flux1-dev.safetensors".to_string(), "clip_l.safetensors".to_string()];
        let d = detect(&flux(), &files);
        assert_eq!(d.get(ComponentRole::T5xxl), None);
        assert_eq!(d.get(ComponentRole::Vae), None);
        assert_eq!(d.required_matched(&flux()), 2);
    }

    #[test]
    fn detect_ignores_junk_filenames() {
        let files = vec!["notes.txt".to_string(), "random.bin".to_string()];
        let d = detect(&flux(), &files);
        assert!(d.assignments.is_empty());
    }

    #[test]
    fn detect_best_picks_flux_for_flux_files() {
        let files = vec![
            "flux1-schnell.safetensors".to_string(),
            "t5xxl_fp16.safetensors".to_string(),
            "clip_l.safetensors".to_string(),
            "ae.safetensors".to_string(),
        ];
        let (recipe, _d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "flux1");
    }

    #[test]
    fn recipe_table_integrity() {
        let all = recipes();
        // Family ids unique.
        let mut ids: Vec<&str> = all.iter().map(|r| r.family).collect();
        ids.sort();
        let mut deduped = ids.clone();
        deduped.dedup();
        assert_eq!(ids, deduped, "family ids must be unique");
        // Every required role in a non-custom recipe has >=1 pattern.
        for r in &all {
            if r.family == "custom" {
                continue;
            }
            for spec in &r.roles {
                if spec.required {
                    assert!(!spec.patterns.is_empty(), "{} {:?} needs a pattern", r.family, spec.role);
                }
            }
            // vae_format / prediction within the engine's known sets.
            const VAE: [&str; 4] = ["auto", "flux", "sd3", "flux2"];
            const PRED: [&str; 6] = ["eps", "v", "edm_v", "sd3_flow", "flux_flow", "flux2_flow"];
            if let Some(v) = r.vae_format {
                assert!(VAE.contains(&v), "{} bad vae_format {v}", r.family);
            }
            if let Some(p) = r.prediction {
                assert!(PRED.contains(&p), "{} bad prediction {p}", r.family);
            }
        }
    }

    #[test]
    fn missing_required_roles_reports_empty_slots() {
        let c = ModelComponents {
            diffusion_model: "/m/flux1-dev.safetensors".into(),
            clip_l: Some("/m/clip_l.safetensors".into()),
            ..Default::default()
        };
        let missing = flux().missing_required_roles(&c);
        assert!(missing.contains(&ComponentRole::T5xxl));
        assert!(missing.contains(&ComponentRole::Vae));
        assert!(!missing.contains(&ComponentRole::Diffusion));
        assert!(!missing.contains(&ComponentRole::ClipL));
    }
}
```

> This test file references `ModelComponents` (with `diffusion_model`, `clip_l`, `vae`, and a `Default`). That type is created in **Task 2**. Do Task 1's Steps 1–2, then Task 2, then run tests — they compile together. (You can `git commit` the two as one logical unit if you prefer; the checkpoints below assume Task 2 is done.)

- [ ] **Step 3: Run the recipes tests (after Task 2 lands `ModelComponents`)**

Run: `cd src-tauri && cargo test --lib recipes::`
Expected: all `recipes::tests::*` PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/recipes.rs src-tauri/src/lib.rs
git commit -m "feat(models): recipe table + filename detection for split models"
```

---

## Task 2: Data model — `ModelComponents`, `ModelRef`, `ModelDefinition`, `missing_components`

Additive to `types.rs`. Does **not** touch `GenerationRequest` yet (that's Task 3's cutover). Introduces the sum type and the file-existence check.

**Files:**
- Modify: `src-tauri/src/types.rs`

- [ ] **Step 1: Add the types (top of `types.rs`, after the `use serde` line)**

```rust
use crate::recipes::ComponentRole;
```

Then add these type definitions (anywhere among the structs, e.g. just before `GenerationRequest`):

```rust
/// Typed component files of a split model, each wired to a specific engine flag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelComponents {
    pub diffusion_model: String, // --diffusion-model (required)
    #[serde(default)]
    pub vae: Option<String>, // --vae
    #[serde(default)]
    pub clip_l: Option<String>, // --clip_l
    #[serde(default)]
    pub clip_g: Option<String>, // --clip_g
    #[serde(default)]
    pub t5xxl: Option<String>, // --t5xxl
    #[serde(default)]
    pub llm: Option<String>, // --llm
    #[serde(default)]
    pub vae_format: Option<String>, // --vae-format
    #[serde(default)]
    pub prediction: Option<String>, // --prediction
}

/// A model reference: single all-in-one file, or split components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelRef {
    /// -> `-m <path>`
    SingleFile { path: String },
    /// -> `--diffusion-model` + friends
    MultiFile(ModelComponents),
}

impl Default for ModelRef {
    fn default() -> Self {
        ModelRef::SingleFile { path: String::new() }
    }
}

/// A saved multi-file model — the library entry shown in the Model dropdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDefinition {
    pub id: String,       // stable, generated
    pub name: String,     // user-facing label
    pub family: String,   // recipe id: "flux1", "qwen-image", …
    pub components: ModelComponents,
}

/// Component slots whose file no longer exists on disk. Empty = all good.
/// Only *set* slots are checked; a `None` optional slot is never reported.
pub fn missing_components(c: &ModelComponents) -> Vec<(ComponentRole, String)> {
    let checks: [(ComponentRole, Option<&String>); 6] = [
        (ComponentRole::Diffusion, Some(&c.diffusion_model)),
        (ComponentRole::Vae, c.vae.as_ref()),
        (ComponentRole::ClipL, c.clip_l.as_ref()),
        (ComponentRole::ClipG, c.clip_g.as_ref()),
        (ComponentRole::T5xxl, c.t5xxl.as_ref()),
        (ComponentRole::Llm, c.llm.as_ref()),
    ];
    let mut out = Vec::new();
    for (role, path) in checks {
        if let Some(p) = path {
            if !p.trim().is_empty() && !std::path::Path::new(p).exists() {
                out.push((role, p.clone()));
            }
        }
    }
    out
}
```

- [ ] **Step 2: Add the tests (inside the existing `#[cfg(test)] mod tests` block in `types.rs`)**

```rust
#[test]
fn model_ref_single_file_wire_form() {
    let m = ModelRef::SingleFile { path: "/m/x.safetensors".into() };
    let json = serde_json::to_string(&m).unwrap();
    assert_eq!(json, r#"{"type":"single_file","path":"/m/x.safetensors"}"#);
    let back: ModelRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back, m);
}

#[test]
fn model_ref_multi_file_flattens_components() {
    let m = ModelRef::MultiFile(ModelComponents {
        diffusion_model: "/m/flux1-dev.safetensors".into(),
        t5xxl: Some("/m/t5xxl.safetensors".into()),
        clip_l: Some("/m/clip_l.safetensors".into()),
        vae: Some("/m/ae.safetensors".into()),
        vae_format: Some("flux".into()),
        prediction: Some("flux_flow".into()),
        ..Default::default()
    });
    let json = serde_json::to_string(&m).unwrap();
    assert!(json.contains(r#""type":"multi_file""#), "got {json}");
    assert!(json.contains(r#""diffusion_model":"/m/flux1-dev.safetensors""#), "got {json}");
    let back: ModelRef = serde_json::from_str(&json).unwrap();
    assert_eq!(back, m);
}

#[test]
fn model_components_omitted_options_default_to_none() {
    let json = r#"{"diffusion_model":"/m/d.safetensors"}"#;
    let c: ModelComponents = serde_json::from_str(json).unwrap();
    assert_eq!(c.diffusion_model, "/m/d.safetensors");
    assert!(c.vae.is_none() && c.t5xxl.is_none() && c.clip_l.is_none());
}

#[test]
fn model_definition_round_trips() {
    let def = ModelDefinition {
        id: "abc123".into(),
        name: "FLUX.1 schnell".into(),
        family: "flux1".into(),
        components: ModelComponents { diffusion_model: "/m/d.safetensors".into(), ..Default::default() },
    };
    let json = serde_json::to_string(&def).unwrap();
    let back: ModelDefinition = serde_json::from_str(&json).unwrap();
    assert_eq!(back, def);
}

#[test]
fn missing_components_reports_only_set_but_absent_paths() {
    let dir = std::env::temp_dir().join(format!("muchai-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let present = dir.join("d.safetensors");
    std::fs::write(&present, b"x").unwrap();

    let c = ModelComponents {
        diffusion_model: present.to_string_lossy().into_owned(), // exists
        t5xxl: Some(dir.join("gone.safetensors").to_string_lossy().into_owned()), // set, absent
        clip_l: None, // optional, unset -> not reported
        ..Default::default()
    };
    let missing = missing_components(&c);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].0, ComponentRole::T5xxl);
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 3: Run Task 1 + Task 2 tests together**

Run: `cd src-tauri && cargo test --lib recipes:: types::`
Expected: all recipes and types tests PASS (both modules now compile).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/types.rs
git commit -m "feat(models): ModelRef/ModelComponents/ModelDefinition + missing_components"
```

---

## Task 3: Cutover — `GenerationRequest.model`, `AppConfig.model_definitions`, `build_args` match

This is the breaking change. It touches `types.rs`, `command_builder.rs`, `config.rs`, and `commands.rs` in one task so the crate compiles and `cargo test` is green at the end. No frontend yet (separate build; done in Task 9).

**Files:**
- Modify: `src-tauri/src/types.rs` (field swap + updated tests)
- Modify: `src-tauri/src/command_builder.rs` (match mapping + tests)
- Modify: `src-tauri/src/config.rs` (default + fix embedded JSON literals)

- [ ] **Step 1: Swap the `GenerationRequest` field**

In `src-tauri/src/types.rs`, in `struct GenerationRequest`, replace:

```rust
    pub model_path: String,
```

with:

```rust
    pub model: ModelRef,
```

In `impl Default for GenerationRequest`, replace `model_path: String::new(),` with:

```rust
            model: ModelRef::default(),
```

- [ ] **Step 2: Add `model_definitions` to `AppConfig`**

In `struct AppConfig`, add this field (just before `last_request`):

```rust
    /// Saved multi-file model library. Empty on pre-feature configs.
    #[serde(default)]
    pub model_definitions: Vec<ModelDefinition>,
```

- [ ] **Step 3: Fix the two embedded-JSON tests in `types.rs`**

The `app_config_defaults_gpu_device_to_none_when_absent` and `generation_request_without_output_format_defaults_to_png` tests embed `"model_path":""`. A `ModelRef` now requires `"model":{"type":"single_file","path":""}`. Update both JSON string literals:

In `app_config_defaults_gpu_device_to_none_when_absent`, change the `last_request` object so `"model_path":""` becomes `"model":{"type":"single_file","path":""}`. Full replacement literal:

```rust
        let json = r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","models_dir":"/tmp/m","extra_model_dirs":[],"last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#;
```

In `generation_request_without_output_format_defaults_to_png`:

```rust
        let json = r#"{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}"#;
```

- [ ] **Step 4: Add an AppConfig serde test for `model_definitions`**

Add to `types.rs` tests:

```rust
#[test]
fn app_config_without_model_definitions_defaults_empty() {
    let json = r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","models_dir":"/tmp/m","extra_model_dirs":[],"last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#;
    let cfg: AppConfig = serde_json::from_str(json).unwrap();
    assert!(cfg.model_definitions.is_empty());
}
```

- [ ] **Step 5: Rewrite `build_args` as a match on `req.model`**

In `src-tauri/src/command_builder.rs`, change the import line:

```rust
use crate::types::{GenerationRequest, ModelRef};
```

Replace the single `push("-m", req.model_path.clone());` line with:

```rust
    match &req.model {
        ModelRef::SingleFile { path } => push("-m", path.clone()),
        ModelRef::MultiFile(c) => {
            push("--diffusion-model", c.diffusion_model.clone());
            if let Some(v) = &c.vae {
                push("--vae", v.clone());
            }
            if let Some(v) = &c.clip_l {
                push("--clip_l", v.clone());
            }
            if let Some(v) = &c.clip_g {
                push("--clip_g", v.clone());
            }
            if let Some(v) = &c.t5xxl {
                push("--t5xxl", v.clone());
            }
            if let Some(v) = &c.llm {
                push("--llm", v.clone());
            }
            if let Some(v) = &c.vae_format {
                push("--vae-format", v.clone());
            }
            if let Some(v) = &c.prediction {
                push("--prediction", v.clone());
            }
        }
    }
```

- [ ] **Step 6: Update the `command_builder` test fixture + add multi-file tests**

In the test `sample()` helper, replace `model_path: "/m/model.safetensors".into(),` with:

```rust
            model: ModelRef::SingleFile { path: "/m/model.safetensors".into() },
```

Add these tests to `command_builder.rs`:

```rust
    #[test]
    fn single_file_emits_dash_m_and_no_diffusion_model() {
        let args = build_args(&sample(), "/out/x.png", None);
        assert_eq!(val_after(&args, "-m"), Some("/m/model.safetensors"));
        assert!(!args.iter().any(|x| x == "--diffusion-model"));
    }

    #[test]
    fn multi_file_maps_each_role_to_its_flag() {
        use crate::types::ModelComponents;
        let mut req = sample();
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/flux1-dev.safetensors".into(),
            t5xxl: Some("/m/t5xxl.safetensors".into()),
            clip_l: Some("/m/clip_l.safetensors".into()),
            vae: Some("/m/ae.safetensors".into()),
            vae_format: Some("flux".into()),
            prediction: Some("flux_flow".into()),
            ..Default::default()
        });
        let args = build_args(&req, "/out/x.png", None);
        assert_eq!(val_after(&args, "--diffusion-model"), Some("/m/flux1-dev.safetensors"));
        assert_eq!(val_after(&args, "--t5xxl"), Some("/m/t5xxl.safetensors"));
        assert_eq!(val_after(&args, "--clip_l"), Some("/m/clip_l.safetensors"));
        assert_eq!(val_after(&args, "--vae"), Some("/m/ae.safetensors"));
        assert_eq!(val_after(&args, "--vae-format"), Some("flux"));
        assert_eq!(val_after(&args, "--prediction"), Some("flux_flow"));
        assert!(!args.iter().any(|x| x == "-m"), "multi-file must not emit -m");
    }

    #[test]
    fn multi_file_omits_absent_optional_roles() {
        use crate::types::ModelComponents;
        let mut req = sample();
        req.model = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/d.safetensors".into(),
            ..Default::default()
        });
        let args = build_args(&req, "/out/x.png", None);
        assert_eq!(val_after(&args, "--diffusion-model"), Some("/m/d.safetensors"));
        for flag in ["--vae", "--clip_l", "--clip_g", "--t5xxl", "--llm", "--vae-format", "--prediction"] {
            assert!(!args.iter().any(|x| x == flag), "{flag} must be absent");
        }
    }
```

- [ ] **Step 7: Fix `config.rs` — default already works, fix its embedded JSON literals**

`default_config()` needs no change (it uses `GenerationRequest::default()`, which now defaults `model`). But the four embedded-JSON test literals in `config.rs` (`old_config_without_params_expanded_defaults_to_collapsed`, `old_config_without_theme_defaults_to_dark`, `old_config_without_onboarded_defaults_to_false`, `old_config_without_model_fields_loads_and_backfills_models_dir`) each contain `"model_path":""`. In **every one**, replace `"model_path":""` with `"model":{"type":"single_file","path":""}` inside the `last_request` object.

For example, the `last_request` fragment becomes:

```
"last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}
```

- [ ] **Step 8: Run the full backend suite**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS (every test, including the updated literals). If any test still references `model_path`, fix it the same way.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/command_builder.rs src-tauri/src/config.rs
git commit -m "feat(models): GenerationRequest.model as ModelRef; build_args flag mapping; config.model_definitions"
```

---

## Task 4: Exclude definition-referenced files from the single-file scan

Component files are `.safetensors` too. Without this, an assembled model's diffusion/encoder files show up as bogus single-file models. Fix: exclude any path referenced by a saved `ModelDefinition`.

**Files:**
- Modify: `src-tauri/src/models.rs`

- [ ] **Step 1: Add the failing test**

Add to `models.rs` tests:

```rust
    #[test]
    fn excludes_paths_referenced_by_definitions() {
        let root = std::env::temp_dir().join(format!("muchai-excl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("loose.safetensors"), 10);
        touch(&root.join("flux1/flux1-dev.safetensors"), 20);
        touch(&root.join("flux1/t5xxl.safetensors"), 30);

        // Canonicalize the two component paths the way scan does.
        let diff = root.join("flux1/flux1-dev.safetensors").canonicalize().unwrap();
        let t5 = root.join("flux1/t5xxl.safetensors").canonicalize().unwrap();
        let exclude: HashSet<PathBuf> = [diff, t5].into_iter().collect();

        let models = scan_models_excluding(&[root.clone()], &exclude);
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["loose"], "only the unreferenced file remains");
        let _ = fs::remove_dir_all(&root);
    }
```

- [ ] **Step 2: Run it (fails: `scan_models_excluding` not defined)**

Run: `cd src-tauri && cargo test --lib models::excludes_paths_referenced_by_definitions`
Expected: FAIL to compile — `scan_models_excluding` not found.

- [ ] **Step 3: Implement the exclusion**

In `models.rs`, thread an exclusion set through `collect`. Change `collect`'s signature and the skip check, and add the new public entry point. Replace the `collect` function and `scan_models` with:

```rust
fn collect(dir: &Path, out: &mut Vec<ModelInfo>, seen: &mut HashSet<PathBuf>, exclude: &HashSet<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // missing / unreadable dirs are skipped
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out, seen, exclude);
        } else if is_model_file(&path) {
            let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
            if exclude.contains(&canon) {
                continue; // referenced by a saved multi-file definition
            }
            if !seen.insert(canon.clone()) {
                continue; // already found via another watched dir
            }
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let name = canon
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(ModelInfo {
                path: canon.to_string_lossy().into_owned(),
                name,
                size_bytes,
            });
        }
    }
}

/// Scan every directory (recursively) for unique model files, sorted by name.
/// Missing/unreadable directories are skipped silently.
pub fn scan_models(dirs: &[PathBuf]) -> Vec<ModelInfo> {
    scan_models_excluding(dirs, &HashSet::new())
}

/// Like `scan_models`, but skips any file whose canonical path is in `exclude`
/// (used to hide component files owned by a saved multi-file definition).
pub fn scan_models_excluding(dirs: &[PathBuf], exclude: &HashSet<PathBuf>) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for dir in dirs {
        collect(dir, &mut out, &mut seen, exclude);
    }
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}
```

- [ ] **Step 4: Run the models tests**

Run: `cd src-tauri && cargo test --lib models::`
Expected: PASS (existing scan tests + the new exclusion test).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs
git commit -m "feat(models): exclude definition-referenced files from single-file scan"
```

---

## Task 5: Downloader — stream to an explicit destination path

Multi-file downloads need a **stable** destination filename (so a shared encoder is reused, not unique-suffixed). Extract `download_to(dest_path)` and rebuild `download_model` on top of it. Behavior of `download_model` is unchanged.

**Files:**
- Modify: `src-tauri/src/downloader.rs`

- [ ] **Step 1: Add `download_to` and refactor `download_model`**

In `downloader.rs`, add `download_to` and replace the body of `download_model` so it derives the filename then delegates. Insert `download_to` above `download_model`, and change `download_model` to:

```rust
/// Download `url` into `dest_dir`, choosing the filename from headers/URL and
/// de-duplicating on collision. Thin wrapper over `download_to`.
pub fn download_model<F: FnMut(u64, Option<u64>)>(
    url: &str,
    token: &str,
    dest_dir: &Path,
    on_progress: F,
    cancel: &AtomicBool,
) -> Result<PathBuf, DownloadError> {
    // We need the filename before the request to compute the unique path, but the
    // server's Content-Disposition may refine it. Derive from the URL up front for
    // the unique-path base; `download_to` streams to exactly the path we choose.
    let filename = derive_filename(None, url);
    let dest = unique_path(dest_dir, &filename);
    download_to(url, token, &dest, on_progress, cancel)?;
    Ok(dest)
}

/// Download `url` to exactly `dest_path`, streaming to a sibling `.part` file and
/// renaming on success. Calls `on_progress(downloaded, total)` as bytes arrive.
/// Aborts promptly when `cancel` flips to true, removing the partial file.
pub fn download_to<F: FnMut(u64, Option<u64>)>(
    url: &str,
    token: &str,
    dest_path: &Path,
    mut on_progress: F,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DownloadError::Io(e.to_string()))?;
    }
    let mut req = ureq::get(url);
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            return Err(DownloadError::Unauthorized)
        }
        Err(ureq::Error::Status(404, _)) => return Err(DownloadError::NotFound),
        Err(ureq::Error::Status(code, _)) => {
            return Err(DownloadError::Network(format!("server returned {code}")))
        }
        Err(ureq::Error::Transport(t)) => return Err(DownloadError::Network(t.to_string())),
    };

    let total: Option<u64> = resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok());
    let part_path = dest_path.with_extension(format!(
        "{}part",
        dest_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{e}."))
            .unwrap_or_default()
    ));

    let mut file = File::create(&part_path).map_err(|e| DownloadError::Io(e.to_string()))?;
    let mut reader = resp.into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;

    loop {
        if cancel.load(Ordering::SeqCst) {
            drop(file);
            let _ = std::fs::remove_file(&part_path);
            return Err(DownloadError::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&part_path);
                return Err(DownloadError::Io(e.to_string()));
            }
        };
        if let Err(e) = file.write_all(&buf[..n]) {
            drop(file);
            let _ = std::fs::remove_file(&part_path);
            return Err(DownloadError::Io(e.to_string()));
        }
        downloaded += n as u64;
        on_progress(downloaded, total);
    }

    file.flush().map_err(|e| {
        let _ = std::fs::remove_file(&part_path);
        DownloadError::Io(e.to_string())
    })?;
    drop(file);
    std::fs::rename(&part_path, dest_path).map_err(|e| {
        let _ = std::fs::remove_file(&part_path);
        DownloadError::Io(e.to_string())
    })?;
    Ok(())
}
```

Delete the old `download_model` body (the streaming loop now lives only in `download_to`). Keep `sanitize_filename`, `derive_filename`, and `unique_path` unchanged.

- [ ] **Step 2: Run the downloader tests**

Run: `cd src-tauri && cargo test --lib downloader::`
Expected: PASS (the existing filename/unique-path tests are unaffected; the refactor preserves them).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/downloader.rs
git commit -m "refactor(downloader): extract download_to for explicit-path streaming"
```

---

## Task 6: Commands (core) — recipes DTO, folder detection, multi-select picker, definition CRUD, generation guard

Wires the pure logic to the UI: expose recipes, detect a folder, pick multiple files, save/delete definitions, exclude their files from the scan, and block generation on a broken multi-file model.

**Files:**
- Modify: `src-tauri/src/recipes.rs` (add serializable `RecipeInfo` DTO)
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (register commands)

- [ ] **Step 1: Add the `RecipeInfo` DTO to `recipes.rs`**

Append to `recipes.rs` (before `#[cfg(test)]`):

```rust
/// A recipe flattened for the frontend: roles with labels + defaulted flags.
#[derive(Debug, Clone, Serialize)]
pub struct RoleInfo {
    pub role: ComponentRole,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecipeInfo {
    pub family: String,
    pub name: String,
    pub roles: Vec<RoleInfo>,
    pub vae_format: Option<String>,
    pub prediction: Option<String>,
}

/// All recipes as frontend DTOs (drives the family picker + role slots).
pub fn recipe_infos() -> Vec<RecipeInfo> {
    recipes()
        .into_iter()
        .map(|r| RecipeInfo {
            family: r.family.to_string(),
            name: r.name.to_string(),
            roles: r.roles.iter().map(|s| RoleInfo { role: s.role, required: s.required }).collect(),
            vae_format: r.vae_format.map(|s| s.to_string()),
            prediction: r.prediction.map(|s| s.to_string()),
        })
        .collect()
}
```

- [ ] **Step 2: Add a `DetectionResult` DTO to `commands.rs`**

Near the top of `commands.rs` (after the imports), add:

```rust
use crate::recipes::{self, ComponentRole};
use crate::types::{ModelComponents, ModelDefinition, ModelRef};

/// One detected role → absolute path, for the folder auto-detect flow.
#[derive(serde::Serialize)]
pub struct DetectedSlot {
    pub role: ComponentRole,
    pub path: String,
}

/// Result of point-at-a-folder detection: best-matching family + pre-filled slots.
#[derive(serde::Serialize)]
pub struct DetectionResult {
    pub family: String,
    pub name: String,
    pub slots: Vec<DetectedSlot>,
}
```

(Adjust the existing `use crate::types::{...}` line to add `ModelComponents, ModelDefinition, ModelRef` if not already imported by the block above; both forms are fine as long as each type is imported once.)

- [ ] **Step 3: Add the referenced-paths helper + swap the scan call**

Add this helper and change `list_models` in `commands.rs`:

```rust
/// Every component file path owned by a saved definition (canonicalized).
fn referenced_paths(cfg: &AppConfig) -> std::collections::HashSet<PathBuf> {
    let mut set = std::collections::HashSet::new();
    for def in &cfg.model_definitions {
        let c = &def.components;
        for p in [
            Some(&c.diffusion_model),
            c.vae.as_ref(),
            c.clip_l.as_ref(),
            c.clip_g.as_ref(),
            c.t5xxl.as_ref(),
            c.llm.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let pb = PathBuf::from(p);
            set.insert(pb.canonicalize().unwrap_or(pb));
        }
    }
    set
}
```

Replace the body of `list_models`:

```rust
#[tauri::command]
pub fn list_models(state: State<AppState>) -> Vec<ModelInfo> {
    let cfg = state.config.lock().unwrap().clone();
    models::scan_models_excluding(&model_dirs(&cfg), &referenced_paths(&cfg))
}
```

- [ ] **Step 4: Add recipe / detection / picker / CRUD commands**

Append these commands to `commands.rs`:

```rust
#[tauri::command]
pub fn list_recipes() -> Vec<recipes::RecipeInfo> {
    recipes::recipe_infos()
}

/// Run filename detection over the files in a folder; return the best-matching
/// family with pre-filled absolute-path slots. Falls back to "custom" (no slots)
/// when nothing matches — never a dead end.
#[tauri::command]
pub fn detect_folder(dir: String) -> DetectionResult {
    let dir = PathBuf::from(&dir);
    let mut entries: Vec<PathBuf> = Vec::new();
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
    let names: Vec<String> = entries
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();

    match recipes::detect_best(&names) {
        Some((recipe, detected)) => {
            let slots = detected
                .assignments
                .iter()
                .filter_map(|(role, fname)| {
                    entries
                        .iter()
                        .find(|p| p.file_name().map(|n| n.to_string_lossy() == *fname.as_str()).unwrap_or(false))
                        .map(|p| DetectedSlot { role: *role, path: p.to_string_lossy().into_owned() })
                })
                .collect();
            DetectionResult { family: recipe.family.to_string(), name: recipe.name.to_string(), slots }
        }
        None => DetectionResult {
            family: "custom".into(),
            name: "Custom (assign files manually)".into(),
            slots: Vec::new(),
        },
    }
}

/// Multi-select model file picker (for manual role assignment).
#[tauri::command]
pub async fn pick_model_files(app: AppHandle) -> Vec<String> {
    use tauri_plugin_dialog::DialogExt;
    let files = app
        .dialog()
        .file()
        .add_filter("Models", &["safetensors"])
        .blocking_pick_files();
    files
        .map(|v| v.into_iter().filter_map(|f| f.into_path().ok()).map(|p| p.to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// Insert or update a definition (matched by id) and persist. Validates that
/// required roles for the family are filled before saving.
#[tauri::command]
pub fn save_model_definition(state: State<AppState>, def: ModelDefinition) -> Result<(), String> {
    if let Some(recipe) = recipes::recipe_for(&def.family) {
        let missing = recipe.missing_required_roles(&def.components);
        if !missing.is_empty() {
            return Err(format!("Missing required components: {missing:?}"));
        }
    } else {
        return Err(format!("Unknown model family: {}", def.family));
    }
    let mut cfg = state.config.lock().unwrap();
    if let Some(existing) = cfg.model_definitions.iter_mut().find(|d| d.id == def.id) {
        *existing = def;
    } else {
        cfg.model_definitions.push(def);
    }
    config::save_config_to(&config::config_file_path(), &cfg).map_err(|e| e.to_string())
}

/// Delete a definition and move its per-model folder to trash. The shared pool
/// is left intact (other models may use it).
#[tauri::command]
pub fn delete_model_definition(state: State<AppState>, id: String) -> Result<(), String> {
    let mut cfg = state.config.lock().unwrap();
    let Some(pos) = cfg.model_definitions.iter().position(|d| d.id == id) else {
        return Err("That model is no longer in the library.".into());
    };
    let def = cfg.model_definitions.remove(pos);
    // Per-model folder = models_dir/<id>. Trash it if present (ignore if absent).
    let folder = PathBuf::from(&cfg.models_dir).join(&def.id);
    if folder.is_dir() {
        let _ = trash::delete(&folder);
    }
    config::save_config_to(&config::config_file_path(), &cfg).map_err(|e| e.to_string())
}
```

- [ ] **Step 5: Guard generation against a broken multi-file model**

In `generate`, right after `let cfg = state.config.lock().unwrap().clone();`, add:

```rust
    if let ModelRef::MultiFile(c) = &request.model {
        let missing = crate::types::missing_components(c);
        if !missing.is_empty() {
            let roles: Vec<String> = missing.iter().map(|(r, _)| format!("{r:?}")).collect();
            return Err(format!(
                "This model is missing component files ({}). Re-assemble or re-download it.",
                roles.join(", ")
            ));
        }
    }
```

- [ ] **Step 6: Register the new commands**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![...]`, add:

```rust
            commands::list_recipes,
            commands::detect_folder,
            commands::pick_model_files,
            commands::save_model_definition,
            commands::delete_model_definition,
```

- [ ] **Step 7: Build + test**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: tests PASS, build succeeds (commands compile and are registered).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/recipes.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(models): recipe/detection/picker commands, definition CRUD, broken-model guard"
```

---

## Task 7: Catalog — curated multi-file entries + download resolution planning

Pure. A catalog entry + its family recipe resolve to the exact `(url, dest)` list to fetch, skipping shared files already in the pool.

**Files:**
- Modify: `src-tauri/src/catalog.rs`
- Modify: `src-tauri/src/recipes.rs` (populate `flux1.shared` with real URLs so a Flux entry is downloadable)

- [ ] **Step 1: Fill in Flux shared components**

In `recipes.rs`, replace the `flux1` recipe's `shared: vec![],` with the family-shared encoders/VAE. These are the public Comfy-Org Flux repackaged files:

```rust
            shared: vec![
                SharedComponent {
                    role: ComponentRole::T5xxl,
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/t5xxl_fp16.safetensors",
                    size_bytes: 9_787_841_024,
                    filename: "t5xxl_fp16.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::ClipL,
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/clip_l.safetensors",
                    size_bytes: 246_144_152,
                    filename: "clip_l.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/black-forest-labs/FLUX.1-schnell/resolve/main/ae.safetensors",
                    size_bytes: 335_304_388,
                    filename: "ae.safetensors",
                },
            ],
```

- [ ] **Step 2: Add catalog types + entries + planner (with failing tests)**

In `catalog.rs`, add at the top:

```rust
use crate::recipes::{ComponentRole, ModelRecipe, SharedComponent};
use std::path::{Path, PathBuf};
```

Add the types, catalog, and planner:

```rust
/// A curated multi-file catalog entry — one downloadable split model.
#[derive(Debug, Clone, Serialize)]
pub struct MultiFileCatalogEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    pub diffusion_url: String,
    pub diffusion_size_bytes: u64,
    /// Roles this model ships its OWN copy of (downloaded into the model folder,
    /// not the shared pool). Usually empty.
    pub overrides: Vec<SharedComponent>,
    pub min_vram_mb: u64,
    pub recommended_vram_mb: u64,
}

/// Built-in curated multi-file models.
pub fn multi_file_catalog() -> Vec<MultiFileCatalogEntry> {
    vec![MultiFileCatalogEntry {
        id: "flux1-schnell".into(),
        name: "FLUX.1 schnell".into(),
        family: "flux1".into(),
        diffusion_url:
            "https://huggingface.co/black-forest-labs/FLUX.1-schnell/resolve/main/flux1-schnell.safetensors"
                .into(),
        diffusion_size_bytes: 23_782_506_688,
        overrides: vec![],
        min_vram_mb: 8192,
        recommended_vram_mb: 16384,
    }]
}

/// One file to fetch during multi-file download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedDownload {
    pub url: String,
    pub dest: PathBuf,
    pub role: ComponentRole,
}

/// Resolve an entry + its recipe into the exact files to download.
/// - Diffusion → always the entry's URL → `models_dir/<id>/<filename>`.
/// - Each family-shared role → into `models_dir/shared/<family>/<filename>`,
///   skipped when `exists(dest)` (already pooled). If the entry OVERRIDES the
///   role, download the override into the model folder instead.
pub fn plan_downloads(
    entry: &MultiFileCatalogEntry,
    recipe: &ModelRecipe,
    models_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<PlannedDownload> {
    let model_dir = models_dir.join(&entry.id);
    let shared_dir = models_dir.join("shared").join(&entry.family);
    let mut plan = Vec::new();

    // Diffusion: always fetched into the per-model folder.
    let diff_name = crate::downloader::derive_filename(None, &entry.diffusion_url);
    plan.push(PlannedDownload {
        url: entry.diffusion_url.clone(),
        dest: model_dir.join(diff_name),
        role: ComponentRole::Diffusion,
    });

    for shared in &recipe.shared {
        if let Some(ov) = entry.overrides.iter().find(|o| o.role == shared.role) {
            // Model ships its own copy → per-model folder, always fetched.
            plan.push(PlannedDownload {
                url: ov.url.to_string(),
                dest: model_dir.join(ov.filename),
                role: ov.role,
            });
        } else {
            let dest = shared_dir.join(shared.filename);
            if !exists(&dest) {
                plan.push(PlannedDownload {
                    url: shared.url.to_string(),
                    dest,
                    role: shared.role,
                });
            }
        }
    }
    plan
}
```

Add these tests to `catalog.rs`:

```rust
    #[test]
    fn plan_includes_diffusion_and_all_shared_when_pool_empty() {
        let entry = &multi_file_catalog()[0];
        let recipe = crate::recipes::recipe_for("flux1").unwrap();
        let root = Path::new("/models");
        let plan = plan_downloads(entry, &recipe, root, &|_| false);
        // diffusion + 3 shared (t5xxl, clip_l, vae)
        assert_eq!(plan.len(), 4);
        let diff = plan.iter().find(|p| p.role == ComponentRole::Diffusion).unwrap();
        assert_eq!(diff.dest, root.join("flux1-schnell").join("flux1-schnell.safetensors"));
        let t5 = plan.iter().find(|p| p.role == ComponentRole::T5xxl).unwrap();
        assert_eq!(t5.dest, root.join("shared").join("flux1").join("t5xxl_fp16.safetensors"));
    }

    #[test]
    fn plan_skips_shared_files_already_in_pool() {
        let entry = &multi_file_catalog()[0];
        let recipe = crate::recipes::recipe_for("flux1").unwrap();
        let root = Path::new("/models");
        // Pretend everything under shared/ already exists → only diffusion planned.
        let plan = plan_downloads(entry, &recipe, root, &|p| p.starts_with(root.join("shared")));
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].role, ComponentRole::Diffusion);
    }

    #[test]
    fn plan_puts_overrides_in_model_folder() {
        let mut entry = multi_file_catalog()[0].clone();
        entry.overrides = vec![SharedComponent {
            role: ComponentRole::Vae,
            url: "https://example/ae-custom.safetensors",
            size_bytes: 1,
            filename: "ae-custom.safetensors",
        }];
        let recipe = crate::recipes::recipe_for("flux1").unwrap();
        let root = Path::new("/models");
        let plan = plan_downloads(&entry, &recipe, root, &|_| false);
        let vae = plan.iter().find(|p| p.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.dest, root.join("flux1-schnell").join("ae-custom.safetensors"));
    }
```

- [ ] **Step 3: Add multi-file rating (reuse the single-file rule)**

Add to `catalog.rs`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct RatedMultiFile {
    #[serde(flatten)]
    pub entry: MultiFileCatalogEntry,
    pub suitability: Suitability,
}

/// The multi-file catalog rated against the given VRAM.
pub fn rated_multi_file_catalog(vram_total_mb: Option<u64>) -> Vec<RatedMultiFile> {
    multi_file_catalog()
        .into_iter()
        .map(|entry| {
            let suitability = match vram_total_mb {
                None => Suitability::Unknown,
                Some(v) if v >= entry.recommended_vram_mb => Suitability::Recommended,
                Some(v) if v >= entry.min_vram_mb => Suitability::Tight,
                Some(_) => Suitability::TooBig,
            };
            RatedMultiFile { entry, suitability }
        })
        .collect()
}
```

- [ ] **Step 4: Run catalog + recipes tests**

Run: `cd src-tauri && cargo test --lib catalog:: recipes::`
Expected: PASS (new planner tests + unchanged recipe integrity test, now with 3 flux shared components).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/catalog.rs src-tauri/src/recipes.rs
git commit -m "feat(catalog): curated multi-file entries + shared-pool download planning"
```

---

## Task 8: Commands — `download_multifile`, `multifile_catalog`, richer progress

Executes the download plan sequentially, emits per-file progress, assembles the definition, persists it, returns it selected.

**Files:**
- Modify: `src-tauri/src/types.rs` (`DownloadProgress` gains optional file context)
- Modify: `src-tauri/src/catalog.rs` (`assemble_components` helper)
- Modify: `src-tauri/src/commands.rs` + `src-tauri/src/lib.rs`

- [ ] **Step 1: Extend `DownloadProgress` with file context**

In `types.rs`, replace the `DownloadProgress` struct (drop `Copy`; add optional fields):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Multi-file context (0-based). Absent/None on single-file downloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}
```

In `commands.rs::download_model`, update the single-file emit to fill the new fields with `None`:

```rust
                    let _ = app2.emit(
                        "model:download:progress",
                        DownloadProgress { downloaded, total, file_index: None, file_count: None, file_name: None },
                    );
```

- [ ] **Step 2: Add `assemble_components` to `catalog.rs`**

```rust
use crate::types::ModelComponents;

/// Final absolute component paths for a fully-downloaded entry (independent of
/// which files actually needed fetching), plus the recipe's format defaults.
pub fn assemble_components(
    entry: &MultiFileCatalogEntry,
    recipe: &ModelRecipe,
    models_dir: &Path,
) -> ModelComponents {
    let model_dir = models_dir.join(&entry.id);
    let shared_dir = models_dir.join("shared").join(&entry.family);
    let diff_name = crate::downloader::derive_filename(None, &entry.diffusion_url);

    let mut c = ModelComponents {
        diffusion_model: model_dir.join(diff_name).to_string_lossy().into_owned(),
        vae_format: recipe.vae_format.map(|s| s.to_string()),
        prediction: recipe.prediction.map(|s| s.to_string()),
        ..Default::default()
    };
    for shared in &recipe.shared {
        let path = if let Some(ov) = entry.overrides.iter().find(|o| o.role == shared.role) {
            model_dir.join(ov.filename)
        } else {
            shared_dir.join(shared.filename)
        }
        .to_string_lossy()
        .into_owned();
        match shared.role {
            ComponentRole::Vae => c.vae = Some(path),
            ComponentRole::ClipL => c.clip_l = Some(path),
            ComponentRole::ClipG => c.clip_g = Some(path),
            ComponentRole::T5xxl => c.t5xxl = Some(path),
            ComponentRole::Llm => c.llm = Some(path),
            ComponentRole::Diffusion => {}
        }
    }
    c
}
```

- [ ] **Step 3: Add `download_multifile` + `multifile_catalog` commands**

Append to `commands.rs`:

```rust
#[tauri::command]
pub fn multifile_catalog(vram_total_mb: Option<u64>) -> Vec<catalog::RatedMultiFile> {
    catalog::rated_multi_file_catalog(vram_total_mb)
}

/// Download a curated multi-file model: fetch diffusion + any missing shared
/// components sequentially, assemble + persist a definition, return it.
/// On cancel/failure: remove the partial per-model folder, persist nothing.
#[tauri::command]
pub async fn download_multifile(
    app: AppHandle,
    state: State<'_, AppState>,
    entry_id: String,
    token: String,
) -> Result<ModelDefinition, String> {
    let (models_dir, entry, recipe) = {
        let cfg = state.config.lock().unwrap();
        let entry = catalog::multi_file_catalog()
            .into_iter()
            .find(|e| e.id == entry_id)
            .ok_or_else(|| "Unknown catalog model.".to_string())?;
        let recipe = recipes::recipe_for(&entry.family).ok_or_else(|| "Unknown family.".to_string())?;
        (PathBuf::from(&cfg.models_dir), entry, recipe)
    };

    let model_dir = models_dir.join(&entry.id);
    let plan = catalog::plan_downloads(&entry, &recipe, &models_dir, &|p| p.exists());
    let file_count = plan.len() as u32;

    let cancel = state.download_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let entry2 = entry.clone();
    let recipe2 = recipe.clone();
    let model_dir2 = model_dir.clone();
    let models_dir2 = models_dir.clone();

    let result = tauri::async_runtime::spawn_blocking(move || -> Result<ModelDefinition, String> {
        for (i, item) in plan.iter().enumerate() {
            let name = item
                .dest
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let mut last_emit: u64 = 0;
            let app3 = app2.clone();
            let name2 = name.clone();
            downloader::download_to(
                &item.url,
                &token,
                &item.dest,
                move |downloaded, total| {
                    if downloaded.saturating_sub(last_emit) >= 4 << 20 || Some(downloaded) == total {
                        last_emit = downloaded;
                        let _ = app3.emit(
                            "model:download:progress",
                            DownloadProgress {
                                downloaded,
                                total,
                                file_index: Some(i as u32),
                                file_count: Some(file_count),
                                file_name: Some(name2.clone()),
                            },
                        );
                    }
                },
                &cancel,
            )
            .map_err(|e| e.message())?;
        }
        let components = catalog::assemble_components(&entry2, &recipe2, &models_dir2);
        Ok(ModelDefinition {
            id: entry2.id.clone(),
            name: entry2.name.clone(),
            family: entry2.family.clone(),
            components,
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(def) => {
            // Persist (upsert by id).
            let mut cfg = state.config.lock().unwrap();
            if let Some(existing) = cfg.model_definitions.iter_mut().find(|d| d.id == def.id) {
                *existing = def.clone();
            } else {
                cfg.model_definitions.push(def.clone());
            }
            config::save_config_to(&config::config_file_path(), &cfg).map_err(|e| e.to_string())?;
            Ok(def)
        }
        Err(e) => {
            // Roll back the per-model folder; leave the shared pool intact.
            if model_dir.is_dir() {
                let _ = std::fs::remove_dir_all(&model_dir);
            }
            Err(e)
        }
    }
}
```

- [ ] **Step 4: Register the commands**

In `lib.rs`, add to `generate_handler![...]`:

```rust
            commands::multifile_catalog,
            commands::download_multifile,
```

- [ ] **Step 5: Build + test**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: PASS + build succeeds.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/catalog.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(catalog): download_multifile with shared pool + per-file progress"
```

---

## Task 9: Frontend types — `ModelRef` union, definitions, recipe/catalog DTOs

Mirror the Rust wire forms. This is where `npm run check` first sees the `model` field change, so update `defaultRequest` and every consumer's imports in the same task.

**Files:**
- Modify: `src/lib/types.ts`

- [ ] **Step 1: Add the model types + role constants**

In `types.ts`, add (near the top, after `Sampler`):

```ts
// Wire values MUST match the Rust `ComponentRole` enum's serde snake_case form
// (src-tauri/src/recipes.rs).
export type ComponentRole = "diffusion" | "vae" | "clip_l" | "clip_g" | "t5xxl" | "llm";

export const ROLE_LABELS: Record<ComponentRole, string> = {
  diffusion: "Diffusion model",
  vae: "VAE",
  clip_l: "CLIP-L text encoder",
  clip_g: "CLIP-G text encoder",
  t5xxl: "T5-XXL text encoder",
  llm: "LLM text encoder",
};

// Engine enums, from src-tauri/fixtures/sd-help.txt. Empty = let engine auto-detect.
export const VAE_FORMATS = ["", "auto", "flux", "sd3", "flux2"] as const;
export const PREDICTIONS = ["", "eps", "v", "edm_v", "sd3_flow", "flux_flow", "flux2_flow"] as const;

// Mirrors Rust `ModelComponents`. Optional roles are omitted or null.
export interface ModelComponents {
  diffusion_model: string;
  vae?: string | null;
  clip_l?: string | null;
  clip_g?: string | null;
  t5xxl?: string | null;
  llm?: string | null;
  vae_format?: string | null;
  prediction?: string | null;
}

// Mirrors Rust `ModelRef` (internally tagged, snake_case). The multi_file variant
// flattens ModelComponents fields alongside `type`.
export type ModelRef =
  | { type: "single_file"; path: string }
  | ({ type: "multi_file" } & ModelComponents);

export interface ModelDefinition {
  id: string;
  name: string;
  family: string;
  components: ModelComponents;
}

/** True when the model is selectable/usable (path or diffusion set). */
export function modelIsSet(m: ModelRef): boolean {
  return m.type === "single_file" ? m.path.trim() !== "" : m.diffusion_model.trim() !== "";
}

/** A short label for a model reference (single-file basename or definition name). */
export function modelLabel(m: ModelRef, definitions: ModelDefinition[] = []): string {
  if (m.type === "single_file") return m.path.split("/").pop() ?? m.path;
  const def = definitions.find((d) => d.components.diffusion_model === m.diffusion_model);
  return def?.name ?? (m.diffusion_model.split("/").pop() ?? "multi-file model");
}
```

- [ ] **Step 2: Swap `GenerationRequest.model_path` → `model`, update `defaultRequest`**

In `interface GenerationRequest`, replace `model_path: string;` with:

```ts
  model: ModelRef;
```

In `defaultRequest()`, replace `model_path: "",` with:

```ts
  model: { type: "single_file", path: "" },
```

- [ ] **Step 3: Add `model_definitions` to `AppConfig`**

In `interface AppConfig`, add:

```ts
  model_definitions: ModelDefinition[];
```

- [ ] **Step 4: Extend `DownloadProgress` + add recipe/catalog DTOs**

Replace the `DownloadProgress` interface:

```ts
export interface DownloadProgress {
  downloaded: number;
  total: number | null;
  // Multi-file context (0-based). Absent on single-file downloads.
  file_index?: number;
  file_count?: number;
  file_name?: string;
}
```

Add (end of file):

```ts
export interface RoleInfo { role: ComponentRole; required: boolean; }
export interface RecipeInfo {
  family: string;
  name: string;
  roles: RoleInfo[];
  vae_format: string | null;
  prediction: string | null;
}

export interface DetectedSlot { role: ComponentRole; path: string; }
export interface DetectionResult { family: string; name: string; slots: DetectedSlot[]; }

export interface RatedMultiFile {
  id: string; name: string; family: string;
  diffusion_url: string; diffusion_size_bytes: number;
  overrides: unknown[]; min_vram_mb: number; recommended_vram_mb: number;
  suitability: Suitability;
}
```

- [ ] **Step 5: Verify (will still show errors in components until Tasks 10–11)**

Run: `npm run check`
Expected: errors ONLY in `stores.ts`, `api.ts`, `ModelLibrary.svelte`, `GenerateBar.svelte`, `ParamsPanel.svelte` referencing the removed `model_path`. `types.ts` itself is clean. Those consumers are fixed in the next tasks. (If you prefer a green checkpoint, do Tasks 9–11 before committing; otherwise commit now knowing check is temporarily red.)

- [ ] **Step 6: Commit**

```bash
git add src/lib/types.ts
git commit -m "feat(ui): ModelRef discriminated union + definition/recipe/catalog types"
```

---

## Task 10: Frontend API + stores

**Files:**
- Modify: `src/lib/api.ts`
- Modify: `src/lib/stores.ts`

- [ ] **Step 1: Add API wrappers**

In `api.ts`, extend the type import to include the new DTOs:

```ts
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats, ModelInfo, RatedModel, DownloadProgress, GpuDevice, RecipeInfo, DetectionResult, RatedMultiFile, ModelDefinition } from "./types";
```

Add these wrappers (after the existing model/download ones):

```ts
export const listRecipes = () => invoke<RecipeInfo[]>("list_recipes");
export const detectFolder = (dir: string) => invoke<DetectionResult>("detect_folder", { dir });
export const pickModelFiles = () => invoke<string[]>("pick_model_files");
export const multifileCatalog = (vramTotalMb: number | null) =>
  invoke<RatedMultiFile[]>("multifile_catalog", { vramTotalMb });
export const downloadMultifile = (entryId: string, token: string) =>
  invoke<ModelDefinition>("download_multifile", { entryId, token });
export const saveModelDefinition = (def: ModelDefinition) =>
  invoke<void>("save_model_definition", { def });
export const deleteModelDefinition = (id: string) =>
  invoke<void>("delete_model_definition", { id });
```

- [ ] **Step 2: Enrich the download store**

In `stores.ts`, update the import to add types + the new API fns:

```ts
import { downloadModel, downloadMultifile, cancelDownload, onDownloadProgress } from "./api";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, ModelInfo, GpuDevice, ModelDefinition } from "./types";
```

Add a definitions store (next to `models`):

```ts
export const definitions = writable<ModelDefinition[]>([]);
```

Replace the `DownloadStatus` type so `active` carries optional file context:

```ts
export type DownloadStatus =
  | { kind: "idle" }
  | {
      kind: "active";
      name: string;
      downloaded: number;
      total: number | null;
      fileIndex?: number;
      fileCount?: number;
      fileName?: string;
    }
  | { kind: "done"; name: string }
  | { kind: "error"; name: string; message: string };
```

In `startDownload`, update the progress listener to copy the file fields:

```ts
  const unlisten = await onDownloadProgress((p) => {
    downloadStatus.update((s) =>
      s.kind === "active"
        ? { ...s, downloaded: p.downloaded, total: p.total, fileIndex: p.file_index, fileCount: p.file_count, fileName: p.file_name }
        : s,
    );
  });
```

- [ ] **Step 3: Add a multi-file download starter**

Append to `stores.ts`:

```ts
/**
 * Start a curated multi-file download in the background (single-flight, like
 * startDownload). Resolves the returned definition into the `definitions` store
 * and selects it. No-op if a download is already active.
 */
export async function startMultiFileDownload(entryId: string, token: string, name: string): Promise<ModelDefinition | null> {
  if (get(downloadStatus).kind === "active") return null;
  cancelRequested = false;
  downloadStatus.set({ kind: "active", name, downloaded: 0, total: null, fileIndex: 0 });
  const unlisten = await onDownloadProgress((p) => {
    downloadStatus.update((s) =>
      s.kind === "active"
        ? { ...s, downloaded: p.downloaded, total: p.total, fileIndex: p.file_index, fileCount: p.file_count, fileName: p.file_name }
        : s,
    );
  });
  try {
    const def = await downloadMultifile(entryId, token);
    definitions.update((d) => {
      const rest = d.filter((x) => x.id !== def.id);
      return [...rest, def];
    });
    downloadStatus.set({ kind: "done", name });
    return def;
  } catch (e) {
    downloadStatus.set(cancelRequested ? { kind: "idle" } : { kind: "error", name, message: String(e) });
    return null;
  } finally {
    unlisten();
  }
}
```

- [ ] **Step 4: Verify (still red in components until Task 11)**

Run: `npm run check`
Expected: `api.ts` and `stores.ts` clean; remaining errors only in `ModelLibrary.svelte`, `GenerateBar.svelte`, `ParamsPanel.svelte`.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts src/lib/stores.ts
git commit -m "feat(ui): multi-file API wrappers, definitions store, richer download status"
```

---

## Task 11: Assembly dialog — folder auto-detect + manual flows

A new self-contained dialog. Loads recipes, lets the user assemble a definition two ways (catalog download is added in Task 13), validates required roles, saves, and hands the definition back via a callback. It does not touch `model_path`, so it compiles independently.

**Files:**
- Create: `src/lib/components/ModelAssembly.svelte`

- [ ] **Step 1: Create the component**

Create `src/lib/components/ModelAssembly.svelte`:

```svelte
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
  const basename = (p: string) => p.split("/").pop() ?? p;

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
```

- [ ] **Step 2: Verify the new component type-checks**

Run: `npm run check`
Expected: no NEW errors from `ModelAssembly.svelte` (the only remaining errors are the still-unmigrated `model_path` consumers, fixed next).

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/ModelAssembly.svelte
git commit -m "feat(ui): multi-file assembly dialog (folder auto-detect + manual)"
```

---

## Task 12: Integrate — bootstrap, GenerateBar, ParamsPanel, ModelLibrary dropdown

Migrate the remaining `model_path` consumers and wire the dropdown to list single-file models **and** saved definitions. `npm run check` returns to green (0/0) at the end.

**Files:**
- Modify: `src/routes/+page.svelte`
- Modify: `src/lib/components/GenerateBar.svelte`
- Modify: `src/lib/components/ParamsPanel.svelte`
- Modify: `src/lib/components/ModelLibrary.svelte`

- [ ] **Step 1: Bootstrap — seed `definitions`, build `request.model`**

In `src/routes/+page.svelte`, add `definitions` to the stores import (from `$lib/stores`). Replace line ~45:

```ts
      request.set({ ...cfg.last_request, model_path: cfg.default_model_path ?? cfg.last_request.model_path });
```

with:

```ts
      // Seed the saved multi-file library.
      definitions.set(cfg.model_definitions ?? []);
      // Build the active model: prefer last_request.model; if it's an empty
      // single-file and a legacy default_model_path exists, use that.
      let model = cfg.last_request.model;
      if (model.type === "single_file" && model.path === "" && cfg.default_model_path) {
        model = { type: "single_file", path: cfg.default_model_path };
      }
      request.set({ ...cfg.last_request, model });
```

- [ ] **Step 2: GenerateBar validation**

In `GenerateBar.svelte`, add `modelIsSet` to the types import:

```ts
  import { modelIsSet } from "../types";
```

Replace the model check:

```ts
    if (!modelIsSet(req.model)) { genStatus.set({ kind: "error", message: "Select a model first." }); return; }
```

- [ ] **Step 3: ParamsPanel display**

In `ParamsPanel.svelte`, add to the types import:

```ts
  import { modelLabel } from "../types";
```

Also import the `definitions` store (add to the existing `$lib/stores` import). Replace the Model row:

```svelte
        <span class="k">Model</span><span class="v" title={modelLabel(r.model, $definitions)}>{modelLabel(r.model, $definitions)}</span>
```

(Add `import { definitions } from "$lib/stores";` if that store isn't already imported in this component.)

- [ ] **Step 4: Rework `ModelLibrary.svelte`**

Replace the entire `<script>` block of `ModelLibrary.svelte` with:

```svelte
<script lang="ts">
  import { request, models, definitions, downloadStatus, cancelActiveDownload } from "../stores";
  import { listModels, deleteModel, deleteModelDefinition } from "../api";
  import DownloadDialog from "./DownloadDialog.svelte";
  import ModelAssembly from "./ModelAssembly.svelte";
  import InfoHint from "./InfoHint.svelte";
  import { HELP } from "../helpText";
  import type { ModelDefinition } from "../types";

  let showDownload = $state(false);
  let showAssembly = $state(false);
  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const NEW = "__new_multifile__";
  const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
  const basename = (p: string) => p.split("/").pop() ?? p;

  // The definition currently selected (multi_file model whose diffusion matches).
  const selectedDef = $derived(
    $request.model.type === "multi_file"
      ? $definitions.find((d) => d.components.diffusion_model === ($request.model as any).diffusion_model) ?? null
      : null,
  );

  // A single-file path selected but not in the scanned library (orphan).
  const orphanPath = $derived(
    $request.model.type === "single_file" && $request.model.path && !$models.some((m) => m.path === ($request.model as any).path)
      ? $request.model.path
      : null,
  );

  // Synthetic <select> value: sf:<path> | mf:<id> | "" .
  const selectValue = $derived(
    $request.model.type === "single_file"
      ? $request.model.path ? `sf:${$request.model.path}` : ""
      : selectedDef ? `mf:${selectedDef.id}` : "",
  );

  async function refresh() {
    models.set(await listModels());
  }

  function selectDefinition(def: ModelDefinition) {
    // Snapshot the resolved components so the request stays reproducible.
    request.update((r) => ({ ...r, model: { type: "multi_file", ...def.components } }));
  }

  function onSelect(e: Event) {
    const v = (e.currentTarget as HTMLSelectElement).value;
    if (v === NEW) {
      showAssembly = true;
      return; // don't commit NEW as a model
    }
    if (v.startsWith("sf:")) {
      request.update((r) => ({ ...r, model: { type: "single_file", path: v.slice(3) } }));
    } else if (v.startsWith("mf:")) {
      const def = $definitions.find((d) => d.id === v.slice(3));
      if (def) selectDefinition(def);
    }
  }

  function onAssembled(def: ModelDefinition) {
    definitions.update((d) => [...d.filter((x) => x.id !== def.id), def]);
    selectDefinition(def);
  }

  async function removeSelected() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      if (selectedDef) {
        await deleteModelDefinition(selectedDef.id);
        definitions.update((d) => d.filter((x) => x.id !== selectedDef.id));
        request.update((r) => ({ ...r, model: { type: "single_file", path: "" } }));
      } else if ($request.model.type === "single_file" && $request.model.path) {
        await deleteModel($request.model.path);
        request.update((r) => ({ ...r, model: { type: "single_file", path: "" } }));
        await refresh();
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      confirming = false;
    }
  }

  const hasSelection = $derived(selectValue !== "");

  const dlPct = $derived(
    $downloadStatus.kind === "active" && $downloadStatus.total
      ? Math.round(($downloadStatus.downloaded / $downloadStatus.total) * 100)
      : 0,
  );
  const dlFileSuffix = $derived(
    $downloadStatus.kind === "active" && $downloadStatus.fileCount && $downloadStatus.fileCount > 1
      ? ` (${($downloadStatus.fileIndex ?? 0) + 1}/${$downloadStatus.fileCount})`
      : "",
  );

  let handledDone = $state<string | null>(null);
  $effect(() => {
    const s = $downloadStatus;
    if (s.kind === "done" && handledDone !== s.name) {
      handledDone = s.name;
      refresh().catch((e) => (error = String(e)));
    } else if (s.kind === "idle" || s.kind === "active") {
      handledDone = null;
    }
  });
</script>
```

- [ ] **Step 5: Rework the `ModelLibrary.svelte` markup (the `<div class="field">` block)**

Replace the `<select>` block and delete-button disabled binding. Change the `<select>`:

```svelte
    <select value={selectValue} onchange={onSelect}>
      {#if !hasSelection}<option value="" disabled selected>Select a model…</option>{/if}
      {#if orphanPath}<option value={`sf:${orphanPath}`}>{basename(orphanPath)} — (not in library)</option>{/if}
      {#if $definitions.length > 0}
        <optgroup label="Multi-file models">
          {#each $definitions as d (d.id)}
            <option value={`mf:${d.id}`}>{d.name} — multi-file</option>
          {/each}
        </optgroup>
      {/if}
      <optgroup label="Single-file models">
        {#each $models as m (m.path)}
          <option value={`sf:${m.path}`}>{m.name} — {fmtSize(m.size_bytes)}</option>
        {/each}
      </optgroup>
      <option value={NEW}>＋ New multi-file model…</option>
    </select>
```

Change the Delete button's `disabled` binding from `!$request.model_path` to:

```svelte
      <button class="btn-secondary" disabled={!hasSelection} onclick={() => (confirming = true)}>Delete</button>
```

Update the "no models" hint condition:

```svelte
  {#if $models.length === 0 && $definitions.length === 0}
    <span class="hint">No models found. Click Download… or add a multi-file model.</span>
  {/if}
```

Add the assembly dialog next to the existing download dialog block (after `{#if showDownload}…{/if}`):

```svelte
{#if showAssembly}
  <ModelAssembly onclose={() => (showAssembly = false)} onsaved={onAssembled} />
{/if}
```

Finally, add the file-count suffix to the active-download text (in the `.dl-text` for the active branch), changing `⬇ {$downloadStatus.name}…` to:

```svelte
      <span class="dl-text">⬇ {$downloadStatus.name}{dlFileSuffix}… {fmtSize($downloadStatus.downloaded)}{$downloadStatus.total ? ` / ${fmtSize($downloadStatus.total)} (${dlPct}%)` : "…"}</span>
```

- [ ] **Step 6: Verify green**

Run: `npm run check`
Expected: **0 errors, 0 warnings.**

- [ ] **Step 7: Commit**

```bash
git add src/routes/+page.svelte src/lib/components/GenerateBar.svelte src/lib/components/ParamsPanel.svelte src/lib/components/ModelLibrary.svelte
git commit -m "feat(ui): model dropdown lists single-file + multi-file definitions"
```

---

## Task 13: Catalog download flow, definition editing, and broken-model badge (Phase B finish)

Completes Phase B: a "Download from catalog" flow inside the assembly dialog (encoders fetched once, reused), the ability to edit a saved definition, and a ⚠ badge in the dropdown when a definition's files have gone missing. `npm run check` and `cargo test --lib` both stay green.

**Files:**
- Modify: `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`
- Modify: `src/lib/api.ts`
- Modify: `src/lib/components/ModelAssembly.svelte`
- Modify: `src/lib/components/ModelLibrary.svelte`

- [ ] **Step 1: Backend — `broken_definitions` command**

A definition is "broken" when a component path it references no longer exists on disk. `missing_components()` (Task 2) already reports set-but-absent paths, so the command is a thin filter over the saved definitions. In `src-tauri/src/commands.rs`, add:

```rust
/// Returns the ids of saved model definitions that reference missing files, so
/// the UI can flag them. Filesystem check lives in `missing_components`.
#[tauri::command]
pub fn broken_definitions(state: State<'_, AppState>) -> Vec<String> {
    let cfg = state.config.lock().unwrap();
    cfg.model_definitions
        .iter()
        .filter(|d| !crate::types::missing_components(&d.components).is_empty())
        .map(|d| d.id.clone())
        .collect()
}
```

- [ ] **Step 2: Register it**

In `src-tauri/src/lib.rs`, inside `tauri::generate_handler![...]`, add:

```rust
            commands::broken_definitions,
```

- [ ] **Step 3: Build + test**

Run: `cd src-tauri && cargo test --lib && cargo build`
Expected: tests PASS, build succeeds.

- [ ] **Step 4: Commit the backend**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(models): broken_definitions command (flag definitions with missing files)"
```

- [ ] **Step 5: API wrapper**

In `src/lib/api.ts`, add (near the other definition wrappers):

```ts
export const brokenDefinitions = () => invoke<string[]>("broken_definitions");
```

- [ ] **Step 6: Add the catalog download flow + edit support to `ModelAssembly.svelte`**

Replace the import block and props/state at the top of the `<script>` with:

```svelte
<script lang="ts">
  import { listRecipes, detectFolder, pickFolder, pickModelFile, saveModelDefinition, multifileCatalog } from "../api";
  import { startMultiFileDownload } from "../stores";
  import { ROLE_LABELS, VAE_FORMATS, PREDICTIONS } from "../types";
  import type { RecipeInfo, ComponentRole, ModelComponents, ModelDefinition, RatedMultiFile } from "../types";
  import { onMount } from "svelte";

  let { onclose, onsaved, edit = null }: {
    onclose: () => void;
    onsaved: (def: ModelDefinition) => void;
    edit?: ModelDefinition | null;
  } = $props();

  type Mode = "choose" | "folder" | "manual" | "catalog";
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
  let token = $state("");

  const recipe = $derived(recipes.find((r) => r.family === family));
  const basename = (p: string) => p.split("/").pop() ?? p;
  const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
</script>
```

> Note: the closing `</script>` above is only shown so this block is self-contained — do **not** add a second `</script>`; keep the single one that already wraps the component body.

Replace the `onMount(...)` block with one that also seeds edit mode:

```svelte
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
```

Add these functions (next to `chooseFolder` / `startManual`):

```svelte
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

  async function downloadEntry() {
    if (!selectedEntry || busy) return;
    busy = true;
    error = null;
    try {
      // The download resolves shared encoders/VAE once and returns the saved
      // definition (already persisted by the backend).
      const def = await startMultiFileDownload(selectedEntry.id, token.trim(), selectedEntry.name);
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
```

Update `save()` so editing reuses the existing id (upsert) instead of minting a new one:

```svelte
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
```

- [ ] **Step 7: Add the catalog markup to `ModelAssembly.svelte`**

In the "choose" branch, add a third button (before the Cancel button):

```svelte
      <button class="btn-secondary" onclick={openCatalog}>Download from catalog</button>
```

Add a new branch for the catalog mode. Change the top-level `{:else}` (which currently opens the name/family/slots form) to `{:else if mode === "catalog"}` … `{:else}`, i.e. insert this block **before** the existing form's `{:else}`:

```svelte
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
        <label class="fld"><span>HF access token (optional, for gated models)</span>
          <input class="in" type="password" bind:value={token} placeholder="hf_…" />
        </label>
        {#if error}<p class="err">{error}</p>{/if}
        <div class="row">
          <button class="btn-primary" disabled={!selectedEntry || busy} onclick={downloadEntry}>Download</button>
          <button class="btn-secondary" onclick={onclose} disabled={busy}>Cancel</button>
        </div>
      {/if}
```

Add the catalog styles to the `<style>` block:

```svelte
  .cat { display:flex; flex-direction:column; gap:.35rem; max-height:44vh; overflow-y:auto; }
  .cat-row { display:flex; flex-direction:column; align-items:flex-start; gap:.15rem; text-align:left;
    padding:.5rem; border:1px solid var(--border-subtle); border-radius:6px; background:var(--dialog-bg); color:inherit; }
  .cat-row.sel { border-color:var(--accent); }
  .cat-name { font-size:.82rem; }
  .cat-meta { font-size:.7rem; opacity:.7; }
```

- [ ] **Step 8: Wire the badge + Edit + catalog into `ModelLibrary.svelte`**

Add to the `../api` import: `brokenDefinitions`. Add `edit` state and a broken-id set to the `<script>`:

```svelte
  import { listModels, deleteModel, deleteModelDefinition, brokenDefinitions } from "../api";
```

```svelte
  let brokenIds = $state<Set<string>>(new Set());
  let editing = $state<ModelDefinition | null>(null);

  async function refreshBroken() {
    brokenIds = new Set(await brokenDefinitions());
  }
```

In the existing `refresh()` add a broken re-check so the badge updates after scans:

```svelte
  async function refresh() {
    models.set(await listModels());
    await refreshBroken();
  }
```

Load broken ids on mount (add near the bottom of the `<script>`):

```svelte
  onMount(() => { refreshBroken().catch((e) => (error = String(e))); });
```

(Add `import { onMount } from "svelte";` to the imports.)

After a definition is assembled or edited, re-check broken state:

```svelte
  function onAssembled(def: ModelDefinition) {
    definitions.update((d) => [...d.filter((x) => x.id !== def.id), def]);
    selectDefinition(def);
    editing = null;
    refreshBroken().catch((e) => (error = String(e)));
  }
```

- [ ] **Step 9: Show the badge + Edit button in `ModelLibrary.svelte` markup**

In the multi-file `<optgroup>`, append a ⚠ marker to broken definitions:

```svelte
        <optgroup label="Multi-file models">
          {#each $definitions as d (d.id)}
            <option value={`mf:${d.id}`}>{brokenIds.has(d.id) ? "⚠ " : ""}{d.name} — multi-file</option>
          {/each}
        </optgroup>
```

Add an Edit button in the `.actions` row (only meaningful for a selected definition), before the Delete button in the non-confirming branch:

```svelte
      {#if selectedDef}
        <button class="btn-secondary" onclick={() => (editing = selectedDef)}>Edit…</button>
      {/if}
```

Show a broken warning line under the row when the current selection is broken:

```svelte
  {#if selectedDef && brokenIds.has(selectedDef.id)}
    <span class="err">⚠ Some component files are missing. Edit to re-assign, or delete.</span>
  {/if}
```

Render the assembly dialog for both new and edit (replace the existing `{#if showAssembly}` block):

```svelte
{#if showAssembly || editing}
  <ModelAssembly
    edit={editing}
    onclose={() => { showAssembly = false; editing = null; }}
    onsaved={onAssembled}
  />
{/if}
```

- [ ] **Step 10: Verify green (both toolchains)**

Run: `npm run check`
Expected: **0 errors, 0 warnings.**

Run: `cd src-tauri && cargo test --lib`
Expected: all tests PASS.

- [ ] **Step 11: Commit**

```bash
git add src/lib/api.ts src/lib/components/ModelAssembly.svelte src/lib/components/ModelLibrary.svelte
git commit -m "feat(ui): catalog download flow, definition editing, broken-model badge"
```

---

## Done

After Task 13 the feature is complete: split models load and generate (Phase A), and the curated catalog downloads shared components once and reuses them (Phase B). Both `cargo test --lib` and `npm run check` are green, and every task committed independently.
