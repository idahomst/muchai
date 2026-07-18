# Model Library & Downloads Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users manage a library of image models (a primary managed folder plus user-added watched folders) and acquire new single-file checkpoints from inside MuchAI via a curated, hardware-aware starter list and a paste-URL downloader.

**Architecture:** Three new Rust modules (`models.rs` scanning, `catalog.rs` starter list + suitability, `downloader.rs` streaming download) plus thin Tauri command adapters, mirrored by new Svelte components (`ModelLibrary`, `DownloadDialog`, `ModelFolders`). Pure logic is unit-tested with `cargo test`; IO/HTTP runs on worker threads exactly like the existing engine; frontend is validated with `npm run check` and manual E2E.

**Tech Stack:** Rust (Tauri v2), `ureq` (rustls) for downloads, `thiserror`, SvelteKit (Svelte 5 runes), existing `serde` snake_case wire contract.

---

## Reference: current code facts (read before starting)

- `src-tauri/src/types.rs` holds all serde wire DTOs (`GenerationRequest`, `GalleryItem`, `AppConfig`, `SystemStats`, `GpuStats`). New DTOs go here unless owned by a module.
- `src-tauri/src/config.rs` has `default_config()`, `load_config_from()`, `save_config_to()`, `default_gallery_dir()`, `config_file_path()`, `project_dirs()` (private).
- `src-tauri/src/commands.rs` defines `pub struct AppState { pub config: Mutex<AppConfig>, pub child: ChildSlot }` and all `#[tauri::command]`s. `pick_gallery_dir`/`pick_model_file` show the dialog pattern. The active model is `last_request.model_path` (consumed by the engine via `-m`).
- `src-tauri/src/lib.rs` builds `AppState`, runs the stats loop, and lists every command in `tauri::generate_handler![...]`.
- `src/lib/types.ts` mirrors the Rust wire types; `src/lib/api.ts` wraps `invoke`; `src/lib/stores.ts` holds writables. Newer components (`GalleryLocation.svelte`) use Svelte 5 runes (`$state`, `onclick`); use that style for new components.
- The engine call and `command_builder.rs` are UNCHANGED by this plan.

---

## Task 1: Config gains a models library location

**Files:**
- Modify: `src-tauri/src/types.rs` (the `AppConfig` struct)
- Modify: `src-tauri/src/config.rs` (`default_config`, `load_config_from`, new `default_models_dir`)

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src-tauri/src/config.rs`:

```rust
    #[test]
    fn default_config_has_models_dir() {
        let cfg = default_config();
        assert!(!cfg.models_dir.is_empty());
        assert!(cfg.extra_model_dirs.is_empty());
    }

    #[test]
    fn old_config_without_model_fields_loads_and_backfills_models_dir() {
        let dir = std::env::temp_dir().join(format!("muchai-cfg-old-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no models_dir / extra_model_dirs keys.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.models_dir.is_empty(), "empty models_dir must be backfilled to default");
        assert!(cfg.extra_model_dirs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test config::`
Expected: FAIL — `no field models_dir on type AppConfig` (compile error).

- [ ] **Step 3: Add the fields to `AppConfig`**

In `src-tauri/src/types.rs`, replace the `AppConfig` struct with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub sd_binary_path: Option<String>, // None => use bundled sidecar
    pub default_model_path: Option<String>,
    pub gallery_dir: String,
    /// Primary managed models folder; downloads land here.
    #[serde(default)]
    pub models_dir: String,
    /// Additional folders MuchAI scans and merges into the model list.
    #[serde(default)]
    pub extra_model_dirs: Vec<String>,
    pub last_request: GenerationRequest,
}
```

- [ ] **Step 4: Implement default + backfill in `config.rs`**

In `src-tauri/src/config.rs`, add a `default_models_dir` next to `default_gallery_dir`:

```rust
pub fn default_models_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().join("models"))
        .unwrap_or_else(|| PathBuf::from("./models"))
}
```

Set it in `default_config()` — replace the struct literal with:

```rust
pub fn default_config() -> AppConfig {
    AppConfig {
        sd_binary_path: None,
        default_model_path: None,
        gallery_dir: default_gallery_dir().to_string_lossy().into_owned(),
        models_dir: default_models_dir().to_string_lossy().into_owned(),
        extra_model_dirs: Vec::new(),
        last_request: GenerationRequest::default(),
    }
}
```

Backfill an empty `models_dir` (old configs) in `load_config_from()` — replace its body with:

```rust
pub fn load_config_from(path: &Path) -> AppConfig {
    let mut cfg = match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| default_config()),
        Err(_) => default_config(),
    };
    if cfg.models_dir.is_empty() {
        cfg.models_dir = default_models_dir().to_string_lossy().into_owned();
    }
    cfg
}
```

- [ ] **Step 5: Fix the existing round-trip test**

The `save_then_load_round_trips` test builds `default_config()` (now with `models_dir` set) and compares — it still passes because save writes the field. No change needed unless it fails; if it does, ensure `default_config()` is used to build the expected value (it already is).

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd src-tauri && cargo test config::`
Expected: PASS (all config tests).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/config.rs
git commit -m "feat(models): add models_dir + extra_model_dirs to config"
```

---

## Task 2: Scan folders into a model list (`models.rs`)

**Files:**
- Modify: `src-tauri/src/types.rs` (add `ModelInfo`)
- Create: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod models;`)

- [ ] **Step 1: Add the `ModelInfo` DTO**

In `src-tauri/src/types.rs`, after `GalleryItem`, add:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    /// Absolute path passed to the engine via `-m`.
    pub path: String,
    /// File stem, shown in the UI.
    pub name: String,
    pub size_bytes: u64,
}
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/models.rs`:

```rust
use crate::types::ModelInfo;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path, bytes: usize) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn finds_model_files_recursively_and_ignores_others() {
        let root = std::env::temp_dir().join(format!("muchai-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("a.safetensors"), 10);
        touch(&root.join("sub/b.ckpt"), 20);
        touch(&root.join("sub/c.gguf"), 30);
        touch(&root.join("notes.txt"), 5);
        touch(&root.join("image.png"), 5);

        let models = scan_models(&[root.clone()]);
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]); // sorted, extensions filtered
        assert_eq!(models[0].size_bytes, 10);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_directory_is_skipped_not_an_error() {
        let models = scan_models(&[PathBuf::from("/no/such/muchai/dir")]);
        assert!(models.is_empty());
    }

    #[test]
    fn deduplicates_when_a_file_is_reachable_via_two_dirs() {
        let root = std::env::temp_dir().join(format!("muchai-dedup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("models/x.safetensors"), 1);
        // Scan the parent AND the child: x is reachable from both.
        let models = scan_models(&[root.clone(), root.join("models")]);
        assert_eq!(models.len(), 1, "same file must appear once");
        let _ = fs::remove_dir_all(&root);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && cargo test models::`
Expected: FAIL — `cannot find function scan_models`.

- [ ] **Step 4: Implement the scanner**

Add to the top of `src-tauri/src/models.rs` (above the test module):

```rust
const MODEL_EXTS: [&str; 3] = ["safetensors", "ckpt", "gguf"];

fn is_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MODEL_EXTS.iter().any(|m| e.eq_ignore_ascii_case(m)))
        .unwrap_or(false)
}

fn collect(dir: &Path, out: &mut Vec<ModelInfo>, seen: &mut HashSet<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // missing / unreadable dirs are skipped
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out, seen);
        } else if is_model_file(&path) {
            let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !seen.insert(canon) {
                continue; // already found via another watched dir
            }
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(ModelInfo {
                path: path.to_string_lossy().into_owned(),
                name,
                size_bytes,
            });
        }
    }
}

/// Scan every directory (recursively), returning unique model files sorted by
/// name. Missing/unreadable directories are skipped silently.
pub fn scan_models(dirs: &[PathBuf]) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for dir in dirs {
        collect(dir, &mut out, &mut seen);
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}
```

Register the module: add `mod models;` to the `mod` list at the top of `src-tauri/src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test models::`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/models.rs src-tauri/src/lib.rs
git commit -m "feat(models): recursive model-folder scanner with dedup"
```

---

## Task 3: Curated starter catalog + hardware suitability (`catalog.rs`)

**Files:**
- Create: `src-tauri/src/catalog.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod catalog;`)

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/catalog.rs`:

```rust
use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty_and_well_formed() {
        let c = starter_catalog();
        assert!(!c.is_empty());
        for m in &c {
            assert!(m.url.starts_with("https://"), "{} must be https", m.id);
            assert!(m.recommended_vram_mb >= m.min_vram_mb);
        }
    }

    #[test]
    fn rate_handles_all_branches() {
        let sdxl = starter_catalog()
            .into_iter()
            .find(|m| matches!(m.kind, ModelKind::Sdxl))
            .unwrap();
        assert_eq!(rate(&sdxl, None), Suitability::Unknown);
        assert_eq!(rate(&sdxl, Some(sdxl.recommended_vram_mb)), Suitability::Recommended);
        assert_eq!(rate(&sdxl, Some(sdxl.recommended_vram_mb + 4096)), Suitability::Recommended);
        assert_eq!(rate(&sdxl, Some(sdxl.min_vram_mb)), Suitability::Tight);
        assert_eq!(rate(&sdxl, Some(sdxl.min_vram_mb - 1)), Suitability::TooBig);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test catalog::`
Expected: FAIL — `cannot find function starter_catalog`.

- [ ] **Step 3: Implement the catalog and rating**

Add above the test module in `src-tauri/src/catalog.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Sd15,
    Sdxl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Suitability {
    Recommended,
    Tight,
    TooBig,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub url: String,
    /// Approximate download size, for display only; the real size comes from
    /// the server's Content-Length at download time.
    pub size_bytes: u64,
    pub kind: ModelKind,
    pub min_vram_mb: u64,
    pub recommended_vram_mb: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RatedModel {
    #[serde(flatten)]
    pub model: CatalogModel,
    pub suitability: Suitability,
}

fn m(id: &str, name: &str, url: &str, size_bytes: u64, kind: ModelKind, min: u64, rec: u64) -> CatalogModel {
    CatalogModel {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        size_bytes,
        kind,
        min_vram_mb: min,
        recommended_vram_mb: rec,
    }
}

/// The built-in single-file starter models. Public, free-to-download checkpoints.
pub fn starter_catalog() -> Vec<CatalogModel> {
    vec![
        m(
            "sd15",
            "Stable Diffusion 1.5",
            "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors",
            4_265_146_304,
            ModelKind::Sd15,
            2048,
            4096,
        ),
        m(
            "sdxl-base",
            "SDXL Base 1.0",
            "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors",
            6_938_040_706,
            ModelKind::Sdxl,
            6144,
            8192,
        ),
    ]
}

/// Rate a model against the first GPU's total VRAM (None = no GPU detected).
pub fn rate(model: &CatalogModel, vram_total_mb: Option<u64>) -> Suitability {
    match vram_total_mb {
        None => Suitability::Unknown,
        Some(v) if v >= model.recommended_vram_mb => Suitability::Recommended,
        Some(v) if v >= model.min_vram_mb => Suitability::Tight,
        Some(_) => Suitability::TooBig,
    }
}

/// The catalog rated against the given VRAM, ready to return to the UI.
pub fn rated_catalog(vram_total_mb: Option<u64>) -> Vec<RatedModel> {
    starter_catalog()
        .into_iter()
        .map(|model| {
            let suitability = rate(&model, vram_total_mb);
            RatedModel { model, suitability }
        })
        .collect()
}
```

Register the module: add `mod catalog;` to `src-tauri/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test catalog::`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/catalog.rs src-tauri/src/lib.rs
git commit -m "feat(models): curated starter catalog + VRAM suitability rating"
```

---

## Task 4: Download filename helpers + `ureq` dependency

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `ureq`)
- Create: `src-tauri/src/downloader.rs` (pure helpers + tests here; streaming added in Task 5)
- Modify: `src-tauri/src/lib.rs` (add `mod downloader;`)

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml`, under `[dependencies]`, add:

```toml
ureq = "2"
```

(`ureq` 2.x bundles rustls + webpki-roots by default, so HTTPS works with no system OpenSSL — keeping the AppImage self-contained.)

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/downloader.rs`:

```rust
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_prefers_content_disposition() {
        let cd = Some("attachment; filename=\"sd_xl_base_1.0.safetensors\"");
        assert_eq!(derive_filename(cd, "https://x/y?dl=1"), "sd_xl_base_1.0.safetensors");
    }

    #[test]
    fn filename_falls_back_to_url_last_segment() {
        assert_eq!(
            derive_filename(None, "https://huggingface.co/a/b/resolve/main/model.safetensors?download=true"),
            "model.safetensors"
        );
    }

    #[test]
    fn filename_sanitizes_and_defaults() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename(""), "model.safetensors");
        assert_eq!(sanitize_filename("a/b\\c.safetensors"), "c.safetensors");
    }

    #[test]
    fn unique_path_suffixes_on_collision() {
        let dir = std::env::temp_dir().join(format!("muchai-uniq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.safetensors"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "m.safetensors").file_name().unwrap(), "m (1).safetensors");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && cargo test downloader::`
Expected: FAIL — `cannot find function derive_filename`.

- [ ] **Step 4: Implement the helpers**

Add above the test module in `src-tauri/src/downloader.rs`:

```rust
/// Strip any directory components and control chars; fall back to a default.
pub fn sanitize_filename(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .replace(|c: char| c.is_control(), "");
    if base.is_empty() {
        "model.safetensors".to_string()
    } else {
        base
    }
}

/// Decide the output filename from the response headers, then the URL.
pub fn derive_filename(content_disposition: Option<&str>, url: &str) -> String {
    if let Some(cd) = content_disposition {
        if let Some(idx) = cd.to_lowercase().find("filename=") {
            let raw = cd[idx + "filename=".len()..]
                .trim()
                .trim_matches('"')
                .split(';')
                .next()
                .unwrap_or("");
            let cleaned = sanitize_filename(raw);
            if cleaned != "model.safetensors" {
                return cleaned;
            }
        }
    }
    let path = url.split('?').next().unwrap_or(url);
    sanitize_filename(path)
}

/// Return `dir/filename`, appending " (n)" before the extension if it exists.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 1.. {
        let name = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}
```

Register the module: add `mod downloader;` to `src-tauri/src/lib.rs`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test downloader::`
Expected: PASS (4 tests). First run also downloads/compiles `ureq`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/downloader.rs src-tauri/src/lib.rs
git commit -m "feat(models): add ureq + download filename helpers"
```

---

## Task 5: Streaming download with progress + cancel (`downloader.rs`)

**Files:**
- Modify: `src-tauri/src/downloader.rs` (add `DownloadError` + `download_model`)

No unit test exercises live HTTP; correctness of the streaming loop is verified by compilation here and manual E2E in Task 11. The pure helpers it relies on are already tested.

- [ ] **Step 1: Implement `DownloadError` and `download_model`**

Add to the top of `src-tauri/src/downloader.rs` (imports + types + function), above the helpers:

```rust
use std::fs::File;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub enum DownloadError {
    Unauthorized,
    NotFound,
    Network(String),
    Io(String),
    Cancelled,
}

impl DownloadError {
    /// User-facing message for the command layer.
    pub fn message(&self) -> String {
        match self {
            DownloadError::Unauthorized => {
                "This model requires an access token. Add one and try again.".into()
            }
            DownloadError::NotFound => "No file was found at that URL.".into(),
            DownloadError::Network(e) => format!("Download failed: {e}"),
            DownloadError::Io(e) => format!("Could not save the file: {e}"),
            DownloadError::Cancelled => "Download cancelled.".into(),
        }
    }
}

/// Download `url` into `dest_dir`, streaming to a `.part` file and renaming on
/// success. Calls `on_progress(downloaded, total)` as bytes arrive (`total` is
/// None when the server omits Content-Length). Aborts promptly when `cancel`
/// flips to true, removing the partial file.
pub fn download_model<F: FnMut(u64, Option<u64>)>(
    url: &str,
    token: &str,
    dest_dir: &Path,
    mut on_progress: F,
    cancel: &AtomicBool,
) -> Result<PathBuf, DownloadError> {
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

    let total: Option<u64> = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok());
    let filename = derive_filename(resp.header("Content-Disposition"), url);
    let final_path = unique_path(dest_dir, &filename);
    let part_path = final_path.with_extension(format!(
        "{}part",
        final_path
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

    file.flush().map_err(|e| DownloadError::Io(e.to_string()))?;
    drop(file);
    std::fs::rename(&part_path, &final_path).map_err(|e| DownloadError::Io(e.to_string()))?;
    Ok(final_path)
}
```

- [ ] **Step 2: Verify it compiles and existing tests still pass**

Run: `cd src-tauri && cargo test downloader::`
Expected: PASS (the 4 helper tests; the new code compiles).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/downloader.rs
git commit -m "feat(models): streaming download with progress and cancel"
```

---

## Task 6: Tauri commands + state wiring

**Files:**
- Modify: `src-tauri/src/types.rs` (add `DownloadProgress`)
- Modify: `src-tauri/src/commands.rs` (AppState field + new commands)
- Modify: `src-tauri/src/lib.rs` (init the new state field + register commands)

- [ ] **Step 1: Add the progress DTO**

In `src-tauri/src/types.rs`, after `ModelInfo`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}
```

- [ ] **Step 2: Extend `AppState`**

In `src-tauri/src/commands.rs`, update the imports and struct. Replace the `use` lines and `AppState` with:

```rust
use crate::engine::{self, ChildSlot, GenError};
use crate::types::{AppConfig, DownloadProgress, GalleryItem, GenerationRequest, ModelInfo};
use crate::{catalog, config, downloader, gallery, models};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub child: ChildSlot,
    pub download_cancel: Arc<AtomicBool>,
}
```

- [ ] **Step 3: Add the commands**

Append to `src-tauri/src/commands.rs`:

```rust
/// All model folders to scan: primary first, then the watched extras.
fn model_dirs(cfg: &AppConfig) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(&cfg.models_dir)];
    dirs.extend(cfg.extra_model_dirs.iter().map(PathBuf::from));
    dirs
}

#[tauri::command]
pub fn list_models(state: State<AppState>) -> Vec<ModelInfo> {
    let cfg = state.config.lock().unwrap().clone();
    models::scan_models(&model_dirs(&cfg))
}

#[tauri::command]
pub fn starter_models(vram_total_mb: Option<u64>) -> Vec<catalog::RatedModel> {
    catalog::rated_catalog(vram_total_mb)
}

#[tauri::command]
pub fn delete_model(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err("That model file no longer exists.".into());
    }
    std::fs::remove_file(&p).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cancel_download(state: State<AppState>) {
    state.download_cancel.store(true, Ordering::SeqCst);
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    token: String,
) -> Result<ModelInfo, String> {
    let dest = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;

    let cancel = state.download_cancel.clone();
    cancel.store(false, Ordering::SeqCst);
    let app2 = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        downloader::download_model(
            &url,
            &token,
            &dest,
            |downloaded, total| {
                let _ = app2.emit("model:download:progress", DownloadProgress { downloaded, total });
            },
            &cancel,
        )
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(path) => {
            let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(ModelInfo {
                path: path.to_string_lossy().into_owned(),
                name,
                size_bytes,
            })
        }
        Err(e) => Err(e.message()),
    }
}

/// Pick a folder (used for adding watched model folders / changing the primary).
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dir = app.dialog().file().blocking_pick_folder();
    dir.and_then(|d| d.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}
```

- [ ] **Step 4: Initialise state + register commands in `lib.rs`**

In `src-tauri/src/lib.rs`, update the imports near the top:

```rust
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
```

In the `.manage(AppState { ... })` block, add the new field:

```rust
        .manage(AppState {
            config: Mutex::new(initial),
            child: Arc::new(Mutex::new(None)),
            download_cancel: Arc::new(AtomicBool::new(false)),
        })
```

Add the new commands to `tauri::generate_handler![...]` (after `commands::open_path,`):

```rust
            commands::list_models,
            commands::starter_models,
            commands::delete_model,
            commands::download_model,
            commands::cancel_download,
            commands::pick_folder,
```

- [ ] **Step 5: Verify the whole backend compiles and all tests pass**

Run: `cd src-tauri && cargo test`
Expected: PASS (all prior tests; new code compiles cleanly, no warnings about unused items).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(models): tauri commands for list/download/delete + state"
```

---

## Task 7: Frontend types, API wrappers, and stores

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/api.ts`
- Modify: `src/lib/stores.ts`

- [ ] **Step 1: Add the wire types**

In `src/lib/types.ts`, extend `AppConfig` and add the new interfaces. Replace the `AppConfig` interface with:

```ts
export interface AppConfig {
  sd_binary_path: string | null;
  default_model_path: string | null;
  gallery_dir: string;
  models_dir: string;
  extra_model_dirs: string[];
  last_request: GenerationRequest;
}
```

Append at the end of the file:

```ts
export interface ModelInfo { path: string; name: string; size_bytes: number; }

export type ModelKind = "sd15" | "sdxl";
export type Suitability = "recommended" | "tight" | "too_big" | "unknown";

export interface RatedModel {
  id: string; name: string; url: string; size_bytes: number;
  kind: ModelKind; min_vram_mb: number; recommended_vram_mb: number;
  suitability: Suitability;
}

export interface DownloadProgress { downloaded: number; total: number | null; }
```

- [ ] **Step 2: Add the API wrappers**

In `src/lib/api.ts`, add imports usage already present (`invoke`, `listen`). Append:

```ts
export const listModels = () => invoke<ModelInfo[]>("list_models");
export const starterModels = (vramTotalMb: number | null) =>
  invoke<RatedModel[]>("starter_models", { vramTotalMb });
export const deleteModel = (path: string) => invoke<void>("delete_model", { path });
export const downloadModel = (url: string, token: string) =>
  invoke<ModelInfo>("download_model", { url, token });
export const cancelDownload = () => invoke<void>("cancel_download");
export const pickFolder = () => invoke<string | null>("pick_folder");

export const onDownloadProgress = (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model:download:progress", (e) => cb(e.payload));
```

Update the type import line at the top of `src/lib/api.ts` to include the new types:

```ts
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats, ModelInfo, RatedModel, DownloadProgress } from "./types";
```

- [ ] **Step 3: Add stores**

In `src/lib/stores.ts`, add after the `history` store:

```ts
export const models = writable<ModelInfo[]>([]);
```

And update its type import to include `ModelInfo`:

```ts
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, ModelInfo } from "./types";
```

- [ ] **Step 4: Type-check**

Run: `npm run check`
Expected: `0 ERRORS` (some wrappers are unused until later tasks — that's fine, no error).

- [ ] **Step 5: Commit**

```bash
git add src/lib/types.ts src/lib/api.ts src/lib/stores.ts
git commit -m "feat(models): frontend types, api wrappers, models store"
```

---

## Task 8: Model library picker (`ModelLibrary.svelte`)

**Files:**
- Create: `src/lib/components/ModelLibrary.svelte`
- Delete: `src/lib/components/ModelPicker.svelte`
- Modify: `src/routes/+page.svelte` (swap component + load models on mount)

- [ ] **Step 1: Create the component**

Create `src/lib/components/ModelLibrary.svelte`:

```svelte
<script lang="ts">
  import { request, models } from "../stores";
  import { listModels, deleteModel } from "../api";
  import DownloadDialog from "./DownloadDialog.svelte";

  let showDownload = $state(false);
  let error = $state<string | null>(null);

  const fmtSize = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);

  async function refresh() {
    models.set(await listModels());
  }

  function onSelect(e: Event) {
    const path = (e.currentTarget as HTMLSelectElement).value;
    request.update((r) => ({ ...r, model_path: path }));
  }

  async function removeSelected() {
    const path = $request.model_path;
    if (!path) return;
    const name = $models.find((m) => m.path === path)?.name ?? path;
    if (!confirm(`Permanently delete "${name}"? This cannot be undone.`)) return;
    error = null;
    try {
      await deleteModel(path);
      request.update((r) => ({ ...r, model_path: "" }));
      await refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function onDownloaded(path: string) {
    await refresh();
    request.update((r) => ({ ...r, model_path: path }));
    showDownload = false;
  }
</script>

<div class="field">
  <span class="label">Model</span>
  <div class="row">
    <select value={$request.model_path} onchange={onSelect}>
      {#if !$request.model_path}<option value="" disabled selected>Select a model…</option>{/if}
      {#each $models as m (m.path)}
        <option value={m.path}>{m.name} — {fmtSize(m.size_bytes)}</option>
      {/each}
    </select>
  </div>
  <div class="row actions">
    <button class="btn-secondary" onclick={() => (showDownload = true)}>Download…</button>
    <button class="btn-secondary" disabled={!$request.model_path} onclick={removeSelected}>Delete</button>
  </div>
  {#if $models.length === 0}
    <span class="hint">No models found. Click Download… to get one.</span>
  {/if}
  {#if error}<span class="err">{error}</span>{/if}
</div>

{#if showDownload}
  <DownloadDialog onclose={() => (showDownload = false)} ondownloaded={onDownloaded} {refresh} />
{/if}

<style>
  .row { display:flex; gap:.5rem; align-items:center; }
  .actions { margin-top:.4rem; }
  select { flex:1; font:inherit; padding:.3rem; min-width:0; }
  button { font:inherit; font-size:.78rem; padding:.3rem .6rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .hint { font-size:.72rem; opacity:.6; margin-top:.3rem; display:block; }
  .err { font-size:.72rem; color:#ff6b6b; margin-top:.3rem; display:block; }
</style>
```

> Note: `DownloadDialog` is created in Task 9. This task will not type-check until Task 9 is done; implement them back-to-back (the spec-review for Task 8 runs after Task 9's file exists). If executing strictly one task at a time, create a minimal placeholder `DownloadDialog.svelte` that renders nothing, then flesh it out in Task 9.

- [ ] **Step 2: Swap the component into the page**

In `src/routes/+page.svelte`:

Replace the import:
```ts
  import ModelPicker from "$lib/components/ModelPicker.svelte";
```
with:
```ts
  import ModelLibrary from "$lib/components/ModelLibrary.svelte";
```

Replace `<ModelPicker />` in the markup with `<ModelLibrary />`.

Add `models` to the store import and load them on mount. Update the stores import line:
```ts
  import { settings, request, history, sysStats, models } from "$lib/stores";
```
Update the api import line to include `listModels`:
```ts
  import { getSettings, listHistory, onSystemStats, listModels } from "$lib/api";
```
Inside the `onMount` async IIFE, after `history.set(await listHistory());`, add:
```ts
      models.set(await listModels());
```

- [ ] **Step 3: Delete the old component**

```bash
git rm src/lib/components/ModelPicker.svelte
```

- [ ] **Step 4: Type-check**

Run: `npm run check`
Expected: `0 ERRORS` (after Task 9 exists, or with the placeholder).

- [ ] **Step 5: Commit**

```bash
git add src/lib/components/ModelLibrary.svelte src/routes/+page.svelte
git commit -m "feat(models): model library picker replacing ModelPicker"
```

---

## Task 9: Download dialog (`DownloadDialog.svelte`)

**Files:**
- Create (or replace placeholder): `src/lib/components/DownloadDialog.svelte`

- [ ] **Step 1: Create the dialog**

Create `src/lib/components/DownloadDialog.svelte`:

```svelte
<script lang="ts">
  import { sysStats } from "../stores";
  import { starterModels, downloadModel, cancelDownload, onDownloadProgress } from "../api";
  import type { RatedModel, Suitability } from "../types";
  import { onMount } from "svelte";

  let { onclose, ondownloaded }: { onclose: () => void; ondownloaded: (path: string) => void } = $props();

  let starters = $state<RatedModel[]>([]);
  let url = $state("");
  let token = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);
  let downloaded = $state(0);
  let total = $state<number | null>(null);
  let unlisten: (() => void) | null = null;

  const fmt = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
  const pct = $derived(total ? Math.round((downloaded / total) * 100) : 0);

  const badge: Record<Suitability, string> = {
    recommended: "✅ Recommended",
    tight: "⚠️ Tight for your GPU",
    too_big: "❌ Likely too big",
    unknown: "— GPU unknown",
  };

  onMount(() => {
    (async () => {
      starters = await starterModels($sysStats?.gpu?.vram_total_mb ?? null);
      unlisten = await onDownloadProgress((p) => { downloaded = p.downloaded; total = p.total; });
    })();
    return () => unlisten?.();
  });

  async function start(downloadUrl: string) {
    if (busy || !downloadUrl) return;
    busy = true; error = null; downloaded = 0; total = null;
    try {
      const info = await downloadModel(downloadUrl, token.trim());
      ondownloaded(info.path);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function cancel() { cancelDownload(); }
</script>

<div class="backdrop" onclick={onclose} role="presentation">
  <div class="dialog" onclick={(e) => e.stopPropagation()} role="dialog" aria-label="Download model">
    <h2>Download a model</h2>

    {#if busy}
      <div class="progress"><div class="fill" style="width:{pct}%"></div></div>
      <p class="status">{fmt(downloaded)}{total ? ` / ${fmt(total)} (${pct}%)` : " downloaded…"}</p>
      <button class="btn-secondary" onclick={cancel}>Cancel</button>
    {:else}
      <section>
        <h3>Starter models</h3>
        {#each starters as s (s.id)}
          <div class="starter">
            <div class="meta">
              <span class="name">{s.name}</span>
              <span class="sub">{fmt(s.size_bytes)} · {badge[s.suitability]}</span>
            </div>
            <button class="btn-secondary" onclick={() => start(s.url)}>Get</button>
          </div>
        {/each}
      </section>

      <section>
        <h3>Or paste a URL</h3>
        <input class="in" type="text" placeholder="https://…/model.safetensors" bind:value={url} />
        <input class="in" type="password" placeholder="Access token (optional, for gated/civitai)" bind:value={token} />
        <div class="row">
          <button class="btn-primary" disabled={!url.trim()} onclick={() => start(url.trim())}>Download</button>
          <button class="btn-secondary" onclick={onclose}>Close</button>
        </div>
      </section>
    {/if}

    {#if error}<p class="err">{error}</p>{/if}
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:rgba(0,0,0,.5); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--bg, #1e1e1e); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(460px, 92vw); max-height:88vh; overflow-y:auto; display:flex; flex-direction:column; gap:.8rem; }
  h2 { margin:0; font-size:1.05rem; }
  h3 { margin:.2rem 0; font-size:.8rem; opacity:.7; }
  .starter { display:flex; align-items:center; justify-content:space-between; gap:.6rem; padding:.35rem 0; }
  .meta { display:flex; flex-direction:column; }
  .name { font-size:.9rem; }
  .sub { font-size:.72rem; opacity:.7; }
  .in { width:100%; font:inherit; padding:.4rem; box-sizing:border-box; margin-bottom:.4rem; }
  .row { display:flex; gap:.5rem; }
  button { font:inherit; font-size:.8rem; padding:.35rem .7rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .progress { height:12px; background:rgba(255,255,255,.1); border-radius:6px; overflow:hidden; }
  .fill { height:100%; background:var(--accent, #6ea8fe); transition:width .15s linear; }
  .status { font-size:.78rem; opacity:.85; margin:.2rem 0; }
  .err { font-size:.75rem; color:#ff6b6b; }
</style>
```

> The `refresh` prop referenced in Task 8's `<DownloadDialog ... {refresh} />` is not used by the dialog (the parent refreshes in `ondownloaded`). Remove `{refresh}` from the Task 8 markup so props match exactly: `<DownloadDialog onclose={() => (showDownload = false)} ondownloaded={onDownloaded} />`.

- [ ] **Step 2: Reconcile the Task 8 props**

Edit `src/lib/components/ModelLibrary.svelte`: change the dialog usage to:
```svelte
  <DownloadDialog onclose={() => (showDownload = false)} ondownloaded={onDownloaded} />
```
and remove the now-unused `refresh` function if nothing else uses it (keep `refresh` — it's still called by `removeSelected` and `onDownloaded`). Only the `{refresh}` prop is removed.

- [ ] **Step 3: Type-check**

Run: `npm run check`
Expected: `0 ERRORS`.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/DownloadDialog.svelte src/lib/components/ModelLibrary.svelte
git commit -m "feat(models): download dialog with starter list + paste URL"
```

---

## Task 10: Models-folder settings (`ModelFolders.svelte`)

**Files:**
- Create: `src/lib/components/ModelFolders.svelte`
- Modify: `src/routes/+page.svelte` (mount it in the stage footer near `GalleryLocation`)

- [ ] **Step 1: Create the component**

Create `src/lib/components/ModelFolders.svelte`:

```svelte
<script lang="ts">
  import { settings, models } from "$lib/stores";
  import { setSettings, pickFolder, listModels } from "$lib/api";

  let busy = $state(false);

  async function persist(next: typeof $settings) {
    if (!next) return;
    await setSettings(next);
    settings.set(next);
    models.set(await listModels());
  }

  async function addFolder() {
    if (!$settings || busy) return;
    busy = true;
    try {
      const dir = await pickFolder();
      if (!dir) return;
      if ($settings.extra_model_dirs.includes(dir) || dir === $settings.models_dir) return;
      await persist({ ...$settings, extra_model_dirs: [...$settings.extra_model_dirs, dir] });
    } finally {
      busy = false;
    }
  }

  async function removeFolder(dir: string) {
    if (!$settings) return;
    await persist({ ...$settings, extra_model_dirs: $settings.extra_model_dirs.filter((d) => d !== dir) });
  }
</script>

<div class="folders">
  <div class="hdr">
    <span class="lbl">Model folders</span>
    <button onclick={addFolder} disabled={!$settings || busy}>Add folder…</button>
  </div>
  <div class="primary" title={$settings?.models_dir ?? ""}>
    {$settings?.models_dir ?? "…"} <span class="tag">primary · downloads</span>
  </div>
  {#each $settings?.extra_model_dirs ?? [] as dir (dir)}
    <div class="extra">
      <span class="path" title={dir}>{dir}</span>
      <button class="x" onclick={() => removeFolder(dir)} aria-label="Remove folder">×</button>
    </div>
  {/each}
</div>

<style>
  .folders { font-size:.75rem; border-top:1px solid var(--border); padding:.45rem .2rem 0; display:flex; flex-direction:column; gap:.25rem; }
  .hdr { display:flex; align-items:center; justify-content:space-between; }
  .lbl { opacity:.6; }
  .primary, .extra { display:flex; align-items:center; gap:.4rem; font-family:monospace; opacity:.9; }
  .path, .primary { flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .tag { font-family:inherit; opacity:.5; }
  button { font:inherit; font-size:.72rem; padding:.2rem .5rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
  .x { padding:.1rem .4rem; }
</style>
```

- [ ] **Step 2: Mount it in the page**

In `src/routes/+page.svelte`, import it:
```ts
  import ModelFolders from "$lib/components/ModelFolders.svelte";
```
Add it in the `.stage` section after `<GalleryLocation />`:
```svelte
    <GalleryLocation />
    <ModelFolders />
```

- [ ] **Step 3: Type-check**

Run: `npm run check`
Expected: `0 ERRORS`.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/ModelFolders.svelte src/routes/+page.svelte
git commit -m "feat(models): manage primary + watched model folders in UI"
```

---

## Task 11: Full build + manual end-to-end verification

**Files:** none (verification only)

- [ ] **Step 1: Full backend test + frontend check**

Run: `cd src-tauri && cargo test && cd .. && npm run check`
Expected: all Rust tests PASS; svelte-check `0 ERRORS`.

- [ ] **Step 2: Dev run, smoke the flows**

Run: `npm run tauri dev`
Verify manually:
- Model dropdown lists files already in the default models folder (drop a `.safetensors` there first, or use the existing one).
- "Add folder…" adds a watched folder (e.g. the folder holding `~/Downloads/v1-5-pruned-emaonly.safetensors`); its model appears in the dropdown and persists after restart.
- "Download…" shows the two starter models with a suitability badge reflecting the RTX 3060 (12 GB → SD 1.5 Recommended, SDXL Recommended). Start a download, watch the progress bar, then Cancel — confirm no leftover `.part` file in the models folder.
- Paste-URL download of a small `.safetensors` completes, appears in the list, and is auto-selected.
- Generate an image with a selected model (engine path unchanged).
- Delete a model → confirm dialog → file removed and dropdown updates.

- [ ] **Step 3: Build the self-contained AppImage**

Run: `bash scripts/build-appimage.sh`
Expected: completes; `src-tauri/target/release/bundle/appimage/muchai_0.1.0_amd64.AppImage` exists; no `libcuda.so.1`/`libnvidia-*` bundled (per the existing build script's strip step).

- [ ] **Step 4: Commit any final touch-ups, then update the roadmap**

If verification surfaced small fixes, commit them. Then note in the user's memory/roadmap that the model-library feature is done and what remains deferred (VAE override, Flux multi-file, smart browsing, JPEG, collapsible params panel).

---

## Self-Review (completed by plan author)

**Spec coverage:**
- Managed primary folder + watched folders → Task 1 (config), Task 6 (`model_dirs`), Task 10 (UI). ✓
- Scan/list/switch → Task 2, Task 8. ✓
- Delete (permanent + confirm) → Task 6 (`delete_model`), Task 8 (confirm dialog). ✓
- Curated starter list + paste-URL + token → Task 3, Task 5, Task 9. ✓
- Hardware-aware suitability (first GPU VRAM) → Task 3 (`rate`), Task 6 (`starter_models`), Task 9 (badges from `sysStats.gpu.vram_total_mb`). ✓
- Streaming download + progress + cancel + `.part` cleanup → Task 5, Task 6, Task 9. ✓
- `ureq`/rustls dependency → Task 4. ✓
- Backward-compatible config → Task 1 (`serde(default)` + backfill). ✓
- Engine/generation unchanged → confirmed (no task touches `command_builder.rs`/`engine.rs`). ✓

**Type consistency:** `ModelInfo {path,name,size_bytes}`, `DownloadProgress {downloaded,total}`, `RatedModel` (flattened `CatalogModel` + `suitability`), `Suitability` snake_case (`too_big`) — Rust serde and `types.ts` match. Command arg names (`vramTotalMb`→`vram_total_mb`, `url`, `token`, `path`) match `invoke` call sites in `api.ts`.

**Placeholder scan:** No TBD/“handle errors”/vague steps; every code step has full code. The only deferred concrete is the exact starter URLs, which are provided in Task 3.

**Cross-task ordering note:** Task 8 references `DownloadDialog` created in Task 9; the plan calls this out and offers a placeholder if executing strictly serially.
