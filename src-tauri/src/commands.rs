use crate::engine::{self, ChildSlot, GenError};
use crate::types::{AppConfig, DownloadProgress, GalleryItem, GenerationRequest, GpuDevice, ModelInfo};
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
    pub gpu_devices: Arc<Mutex<Option<Vec<GpuDevice>>>>,
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn engine_binary_name() -> &'static str {
    if cfg!(windows) { "sd-cli.exe" } else { "sd-cli" }
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

#[tauri::command]
pub fn set_settings(
    app: AppHandle,
    state: State<AppState>,
    config: AppConfig,
) -> Result<(), String> {
    config::save_config_to(&config::config_file_path(), &config).map_err(|e| e.to_string())?;
    // Keep the asset-protocol scope in sync so images in a newly chosen gallery
    // dir can be displayed without restarting the app.
    let _ = app
        .asset_protocol_scope()
        .allow_directory(&config.gallery_dir, true);
    // A changed engine path means a different binary that may enumerate devices
    // in a different order — drop the cached list so the next probe re-reads it,
    // preserving index parity with `--backend vulkanN`.
    {
        let mut cfg = state.config.lock().unwrap();
        if cfg.sd_binary_path != config.sd_binary_path {
            *state.gpu_devices.lock().unwrap() = None;
        }
        *cfg = config;
    }
    Ok(())
}

#[tauri::command]
pub fn list_history(state: State<AppState>) -> Vec<GalleryItem> {
    let dir = state.config.lock().unwrap().gallery_dir.clone();
    gallery::list_items(std::path::Path::new(&dir))
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
) -> Result<Vec<GalleryItem>, String> {
    let cfg = state.config.lock().unwrap().clone();
    // Validate the saved device against the enumerated list (cached); a stale
    // selection silently falls back to the engine default.
    let binary = resolve_binary(&app, &cfg)
        .ok_or_else(|| "stable-diffusion engine not found. Set its path in Settings.".to_string())?;
    let backend = {
        // Enumerate on demand (and cache) if the picker never warmed the list,
        // so a stored selection is always validated before mapping to a backend.
        let mut guard = state.gpu_devices.lock().unwrap();
        let devices = guard
            .get_or_insert_with(|| crate::devices::enumerate(&binary))
            .clone();
        crate::devices::validate_gpu_selection(cfg.gpu_device.clone(), &devices)
            .map(|s| format!("vulkan{}", s.index))
    };

    let gallery_dir = PathBuf::from(&cfg.gallery_dir);
    std::fs::create_dir_all(&gallery_dir).map_err(|e| e.to_string())?;
    let id = uuid::Uuid::new_v4().to_string();
    let image_path = gallery_dir.join(format!("{id}.png"));

    let slot = state.child.clone();
    let app2 = app.clone();
    let req = request.clone();
    let img = image_path.clone();
    let backend_owned = backend;

    // Run the (blocking) engine on a worker thread so the async command yields.
    let result = tauri::async_runtime::spawn_blocking(move || {
        engine::run_generation(&binary, &req, &img, backend_owned.as_deref(), &slot, |p| {
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
                let p = gallery_dir.join(format!("{id}_{i}.png"));
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
                    let _ = app2
                        .emit("model:download:progress", DownloadProgress { downloaded, total });
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

/// Pick a folder (used for adding watched model folders / changing the primary).
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dir = app.dialog().file().blocking_pick_folder();
    dir.and_then(|d| d.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}
