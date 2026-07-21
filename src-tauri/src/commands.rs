use crate::engine::{self, ChildSlot, GenError};
use crate::types::{AppConfig, DownloadProgress, GalleryItem, GenerationRequest, GpuDevice, ModelInfo};
use crate::recipes::{self, ComponentRole};
use crate::types::{GenDefaults, ModelDefinition, ModelRef};
use crate::{catalog, config, downloader, gallery, hf, library, manifest, models};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State};

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

pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub child: ChildSlot,
    pub download_cancel: Arc<AtomicBool>,
    pub gpu_devices: Arc<Mutex<Option<Vec<GpuDevice>>>>,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn engine_binary_name() -> &'static str {
    if cfg!(windows) { "sd-cli.exe" } else { "sd-cli" }
}

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

/// Directory holding `sd-cli` and its sibling `.so` files. Bundled apps resolve
/// it from the Tauri resource dir (`<resources>/engine`); dev falls back to the
/// source tree. `RUNPATH=$ORIGIN` then loads the siblings next to `sd-cli`.
fn engine_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(res) = app.path().resource_dir() {
        let d = res.join("engine");
        if d.join(engine_binary_name()).exists() {
            return Some(d);
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/engine");
    if dev.join(engine_binary_name()).exists() {
        return Some(dev);
    }
    None
}

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

/// Resolve the engine binary: explicit config override, else the bundled engine.
fn resolve_binary(app: &AppHandle, cfg: &AppConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.sd_binary_path {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let bin = engine_dir(app)?.join(engine_binary_name());
    bin.exists().then_some(bin)
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> AppConfig {
    state.config.lock().unwrap().clone()
}

#[tauri::command]
pub fn list_gpu_devices(app: AppHandle, state: State<AppState>) -> Vec<GpuDevice> {
    if let Some(cached) = state.gpu_devices.lock().unwrap().as_ref() {
        return cached.clone();
    }
    let cfg = state.config.lock().unwrap().clone();
    let devices = match resolve_binary(&app, &cfg) {
        Some(bin) => crate::devices::enumerate(&bin),
        None => Vec::new(),
    };
    *state.gpu_devices.lock().unwrap() = Some(devices.clone());
    devices
}

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

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<AppState>,
    config: AppConfig,
) -> Result<(), String> {
    // Merge AND persist under the lock so the preserved backend-owned fields
    // reflect the latest state and no concurrent mutator (e.g. download_multifile
    // adding a ModelDefinition) can slip a write in between our merge and save —
    // that would let us clobber their definition. Matches the sibling mutators,
    // which all persist while still holding the lock.
    let gallery_dir = {
        let mut cfg = state.config.lock().unwrap();
        // A changed engine path means a different binary that may enumerate devices
        // in a different order — drop the cached list so the next probe re-reads it,
        // preserving index parity with `--backend vulkanN`.
        if cfg.sd_binary_path != config.sd_binary_path {
            *state.gpu_devices.lock().unwrap() = None;
        }
        *cfg = merged_settings(&cfg, config);
        config::save_config_to(&config::config_file_path(), &cfg).map_err(|e| e.to_string())?;
        cfg.gallery_dir.clone()
    };
    // Keep the asset-protocol scope in sync so images in a newly chosen gallery
    // dir can be displayed without restarting the app.
    let _ = app.asset_protocol_scope().allow_directory(&gallery_dir, true);
    Ok(())
}

#[tauri::command]
pub fn list_history(state: State<AppState>) -> Vec<GalleryItem> {
    let dir = state.config.lock().unwrap().gallery_dir.clone();
    gallery::list_items(std::path::Path::new(&dir))
}

#[tauri::command]
pub fn delete_image(image_path: String) -> Result<(), String> {
    gallery::delete_to_trash(std::path::Path::new(&image_path))
}

#[tauri::command]
pub fn cancel_generation(state: State<AppState>) {
    if let Some(mut child) = state.child.lock().unwrap().take() {
        let _ = child.kill();
    }
}

#[tauri::command]
pub async fn generate(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GenerationRequest,
    device_vram_mb: Option<u64>,
) -> Result<Vec<GalleryItem>, String> {
    let cfg = state.config.lock().unwrap().clone();
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
    // Validate the saved device against the enumerated list (cached) and map it
    // to a backend; a stale/absent selection falls back to the engine default
    // when a real GPU exists, or to the CPU backend when none does.
    let binary = resolve_binary(&app, &cfg)
        .ok_or_else(|| "stable-diffusion engine not found. Set its path in Settings.".to_string())?;
    let backend = {
        // Enumerate on demand (and cache) if the picker never warmed the list,
        // so a stored selection is always validated before mapping to a backend.
        let mut guard = state.gpu_devices.lock().unwrap();
        let devices = guard
            .get_or_insert_with(|| crate::devices::enumerate(&binary))
            .clone();
        crate::devices::resolve_backend(cfg.gpu_device.clone(), &devices)
    };

    let gallery_dir = PathBuf::from(&cfg.gallery_dir);
    std::fs::create_dir_all(&gallery_dir).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let ext = request.output_format.extension();
    let image_path = gallery_dir.join(format!("{id}.{ext}"));

    let slot = state.child.clone();
    let app2 = app.clone();
    let req = request.clone();
    let img = image_path.clone();
    let backend_owned = backend;
    // Decide Low-VRAM for THIS run: manual toggle forces it on; otherwise
    // auto-engage when the summed weight bytes won't fit the selected GPU's VRAM.
    // Weights are summed in bytes (what estimate_vram_mb expects). A broken model
    // (some file un-stat'able) yields None → treated as "unknown", no auto-engage.
    let is_cpu = backend_owned.as_deref() == Some("cpu");
    let weights_bytes = models::sum_file_sizes(&request.model.component_paths());
    let (low_vram, auto_engaged) =
        crate::fit::resolve_low_vram(cfg.low_vram, weights_bytes, device_vram_mb, is_cpu);
    if auto_engaged {
        // One-time, payload-free signal; the note text lives in the frontend.
        let _ = app.emit("generation:low_vram_auto", ());
    }
    let engine_opts = crate::command_builder::EngineOptions { low_vram };

    // Run the (blocking) engine on a worker thread so the async command yields.
    let result = tauri::async_runtime::spawn_blocking(move || {
        engine::run_generation(&binary, &req, &img, backend_owned.as_deref(), engine_opts, &slot, |p| {
            let _ = app2.emit("generation:progress", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?;

    match result {
        Ok(seeds) => {
            // For batch_count > 1 the engine writes "{id}_0.png", "{id}_1.png", …;
            // for a single image it writes "{id}.png". Discover whichever exist so
            // every produced image becomes its own selectable, reproducible item.
            let batch = request.batch_count.max(1) as usize;
            let mut produced: Vec<(usize, PathBuf)> = Vec::new();
            for i in 0..batch {
                let p = gallery_dir.join(format!("{id}_{i}.{ext}"));
                if p.exists() {
                    produced.push((i, p));
                }
            }
            if produced.is_empty() && image_path.exists() {
                produced.push((0, image_path.clone()));
            }
            if produced.is_empty() {
                return Err("Engine finished but no image file was found.".to_string());
            }

            let produced_len = produced.len();
            let multi = produced_len > 1;
            let mut items = Vec::with_capacity(produced.len());
            for (i, path) in produced {
                // Prefer the engine-reported seed; otherwise derive it (base + i,
                // or leave -1 when the base was random and unreported).
                let seed = seeds.get(i).copied().unwrap_or(if request.seed >= 0 {
                    request.seed + i as i64
                } else {
                    -1
                });
                let mut req_i = request.clone();
                req_i.seed = seed;
                req_i.batch_count = 1; // each item is one concrete image
                let item = GalleryItem {
                    id: if multi { format!("{id}_{i}") } else { id.clone() },
                    image_path: path.to_string_lossy().into_owned(),
                    request: req_i,
                    created_at_unix: now_unix(),
                    batch_id: id.clone(),
                    batch_index: i as u32,
                    batch_size: produced_len as u32,
                };
                gallery::write_sidecar(&path, &item).map_err(|e| e.to_string())?;
                items.push(item);
            }
            // persist last-used request (the original, with its batch settings)
            {
                let mut c = state.config.lock().unwrap();
                c.last_request = request.clone();
                let _ = config::save_config_to(&config::config_file_path(), &c);
            }
            Ok(items)
        }
        Err(GenError::NonZero { oom: true, .. }) => Err(
            "Out of GPU memory. Try a smaller width/height or batch count.".to_string(),
        ),
        // Surface the engine's own stderr so failures are diagnosable instead
        // of an opaque "exited with code N".
        Err(GenError::NonZero { code, stderr_tail, .. }) => {
            let tail = stderr_tail.trim();
            if tail.is_empty() {
                Err(format!("Image generation failed (engine exited with code {code:?})."))
            } else {
                Err(format!("Image generation failed (code {code:?}):\n{tail}"))
            }
        }
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn pick_model_file(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let file = app
        .dialog()
        .file()
        .add_filter("Models", &["safetensors", "gguf", "ckpt"])
        .blocking_pick_file();
    file.and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

/// Open a folder (or file) in the OS file manager / default app. Runs the
/// opener via its Rust API so it isn't subject to the JS command's path scope
/// (the gallery dir is user-chosen and trusted).
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pick_gallery_dir(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dir = app.dialog().file().blocking_pick_folder();
    dir.and_then(|d| d.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

/// All model folders to scan: primary first, then the watched extras.
fn model_dirs(cfg: &AppConfig) -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from(&cfg.models_dir)];
    dirs.extend(cfg.extra_model_dirs.iter().map(PathBuf::from));
    dirs
}

/// Every component file path owned by a saved definition (canonicalized the
/// same way `models::scan_models_excluding` matches internally, so the exclusion
/// isn't silently a no-op).
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

/// Single-file model list for the UI: scan every watched dir, then drop both the
/// definition-owned component files AND the entire shared component pool under
/// `<models_dir>/shared`. The pool exclusion is belt-and-suspenders alongside the
/// `exclude` set: shared encoder/VAE files are never valid standalone models, so
/// even when orphaned — the last model of a family is deleted (the pool is left
/// intact), or a catalog download is cancelled after some shared files landed —
/// they must not reappear as bogus, ungeneratable single-file models.
fn scan_single_file_models(
    dirs: &[PathBuf],
    exclude: &std::collections::HashSet<PathBuf>,
    models_dir: &std::path::Path,
) -> Vec<ModelInfo> {
    let mut models = models::scan_models_excluding(dirs, exclude);
    // Canonicalize the shared dir the same way scan canonicalizes file paths so
    // the prefix test isn't silently a no-op. When it doesn't exist there are no
    // files under it to filter anyway, so the fallback is harmless.
    let shared = models_dir.join("shared");
    let shared = shared.canonicalize().unwrap_or(shared);
    models.retain(|m| !std::path::Path::new(&m.path).starts_with(&shared));
    models
}

#[tauri::command]
pub fn list_models(state: State<AppState>) -> Vec<ModelInfo> {
    let cfg = state.config.lock().unwrap().clone();
    scan_single_file_models(
        &model_dirs(&cfg),
        &referenced_paths(&cfg),
        std::path::Path::new(&cfg.models_dir),
    )
}

#[tauri::command]
pub fn list_library(state: State<AppState>) -> Vec<crate::library::LibraryEntry> {
    let models_dir = state.config.lock().unwrap().models_dir.clone();
    crate::library::scan_library(std::path::Path::new(&models_dir))
}

#[tauri::command]
pub fn delete_model(path: String) -> Result<(), String> {
    let p = PathBuf::from(&path);
    if !p.is_file() {
        return Err("That model file no longer exists.".into());
    }
    // Move to the OS trash (recoverable from the file manager) rather than
    // unlinking, mirroring image deletion via gallery::delete_to_trash.
    trash::delete(&p).map_err(|e| e.to_string())
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
    // The UI triggers at most one download at a time; this reset assumes that
    // single-flight invariant (concurrent downloads would share one cancel flag).
    cancel.store(false, Ordering::SeqCst);
    let app2 = app.clone();

    let result = tauri::async_runtime::spawn_blocking(move || {
        // Throttle: emit at most every ~4 MiB (or on completion) so multi-GB
        // downloads don't flood the frontend with ~100k IPC events.
        let mut last_emit: u64 = 0;
        downloader::download_model(
            &url,
            &token,
            &dest,
            move |downloaded, total| {
                if downloaded.saturating_sub(last_emit) >= 4 << 20 || Some(downloaded) == total {
                    last_emit = downloaded;
                    let _ = app2.emit(
                        "model:download:progress",
                        DownloadProgress { downloaded, total, file_index: None, file_count: None, file_name: None },
                    );
                }
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

/// A filesystem-safe unique model id.
fn new_model_id() -> String {
    format!("model-{}", uuid::Uuid::new_v4())
}

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

/// Download every file a catalog entry needs (diffusion + any pooled/override
/// shared components) and write its `model.json`. Shared components already
/// present in the pool (from a prior install) are skipped, not re-downloaded.
/// On any download failure the freshly-created per-model folder is removed;
/// the shared pool is left untouched since other models may depend on it.
#[tauri::command]
pub async fn add_catalog_model(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog_id: String,
) -> Result<library::LibraryEntry, String> {
    let (models_dir, hf_token, civitai_token) = {
        let cfg = state.config.lock().unwrap();
        (PathBuf::from(&cfg.models_dir), cfg.hf_token.clone().unwrap_or_default(), cfg.civitai_token.clone().unwrap_or_default())
    };
    if models_dir.as_os_str().is_empty() {
        return Err("models directory is not set".into());
    }
    let entry = load_bundled_catalog(&app)
        .into_iter()
        .find(|e| e.id == catalog_id)
        .ok_or_else(|| format!("unknown catalog entry {catalog_id}"))?;

    if safe_child_dir(&models_dir, &entry.id).is_none() {
        return Err("invalid model id".into());
    }
    let plan = catalog::plan_entry_downloads(&entry, &models_dir);
    let model_dir = plan.model_dir.clone();
    let file_count = plan.files.len() as u32;

    let cancel = state.download_cancel.clone();
    // The UI triggers at most one download at a time; this reset assumes that
    // single-flight invariant (concurrent downloads would share one cancel flag).
    cancel.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let files = plan.files.clone();

    let dl = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        for (i, file) in files.iter().enumerate() {
            if file.shared && file.dest.exists() {
                continue;
            }
            let token = token_for_url(&file.url, &hf_token, &civitai_token);
            let name = file.dest.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            // Throttle: emit at most every ~4 MiB (or on completion) so multi-GB
            // downloads don't flood the frontend with ~100k IPC events.
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
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = dl {
        let _ = std::fs::remove_dir_all(&model_dir);
        return Err(e);
    }

    let mut components = manifest::ManifestComponents::default();
    for file in &plan.files {
        components.set_role(file.role, manifest::relativize(&model_dir, &file.dest.to_string_lossy()));
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

/// Download a single user-supplied URL into its own `models_dir/<id>/` folder,
/// infer its family from the filename, and write a manifest. Mirrors
/// `add_catalog_model`'s download/error-cleanup shape but for one file with no
/// pooled/shared components.
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
    let (models_dir, hf_token, civitai_token) = {
        let cfg = state.config.lock().unwrap();
        (PathBuf::from(&cfg.models_dir), cfg.hf_token.clone().unwrap_or_default(), cfg.civitai_token.clone().unwrap_or_default())
    };
    if models_dir.as_os_str().is_empty() {
        return Err("models directory is not set".into());
    }
    let id = new_model_id();
    let model_dir = safe_child_dir(&models_dir, &id).ok_or_else(|| "invalid model id".to_string())?;
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

    let filename = downloader::derive_filename(None, &url);
    let dest = model_dir.join(&filename);

    let cancel = state.download_cancel.clone();
    // The UI triggers at most one download at a time; this reset assumes that
    // single-flight invariant (concurrent downloads would share one cancel flag).
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
                    let _ = app2.emit(
                        "model:download:progress",
                        DownloadProgress {
                            downloaded,
                            total,
                            file_index: Some(0),
                            file_count: Some(1),
                            file_name: Some(name_for_event.clone()),
                        },
                    );
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
        manifest::relativize(&model_dir, &dest.to_string_lossy()),
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

/// Discover a HuggingFace model's downloadable variants, each with a size + fit
/// verdict against the given VRAM. Repo URLs enumerate the tree; a direct file
/// URL yields a single-row picker (size unknown until download).
#[tauri::command]
pub async fn list_hf_variants(
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
            // Network I/O runs off the main thread (mirrors download_model /
            // download_multifile) so a slow or hung HF request can't freeze the UI.
            let repo_for_fetch = repo.clone();
            let entries = tauri::async_runtime::spawn_blocking(move || hf::fetch_tree(&repo_for_fetch, &token))
                .await
                .map_err(|e| e.to_string())??;
            let variants = hf::classify_variants(&entries);
            if variants.is_empty() {
                return Err("No downloadable diffusion model found in that repo. Paste a direct file URL instead.".into());
            }
            Ok(variants.iter().map(|v| hf::rate_variant(&repo, v, vram_total_mb)).collect())
        }
    }
}

/// Pick a folder (used for adding watched model folders / changing the primary).
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dir = app.dialog().file().blocking_pick_folder();
    dir.and_then(|d| d.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn list_recipes() -> Vec<recipes::RecipeInfo> {
    recipes::recipe_infos()
}

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
            // Accept every recognized model extension (safetensors/ckpt/gguf) so a
            // .gguf diffusion model is auto-detected too. `recipes::detect` matches
            // on filename substrings, so it is already extension-agnostic.
            if p.is_file() && models::is_model_file(&p) {
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

/// Validate a definition before persisting: id must be non-blank and all
/// required roles for the family must be filled.
fn validate_model_definition(def: &ModelDefinition) -> Result<(), String> {
    if def.id.trim().is_empty() {
        return Err("Model id must not be empty.".into());
    }
    if let Some(recipe) = recipes::recipe_for(&def.family) {
        let missing = recipe.missing_required_roles(&def.components);
        if !missing.is_empty() {
            return Err(format!("Missing required components: {missing:?}"));
        }
    } else {
        return Err(format!("Unknown model family: {}", def.family));
    }
    Ok(())
}

/// Insert or update a definition (matched by id) and persist. Validates that
/// the id is non-blank and required roles for the family are filled before saving.
#[tauri::command]
pub fn save_model_definition(state: State<AppState>, def: ModelDefinition) -> Result<(), String> {
    validate_model_definition(&def)?;
    let mut cfg = state.config.lock().unwrap();
    if let Some(existing) = cfg.model_definitions.iter_mut().find(|d| d.id == def.id) {
        *existing = def;
    } else {
        cfg.model_definitions.push(def);
    }
    config::save_config_to(&config::config_file_path(), &cfg).map_err(|e| e.to_string())
}

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

/// Pure id guard, no filesystem check: `id` must be non-blank, not the shared
/// pool name, and free of path separators, and the resulting path must land
/// directly under `models_dir`. Used to validate an id *before* its folder
/// exists (e.g. a fresh catalog install), unlike `safe_model_dir` below.
fn safe_child_dir(models_dir: &std::path::Path, id: &str) -> Option<PathBuf> {
    if id.trim().is_empty() || id == "shared" || id.contains('/') || id.contains('\\') {
        return None;
    }
    let folder = models_dir.join(id);
    (folder.parent() == Some(models_dir)).then_some(folder)
}

/// The per-model folder `<models_dir>/<id>`, but only when `id` names a safe,
/// direct child that actually exists as a directory: non-blank, not the shared
/// pool, and free of path separators. Returns `None` otherwise so callers can
/// never trash the shared pool or escape `models_dir` via a crafted id. Used
/// by the delete path, which only ever targets an already-existing folder
/// (see `safe_child_dir` above for the pre-existence variant used before a
/// fresh download creates the folder).
fn safe_model_dir(models_dir: &std::path::Path, id: &str) -> Option<PathBuf> {
    safe_child_dir(models_dir, id).filter(|f| f.is_dir())
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
    // The guard refuses a blank id, "shared", or a separator-bearing id so a
    // crafted definition can never trash the pool or escape models_dir.
    if let Some(folder) = safe_model_dir(std::path::Path::new(&cfg.models_dir), &def.id) {
        let _ = trash::delete(&folder);
    }
    config::save_config_to(&config::config_file_path(), &cfg).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ModelComponents;

    fn flux_def(id: &str) -> ModelDefinition {
        ModelDefinition {
            id: id.into(),
            name: "Test".into(),
            family: "flux1".into(),
            components: ModelComponents {
                diffusion_model: "flux1-schnell.safetensors".into(),
                clip_l: Some("clip_l.safetensors".into()),
                t5xxl: Some("t5xxl_fp16.safetensors".into()),
                vae: Some("ae.safetensors".into()),
                ..Default::default()
            },
        }
    }

    #[test]
    fn infers_family_from_filename() {
        assert_eq!(infer_single_file_family("flux1-schnell-Q4_K_M.gguf"), "flux1");
        assert_eq!(infer_single_file_family("sd_xl_base_1.0.safetensors"), "sdxl");
        assert_eq!(infer_single_file_family("v1-5-pruned-emaonly.safetensors"), "sd15");
        assert_eq!(infer_single_file_family("qwen-image-Q4.gguf"), "qwen-image");
    }

    #[test]
    fn new_model_id_is_unique() {
        assert_ne!(new_model_id(), new_model_id());
    }

    #[test]
    fn token_for_url_selects_by_host() {
        assert_eq!(token_for_url("https://huggingface.co/x/y.gguf", "HF", "CV"), "HF");
        assert_eq!(token_for_url("https://civitai.com/api/download/1", "HF", "CV"), "CV");
        assert_eq!(token_for_url("https://example.com/a.safetensors", "HF", "CV"), "");
    }

    #[test]
    fn rejects_empty_definition_id() {
        let err = validate_model_definition(&flux_def("")).unwrap_err();
        assert_eq!(err, "Model id must not be empty.");
    }

    #[test]
    fn rejects_blank_definition_id() {
        assert!(validate_model_definition(&flux_def("   ")).is_err());
    }

    #[test]
    fn accepts_valid_definition() {
        assert!(validate_model_definition(&flux_def("flux1-schnell")).is_ok());
    }

    #[test]
    fn scan_excludes_shared_pool_but_keeps_real_models() {
        use std::collections::HashSet;
        let root = std::env::temp_dir().join(format!("muchai-shared-excl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let real = root.join("realmodel.safetensors");
        let shared = root.join("shared/flux1/t5xxl.safetensors");
        for p in [&real, &shared] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }

        let models = scan_single_file_models(&[root.clone()], &HashSet::new(), &root);
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["realmodel"], "shared-pool component must be excluded, real model kept");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_model_dir_guards_shared_and_separators() {
        let root = std::env::temp_dir().join(format!("muchai-safedir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("shared")).unwrap();
        std::fs::create_dir_all(root.join("good")).unwrap();

        // Valid id → the real per-model folder.
        assert_eq!(safe_model_dir(&root, "good"), Some(root.join("good")));
        // Dangerous ids → refused, even though <root>/shared exists on disk.
        assert!(safe_model_dir(&root, "shared").is_none(), "must never target the shared pool");
        assert!(safe_model_dir(&root, "a/b").is_none(), "no forward-slash escape");
        assert!(safe_model_dir(&root, "a\\b").is_none(), "no backslash escape");
        assert!(safe_model_dir(&root, "").is_none());
        assert!(safe_model_dir(&root, "   ").is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

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
}
