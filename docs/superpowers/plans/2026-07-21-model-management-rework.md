# Model Management Rework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the three-shape, config-driven model storage with uniform per-model folders backed by an on-disk `model.json` manifest, split the overloaded model UI into a selection-only surface plus a source-first "New…" dialog, move the catalog to a bundled `catalog.json`, and formalize GGUF.

**Architecture:** A new `manifest` module owns the `model.json` schema, path resolution (relative-in-folder / absolute-pooled), and the manifest→`ModelRef` rule; a new `library` scan builds the model list from `models_dir/*/model.json` (manifest-only, no loose-file scanning). `AppConfig.model_definitions` is removed — on-disk manifests are the sole source of truth; selection still persists via `last_request.model`. The catalog becomes a bundled JSON resource loaded at startup, degrading to empty on malformed input. The UI splits into an inline Select list (New/Edit/Delete) and a source-first New dialog (Catalog / URL / From disk), with duplicated helpers folded into shared modules.

**Tech Stack:** Rust (Tauri v2 commands, serde, `trash`, `uuid`), Svelte 5 runes, TypeScript. Gates: `cargo test` (from `src-tauri/`), `npm run check` (svelte-check). No JS unit-test runner — frontend tasks gate on `npm run check` plus explicit manual verification steps.

**Reference spec:** `docs/superpowers/specs/2026-07-21-model-management-rework-design.md`

**Ordering principle:** Backend is added *additively* first (manifest → library → catalog.json → new commands), the frontend is switched to the new commands, and only then is the old `model_definitions` machinery removed. Every task leaves the tree compiling (`cargo test` + `npm run check` green).

**Security constraints (in force for every task):**
- Plaintext token storage in `config.json` is an APPROVED decision.
- Code must NEVER `{:?}`/log the whole `AppConfig` or any token (plaintext HF/Civitai tokens).
- The Secrets UI (`PreferencesDialog.svelte`) must keep the read-only-token recommendation notice.

---

## File Structure

**New backend files:**
- `src-tauri/src/manifest.rs` — `ModelManifest`, `ManifestSource`, `ManifestComponents`, `ManifestFlags`; serde round-trip; path resolution helpers; `to_model_ref` / `to_components`; the write helper that relativizes in-folder paths.
- `src-tauri/src/library.rs` — `LibraryEntry` DTO + `scan_library(models_dir)` (manifest-only scan, broken detection).
- `src-tauri/resources/catalog.json` — bundled curated catalog (author-seeded).

**Modified backend files:**
- `src-tauri/src/catalog.rs` — replace hardcoded consts with a `catalog.json` loader/validator; unified `CatalogEntry` shape; keep VRAM rating.
- `src-tauri/src/types.rs` — remove `model_definitions` from `AppConfig`; keep `ModelDefinition` only if still referenced (it is removed — see Task 12).
- `src-tauri/src/config.rs` — drop `model_definitions` from `default_config`.
- `src-tauri/src/commands.rs` — new commands (`list_library`, `add_catalog_model`, `add_url_model`, `add_local_model`, `edit_model`, `delete_model_entry`); rewrite `recommended_settings` to consult the manifest; remove `model_definitions`-based code (`referenced_paths`, `broken_definitions`, `save/delete_model_definition`, `download_multifile`, `merged_settings` field).
- `src-tauri/src/lib.rs` — register/deregister commands; declare `mod manifest; mod library;`.
- `src-tauri/tauri.conf.json` — bundle `resources/catalog.json`.

**Modified frontend files:**
- `src/lib/types.ts` — add `ModelManifest`, `LibraryEntry`, `CatalogEntry`, `CatalogSource`; remove `ModelDefinition`, `RatedMultiFile`, `RatedModel` (replaced by `CatalogEntry` + rating), `model_definitions` from `AppConfig`.
- `src/lib/api.ts` — new wrappers; remove obsolete ones.
- `src/lib/stores.ts` — `library` store replaces `models` + `definitions`; single `startDownload` helper; single upsert.
- `src/lib/modelFormat.ts` (new) — shared `fmtSize`, `basename`, VRAM accessor, suitability→badge helper.
- `src/lib/components/ModelLibrary.svelte` — rewritten Select surface (inline list + equal New/Edit/Delete).
- `src/lib/components/NewModelDialog.svelte` (new) — source-first dialog (Catalog / URL / From disk).
- `src/lib/components/ModelEditor.svelte` (new, replaces `ModelAssembly.svelte`) — edit a manifest.
- `src/lib/components/DownloadDialog.svelte` — deleted (folded into New dialog).
- `src/routes/+page.svelte` — load `library` instead of `models`+`definitions` on mount.

---

## Task Index

1. Manifest types + serde round-trip (`manifest.rs`)
2. Manifest path resolution (relative ↔ absolute)
3. Manifest → `ModelRef` rule + `to_components`
4. Manifest write helper (relativize in-folder paths) + `save`/`load`
5. Library scan (`library.rs`) — manifest-only, broken detection
6. `list_library` command + wiring
7. Bundled `catalog.json` schema + loader/validator (`catalog.rs`)
8. Author-seed `catalog.json` (Draw Things mining) + GGUF validation
9. `add_catalog_model` command (download + pool + manifest)
10. `add_url_model` command (single vs multi branch)
11. `add_local_model` command (reference in place) + `edit_model` + `delete_model_entry`
12. `recommended_settings` from manifest; remove `model_definitions` from backend
13. Frontend types + api wrappers
14. Frontend stores dedup (`library`, single download helper) + shared `modelFormat.ts`
15. Select surface rewrite (`ModelLibrary.svelte`)
16. New dialog (`NewModelDialog.svelte`)
17. Model editor (`ModelEditor.svelte`), delete `ModelAssembly.svelte`/`DownloadDialog.svelte`
18. Page wiring + full-suite verification

---

### Task 1: Manifest types + serde round-trip

**Files:**
- Create: `src-tauri/src/manifest.rs`
- Modify: `src-tauri/src/lib.rs:1-15` (add `mod manifest;`)
- Test: in `src-tauri/src/manifest.rs` (`#[cfg(test)] mod tests`)

This task defines the on-disk `model.json` shape and proves it round-trips through JSON, including the `#[serde(default)]` behavior for optional keys and unknown-key tolerance.

- [ ] **Step 1: Declare the module**

In `src-tauri/src/lib.rs`, add `mod manifest;` to the module list (keep alphabetical-ish order, e.g. after `mod hf;`):

```rust
mod hf;
mod library;
mod manifest;
mod models;
```

(You will create `library.rs` in Task 5; declaring it now is fine only once the file exists. For THIS task add only `mod manifest;` and leave `mod library;` out until Task 5.)

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/manifest.rs` with the types and this test module:

```rust
use crate::types::GenDefaults;
use serde::{Deserialize, Serialize};

pub const MANIFEST_FILENAME: &str = "model.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Where a model came from. Serialized with an internal `kind` tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ManifestSource {
    Catalog { catalog_id: String, url: String },
    Url { url: String },
    Local { original_path: String },
}

/// role → stored path. Relative to the model folder when the file lives inside
/// it; absolute when pooled (shared/) or referenced-in-place (local).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestComponents {
    pub diffusion_model: String,
    #[serde(default)]
    pub vae: Option<String>,
    #[serde(default)]
    pub clip_l: Option<String>,
    #[serde(default)]
    pub clip_g: Option<String>,
    #[serde(default)]
    pub t5xxl: Option<String>,
    #[serde(default)]
    pub llm: Option<String>,
}

/// Engine flags (not files).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFlags {
    #[serde(default)]
    pub vae_format: Option<String>,
    #[serde(default)]
    pub prediction: Option<String>,
}

/// The `model.json` document: the on-disk source of truth for one model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub family: String,
    pub source: ManifestSource,
    pub components: ManifestComponents,
    #[serde(default)]
    pub flags: ManifestFlags,
    #[serde(default)]
    pub recommended_settings: Option<GenDefaults>,
}

impl ManifestComponents {
    /// Store a component's path under the field for its role. `Diffusion`
    /// sets the required `String`; every other role sets its `Option<String>`.
    pub fn set_role(&mut self, role: crate::recipes::ComponentRole, stored: String) {
        use crate::recipes::ComponentRole::*;
        match role {
            Diffusion => self.diffusion_model = stored,
            Vae => self.vae = Some(stored),
            ClipL => self.clip_l = Some(stored),
            ClipG => self.clip_g = Some(stored),
            T5xxl => self.t5xxl = Some(stored),
            Llm => self.llm = Some(stored),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ModelManifest {
        ModelManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: "flux1-schnell-def456".into(),
            name: "FLUX.1 schnell (Q4)".into(),
            family: "flux1".into(),
            source: ManifestSource::Catalog {
                catalog_id: "flux1-schnell-q4".into(),
                url: "https://example/flux1-schnell-Q4.gguf".into(),
            },
            components: ManifestComponents {
                diffusion_model: "flux1-schnell-Q4.gguf".into(),
                t5xxl: Some("/models/shared/flux1/t5xxl_fp16.safetensors".into()),
                clip_l: Some("/models/shared/flux1/clip_l.safetensors".into()),
                vae: Some("/models/shared/flux1/ae.safetensors".into()),
                ..Default::default()
            },
            flags: ManifestFlags::default(),
            recommended_settings: None,
        }
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        let back: ModelManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn source_tag_is_kind_snake_case() {
        let json = serde_json::to_string(&sample().source).unwrap();
        assert!(json.contains(r#""kind":"catalog""#), "got {json}");
    }

    #[test]
    fn optional_component_keys_default_to_none() {
        let json = r#"{"diffusion_model":"d.gguf"}"#;
        let c: ManifestComponents = serde_json::from_str(json).unwrap();
        assert_eq!(c.diffusion_model, "d.gguf");
        assert!(c.vae.is_none() && c.t5xxl.is_none() && c.clip_l.is_none());
    }

    #[test]
    fn missing_flags_and_recommended_default() {
        // A minimal manifest lacking flags + recommended_settings must load.
        let json = r#"{
            "schema_version":1,"id":"x","name":"X","family":"sd15",
            "source":{"kind":"local","original_path":"/m/x.safetensors"},
            "components":{"diffusion_model":"/m/x.safetensors"}
        }"#;
        let m: ModelManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.flags, ManifestFlags::default());
        assert!(m.recommended_settings.is_none());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        // Forward-compat: a manifest with a future key (e.g. "tags") still loads.
        let json = r#"{
            "schema_version":1,"id":"x","name":"X","family":"sd15",
            "source":{"kind":"url","url":"https://e/x.safetensors"},
            "components":{"diffusion_model":"x.safetensors"},
            "tags":["anime"],"thumbnail":"t.png"
        }"#;
        let m: ModelManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.id, "x");
    }

    #[test]
    fn set_role_targets_the_right_field() {
        use crate::recipes::ComponentRole;
        let mut c = ManifestComponents::default();
        c.set_role(ComponentRole::Diffusion, "d.gguf".into());
        c.set_role(ComponentRole::T5xxl, "/pool/t5.safetensors".into());
        assert_eq!(c.diffusion_model, "d.gguf");
        assert_eq!(c.t5xxl.as_deref(), Some("/pool/t5.safetensors"));
        assert!(c.vae.is_none() && c.clip_l.is_none());
    }
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd src-tauri && cargo test manifest::`
Expected: 6 tests pass (types compile, round-trip + defaults + unknown-key tolerance + `set_role` hold). serde ignores unknown keys by default, so `unknown_keys_are_ignored` passes without `#[serde(deny_unknown_fields)]`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/manifest.rs src-tauri/src/lib.rs
git commit -m "feat(manifest): add model.json manifest types with serde round-trip"
```

---

### Task 2: Manifest path resolution (relative ↔ absolute)

**Files:**
- Modify: `src-tauri/src/manifest.rs`
- Test: `src-tauri/src/manifest.rs` tests

Component paths are stored relative to the model folder when in-folder, absolute when pooled/referenced. Add resolution that turns a stored path into an absolute path against the model folder.

- [ ] **Step 1: Write the failing test**

Add to `manifest.rs` tests:

```rust
    #[test]
    fn resolve_path_joins_relative_against_model_dir() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        assert_eq!(
            resolve_path(dir, "flux1-schnell-Q4.gguf"),
            std::path::PathBuf::from("/models/flux1-schnell-def456/flux1-schnell-Q4.gguf")
        );
    }

    #[test]
    fn resolve_path_passes_absolute_through() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        let abs = "/models/shared/flux1/t5xxl_fp16.safetensors";
        assert_eq!(resolve_path(dir, abs), std::path::PathBuf::from(abs));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test manifest::tests::resolve_path`
Expected: FAIL — `cannot find function resolve_path`.

- [ ] **Step 3: Implement**

Add to `manifest.rs` (above the tests module):

```rust
use std::path::{Path, PathBuf};

/// Resolve a stored component path to an absolute path. Relative paths resolve
/// against the model's own folder; absolute paths pass through unchanged.
pub fn resolve_path(model_dir: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        model_dir.join(p)
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test manifest::`
Expected: PASS (all manifest tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/manifest.rs
git commit -m "feat(manifest): resolve relative component paths against model folder"
```

---

### Task 3: Manifest → `ModelRef` rule + `to_components`

**Files:**
- Modify: `src-tauri/src/manifest.rs`
- Test: `src-tauri/src/manifest.rs` tests

Implements the spec's manifest→`ModelRef` rule: only `diffusion_model` set AND no flags → `SingleFile` (engine `-m`); any companion or any flag → `MultiFile` (engine `--diffusion-model`). All paths are resolved to absolute first.

- [ ] **Step 1: Write the failing test**

Add to `manifest.rs` tests:

```rust
    use crate::types::{ModelComponents, ModelRef};

    #[test]
    fn to_model_ref_single_when_only_diffusion_and_no_flags() {
        let dir = std::path::Path::new("/models/my-sdxl");
        let m = ModelManifest {
            schema_version: 1,
            id: "my-sdxl".into(),
            name: "My SDXL".into(),
            family: "sdxl".into(),
            source: ManifestSource::Local { original_path: "/dl/sdxl.safetensors".into() },
            components: ManifestComponents {
                diffusion_model: "/dl/sdxl.safetensors".into(),
                ..Default::default()
            },
            flags: ManifestFlags::default(),
            recommended_settings: None,
        };
        assert_eq!(
            m.to_model_ref(dir),
            ModelRef::SingleFile { path: "/dl/sdxl.safetensors".into() }
        );
    }

    #[test]
    fn to_model_ref_multi_when_a_companion_is_set() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        let m = sample(); // has t5xxl/clip_l/vae companions
        match m.to_model_ref(dir) {
            ModelRef::MultiFile(c) => {
                // diffusion relative → resolved against model dir
                assert_eq!(c.diffusion_model, "/models/flux1-schnell-def456/flux1-schnell-Q4.gguf");
                // pooled companion absolute → passed through
                assert_eq!(c.t5xxl.as_deref(), Some("/models/shared/flux1/t5xxl_fp16.safetensors"));
            }
            other => panic!("expected MultiFile, got {other:?}"),
        }
    }

    #[test]
    fn to_model_ref_multi_when_only_flag_set() {
        // No companions, but a vae_format flag forces the --diffusion-model path.
        let dir = std::path::Path::new("/models/sd3");
        let m = ModelManifest {
            schema_version: 1,
            id: "sd3".into(),
            name: "SD3".into(),
            family: "sd3".into(),
            source: ManifestSource::Url { url: "https://e/sd3.safetensors".into() },
            components: ManifestComponents { diffusion_model: "sd3.safetensors".into(), ..Default::default() },
            flags: ManifestFlags { vae_format: Some("sd3".into()), prediction: None },
            recommended_settings: None,
        };
        match m.to_model_ref(dir) {
            ModelRef::MultiFile(c) => assert_eq!(c.vae_format.as_deref(), Some("sd3")),
            other => panic!("expected MultiFile, got {other:?}"),
        }
    }

    #[test]
    fn to_components_resolves_all_paths_and_copies_flags() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        let c: ModelComponents = sample().to_components(dir);
        assert_eq!(c.diffusion_model, "/models/flux1-schnell-def456/flux1-schnell-Q4.gguf");
        assert_eq!(c.vae.as_deref(), Some("/models/shared/flux1/ae.safetensors"));
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test manifest::tests::to_`
Expected: FAIL — `no method named to_model_ref`.

- [ ] **Step 3: Implement**

Add an `impl ModelManifest` block to `manifest.rs`:

```rust
use crate::types::{ModelComponents, ModelRef};

impl ModelManifest {
    /// True when the manifest has no companion files and no engine flags — i.e.
    /// a plain single checkpoint that the engine loads with `-m`.
    fn is_single_file(&self) -> bool {
        let c = &self.components;
        let no_companions = [&c.vae, &c.clip_l, &c.clip_g, &c.t5xxl, &c.llm]
            .into_iter()
            .all(|o| o.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true));
        let no_flags = self.flags.vae_format.is_none() && self.flags.prediction.is_none();
        no_companions && no_flags
    }

    /// Resolve every set component path to absolute (relative → against
    /// `model_dir`) and carry the engine flags. Diffusion is always present.
    pub fn to_components(&self, model_dir: &Path) -> ModelComponents {
        let c = &self.components;
        let opt = |o: &Option<String>| {
            o.as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(|s| resolve_path(model_dir, s).to_string_lossy().into_owned())
        };
        ModelComponents {
            diffusion_model: resolve_path(model_dir, &c.diffusion_model)
                .to_string_lossy()
                .into_owned(),
            vae: opt(&c.vae),
            clip_l: opt(&c.clip_l),
            clip_g: opt(&c.clip_g),
            t5xxl: opt(&c.t5xxl),
            llm: opt(&c.llm),
            vae_format: self.flags.vae_format.clone(),
            prediction: self.flags.prediction.clone(),
        }
    }

    /// The engine-ready reference. Single checkpoint → `SingleFile { -m path }`;
    /// any companion or flag → `MultiFile(components)`.
    pub fn to_model_ref(&self, model_dir: &Path) -> ModelRef {
        if self.is_single_file() {
            ModelRef::SingleFile {
                path: resolve_path(model_dir, &self.components.diffusion_model)
                    .to_string_lossy()
                    .into_owned(),
            }
        } else {
            ModelRef::MultiFile(self.to_components(model_dir))
        }
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test manifest::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/manifest.rs
git commit -m "feat(manifest): resolve manifests to ModelRef (single vs multi rule)"
```

---

### Task 4: Manifest write helper (relativize in-folder paths) + save/load

**Files:**
- Modify: `src-tauri/src/manifest.rs`
- Test: `src-tauri/src/manifest.rs` tests

Writing a manifest must store in-folder files as relative paths (portable folder) and everything else as absolute. Add `relativize`, plus `save_to` / `load_from` for `<model_dir>/model.json`.

- [ ] **Step 1: Write the failing test**

Add to `manifest.rs` tests:

```rust
    #[test]
    fn relativize_makes_in_folder_paths_relative() {
        let dir = std::path::Path::new("/models/abc");
        assert_eq!(relativize(dir, "/models/abc/flux1.gguf"), "flux1.gguf");
    }

    #[test]
    fn relativize_leaves_pooled_and_external_absolute() {
        let dir = std::path::Path::new("/models/abc");
        assert_eq!(
            relativize(dir, "/models/shared/flux1/ae.safetensors"),
            "/models/shared/flux1/ae.safetensors"
        );
        assert_eq!(relativize(dir, "/home/me/dl/x.safetensors"), "/home/me/dl/x.safetensors");
    }

    #[test]
    fn save_then_load_round_trips_on_disk() {
        let dir = std::env::temp_dir().join(format!("muchai-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let m = sample();
        save_to(&dir, &m).unwrap();
        assert!(dir.join(MANIFEST_FILENAME).exists());
        let back = load_from(&dir).unwrap();
        assert_eq!(m, back);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test manifest::tests::relativize`
Expected: FAIL — `cannot find function relativize`.

- [ ] **Step 3: Implement**

Add to `manifest.rs`:

```rust
/// Inverse of `resolve_path`: a path that lives directly under `model_dir`
/// becomes relative (just the tail); anything else stays absolute.
pub fn relativize(model_dir: &Path, abs: &str) -> String {
    match Path::new(abs).strip_prefix(model_dir) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => abs.to_string(),
    }
}

/// Write `<model_dir>/model.json` (pretty). Creates the folder if absent.
pub fn save_to(model_dir: &Path, manifest: &ModelManifest) -> std::io::Result<()> {
    std::fs::create_dir_all(model_dir)?;
    let s = serde_json::to_string_pretty(manifest).expect("manifest serializes");
    std::fs::write(model_dir.join(MANIFEST_FILENAME), s)
}

/// Read + parse `<model_dir>/model.json`. Errors on missing/invalid JSON.
pub fn load_from(model_dir: &Path) -> Result<ModelManifest, String> {
    let path = model_dir.join(MANIFEST_FILENAME);
    let s = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test manifest::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/manifest.rs
git commit -m "feat(manifest): relativize in-folder paths; save/load model.json"
```

---

### Task 5: Library scan (`library.rs`) — manifest-only, broken detection

**Files:**
- Create: `src-tauri/src/library.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod library;`)
- Test: `src-tauri/src/library.rs` tests

Builds the model list by scanning `models_dir/*/model.json`. A folder without `model.json` is ignored (manifest-only). A manifest whose set components are missing on disk is flagged `broken`.

- [ ] **Step 1: Add the module declaration**

In `src-tauri/src/lib.rs` add `mod library;` next to `mod manifest;`.

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/library.rs`:

```rust
use crate::manifest::{self, ModelManifest};
use crate::types::{missing_components, ModelRef};
use serde::Serialize;
use std::path::Path;

/// One row in the model list, resolved from a `model.json` manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    /// Engine-ready reference (single-file or multi-file), all paths absolute.
    pub model: ModelRef,
    /// Engine flags copied from the manifest (so the editor can pre-load them).
    pub flags: crate::manifest::ManifestFlags,
    /// True when one or more SET component files are missing on disk.
    pub broken: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestComponents, ManifestFlags, ManifestSource};

    fn write_manifest(models_dir: &Path, id: &str, diffusion_rel: &str, with_file: bool) {
        let dir = models_dir.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        if with_file {
            std::fs::write(dir.join(diffusion_rel), b"x").unwrap();
        }
        let m = ModelManifest {
            schema_version: 1,
            id: id.into(),
            name: format!("Model {id}"),
            family: "sd15".into(),
            source: ManifestSource::Url { url: "https://e/x.safetensors".into() },
            components: ManifestComponents { diffusion_model: diffusion_rel.into(), ..Default::default() },
            flags: ManifestFlags::default(),
            recommended_settings: None,
        };
        manifest::save_to(&dir, &m).unwrap();
    }

    #[test]
    fn scans_manifest_folders_and_sorts_by_name() {
        let root = std::env::temp_dir().join(format!("muchai-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_manifest(&root, "b-model", "b.safetensors", true);
        write_manifest(&root, "a-model", "a.safetensors", true);
        let lib = scan_library(&root);
        let names: Vec<&str> = lib.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Model a-model", "Model b-model"]);
        assert!(lib.iter().all(|e| !e.broken));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_without_manifest_is_ignored() {
        let root = std::env::temp_dir().join(format!("muchai-lib2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // A bare weights folder from the old layout — no model.json.
        std::fs::create_dir_all(root.join("old-layout")).unwrap();
        std::fs::write(root.join("old-layout/loose.safetensors"), b"x").unwrap();
        write_manifest(&root, "real", "r.safetensors", true);
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].id, "real");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_component_marks_entry_broken() {
        let root = std::env::temp_dir().join(format!("muchai-lib3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_manifest(&root, "gone", "missing.safetensors", false); // manifest but no file
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert!(lib[0].broken, "entry with a missing diffusion file must be broken");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_manifest_is_skipped() {
        let root = std::env::temp_dir().join(format!("muchai-lib4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bad")).unwrap();
        std::fs::write(root.join("bad/model.json"), b"{ not valid json ]").unwrap();
        write_manifest(&root, "good", "g.safetensors", true);
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].id, "good");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_pool_folder_is_not_a_model() {
        let root = std::env::temp_dir().join(format!("muchai-lib5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // shared/ holds no model.json → never an entry.
        std::fs::create_dir_all(root.join("shared/flux1")).unwrap();
        std::fs::write(root.join("shared/flux1/ae.safetensors"), b"x").unwrap();
        write_manifest(&root, "real", "r.safetensors", true);
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].id, "real");
        let _ = std::fs::remove_dir_all(&root);
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cd src-tauri && cargo test library::`
Expected: FAIL — `cannot find function scan_library`.

- [ ] **Step 4: Implement**

Add to `library.rs` (above the tests):

```rust
/// Scan `models_dir/*/model.json` into library entries, sorted by name
/// (case-insensitive). Folders without a valid manifest are ignored
/// (manifest-only). A missing/unreadable `models_dir` yields an empty list.
pub fn scan_library(models_dir: &Path) -> Vec<LibraryEntry> {
    let mut out: Vec<LibraryEntry> = Vec::new();
    let entries = match std::fs::read_dir(models_dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let m = match manifest::load_from(&dir) {
            Ok(m) => m,
            Err(_) => continue, // no/invalid manifest → not a model
        };
        out.push(entry_from_manifest(&dir, &m));
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Build one library entry from an already-loaded manifest + its folder.
/// The single-manifest half of `scan_library`, reused by the add/edit commands.
pub fn entry_from_manifest(model_dir: &Path, m: &ModelManifest) -> LibraryEntry {
    let components = m.to_components(model_dir);
    let broken = !missing_components(&components).is_empty();
    LibraryEntry {
        id: m.id.clone(),
        name: m.name.clone(),
        family: m.family.clone(),
        model: m.to_model_ref(model_dir),
        flags: m.flags.clone(),
        broken,
    }
}
```

Note: `missing_components` takes `&ModelComponents` (from `types.rs`) — `to_components` returns exactly that, with absolute paths, so the existence check is correct. The `shared/` folder has no `model.json`, so `load_from` errors and it's skipped — the dedicated test pins this. `entry_from_manifest` is factored out here (not in Task 9) so the add/edit commands can build an entry without re-scanning; its `Result` is infallible here (returns a value), so the Task 9/11 callers use it directly — see the note in those tasks.

- [ ] **Step 5: Run to verify pass**

Run: `cd src-tauri && cargo test library::`
Expected: PASS (5 tests).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/library.rs src-tauri/src/lib.rs
git commit -m "feat(library): manifest-only scan with broken detection"
```

---

### Task 6: `list_library` command + wiring

**Files:**
- Modify: `src-tauri/src/commands.rs` (add command + import)
- Modify: `src-tauri/src/lib.rs:73-99` (register command)
- Test: none (thin wrapper; logic is covered by `library::` tests)

Exposes the scan to the frontend. It scans the primary `models_dir` only (the shared pool and per-model folders live there; `extra_model_dirs` remain a single-file-scan concept that the manifest world doesn't use).

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, add near the other model commands:

```rust
#[tauri::command]
pub fn list_library(state: State<AppState>) -> Vec<crate::library::LibraryEntry> {
    let models_dir = state.config.lock().unwrap().models_dir.clone();
    crate::library::scan_library(std::path::Path::new(&models_dir))
}
```

- [ ] **Step 2: Register it**

In `src-tauri/src/lib.rs`, add `commands::list_library,` to the `generate_handler!` list.

- [ ] **Step 3: Verify it compiles**

Run: `cd src-tauri && cargo test`
Expected: PASS — whole suite still green (no behavior removed yet).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add list_library command"
```

---

### Task 7: Bundled `catalog.json` schema + loader/validator (`catalog.rs`)

**Files:**
- Create: `src-tauri/resources/catalog.json` (minimal, expanded in Task 8)
- Modify: `src-tauri/tauri.conf.json:31-33` (bundle the resource)
- Modify: `src-tauri/src/catalog.rs` (add loader; keep rating)
- Test: `src-tauri/src/catalog.rs` tests

Replaces the hardcoded `starter_catalog()`/`multi_file_catalog()` consts with a bundled JSON file. One unified `CatalogEntry` shape covers single- and multi-file. Malformed JSON degrades to an empty catalog with a logged warning (never a crash).

- [ ] **Step 1: Create a minimal `catalog.json`**

Create `src-tauri/resources/catalog.json` (Task 8 fills in real entries):

```json
{
  "schema_version": 1,
  "entries": [
    {
      "id": "sd15",
      "name": "Stable Diffusion 1.5",
      "family": "sd15",
      "license": "CreativeML-OpenRAIL-M",
      "source_url": "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5",
      "diffusion": {
        "url": "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors",
        "filename": "v1-5-pruned-emaonly.safetensors",
        "size_bytes": 4265146304
      },
      "shared": [],
      "min_vram_mb": 2048,
      "recommended_vram_mb": 4096
    }
  ]
}
```

- [ ] **Step 2: Bundle the resource**

In `src-tauri/tauri.conf.json`, change the `resources` map to include the catalog:

```json
    "resources": {
      "binaries/engine": "engine",
      "resources/catalog.json": "catalog.json"
    },
```

- [ ] **Step 3: Write the failing test**

The catalog is parsed from a string so the parser is unit-testable without the resource dir. Add to `src-tauri/src/catalog.rs`:

```rust
    #[test]
    fn parses_unified_catalog_json() {
        let json = r#"{
          "schema_version": 1,
          "entries": [
            {"id":"sd15","name":"SD 1.5","family":"sd15","license":"OpenRAIL",
             "source_url":"https://h/sd15",
             "diffusion":{"url":"https://h/sd15.safetensors","filename":"sd15.safetensors","size_bytes":10},
             "shared":[],"min_vram_mb":2048,"recommended_vram_mb":4096}
          ]
        }"#;
        let cat = parse_catalog(json).expect("valid catalog parses");
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].id, "sd15");
        assert_eq!(cat[0].license, "OpenRAIL");
        assert_eq!(cat[0].diffusion.filename, "sd15.safetensors");
    }

    #[test]
    fn malformed_catalog_degrades_to_empty() {
        assert!(parse_catalog("{ not json ]").is_none());
        // load_catalog_or_empty must never panic; wraps parse_catalog.
        let empty = load_catalog_from_str("{ not json ]");
        assert!(empty.is_empty());
    }

    #[test]
    fn accepts_gguf_diffusion_entry() {
        let json = r#"{"schema_version":1,"entries":[
          {"id":"flux1-schnell-q4","name":"FLUX.1 schnell Q4","family":"flux1","license":"Apache-2.0",
           "source_url":"https://h/flux","diffusion":{"url":"https://h/flux1-schnell-Q4_K_M.gguf","filename":"flux1-schnell-Q4_K_M.gguf","size_bytes":0},
           "shared":[],"min_vram_mb":8192,"recommended_vram_mb":12288}
        ]}"#;
        let cat = parse_catalog(json).unwrap();
        assert!(cat[0].diffusion.filename.ends_with(".gguf"));
    }
```

- [ ] **Step 4: Run to verify it fails**

Run: `cd src-tauri && cargo test catalog::tests::parses_unified`
Expected: FAIL — `cannot find function parse_catalog` / types missing.

- [ ] **Step 5: Implement the new catalog types + loader**

In `src-tauri/src/catalog.rs`, add the new types and functions. Keep the existing `Suitability` enum and `rate`-style VRAM logic; replace the const catalogs. Add:

```rust
use serde::Deserialize;

/// One diffusion file in a catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub size_bytes: u64,
}

/// A per-entry component override (ships its own copy instead of the pool).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogShared {
    pub role: crate::recipes::ComponentRole,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub size_bytes: u64,
}

/// A curated catalog entry (single- or multi-file; multi if the family recipe
/// has a non-empty shared list, or the entry lists its own `shared`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    pub license: String,
    pub source_url: String,
    pub diffusion: CatalogFile,
    #[serde(default)]
    pub shared: Vec<CatalogShared>,
    pub min_vram_mb: u64,
    pub recommended_vram_mb: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogDoc {
    #[allow(dead_code)]
    schema_version: u32,
    entries: Vec<CatalogEntry>,
}

/// Parse a catalog document, returning its entries. `None` on malformed JSON.
pub fn parse_catalog(json: &str) -> Option<Vec<CatalogEntry>> {
    serde_json::from_str::<CatalogDoc>(json).ok().map(|d| d.entries)
}

/// Parse, or return an empty catalog (never panics). Used by the loader.
pub fn load_catalog_from_str(json: &str) -> Vec<CatalogEntry> {
    match parse_catalog(json) {
        Some(entries) => entries,
        None => {
            eprintln!("catalog.json is malformed; using an empty catalog");
            Vec::new()
        }
    }
}
```

Add `Serialize` to the derive lists so entries can be returned to the UI (`CatalogEntry`, `CatalogFile`, `CatalogShared` derive both `Serialize, Deserialize`). Ensure `use serde::Serialize;` is present (it already is at the top of `catalog.rs`).

- [ ] **Step 6: Add a rated wrapper for the UI**

Replace `rated_catalog`/`rated_multi_file_catalog` with one rated shape. Add:

```rust
/// A catalog entry plus its VRAM fit verdict, for the New… dialog.
#[derive(Debug, Clone, Serialize)]
pub struct RatedCatalogEntry {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub suitability: Suitability,
}

/// Rate an entry against total VRAM (mirrors the old `rate`).
pub fn rate_entry(entry: &CatalogEntry, vram_total_mb: Option<u64>) -> Suitability {
    match vram_total_mb {
        None => Suitability::Unknown,
        Some(v) if v >= entry.recommended_vram_mb => Suitability::Recommended,
        Some(v) if v >= entry.min_vram_mb => Suitability::Tight,
        Some(_) => Suitability::TooBig,
    }
}

/// The full catalog rated against the given VRAM.
pub fn rated_catalog_entries(entries: Vec<CatalogEntry>, vram_total_mb: Option<u64>) -> Vec<RatedCatalogEntry> {
    entries
        .into_iter()
        .map(|entry| {
            let suitability = rate_entry(&entry, vram_total_mb);
            RatedCatalogEntry { entry, suitability }
        })
        .collect()
}
```

Delete the old `starter_catalog`, `multi_file_catalog`, `rated_catalog`, `rated_multi_file_catalog`, `MultiFileCatalogEntry`, `RatedMultiFile`, `RatedModel`, `CatalogModel`, `ModelKind`, `m()` helper, and their tests (`catalog_is_non_empty_and_well_formed`, `rate_handles_all_branches`, `plan_includes_*`, `plan_skips_*`, `assemble_components_paths_match_plan_downloads`, `plan_puts_overrides_in_model_folder`). The download-planning functions (`plan_downloads`, `assemble_components`, `PlannedDownload`) move to the catalog-download command in Task 9 in an entry-driven form; delete the entry-typed versions here now — they are re-created against `CatalogEntry` in Task 9.

> NOTE for the implementer: Tasks 7 removes the download planning helpers, and Task 9 recreates them against `CatalogEntry`. Between these tasks `download_multifile` will not compile. To keep the tree green, in THIS task also temporarily remove `download_multifile`, `multifile_catalog`, and `starter_models` from `commands.rs` and from the `generate_handler!` list (they are superseded by Tasks 9–10). The frontend still references them until Task 13, but the backend and `cargo test` must stay green; the frontend `npm run check` is not run until Task 13. If you prefer a strictly green frontend at every step, keep the old commands until Task 12 — but the recommended order is to remove them here and accept that `npm run build` is not exercised until Task 18.

- [ ] **Step 7: Run to verify pass**

Run: `cd src-tauri && cargo test catalog::`
Expected: PASS (new parser tests green; old catalog tests deleted).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/catalog.rs src-tauri/resources/catalog.json src-tauri/tauri.conf.json src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(catalog): load bundled catalog.json; unified entry shape"
```

---

### Task 8: Author-seed `catalog.json` + resource loader + GGUF validation

**Files:**
- Modify: `src-tauri/resources/catalog.json` (real entries)
- Modify: `src-tauri/src/catalog.rs` (add `validate_entry`)
- Modify: `src-tauri/src/commands.rs` (resource-dir loader for the runtime)
- Test: `src-tauri/src/catalog.rs` tests

Populate the catalog with ~6–10 verified entries and add validation so a bad entry is dropped (not fatal). This task includes an **authoring activity**: mine `github.com/drawthingsai/community-models` for public HF/Civitai source URLs + licenses, and verify a few load in sd.cpp before inclusion.

- [ ] **Step 1: Write the failing validation test**

Add to `catalog.rs` tests:

```rust
    #[test]
    fn validate_entry_requires_https_and_known_family() {
        let ok = CatalogEntry {
            id: "e".into(), name: "E".into(), family: "flux1".into(),
            license: "Apache-2.0".into(), source_url: "https://h/e".into(),
            diffusion: CatalogFile { url: "https://h/e.gguf".into(), filename: "e.gguf".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 8192, recommended_vram_mb: 12288,
        };
        assert!(validate_entry(&ok).is_ok());

        let mut bad_url = ok.clone();
        bad_url.diffusion.url = "http://insecure/e.gguf".into();
        assert!(validate_entry(&bad_url).is_err(), "non-https diffusion url rejected");

        let mut bad_fam = ok.clone();
        bad_fam.family = "no-such-family".into();
        assert!(validate_entry(&bad_fam).is_err(), "unknown family rejected");

        let mut bad_vram = ok.clone();
        bad_vram.recommended_vram_mb = 1; // < min
        assert!(validate_entry(&bad_vram).is_err(), "recommended < min rejected");
    }

    #[test]
    fn load_catalog_from_str_drops_invalid_entries() {
        // One good gguf entry + one with a bad url → only the good one survives.
        let json = r#"{"schema_version":1,"entries":[
          {"id":"good","name":"Good","family":"flux1","license":"Apache-2.0","source_url":"https://h/g",
           "diffusion":{"url":"https://h/g.gguf","filename":"g.gguf","size_bytes":0},"shared":[],
           "min_vram_mb":8192,"recommended_vram_mb":12288},
          {"id":"bad","name":"Bad","family":"flux1","license":"Apache-2.0","source_url":"https://h/b",
           "diffusion":{"url":"http://insecure/b.gguf","filename":"b.gguf","size_bytes":0},"shared":[],
           "min_vram_mb":8192,"recommended_vram_mb":12288}
        ]}"#;
        let cat = load_catalog_from_str(json);
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].id, "good");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test catalog::tests::validate_entry`
Expected: FAIL — `cannot find function validate_entry`.

- [ ] **Step 3: Implement validation and fold it into the loader**

Add to `catalog.rs`:

```rust
/// Validate a catalog entry: https urls, known family, sane VRAM ordering,
/// non-blank id/name. Accepts `.gguf` and `.safetensors`/`.ckpt` diffusion files.
pub fn validate_entry(e: &CatalogEntry) -> Result<(), String> {
    if e.id.trim().is_empty() || e.name.trim().is_empty() {
        return Err("entry id/name must be non-empty".into());
    }
    if crate::recipes::recipe_for(&e.family).is_none() && e.family != "sdxl" && e.family != "sd15" {
        return Err(format!("unknown family {}", e.family));
    }
    if !e.diffusion.url.starts_with("https://") {
        return Err("diffusion url must be https".into());
    }
    for s in &e.shared {
        if !s.url.starts_with("https://") {
            return Err("shared url must be https".into());
        }
    }
    if e.recommended_vram_mb < e.min_vram_mb {
        return Err("recommended_vram_mb < min_vram_mb".into());
    }
    Ok(())
}
```

Update `load_catalog_from_str` to filter with a warning:

```rust
pub fn load_catalog_from_str(json: &str) -> Vec<CatalogEntry> {
    let entries = match parse_catalog(json) {
        Some(e) => e,
        None => {
            eprintln!("catalog.json is malformed; using an empty catalog");
            return Vec::new();
        }
    };
    entries
        .into_iter()
        .filter(|e| match validate_entry(e) {
            Ok(()) => true,
            Err(why) => {
                eprintln!("catalog entry {} dropped: {why}", e.id);
                false
            }
        })
        .collect()
}
```

Note: `sdxl`/`sd15` are single-file families with no recipe entry (they use the name heuristic), so `validate_entry` allows them explicitly. `flux1`/`flux2`/`sd3`/`qwen-image`/`custom` come from `recipe_for`.

- [ ] **Step 4: Add the runtime resource loader**

In `src-tauri/src/commands.rs`, add a helper mirroring `engine_dir`'s resource resolution:

```rust
/// Load the bundled catalog from the Tauri resource dir, with a dev fallback to
/// the source tree. Missing/malformed → empty catalog (never fatal).
fn load_bundled_catalog(app: &AppHandle) -> Vec<catalog::CatalogEntry> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(res) = app.path().resource_dir() {
        candidates.push(res.join("catalog.json"));
    }
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/catalog.json"));
    for path in candidates {
        if let Ok(s) = std::fs::read_to_string(&path) {
            return catalog::load_catalog_from_str(&s);
        }
    }
    Vec::new()
}

#[tauri::command]
pub fn catalog_entries(app: AppHandle, vram_total_mb: Option<u64>) -> Vec<catalog::RatedCatalogEntry> {
    catalog::rated_catalog_entries(load_bundled_catalog(&app), vram_total_mb)
}
```

Register `commands::catalog_entries` in `lib.rs`'s `generate_handler!`.

- [ ] **Step 5: Author the real catalog entries**

Mine `github.com/drawthingsai/community-models` for entries whose LICENSE points at a public HF/Civitai source. Populate `resources/catalog.json` with ~6–10 entries across `sd15`, `sdxl`, `flux1` (schnell + dev), `sd3`, `qwen-image`, `flux2`, prioritizing **quantized GGUF variants that run on 12GB** (RTX 3060 target). Each entry needs: `id`, `name`, `family`, `license` (the underlying weight license, NOT CC0), `source_url`, `diffusion {url, filename, size_bytes}`, optional `shared` overrides, `min_vram_mb`, `recommended_vram_mb`. For families with a recipe `shared` list (flux1), leave `shared: []` so the family pool is used. **Verify at least the flux1 GGUF and one SD entry actually download + load in sd.cpp** before finalizing. Record provenance in a comment-free JSON (JSON has no comments — keep a note in the commit message instead).

- [ ] **Step 6: Run tests + a smoke check**

Run: `cd src-tauri && cargo test catalog::`
Expected: PASS. Then verify the real file parses and every entry validates:

Run: `cd src-tauri && cargo test` (full suite)
Add a test that loads the actual bundled file and asserts it is non-empty + all entries valid:

```rust
    #[test]
    fn bundled_catalog_file_is_valid() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/catalog.json");
        let s = std::fs::read_to_string(path).expect("bundled catalog exists");
        let entries = parse_catalog(&s).expect("bundled catalog parses");
        assert!(!entries.is_empty(), "seed at least one entry");
        for e in &entries {
            validate_entry(e).unwrap_or_else(|why| panic!("{} invalid: {why}", e.id));
        }
    }
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/resources/catalog.json src-tauri/src/catalog.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(catalog): seed catalog.json from Draw Things sources; validate + gguf-first"
```

---

### Task 9: `add_catalog_model` command (download + write manifest)

**Files:**
- Modify: `src-tauri/src/catalog.rs` (entry-driven download plan)
- Modify: `src-tauri/src/commands.rs` (new command)
- Modify: `src-tauri/src/lib.rs` (register)
- Test: `src-tauri/src/catalog.rs` tests

Downloads a catalog entry into `models_dir/<id>/` (pooling shared components into `models_dir/shared/<family>/`) and writes a `model.json` manifest. Replaces the old `download_multifile`.

- [ ] **Step 1: Write the failing test (download plan)**

The pure planning logic is unit-tested; the actual HTTP + emit lives in the command. Add to `catalog.rs`:

```rust
    #[test]
    fn plan_entry_downloads_pools_shared_and_folds_diffusion() {
        let entry = CatalogEntry {
            id: "flux1-schnell".into(), name: "FLUX.1 schnell".into(), family: "flux1".into(),
            license: "Apache-2.0".into(), source_url: "https://h/flux".into(),
            diffusion: CatalogFile { url: "https://h/flux1-schnell.gguf".into(), filename: "flux1-schnell.gguf".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 8192, recommended_vram_mb: 12288,
        };
        let models_dir = std::path::Path::new("/models");
        let plan = plan_entry_downloads(&entry, models_dir);
        // diffusion goes in the model's own folder
        assert_eq!(plan.model_dir, models_dir.join("flux1-schnell"));
        assert!(plan.files.iter().any(|f| f.dest == plan.model_dir.join("flux1-schnell.gguf")));
        // flux1 recipe shared components (t5xxl/clip_l/ae) pool under shared/flux1
        let shared_dir = models_dir.join("shared").join("flux1");
        assert!(
            plan.files.iter().any(|f| f.dest.starts_with(&shared_dir)),
            "family shared components pooled under shared/<family>"
        );
    }

    #[test]
    fn plan_entry_downloads_single_file_family_has_no_shared() {
        let entry = CatalogEntry {
            id: "sd15".into(), name: "SD 1.5".into(), family: "sd15".into(),
            license: "OpenRAIL".into(), source_url: "https://h/sd15".into(),
            diffusion: CatalogFile { url: "https://h/sd15.safetensors".into(), filename: "sd15.safetensors".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 2048, recommended_vram_mb: 4096,
        };
        let plan = plan_entry_downloads(&entry, std::path::Path::new("/models"));
        assert_eq!(plan.files.len(), 1, "single-file family downloads only the diffusion weight");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test catalog::tests::plan_entry`
Expected: FAIL — `cannot find function plan_entry_downloads`.

- [ ] **Step 3: Implement the entry-driven planner**

Add to `catalog.rs`. The shared components come from the family recipe (`recipe_for(family)`), whose `shared` list carries `ComponentRole` + source URL + filename. Per-entry `shared` overrides take precedence and land in the model's own folder.

```rust
use std::path::{Path, PathBuf};

/// One file to fetch and where to write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub url: String,
    pub dest: PathBuf,
    pub role: crate::recipes::ComponentRole,
    pub size_bytes: u64,
    /// true if pooled under shared/<family> (skip if already present).
    pub shared: bool,
}

/// The full plan for materializing a catalog entry on disk.
#[derive(Debug, Clone)]
pub struct EntryPlan {
    pub model_dir: PathBuf,
    pub shared_dir: PathBuf,
    pub files: Vec<PlannedFile>,
}

/// Compute every file to download for `entry` and its on-disk destination.
/// Diffusion → model folder. Recipe shared components → shared/<family> (pooled).
/// Per-entry `shared` overrides → model folder (not pooled).
pub fn plan_entry_downloads(entry: &CatalogEntry, models_dir: &Path) -> EntryPlan {
    let model_dir = models_dir.join(&entry.id);
    let shared_dir = models_dir.join("shared").join(&entry.family);
    let mut files = Vec::new();

    files.push(PlannedFile {
        url: entry.diffusion.url.clone(),
        dest: model_dir.join(&entry.diffusion.filename),
        role: crate::recipes::ComponentRole::Diffusion,
        size_bytes: entry.diffusion.size_bytes,
        shared: false,
    });

    // Per-entry overrides ship in the model folder.
    let override_roles: std::collections::HashSet<_> =
        entry.shared.iter().map(|s| s.role).collect();
    for s in &entry.shared {
        files.push(PlannedFile {
            url: s.url.clone(),
            dest: model_dir.join(&s.filename),
            role: s.role,
            size_bytes: s.size_bytes,
            shared: false,
        });
    }

    // Family recipe shared components pool under shared/<family>, unless overridden.
    if let Some(recipe) = crate::recipes::recipe_for(&entry.family) {
        for comp in &recipe.shared {
            if override_roles.contains(&comp.role) {
                continue;
            }
            files.push(PlannedFile {
                url: comp.url.to_string(),
                dest: shared_dir.join(comp.filename),
                role: comp.role,
                size_bytes: comp.size_bytes,
                shared: true,
            });
        }
    }

    EntryPlan { model_dir, shared_dir, files }
}
```

> IMPLEMENTER NOTE: No recipe change is needed — `recipes::SharedComponent` already carries `{ role, url: &'static str, size_bytes, filename: &'static str }` and `ModelRecipe.shared` is a public `Vec<SharedComponent>`. `recipe_for(family)` returns an owned `Option<ModelRecipe>`. Convert the `&'static str` url with `.to_string()` and join the `&'static str` filename directly.

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test catalog::tests::plan_entry`
Expected: PASS.

- [ ] **Step 5: Add the `add_catalog_model` command**

In `commands.rs`, add. It resolves the entry from the bundled catalog by id, plans downloads, fetches each file inside `spawn_blocking` (skipping shared files that already exist), rolls back the model folder on failure, then builds + saves a `ModelManifest`. The download/emit/cancel wiring mirrors the existing `download_multifile` (`commands.rs:522`) EXACTLY — `download_to` is synchronous (a progress callback, not `.await`), runs inside `tauri::async_runtime::spawn_blocking`, and emits `model:download:progress` with `DownloadProgress`.

```rust
#[tauri::command]
pub async fn add_catalog_model(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog_id: String,
) -> Result<library::LibraryEntry, String> {
    // Read models_dir + tokens once (never log tokens).
    let (models_dir, hf_token, civitai_token) = {
        let cfg = state.config.lock().unwrap();
        (PathBuf::from(&cfg.models_dir), cfg.hf_token.clone(), cfg.civitai_token.clone())
    };
    if models_dir.as_os_str().is_empty() {
        return Err("models directory is not set".into());
    }
    let entry = load_bundled_catalog(&app)
        .into_iter()
        .find(|e| e.id == catalog_id)
        .ok_or_else(|| format!("unknown catalog entry {catalog_id}"))?;

    // Guard yields a genuine direct subfolder of models_dir (never blank/"shared"/separators).
    let model_dir = safe_model_dir(&models_dir, &entry.id).ok_or_else(|| "invalid model id".to_string())?;
    let plan = catalog::plan_entry_downloads(&entry, &models_dir);
    let file_count = plan.files.len() as u32;

    let cancel = state.download_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let files = plan.files.clone();

    let dl = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        for (i, file) in files.iter().enumerate() {
            if file.shared && file.dest.exists() {
                continue; // pooled component already present
            }
            let token = token_for_url(&file.url, &hf_token, &civitai_token);
            let name = file.dest.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mut last_emit: u64 = 0;
            let app3 = app2.clone();
            let name2 = name.clone();
            downloader::download_to(
                &file.url,
                &token,
                &file.dest,
                move |downloaded, total| {
                    if downloaded.saturating_sub(last_emit) >= 4 << 20 || Some(downloaded) == total {
                        last_emit = downloaded;
                        let _ = app3.emit("model:download:progress", DownloadProgress {
                            downloaded,
                            total,
                            file_index: Some(i as u32),
                            file_count: Some(file_count),
                            file_name: Some(name2.clone()),
                        });
                    }
                },
                &cancel,
            )
            .map_err(|e| e.message())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = dl {
        // Roll back the per-model folder; leave the shared pool intact.
        let _ = std::fs::remove_dir_all(&model_dir);
        return Err(e);
    }

    // Build + save the manifest from the plan (all files now on disk).
    let mut components = manifest::ManifestComponents::default();
    for file in &plan.files {
        components.set_role(file.role, manifest::relativize(&model_dir, &file.dest));
    }
    let man = manifest::ModelManifest {
        schema_version: manifest::MANIFEST_SCHEMA_VERSION,
        id: entry.id.clone(),
        name: entry.name.clone(),
        family: entry.family.clone(),
        source: manifest::ManifestSource::Catalog {
            catalog_id: entry.id.clone(),
            url: entry.source_url.clone(),
        },
        components,
        flags: manifest::ManifestFlags::default(),
        recommended_settings: None,
    };
    manifest::save_to(&model_dir, &man).map_err(|e| e.to_string())?;

    Ok(library::entry_from_manifest(&model_dir, &man))
}
```

Add the token-selection helper (used here and by `add_url_model`) near the other command helpers in `commands.rs`:

```rust
/// Pick the auth token for a download URL by host. HuggingFace → `hf_token`;
/// Civitai → `civitai_token`; anything else → no token (empty string).
/// Bearer auth is applied by `downloader::download_to` only when non-empty.
fn token_for_url(url: &str, hf_token: &str, civitai_token: &str) -> String {
    let u = url.to_lowercase();
    if u.contains("huggingface.co") || u.contains("hf.co") {
        hf_token.to_string()
    } else if u.contains("civitai.com") {
        civitai_token.to_string()
    } else {
        String::new()
    }
}
```

Add a test:

```rust
    #[test]
    fn token_for_url_selects_by_host() {
        assert_eq!(token_for_url("https://huggingface.co/x/y.gguf", "HF", "CV"), "HF");
        assert_eq!(token_for_url("https://civitai.com/api/download/1", "HF", "CV"), "CV");
        assert_eq!(token_for_url("https://example.com/a.safetensors", "HF", "CV"), "");
    }
```

> IMPLEMENTER NOTE:
> - `manifest::ManifestComponents::set_role(role, stored_path)` is defined in Task 1: `Diffusion` sets the `String` `diffusion_model`; all other roles set the matching `Option<String>` field (`Vae`→`vae`, `ClipL`→`clip_l`, `ClipG`→`clip_g`, `T5xxl`→`t5xxl`, `Llm`→`llm`).
> - `library::entry_from_manifest(model_dir, &manifest) -> LibraryEntry` was added in Task 5 (infallible — returns a value, not a `Result`). `manifest::save_to`/`load_from` are free functions (Task 4), not methods.
> - `safe_model_dir(models_dir, id) -> Option<PathBuf>` (existing, `commands.rs:748`) returns `Option`, not `Result` — use `.ok_or_else(...)`.
> - `downloader::download_to(url, token, dest, on_progress, cancel)` is SYNCHRONOUS (`commands.rs:522` shows the exact usage): it takes a `FnMut(u64, Option<u64>)` progress callback and returns `Result<(), DownloadError>` (map the error with `.map_err(|e| e.message())`). It MUST run inside `tauri::async_runtime::spawn_blocking`. `Ordering`, `DownloadProgress`, and `use tauri::Emitter` are already imported in `commands.rs`. `PlannedFile` derives `Clone` (Task 9 Step 3) so `plan.files.clone()` moves into the closure while `plan.files` stays usable for manifest building. NEVER log the token value (security constraint).

- [ ] **Step 6: Register + build**

Add `commands::add_catalog_model` to `generate_handler!` in `lib.rs`.
Run: `cd src-tauri && cargo test` and `cargo build`.
Expected: PASS / compiles.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/catalog.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/manifest.rs src-tauri/src/library.rs src-tauri/src/recipes.rs
git commit -m "feat(commands): add_catalog_model downloads entry + writes manifest"
```

---

### Task 10: `add_url_model` command (single URL → manifest)

**Files:**
- Modify: `src-tauri/src/commands.rs` (new command)
- Modify: `src-tauri/src/lib.rs` (register)
- Test: `src-tauri/src/commands.rs` tests (family inference helper)

Downloads one user-supplied URL (single-file model) into `models_dir/<id>/`, infers family from the filename, and writes a manifest. Replaces the old single-file `download_model` path.

- [ ] **Step 1: Write the failing test (family inference)**

The command's pure part is family inference from a filename. Add to `commands.rs` tests:

```rust
    #[test]
    fn infers_family_from_filename() {
        assert_eq!(infer_single_file_family("flux1-schnell-Q4_K_M.gguf"), "flux1");
        assert_eq!(infer_single_file_family("sd_xl_base_1.0.safetensors"), "sdxl");
        assert_eq!(infer_single_file_family("v1-5-pruned-emaonly.safetensors"), "sd15");
        assert_eq!(infer_single_file_family("qwen-image-Q4.gguf"), "qwen-image");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test commands::tests::infers_family`
Expected: FAIL — `cannot find function infer_single_file_family`.

- [ ] **Step 3: Implement family inference**

This generalizes the old `single_file_family` (which only knew xl→sdxl). Add to `commands.rs`:

```rust
/// Infer a model family from a diffusion filename. Falls back to the sd15/sdxl
/// name heuristic when no family keyword matches.
fn infer_single_file_family(filename: &str) -> String {
    let lower = filename.to_lowercase();
    for (needle, family) in [
        ("flux2", "flux2"),
        ("flux", "flux1"),
        ("qwen", "qwen-image"),
        ("sd3", "sd3"),
    ] {
        if lower.contains(needle) {
            return family.to_string();
        }
    }
    if lower.contains("xl") { "sdxl".into() } else { "sd15".into() }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test commands::tests::infers_family`
Expected: PASS.

- [ ] **Step 5: Add the `add_url_model` command**

The download wiring mirrors `add_catalog_model` (Task 9) EXACTLY — synchronous `downloader::download_to` with a progress callback inside `tauri::async_runtime::spawn_blocking`, emitting `model:download:progress`. There is only ever one file, so `file_index`/`file_count` are `Some(0)`/`Some(1)`.

```rust
#[tauri::command]
pub async fn add_url_model(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    name: String,
) -> Result<library::LibraryEntry, String> {
    if !url.starts_with("https://") {
        return Err("URL must be https".into());
    }
    // Read models_dir + tokens once (never log tokens).
    let (models_dir, hf_token, civitai_token) = {
        let cfg = state.config.lock().unwrap();
        (PathBuf::from(&cfg.models_dir), cfg.hf_token.clone(), cfg.civitai_token.clone())
    };
    if models_dir.as_os_str().is_empty() {
        return Err("models directory is not set".into());
    }
    let id = new_model_id(); // uuid-based, see note
    let model_dir = safe_model_dir(&models_dir, &id).ok_or_else(|| "invalid model id".to_string())?;
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

    let filename = downloader::derive_filename(None, &url);
    let dest = model_dir.join(&filename);

    let cancel = state.download_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let url2 = url.clone();
    let dest2 = dest.clone();
    let name_for_event = filename.clone();

    let dl = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let token = token_for_url(&url2, &hf_token, &civitai_token);
        let mut last_emit: u64 = 0;
        downloader::download_to(
            &url2,
            &token,
            &dest2,
            move |downloaded, total| {
                if downloaded.saturating_sub(last_emit) >= 4 << 20 || Some(downloaded) == total {
                    last_emit = downloaded;
                    let _ = app2.emit("model:download:progress", DownloadProgress {
                        downloaded,
                        total,
                        file_index: Some(0),
                        file_count: Some(1),
                        file_name: Some(name_for_event.clone()),
                    });
                }
            },
            &cancel,
        )
        .map_err(|e| e.message())
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = dl {
        let _ = std::fs::remove_dir_all(&model_dir);
        return Err(e);
    }

    let family = infer_single_file_family(&filename);
    let mut components = manifest::ManifestComponents::default();
    components.set_role(
        crate::recipes::ComponentRole::Diffusion,
        manifest::relativize(&model_dir, &dest),
    );
    let man = manifest::ModelManifest {
        schema_version: manifest::MANIFEST_SCHEMA_VERSION,
        id: id.clone(),
        name: if name.trim().is_empty() { filename.clone() } else { name },
        family,
        source: manifest::ManifestSource::Url { url },
        components,
        flags: manifest::ManifestFlags::default(),
        recommended_settings: None,
    };
    manifest::save_to(&model_dir, &man).map_err(|e| e.to_string())?;
    Ok(library::entry_from_manifest(&model_dir, &man))
}
```

> IMPLEMENTER NOTE:
> - `new_model_id()` returns a filesystem-safe unique id. Implement it with the `uuid` crate: `format!("model-{}", uuid::Uuid::new_v4())`. Confirm `uuid` is a dependency in `src-tauri/Cargo.toml`; if not, add `uuid = { version = "1", features = ["v4"] }`. Add a small test asserting `new_model_id() != new_model_id()`.
> - `safe_model_dir(models_dir, id) -> Option<PathBuf>` returns `Option`, not `Result` — use `.ok_or_else(...)`. It returns the joined `models_dir/<id>` path.
> - `token_for_url` is defined in Task 9. `download_to` is SYNCHRONOUS (see Task 9 note) — the `.await` is only on the `spawn_blocking` join handle, never on `download_to`. Never log the token value.

- [ ] **Step 6: Register + build**

Add `commands::add_url_model` to `generate_handler!`.
Run: `cd src-tauri && cargo test && cargo build`.
Expected: PASS / compiles.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat(commands): add_url_model downloads single URL + writes manifest"
```

---

### Task 11: `add_local_model`, `edit_model`, `delete_model_entry` commands

**Files:**
- Modify: `src-tauri/src/commands.rs` (three commands)
- Modify: `src-tauri/src/lib.rs` (register)
- Test: `src-tauri/src/commands.rs` / `src-tauri/src/manifest.rs` tests

Registers a model from files already on disk (referenced in place, absolute paths), edits an existing manifest's name/family/flags, and deletes a library entry (manifest folder to trash; pooled shared components left intact).

- [ ] **Step 1: Write the failing test (edit applies to manifest)**

Editing is a pure transform on a manifest. Add to `manifest.rs` tests:

```rust
    #[test]
    fn apply_edit_changes_name_family_flags() {
        let mut man = ModelManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: "m".into(), name: "Old".into(), family: "sd15".into(),
            source: ManifestSource::Local { original_path: "/a/b.safetensors".into() },
            components: ManifestComponents::default(),
            flags: ManifestFlags::default(),
            recommended_settings: None,
        };
        man.apply_edit(
            Some("New".into()),
            Some("sdxl".into()),
            Some(ManifestFlags { vae_format: Some("fp16".into()), prediction: None }),
        );
        assert_eq!(man.name, "New");
        assert_eq!(man.family, "sdxl");
        assert_eq!(man.flags.vae_format.as_deref(), Some("fp16"));
        // None leaves fields unchanged
        man.apply_edit(None, None, None);
        assert_eq!(man.name, "New");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test manifest::tests::apply_edit`
Expected: FAIL — no method `apply_edit`.

- [ ] **Step 3: Implement `apply_edit`**

Add to `impl ModelManifest` in `manifest.rs`:

```rust
    /// Apply optional edits. `None` leaves a field unchanged.
    pub fn apply_edit(
        &mut self,
        name: Option<String>,
        family: Option<String>,
        flags: Option<ManifestFlags>,
    ) {
        if let Some(n) = name {
            self.name = n;
        }
        if let Some(f) = family {
            self.family = f;
        }
        if let Some(fl) = flags {
            self.flags = fl;
        }
    }
```

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test manifest::tests::apply_edit`
Expected: PASS.

- [ ] **Step 5: Add the three commands**

```rust
#[tauri::command]
pub fn add_local_model(
    state: tauri::State<'_, AppState>,
    diffusion_path: String,
    name: String,
    family: Option<String>,
) -> Result<library::LibraryEntry, String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    if models_dir.as_os_str().is_empty() {
        return Err("models directory is not set".into());
    }
    let src = PathBuf::from(&diffusion_path);
    if !src.is_file() {
        return Err(format!("no such file: {diffusion_path}"));
    }
    let id = new_model_id();
    let model_dir = safe_model_dir(&models_dir, &id).ok_or_else(|| "invalid model id".to_string())?;
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

    let filename = basename(&diffusion_path);
    let fam = family.unwrap_or_else(|| infer_single_file_family(&filename));
    let mut components = manifest::ManifestComponents::default();
    // Referenced-local: store the ABSOLUTE path (not relativized).
    components.set_role(crate::recipes::ComponentRole::Diffusion, diffusion_path.clone());
    let man = manifest::ModelManifest {
        schema_version: manifest::MANIFEST_SCHEMA_VERSION,
        id: id.clone(),
        name: if name.trim().is_empty() { filename } else { name },
        family: fam,
        source: manifest::ManifestSource::Local { original_path: diffusion_path },
        components,
        flags: manifest::ManifestFlags::default(),
        recommended_settings: None,
    };
    manifest::save_to(&model_dir, &man).map_err(|e| e.to_string())?;
    Ok(library::entry_from_manifest(&model_dir, &man))
}

#[tauri::command]
pub fn edit_model(
    state: tauri::State<'_, AppState>,
    id: String,
    name: Option<String>,
    family: Option<String>,
    flags: Option<manifest::ManifestFlags>,
) -> Result<library::LibraryEntry, String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    let model_dir = models_dir.join(&id);
    let mut man = manifest::load_from(&model_dir).map_err(|e| e.to_string())?;
    man.apply_edit(name, family, flags);
    manifest::save_to(&model_dir, &man).map_err(|e| e.to_string())?;
    Ok(library::entry_from_manifest(&model_dir, &man))
}

#[tauri::command]
pub fn delete_model_entry(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    let model_dir = safe_model_dir(&models_dir, &id).ok_or_else(|| "invalid model id".to_string())?;
    if !model_dir.is_dir() {
        return Err(format!("no such model: {id}"));
    }
    // Trash the whole model folder. Pooled shared/<family> components are left intact.
    trash::delete(&model_dir).map_err(|e| e.to_string())
}
```

> IMPLEMENTER NOTE: `basename` already exists in `commands.rs`. `trash` crate is already a dependency (used by the old delete path). `safe_model_dir` already rejects blank/"shared"/separators — reuse it as the guard so `delete_model_entry` can never trash the shared pool or escape `models_dir`.

- [ ] **Step 6: Register + build**

Add `commands::add_local_model`, `commands::edit_model`, `commands::delete_model_entry` to `generate_handler!`.
Run: `cd src-tauri && cargo test && cargo build`.
Expected: PASS / compiles.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/manifest.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add_local_model, edit_model, delete_model_entry"
```

---

### Task 12: Manifest-based `recommended_settings` + remove `model_definitions`

**Files:**
- Modify: `src-tauri/src/commands.rs` (`recommended_settings` reads manifest)
- Modify: `src-tauri/src/types.rs` (remove `ModelDefinition`, `AppConfig.model_definitions`)
- Modify: `src-tauri/src/config.rs` (`default_config` no longer sets `model_definitions`)
- Remove: `download_model`, `save_model_definition`, `delete_model_definition`, `broken_definitions` (superseded)
- Modify: `src-tauri/src/lib.rs` (drop removed commands from `generate_handler!`)
- Test: `src-tauri/src/commands.rs` tests

This is the "remove the old world" task. After it, `model_definitions` no longer exists anywhere in the backend and `recommended_settings` derives gen defaults from a model's manifest family.

- [ ] **Step 1: Write the failing test**

`recommended_settings` should take a model id, load its manifest, and return `family_defaults(family, diffusion_filename)`. Add to `commands.rs` tests:

```rust
    #[test]
    fn recommended_settings_uses_manifest_family() {
        // flux1 schnell → 4 steps per recipes::family_defaults
        let defaults = recommended_for_family("flux1", "flux1-schnell-Q4_K_M.gguf").unwrap();
        assert_eq!(defaults.steps, 4);
        // flux1 dev → 20 steps
        let dev = recommended_for_family("flux1", "flux1-dev.safetensors").unwrap();
        assert_eq!(dev.steps, 20);
        // custom/unknown family → None (UI hides the button)
        assert!(recommended_for_family("custom", "whatever.safetensors").is_none());
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri && cargo test commands::tests::recommended_settings_uses_manifest`
Expected: FAIL — `cannot find function recommended_for_family`.

- [ ] **Step 3: Implement the family-defaults wrapper + command**

Add to `commands.rs` (thin wrapper over the existing `recipes::family_defaults`, extracted so it is unit-testable without a manifest on disk):

```rust
/// Gen defaults for a family + its diffusion filename (schnell/dev detection).
/// `None` for families without a preset (custom/unknown) — the UI hides the button.
fn recommended_for_family(family: &str, diffusion_filename: &str) -> Option<types::GenDefaults> {
    crate::recipes::family_defaults(family, Some(diffusion_filename))
}

#[tauri::command]
pub fn recommended_settings(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<Option<types::GenDefaults>, String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    let model_dir = models_dir.join(&id);
    let man = manifest::load_from(&model_dir).map_err(|e| e.to_string())?;
    let diffusion = basename(&man.components.diffusion_model);
    Ok(recommended_for_family(&man.family, &diffusion))
}
```

> IMPLEMENTER NOTE: The OLD `recommended_settings(model: ModelRef) -> Option<GenDefaults>` signature changes to `(id: String) -> Result<Option<GenDefaults>, String>` — the `Option` semantics are preserved (custom/unknown families return `None` so the UI hides the "Use recommended settings" button). `recipes::family_defaults` takes `Option<&str>` and returns `Option<GenDefaults>`; `GenDefaults` does NOT implement `Default`, so do not `unwrap_or_default()` — return the `Option` through. `ManifestComponents.diffusion_model` is a plain `String` (always set — Task 1), so `basename(&man.components.diffusion_model)` yields the filename. `manifest::load_from` is a free function (Task 4), not a method.

- [ ] **Step 4: Run to verify pass**

Run: `cd src-tauri && cargo test commands::tests::recommended_settings_uses_manifest`
Expected: PASS.

- [ ] **Step 5: Remove `model_definitions` and dead commands**

1. In `types.rs`: delete the `ModelDefinition` struct and the `model_definitions: Vec<ModelDefinition>` field from `AppConfig`. Keep `ModelRef`, `ModelComponents`, `GenDefaults`, tokens, `low_vram`, `last_request`.
2. In `config.rs`: remove `model_definitions: Vec::new()` from `default_config()`. `#[serde(default)]` on remaining fields means old configs with a stray `model_definitions` key still load (serde ignores unknown fields by default — confirm `AppConfig` does NOT use `#[serde(deny_unknown_fields)]`; it must not, so legacy configs don't fail).
3. In `commands.rs`: delete `download_model` (single-file legacy), `save_model_definition`, `delete_model_definition`, `broken_definitions`, `referenced_paths` (if only used by the old delete path), and the old `single_file_family`. Update `merged_settings` to stop preserving `model_definitions` (that field is gone) while still preserving `last_request` + tokens.
4. In `lib.rs`: remove the deleted commands from `generate_handler!`.

- [ ] **Step 6: Verify the whole backend**

Run: `cd src-tauri && cargo test && cargo build`
Expected: PASS / compiles. If `merged_settings` or config tests reference `model_definitions`, update them to drop that field.

> SECURITY CHECK (in force from the spec): confirm no code `{:?}`-logs `AppConfig` or any token. Grep: `rg 'debug|{:\?}|println|eprintln' src-tauri/src | rg -i 'config|token'` — there must be no line that prints the whole config or a token value.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/types.rs src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "refactor: manifest-based recommended_settings; remove model_definitions"
```

---

> **Frontend gate:** There is no JS unit-test runner in this project. Every frontend task gates on `npm run check` (svelte-check, run from repo root) plus the stated manual verification. Treat a clean `npm run check` as the "tests pass" step.

### Task 13: Frontend types + api wrappers

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/api.ts`
- Verify: `npm run check`

Align the TS surface with the new backend: a `LibraryEntry` type, a `RatedCatalogEntry` type, new invoke wrappers, and removal of `ModelDefinition` + `model_definitions`.

- [ ] **Step 1: Update `types.ts`**

Add:

```ts
export type ManifestFlags = {
  vae_format: string | null;
  prediction: string | null;
};

export type LibraryEntry = {
  id: string;
  name: string;
  family: string;
  model: ModelRef;
  flags: ManifestFlags;
  broken: boolean;
};

export type CatalogFile = { url: string; filename: string; size_bytes: number };
export type CatalogShared = { role: string; url: string; filename: string; size_bytes: number };
export type CatalogEntry = {
  id: string;
  name: string;
  family: string;
  license: string;
  source_url: string;
  diffusion: CatalogFile;
  shared: CatalogShared[];
  min_vram_mb: number;
  recommended_vram_mb: number;
};
export type Suitability = "recommended" | "tight" | "too_big" | "unknown";
export type RatedCatalogEntry = CatalogEntry & { suitability: Suitability };
```

Remove: the `ModelDefinition` type and the `model_definitions` field on `AppConfig`. Keep `ModelRef`, `ModelComponents`, `GenDefaults`, `modelIsSet`, `modelLabel`, `ROLE_LABELS`, `VAE_FORMATS`, `PREDICTIONS`, `SAMPLERS`.

> IMPLEMENTER NOTE: `ManifestFlags` mirrors the backend struct from Task 1 exactly — `vae_format` and `prediction` (both nullable). These are the two engine flags that force the multi-file (`--diffusion-model`) code path. Serde is the contract: the TS field names must match the Rust field names verbatim.

- [ ] **Step 2: Update `api.ts`**

Add wrappers and remove superseded ones:

```ts
import { invoke } from "@tauri-apps/api/core";
import type { LibraryEntry, RatedCatalogEntry, GenDefaults, ManifestFlags } from "./types";

export const listLibrary = () => invoke<LibraryEntry[]>("list_library");

export const catalogEntries = (vramTotalMb: number | null) =>
  invoke<RatedCatalogEntry[]>("catalog_entries", { vramTotalMb });

export const addCatalogModel = (catalogId: string) =>
  invoke<LibraryEntry>("add_catalog_model", { catalogId });

export const addUrlModel = (url: string, name: string) =>
  invoke<LibraryEntry>("add_url_model", { url, name });

export const addLocalModel = (diffusionPath: string, name: string, family: string | null) =>
  invoke<LibraryEntry>("add_local_model", { diffusionPath, name, family });

export const editModel = (
  id: string,
  name: string | null,
  family: string | null,
  flags: ManifestFlags | null,
) => invoke<LibraryEntry>("edit_model", { id, name, family, flags });

export const deleteModelEntry = (id: string) =>
  invoke<void>("delete_model_entry", { id });

export const recommendedSettings = (id: string) =>
  invoke<GenDefaults | null>("recommended_settings", { id });
```

Remove: `listModels`, `downloadModel`, `downloadMultifile`, `saveModelDefinition`, `deleteModelDefinition`, `brokenDefinitions`, `multifileCatalog`, `starterModels`, `listHfVariants` (all superseded). Keep `detectFolder` only if still used by the local-add flow; otherwise remove it too.

> IMPLEMENTER NOTE: Tauri v2's DEFAULT converts camelCase JS keys → snake_case Rust args automatically — confirmed by existing calls in `src/lib/api.ts`: `download_multifile` is invoked with `{ entryId, token }` binding to Rust `entry_id: String, token: String`, and `generate` with `{ request, deviceVramMb }` binding to `device_vram_mb`. No command in this project uses `#[tauri::command(rename_all = ...)]`. So: single-word args (`id`, `url`, `name`, `config`) pass verbatim; multi-word JS args go camelCase (e.g. `catalogId` → Rust `catalog_id`, `diffusionPath` → `diffusion_path`). Match this in every new wrapper.

- [ ] **Step 3: Verify**

Run: `npm run check`
Expected: type errors ONLY in files not yet migrated (stores.ts, components). That's expected mid-migration; the next tasks fix them. Confirm `types.ts` and `api.ts` themselves report no errors.

- [ ] **Step 4: Commit**

```bash
git add src/lib/types.ts src/lib/api.ts
git commit -m "feat(ui): library + catalog types and api wrappers"
```

---

### Task 14: Stores rewrite — single library store + `modelFormat.ts`

**Files:**
- Modify: `src/lib/stores.ts`
- Create: `src/lib/modelFormat.ts`
- Verify: `npm run check`

Collapse `models` + `definitions` into one `library` store; collapse the three near-duplicate download helpers into one; extract label/format helpers.

- [ ] **Step 1: Create `modelFormat.ts`**

```ts
import type { LibraryEntry, Suitability } from "./types";

/** Human label for a library row: name + family badge text. */
export function entryLabel(entry: LibraryEntry): string {
  return entry.name;
}

/** Short family badge. */
export function familyBadge(entry: LibraryEntry): string {
  return entry.family;
}

/** VRAM-fit badge text + tone for a catalog row. */
export function suitabilityBadge(s: Suitability): { text: string; tone: "good" | "warn" | "bad" | "muted" } {
  switch (s) {
    case "recommended": return { text: "Recommended", tone: "good" };
    case "tight": return { text: "Tight fit", tone: "warn" };
    case "too_big": return { text: "Too big", tone: "bad" };
    default: return { text: "Unknown", tone: "muted" };
  }
}
```

- [ ] **Step 2: Rewrite the store**

Replace the `models` + `definitions` stores and the three download helpers with:

```ts
import { writable } from "svelte/store";
import type { LibraryEntry } from "./types";
import { listLibrary } from "./api";

export const library = writable<LibraryEntry[]>([]);

/** Reload the model library from disk. Call after any add/edit/delete. */
export async function refreshLibrary(): Promise<void> {
  library.set(await listLibrary());
}
```

Remove `startDownload`, `startFileDownload`, `startMultiFileDownload`, the `definitions` store, the `models` store, and `definitions.subscribe(...)` mirroring. Selection lives in `request.model` (existing) — keep the `request`/`settings`/`gpuDevices` stores. Where old code set `request.model` from a definition, set it from `LibraryEntry.model` instead.

> IMPLEMENTER NOTE: Search the codebase for every importer of `models`, `definitions`, `startDownload`, `startFileDownload`, `startMultiFileDownload` (`rg "from .*stores" src`) and note them — Tasks 15–18 migrate each. `npm run check` will list them as errors until then.

- [ ] **Step 3: Verify**

Run: `npm run check`
Expected: `stores.ts` + `modelFormat.ts` themselves clean; remaining errors only in components migrated by Tasks 15–17.

- [ ] **Step 4: Commit**

```bash
git add src/lib/stores.ts src/lib/modelFormat.ts
git commit -m "feat(ui): single library store + modelFormat helpers"
```

---

### Task 15: Selection surface — `ModelLibrary.svelte` (inline list, Option B)

**Files:**
- Modify: `src/lib/components/ModelLibrary.svelte` (full rewrite)
- Verify: `npm run check` + manual

Rewrite the overloaded dropdown into the approved **Option B · inline sidebar list**: a selection list of library rows (name + family badge, ⚠ for broken), with `＋ New`, `Edit`, `Delete` buttons of equal width below. Adding is fully removed from this surface (opens `NewModelDialog`, Task 16). Edit opens `ModelEditor` (Task 17).

- [ ] **Step 1: Rewrite the component (Svelte 5 runes)**

```svelte
<script lang="ts">
  import { library, request } from "../stores";
  import { familyBadge } from "../modelFormat";
  import type { LibraryEntry } from "../types";

  // NOTE: `onDelete` is added in Task 17. Until then the Delete button routes
  // through `onEdit` as an interim so this component compiles on its own.
  let { onNew, onEdit }: { onNew: () => void; onEdit: (entry: LibraryEntry) => void } = $props();

  let entries = $state<LibraryEntry[]>([]);
  library.subscribe((v) => (entries = v));

  let selectedId = $state<string | null>(null);
  $effect(() => {
    // Keep a valid selection as the library changes.
    if (entries.length && !entries.some((e) => e.id === selectedId)) {
      selectedId = entries[0].id;
    }
  });

  const selected = $derived(entries.find((e) => e.id === selectedId) ?? null);

  function select(entry: LibraryEntry) {
    selectedId = entry.id;
    if (entry.broken) return;
    request.update((r) => ({ ...r, model: entry.model }));
  }
</script>

<div class="model-library">
  <div class="label">Model</div>
  {#if entries.length === 0}
    <p class="empty">No models yet. Click ＋ New to add one.</p>
  {:else}
    <ul class="rows">
      {#each entries as entry (entry.id)}
        <li>
          <button
            class="row"
            class:selected={entry.id === selectedId}
            class:broken={entry.broken}
            onclick={() => select(entry)}
          >
            {#if entry.broken}⚠ {/if}{entry.name}
            <span class="badge">{familyBadge(entry)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <div class="actions">
    <button class="btn" onclick={onNew}>＋ New</button>
    <button class="btn" disabled={!selected} onclick={() => selected && onEdit(selected)}>Edit</button>
    <button class="btn" disabled={!selected} onclick={() => selected && onEdit(selected)}>Delete</button>
  </div>
</div>

<style>
  .model-library { display: flex; flex-direction: column; gap: 8px; }
  .rows { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 5px; }
  .row {
    width: 100%; text-align: left; padding: 6px 8px; border-radius: 6px;
    border: 1px solid var(--border); background: transparent; cursor: pointer;
    display: flex; justify-content: space-between; align-items: center; gap: 8px;
  }
  .row.selected { background: var(--accent-tint, #223); border-color: var(--accent, #46f); }
  .row.broken { opacity: 0.7; }
  .badge { font-size: 11px; opacity: 0.6; }
  .empty { font-size: 13px; opacity: 0.7; margin: 0; }
  /* Equal-width action buttons (user requirement: New == Edit == Delete). */
  .actions { display: flex; gap: 6px; }
  .actions .btn { flex: 1; font-size: 12px; padding: 5px 0; }
</style>
```

> IMPLEMENTER NOTE: In THIS task both Edit and Delete call `onEdit` (interim — the editor hosts delete anyway). Task 17 adds the `onDelete` prop and points the Delete button at it. The user requirement is explicit: **New, Edit, Delete are equal width** (`flex:1` each) — do not make New span free space. Match existing button classes/tokens in the app (inspect a sibling component for the real `.btn` class name; reuse it rather than inventing `.btn`). `selectedModelId.set(entry.id)` is added to `select()` in Task 18 — don't add it here (the store doesn't exist yet).

- [ ] **Step 2: Verify**

Run: `npm run check` (expect errors only from the not-yet-created `NewModelDialog`/`ModelEditor` imports in the page — added in Tasks 16–17).
Manual: after Task 18 wiring, the list renders library rows, selection updates the request, broken rows are non-selectable.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/ModelLibrary.svelte
git commit -m "feat(ui): inline library selection list (Option B)"
```

---

### Task 16: `NewModelDialog.svelte` — source-first add flow

**Files:**
- Create: `src/lib/components/NewModelDialog.svelte`
- Verify: `npm run check` + manual

The approved source-first dialog: three tabs — **Catalog** (curated list w/ VRAM badges → `addCatalogModel`), **URL** (paste https link + name → `addUrlModel`), **Local file** (pick a file already on disk → `addLocalModel`). On success it refreshes the library and closes.

- [ ] **Step 1: Create the dialog**

```svelte
<script lang="ts">
  import { catalogEntries, addCatalogModel, addUrlModel, addLocalModel } from "../api";
  import { refreshLibrary } from "../stores";
  import { suitabilityBadge } from "../modelFormat";
  import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
  import type { RatedCatalogEntry } from "../types";

  let { vramTotalMb, onClose }: { vramTotalMb: number | null; onClose: () => void } = $props();

  type Tab = "catalog" | "url" | "local";
  let tab = $state<Tab>("catalog");
  let busy = $state(false);
  let error = $state<string | null>(null);

  let catalog = $state<RatedCatalogEntry[]>([]);
  $effect(() => {
    catalogEntries(vramTotalMb).then((c) => (catalog = c)).catch((e) => (error = String(e)));
  });

  let url = $state("");
  let urlName = $state("");
  let localPath = $state("");
  let localName = $state("");

  async function run(fn: () => Promise<unknown>) {
    busy = true; error = null;
    try {
      await fn();
      await refreshLibrary();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function pickLocal() {
    const picked = await openFileDialog({
      multiple: false,
      filters: [{ name: "Model", extensions: ["safetensors", "ckpt", "gguf"] }],
    });
    if (typeof picked === "string") localPath = picked;
  }
</script>

<div class="backdrop" onclick={onClose} role="presentation">
  <div class="dialog" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
    <header>
      <b>Add a model</b>
      <button class="x" onclick={onClose} aria-label="Close">✕</button>
    </header>

    <nav class="tabs">
      <button class:active={tab === "catalog"} onclick={() => (tab = "catalog")}>Catalog</button>
      <button class:active={tab === "url"} onclick={() => (tab = "url")}>URL</button>
      <button class:active={tab === "local"} onclick={() => (tab = "local")}>Local file</button>
    </nav>

    {#if error}<p class="error">{error}</p>{/if}

    {#if tab === "catalog"}
      <ul class="catalog">
        {#each catalog as e (e.id)}
          {@const b = suitabilityBadge(e.suitability)}
          <li>
            <div class="ci">
              <b>{e.name}</b>
              <span class="fam">{e.family}</span>
              <span class="fit {b.tone}">{b.text}</span>
              <span class="lic">{e.license}</span>
            </div>
            <button disabled={busy} onclick={() => run(() => addCatalogModel(e.id))}>Add</button>
          </li>
        {/each}
      </ul>
    {:else if tab === "url"}
      <div class="form">
        <label>URL (https)<input bind:value={url} placeholder="https://…" /></label>
        <label>Name<input bind:value={urlName} placeholder="My model" /></label>
        <button disabled={busy || !url.startsWith("https://")} onclick={() => run(() => addUrlModel(url, urlName))}>
          Download & add
        </button>
      </div>
    {:else}
      <div class="form">
        <label>File
          <div class="pick">
            <input readonly value={localPath} placeholder="Choose a .safetensors/.gguf…" />
            <button onclick={pickLocal}>Browse…</button>
          </div>
        </label>
        <label>Name<input bind:value={localName} placeholder="My model" /></label>
        <button disabled={busy || !localPath} onclick={() => run(() => addLocalModel(localPath, localName, null))}>
          Add (reference in place)
        </button>
      </div>
    {/if}
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(0,0,0,.5); display: flex; align-items: center; justify-content: center; z-index: 50; }
  .dialog { background: var(--panel, #1c1c22); border: 1px solid var(--border); border-radius: 10px; width: min(560px, 92vw); max-height: 82vh; overflow: auto; padding: 14px; }
  header { display: flex; justify-content: space-between; align-items: center; margin-bottom: 10px; }
  .x { background: none; border: none; cursor: pointer; font-size: 15px; }
  .tabs { display: flex; gap: 4px; margin-bottom: 12px; }
  .tabs button { flex: 1; padding: 6px; border: 1px solid var(--border); background: transparent; border-radius: 6px; cursor: pointer; }
  .tabs button.active { background: var(--accent-tint, #223); border-color: var(--accent, #46f); }
  .catalog { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 6px; }
  .catalog li { display: flex; justify-content: space-between; align-items: center; gap: 8px; padding: 8px; border: 1px solid var(--border); border-radius: 6px; }
  .ci { display: flex; flex-direction: column; gap: 2px; }
  .fam, .lic { font-size: 11px; opacity: .6; }
  .fit { font-size: 11px; }
  .fit.good { color: #4caf50; } .fit.warn { color: #e0a030; } .fit.bad { color: #e05252; } .fit.muted { opacity: .6; }
  .form { display: flex; flex-direction: column; gap: 10px; }
  .form label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
  .pick { display: flex; gap: 6px; }
  .pick input { flex: 1; }
  .error { color: #e05252; font-size: 12px; }
</style>
```

> IMPLEMENTER NOTE: Confirm `@tauri-apps/plugin-dialog` is installed (the old local-add / folder-detect flow used a file/dir picker — reuse that same import). If the project used a different dialog import, match it. Reuse existing form/input/button classes from a sibling dialog (e.g. `PreferencesDialog.svelte`) rather than inventing styles — this style block is a fallback if none exist.

- [ ] **Step 2: Verify**

Run: `npm run check` (clean for this file once its imports resolve).
Manual (after Task 18): each tab adds a model and the new row appears in the list.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/NewModelDialog.svelte
git commit -m "feat(ui): source-first NewModelDialog (catalog/url/local)"
```

---

### Task 17: `ModelEditor.svelte` + delete confirmation; remove old components

**Files:**
- Create: `src/lib/components/ModelEditor.svelte`
- Modify: `src/lib/components/ModelLibrary.svelte` (add `onDelete` prop; remove stubs)
- Delete: `src/lib/components/ModelAssembly.svelte`, `src/lib/components/DownloadDialog.svelte`
- Verify: `npm run check` + manual

Editor lets the user rename, change family, and toggle flags (`editModel`), plus a delete confirmation that calls `deleteModelEntry`. Then delete the two dead components.

- [ ] **Step 1: Create `ModelEditor.svelte`**

```svelte
<script lang="ts">
  import { editModel, deleteModelEntry } from "../api";
  import { refreshLibrary } from "../stores";
  import { VAE_FORMATS, PREDICTIONS } from "../types";
  import type { LibraryEntry, ManifestFlags } from "../types";

  let { entry, onClose }: { entry: LibraryEntry; onClose: () => void } = $props();

  let name = $state(entry.name);
  let family = $state(entry.family);
  // "" == default (null). Pre-load from the manifest flags carried on the entry.
  let vaeFormat = $state(entry.flags.vae_format ?? "");
  let prediction = $state(entry.flags.prediction ?? "");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmingDelete = $state(false);

  const FAMILIES = ["sd15", "sdxl", "flux1", "flux2", "sd3", "qwen-image", "custom"];

  async function save() {
    busy = true; error = null;
    try {
      const flags: ManifestFlags = {
        vae_format: vaeFormat === "" ? null : vaeFormat,
        prediction: prediction === "" ? null : prediction,
      };
      await editModel(entry.id, name, family, flags);
      await refreshLibrary();
      onClose();
    } catch (e) { error = String(e); } finally { busy = false; }
  }

  async function doDelete() {
    busy = true; error = null;
    try {
      await deleteModelEntry(entry.id);
      await refreshLibrary();
      onClose();
    } catch (e) { error = String(e); } finally { busy = false; }
  }
</script>

<div class="backdrop" onclick={onClose} role="presentation">
  <div class="dialog" onclick={(e) => e.stopPropagation()} role="dialog" aria-modal="true">
    <header><b>Edit model</b><button class="x" onclick={onClose} aria-label="Close">✕</button></header>
    {#if error}<p class="error">{error}</p>{/if}

    <label>Name<input bind:value={name} /></label>
    <label>Family
      <select bind:value={family}>
        {#each FAMILIES as f}<option value={f}>{f}</option>{/each}
      </select>
    </label>
    <label>VAE format
      <select bind:value={vaeFormat}>
        <option value="">Default</option>
        {#each VAE_FORMATS as v}<option value={v}>{v}</option>{/each}
      </select>
    </label>
    <label>Prediction
      <select bind:value={prediction}>
        <option value="">Default</option>
        {#each PREDICTIONS as p}<option value={p}>{p}</option>{/each}
      </select>
    </label>

    <div class="footer">
      {#if confirmingDelete}
        <span class="warn">Delete “{entry.name}”? Files go to trash.</span>
        <button class="danger" disabled={busy} onclick={doDelete}>Confirm delete</button>
        <button disabled={busy} onclick={() => (confirmingDelete = false)}>Cancel</button>
      {:else}
        <button class="danger" onclick={() => (confirmingDelete = true)}>Delete…</button>
        <span class="spacer"></span>
        <button disabled={busy} onclick={save}>Save</button>
      {/if}
    </div>
  </div>
</div>

<style>
  .backdrop { position: fixed; inset: 0; background: rgba(0,0,0,.5); display: flex; align-items: center; justify-content: center; z-index: 50; }
  .dialog { background: var(--panel, #1c1c22); border: 1px solid var(--border); border-radius: 10px; width: min(440px, 92vw); padding: 14px; display: flex; flex-direction: column; gap: 10px; }
  header { display: flex; justify-content: space-between; align-items: center; }
  .x { background: none; border: none; cursor: pointer; }
  label { display: flex; flex-direction: column; gap: 4px; font-size: 12px; }
  .footer { display: flex; gap: 8px; align-items: center; margin-top: 6px; }
  .spacer { flex: 1; }
  .danger { color: #e05252; }
  .warn { font-size: 12px; color: #e0a030; }
  .error { color: #e05252; font-size: 12px; }
</style>
```

> IMPLEMENTER NOTE: `flags` is already on `LibraryEntry` (Rust struct in Task 5, TS type in Task 13), populated by `entry_from_manifest`. The editor pre-loads `vae_format`/`prediction` from `entry.flags` so Save never silently resets a user's flags. `VAE_FORMATS`/`PREDICTIONS` are existing constants in `types.ts` — confirm their exact exported names and values and use them verbatim.

- [ ] **Step 2: Wire `onDelete` into `ModelLibrary.svelte`**

Add the `onDelete` prop (Task 15 left Delete routing through `onEdit` as an interim). Update the props line:

```svelte
  let { onNew, onEdit, onDelete }:
    { onNew: () => void; onEdit: (e: LibraryEntry) => void; onDelete: (e: LibraryEntry) => void } = $props();
```

Change the Delete button's handler from the interim `onEdit` to `onclick={() => selected && onDelete(selected)}`. Also update the top-of-file NOTE comment (it no longer routes Delete through `onEdit`).

- [ ] **Step 3: Delete dead components**

```bash
git rm src/lib/components/ModelAssembly.svelte src/lib/components/DownloadDialog.svelte
```

- [ ] **Step 4: Verify**

Run: `npm run check`
Expected: errors now only in the page that still imports the deleted components / old props — fixed in Task 18.

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/ModelEditor.svelte src/lib/components/ModelLibrary.svelte
git commit -m "feat(ui): ModelEditor + delete confirmation; remove ModelAssembly/DownloadDialog"
```

---

### Task 18: Page wiring + full verification

**Files:**
- Modify: the page/route hosting the model sidebar (e.g. `src/routes/+page.svelte` or `src/App.svelte` — find the current importer of `ModelLibrary`)
- Modify: `src/lib/components/SettingsPanel.svelte` (recommended-settings call → id)
- Verify: `npm run check`, `cargo test`, manual end-to-end

Wire the new dialogs into the page, feed VRAM total to the New dialog, refresh the library on startup, and update the "Use recommended settings" call to pass the selected model id. Then run the full manual flow.

- [ ] **Step 1: Find the host + current wiring**

Run: `rg -l "ModelLibrary" src` and open the host component. Note how it imported `ModelLibrary`, `ModelAssembly`, `DownloadDialog`, `models`, `definitions` previously.

- [ ] **Step 2: Wire the new surface**

In the host component:

```svelte
<script lang="ts">
  import ModelLibrary from "$lib/components/ModelLibrary.svelte";
  import NewModelDialog from "$lib/components/NewModelDialog.svelte";
  import ModelEditor from "$lib/components/ModelEditor.svelte";
  import { refreshLibrary, gpuDevices } from "$lib/stores";
  import type { LibraryEntry } from "$lib/types";
  import { onMount } from "svelte";

  let showNew = $state(false);
  let editing = $state<LibraryEntry | null>(null);
  let deleting = $state<LibraryEntry | null>(null);

  // total VRAM for catalog rating (reuse existing gpuDevices store shape)
  let vramTotalMb = $state<number | null>(null);
  gpuDevices.subscribe((devs) => {
    const total = devs?.reduce?.((sum, d) => sum + (d.vram_mb ?? 0), 0) ?? 0;
    vramTotalMb = total > 0 ? total : null;
  });

  onMount(refreshLibrary);
</script>

<ModelLibrary
  onNew={() => (showNew = true)}
  onEdit={(e) => (editing = e)}
  onDelete={(e) => (editing = e)}
/>

{#if showNew}
  <NewModelDialog {vramTotalMb} onClose={() => (showNew = false)} />
{/if}
{#if editing}
  <ModelEditor entry={editing} onClose={() => (editing = null)} />
{/if}
```

> IMPLEMENTER NOTE: Delete routes through the editor's Delete… button (single surface for edit+delete), so `onDelete` opens the editor too. If you prefer a separate confirm dialog, keep `deleting` and render a small confirm — but the simplest correct wiring is the one above. Match the host's real import alias (`$lib` vs relative) and its existing GPU-device store shape (`vram_mb` field name — verify against `types.ts`/`stores.ts`).

- [ ] **Step 3: Update SettingsPanel recommended-settings call**

In `SettingsPanel.svelte`, the "Use recommended settings" button previously called `recommendedSettings(model: ModelRef)`. Change it to pass the selected model id. The selected entry's id is needed — source it from the `library` store matched against `request.model`, or lift the selected id into a shared store. Simplest: add a `selectedModelId` writable to `stores.ts`, set it in `ModelLibrary.select()`, and read it here.

```ts
// stores.ts
export const selectedModelId = writable<string | null>(null);
```

```svelte
<!-- SettingsPanel.svelte -->
import { recommendedSettings } from "$lib/api";
import { selectedModelId, settings } from "$lib/stores";
let id: string | null = null;
selectedModelId.subscribe((v) => (id = v));

async function useRecommended() {
  if (!id) return;
  const requested = id;             // capture for the stale-fetch guard
  const d = await recommendedSettings(requested);
  if (requested !== id) return;     // selection changed mid-flight → discard (preserves a501753)
  if (!d) return;                   // custom/unknown family → no preset
  settings.update((s) => ({ ...s, steps: d.steps, cfg_scale: d.cfg_scale, sampler: d.sampler, width: d.width, height: d.height }));
}
```

And in `ModelLibrary.svelte`'s `select()`, add `selectedModelId.set(entry.id)` (import it). This preserves the stale-fetch-guard intent from commit `a501753` — the `requested !== id` check above discards a recommended-settings response if the selection changed mid-flight.

> IMPLEMENTER NOTE: Commit `a501753` added "discard stale recommended-settings fetch on model switch". Preserve that behavior: in `useRecommended`, capture `const requested = id;` before the await and, after it resolves, `if (requested !== id) return;` before applying. Do not regress this fix.

- [ ] **Step 4: Full verification**

Run all gates:
```bash
npm run check          # 0 errors
cd src-tauri && cargo test && cargo build && cd ..
npm run build          # production build compiles
```
Expected: all clean.

- [ ] **Step 5: Manual end-to-end (the spec's acceptance path)**

With a dev build (`npm run tauri dev`):
1. Fresh models dir (empty) → list shows "No models yet".
2. ＋ New → Catalog tab shows entries with VRAM badges matching the machine (RTX 3060 = 12GB → flux1 GGUF "Recommended/Tight", big fp16 "Too big").
3. Add a small catalog model → download progresses → row appears → `models_dir/<id>/model.json` exists and weights are in the folder; flux1 shared components pooled under `models_dir/shared/flux1/`.
4. Select it → generate an image successfully.
5. ＋ New → URL tab → paste an https single-file link + name → downloads + appears.
6. ＋ New → Local file → pick an on-disk weight → appears (referenced in place; file NOT copied).
7. Edit → rename + change family + set VAE format / prediction flags → Save → row updates; re-open shows persisted `flags` values.
8. Delete… → Confirm → row disappears; folder is in trash; pooled `shared/flux1` remains for other flux1 models.
9. "Use recommended settings" for a flux1 schnell model sets steps=4; switching selection mid-fetch does not clobber (stale-guard holds).
10. Restart app → library re-scans from manifests; selection (last_request.model) restored.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(ui): wire library sidebar, New/Edit dialogs, id-based recommended settings"
```

---

## Verification Summary (all tasks)

- **Backend gate:** `cd src-tauri && cargo test && cargo build` — green after every backend task.
- **Frontend gate:** `npm run check` — clean after Task 18 (mid-migration errors are expected and localized).
- **Production build:** `npm run build` — compiles (Task 18).
- **Security:** no `{:?}`/log of `AppConfig` or tokens (Task 12 grep); PreferencesDialog keeps the read-only-token recommendation notice (unchanged by this rework — verify it still renders).
- **Manual acceptance:** the 10-step end-to-end path in Task 18 Step 5.
- **Every task leaves the tree compiling** on the backend; the frontend switches over Tasks 13–18 with `npm run check` green at Task 18.
