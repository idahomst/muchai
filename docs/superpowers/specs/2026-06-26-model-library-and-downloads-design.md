# Model Library & Downloads — Design

**Status:** Approved (design phase)
**Date:** 2026-06-26
**Branch:** `feat/beta-image-generation` (or a new `feat/model-library`)

## Goal

Let users manage a library of image models and acquire new ones from inside
MuchAI, instead of hand-picking a single `.safetensors` file every time. This
serves MuchAI's core value: simple, self-contained, no dependency hell — a new
user with an empty library should be able to get a working model in a few clicks.

## Scope

**In (v1):**
- A managed models library: one primary folder plus user-added watched folders,
  all scanned and merged into a single list.
- List / switch / delete models.
- Download single-file checkpoints via a curated starter list **and** a
  paste-URL field with an optional access token.
- Hardware-aware suitability badges driven by the first detected GPU's VRAM.

**Out (later iterations):**
- Separate-VAE override (`--vae`).
- Multi-file Flux / SD3 models (`--diffusion-model`, `--clip_l`, `--t5xxl`).
- Smart HuggingFace / civitai API browsing (page URL → version/file picker).

Rationale: SD 1.5 and SDXL — the overwhelming majority of downloads — ship as a
single self-contained checkpoint that bundles the diffusion model, text encoder,
and VAE. v1 targets that case; the engine already supports the multi-file flags
for a later iteration.

## Architecture

New, focused units following the existing codebase pattern (small files, pure
logic unit-tested, Tauri commands as thin adapters, engine/IO on worker threads).

### Rust modules

**`models.rs` — library scanning**
- `pub struct ModelInfo { path: String, name: String, size_bytes: u64 }`
  (serde, snake_case wire form; mirrored in `types.ts`).
- `pub fn scan_models(dirs: &[PathBuf]) -> Vec<ModelInfo>`
  - Recursively walks each directory.
  - Includes files with extension `.safetensors`, `.ckpt`, or `.gguf`
    (case-insensitive).
  - `name` = file stem; `size_bytes` from metadata.
  - Dedups by canonical path (a file reachable via two watched folders appears
    once).
  - Sorts by `name` (case-insensitive).
  - Missing / unreadable directories are skipped, not errors.

**`downloader.rs` — streaming download**
- `pub fn download_model(url, token, dest_dir, on_progress, cancel) -> Result<PathBuf, DownloadError>`
  - Blocking `ureq` GET (rustls TLS — no system OpenSSL, keeps the AppImage
    self-contained). Runs on a worker thread, mirroring `engine::run_generation`.
  - If `token` is non-empty, sends `Authorization: Bearer <token>` (covers HF
    gated models and civitai).
  - Resolves the filename from `Content-Disposition` if present, else the last
    path segment of the URL; sanitized (strip path separators / control chars;
    fall back to a generated name if empty).
  - Streams to `dest_dir/<name>.part`, calling `on_progress(downloaded, total)`
    where `total` comes from `Content-Length` (may be `None` → indeterminate).
  - On success, atomically renames `.part` → final name. On any error or
    cancel, removes the `.part` file.
  - `cancel` is a shared `AtomicBool` checked each chunk (same cancellation
    style as the engine's child slot).
  - Filename collision with an existing finished file → append ` (n)` before the
    extension.
- `pub enum DownloadError { Network(String), Unauthorized, NotFound, Io(String), Cancelled }`
  mapped to friendly messages in the command layer.

**`catalog.rs` — curated starter models + suitability**
- `pub struct CatalogModel { id, name, url, size_bytes, kind: ModelKind, min_vram_mb, recommended_vram_mb }`
- `pub enum ModelKind { Sd15, Sdxl }`
- `pub fn starter_catalog() -> Vec<CatalogModel>` — static list. v1 entries:
  - Stable Diffusion 1.5 (full checkpoint) — min 2048, recommended 4096.
  - SDXL Base 1.0 (full checkpoint) — min 6144, recommended 8192.
  - (Exact download URLs chosen during implementation; prefer stable HF
    `resolve` links to a maintained mirror. If a link is gated, the user can
    still paste a URL + token.)
- `pub enum Suitability { Recommended, Tight, TooBig, Unknown }`
- `pub fn rate(model: &CatalogModel, vram_total_mb: Option<u64>) -> Suitability`
  - `None` (no GPU) → `Unknown`.
  - `vram >= recommended_vram_mb` → `Recommended`.
  - `vram >= min_vram_mb` → `Tight`.
  - else → `TooBig`.

### Config (`types.rs` `AppConfig`)

Add:
- `models_dir: String` — primary managed folder; downloads land here.
  Default `<data_dir>/models` (via `directories`, alongside `gallery`).
- `extra_model_dirs: Vec<String>` — user-added watched folders (default empty).

The active model continues to live in `last_request.model_path` (an absolute
path the engine consumes via `-m`). The library only helps choose that path.

`config.rs::default_config()` and the existing round-trip tests updated for the
new fields. Backward compatibility: `serde(default)` on the new fields so an
older `config.json` still loads.

### Tauri commands (`commands.rs`)

- `list_models(state) -> Vec<ModelInfo>` — scans `models_dir` + `extra_model_dirs`.
- `download_model(app, state, url: String, token: String) -> Result<ModelInfo, String>`
  - Spawns the blocking download on a worker thread.
  - Emits `model:download:progress` `{ downloaded: u64, total: u64 | null }`.
  - Stores the cancel flag in `AppState` (new field) so `cancel_download` can
    flip it.
  - On success returns the new `ModelInfo`; UI refreshes the list and selects it.
- `cancel_download(state) -> ()`.
- `delete_model(path: String) -> Result<(), String>` — permanently deletes the
  file (confirm happens in the UI before calling).
- `starter_models(state) -> Vec<{ model: CatalogModel, suitability }>` — catalog
  rated against the last-known first-GPU VRAM.
- `pick_dir() -> Option<String>` — generalize the existing `pick_gallery_dir`
  pattern for choosing a models folder (or add `pick_models_dir`).

### Frontend (`src/lib`)

- `types.ts`: add `ModelInfo`, `CatalogModel`, `Suitability`, download progress
  type; extend `AppConfig`.
- `api.ts`: `listModels`, `downloadModel`, `cancelDownload`, `deleteModel`,
  `starterModels`, `onDownloadProgress`, dir picker.
- `stores.ts`: `models` (list), `downloadStatus` (idle/running/error).
- **`ModelLibrary.svelte`** (replaces `ModelPicker.svelte`):
  - Dropdown of scanned models (`name — size`), selecting sets
    `request.model_path`.
  - Buttons: **Download…**, **Add folder…**, **Delete** (with confirm).
- **`DownloadDialog.svelte`**:
  - Starter list with suitability badges (✅ Recommended / ⚠️ Tight / ❌ Likely
    too big / — Unknown), one-click Get.
  - Paste-URL field + optional token + Download / Cancel + progress bar
    (indeterminate when `total` is null).
- **Settings** (`SettingsPanel` or `GalleryLocation` sibling): show/change the
  primary models folder; add/remove watched folders.

## Data flow

1. App start → `getSettings` (now includes `models_dir`, `extra_model_dirs`) →
   `listModels` populates the dropdown.
2. Selecting a model sets `request.model_path`; generation is unchanged (`-m`).
3. Download: user picks a starter or pastes a URL → `download_model` streams to
   the primary folder, emitting progress → on completion the list refreshes and
   the new model is selected.
4. Suitability: `SystemStats.gpu.vram_total_mb` (first GPU) feeds `rate()`.

## Error handling

- Download: `401/403` → "This model requires an access token." `404` → "File not
  found at that URL." Network/IO → the underlying message. Cancel → silent,
  `.part` removed. Disk-full surfaces the IO error.
- Scan: unreadable/missing dirs skipped silently.
- Delete: confirm in UI; surface IO errors (e.g. permission denied).
- Filename collisions resolved with a ` (n)` suffix.

## Testing

Pure logic via `cargo test` (consistent with existing modules):
- `scan_models`: extension filtering, recursion, dedup by path, size, sort,
  skipping missing dirs — over temp directories with fake files.
- `downloader`: filename derivation from `Content-Disposition` and from URL,
  sanitization, collision suffixing (pure helpers extracted from the IO path).
- `catalog::rate`: each suitability branch incl. `None` VRAM.
- `config`: round-trip with new fields; old config without the fields loads
  (serde default).

Live HTTP and the Tauri command wiring are verified manually on the dev RTX 3060
(download a small model, switch, delete), as with prior end-to-end checks.

## Dependencies

- Add `ureq` (with `rustls` TLS feature) to `src-tauri/Cargo.toml`. Chosen over
  `reqwest` for a smaller, blocking, OpenSSL-free footprint that fits the
  self-contained AppImage and the existing worker-thread IO model.

## Out-of-scope follow-ups (tracked in roadmap)

- Separate-VAE override and multi-file Flux/SD3 download + selection.
- Smart HF/civitai API browsing.
- Collapsible parameters panel (deferred UX item).
- JPEG output format.
