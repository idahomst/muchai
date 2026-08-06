use crate::engine::{self, ChildSlot, GenError};
use crate::types::{
    AppConfig, DownloadProgress, EngineSelection, GalleryItem, GenerationRequest, GpuDevice,
};
use crate::recipes::{self, ComponentRole};
use crate::types::ModelRef;
use crate::{catalog, config, downloader, fit, gallery, hf, library, loras, manifest, models, types};
use std::path::{Path, PathBuf};
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
    pub engine_version: Arc<Mutex<Option<Option<String>>>>,
    /// Set for the whole of a `generate` call — see [`RunGuard`].
    pub generating: Arc<AtomicBool>,
}

/// Holds the single-flight claim on generation, releasing it on drop.
///
/// Generation shares three singletons: the one `child` slot (so Cancel kills
/// whichever run registered last), the one fixed live-preview path (so two runs
/// interleave frames, and the first to finish deletes the other's draft), and
/// one un-tagged stream of `generation:*` events. A second concurrent run
/// corrupts all three. The Generate button already gates this in the UI; the
/// guard makes it the backend's own invariant, so any future caller — a queue,
/// a shortcut, a local API — is turned away instead of quietly breaking it.
///
/// Drop-based release rather than an explicit clear: `generate` has many `?`
/// early returns, and a leaked flag would refuse every later run for the rest
/// of the session.
struct RunGuard(Arc<AtomicBool>);

impl RunGuard {
    /// `Some` when this call claimed the slot, `None` when a run is already active.
    fn claim(flag: &Arc<AtomicBool>) -> Option<Self> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(flag.clone()))
    }
}

impl Drop for RunGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(crate) fn engine_binary_name() -> &'static str {
    if cfg!(windows) { "sd-cli.exe" } else { "sd-cli" }
}

/// Last path segment (handles both `/` and `\` separators), for family
/// heuristics. Returns the whole string when there is no separator.
fn basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
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
pub fn catalog_entries(
    app: AppHandle,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> Vec<catalog::RatedCatalogEntry> {
    catalog::rated_catalog_entries(load_bundled_catalog(&app), vram_total_mb, ram_total_mb)
}

/// One installed model's VRAM-fit rating for the selector popover. Estimate is
/// `None` when component files are missing (broken entry).
#[derive(serde::Serialize)]
pub struct LibraryFit {
    pub id: String,
    pub estimate_mb: Option<u64>,
    pub verdict: fit::FitVerdict,
}

/// Rate every installed library model against the detected VRAM budget. Thin
/// glue over `models::sum_memory_bytes` + `fit::estimate_and_verdict`.
///
/// Deliberately *memory* bytes, not file bytes: an fp8 checkpoint occupies
/// roughly twice its file size once loaded, and rating it by file size showed
/// models as "tight" that cannot physically fit.
#[tauri::command]
pub fn rate_library(state: State<AppState>, vram_total_mb: Option<u64>) -> Vec<LibraryFit> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    library::scan_library(&models_dir)
        .into_iter()
        .map(|e| {
            let bytes = models::sum_memory_bytes(&e.model.component_paths());
            let (estimate_mb, verdict) = fit::estimate_and_verdict(bytes, vram_total_mb);
            LibraryFit { id: e.id, estimate_mb, verdict }
        })
        .collect()
}

/// A tag is used directly as a directory name under the engine store, so it
/// must be a single, ordinary path component. A hand-edited config could
/// otherwise supply `../models` or `/etc` — `Path::join` discards the base
/// entirely when handed an absolute path — and point the spawn at anything.
pub(crate) fn is_valid_engine_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag != "."
        && tag != ".."
        && tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Liveness test for a candidate engine binary.
///
/// `Path::exists()` is too weak here: it accepts a directory (the user picks
/// the folder instead of the binary in the file dialog) and a file that lost
/// its `+x` bit (zip extraction, or a store on a `noexec` mount). Both would
/// be *returned* rather than falling back, and the user would get
/// "Permission denied" from `Command::new` — a message that points at the
/// wrong problem. Folding both into the fallback means the app self-heals
/// instead.
fn is_runnable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// Pure resolution of an `EngineSelection` to a binary path, given the built-in
/// engine's directory and the writable engine store. Extracted from
/// `resolve_binary` so every branch is testable without a Tauri handle.
///
/// `Downloaded` and `Custom` deliberately fall back to the built-in engine when
/// their target is gone. A user stuck on a bad build who cannot reach
/// Preferences can delete one directory and the app returns to the shipped
/// engine by itself, instead of refusing to start.
fn resolve_engine_path(
    sel: &EngineSelection,
    builtin_dir: Option<&Path>,
    engines_root: &Path,
) -> Option<PathBuf> {
    let builtin = || {
        builtin_dir
            .map(|d| d.join(engine_binary_name()))
            .filter(|p| is_runnable(p))
    };
    let target = match sel {
        EngineSelection::Builtin => None,
        EngineSelection::Downloaded { tag } if is_valid_engine_tag(tag) => {
            Some(engines_root.join(tag).join(engine_binary_name()))
        }
        EngineSelection::Downloaded { .. } => None,
        EngineSelection::Custom { path } => Some(PathBuf::from(path)),
    };
    target.filter(|p| is_runnable(p)).or_else(builtin)
}

/// Resolve the engine binary the app should spawn, honouring the configured
/// `EngineSelection`.
fn resolve_binary(app: &AppHandle, cfg: &AppConfig) -> Option<PathBuf> {
    let builtin = engine_dir(app);
    resolve_engine_path(&cfg.engine, builtin.as_deref(), &config::engines_dir())
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
pub fn engine_version(app: AppHandle, state: State<AppState>) -> Option<String> {
    if let Some(cached) = state.engine_version.lock().unwrap().as_ref() {
        return cached.clone();
    }
    let cfg = state.config.lock().unwrap().clone();
    let version = match resolve_binary(&app, &cfg) {
        Some(bin) => crate::devices::engine_version(&bin),
        None => None,
    };
    *state.engine_version.lock().unwrap() = Some(version.clone());
    version
}

/// Merge an incoming settings payload with the current backend state, keeping the
/// backend-owned `last_request` from `current` (the UI's copy can be stale; it has
/// its own dedicated command, so a preference save must never clobber it). Pure so
/// it is unit-testable.
fn merged_settings(current: &AppConfig, incoming: AppConfig) -> AppConfig {
    AppConfig {
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
    // Merge AND persist under the lock so the preserved backend-owned
    // `last_request` reflects the latest state and no concurrent mutator can slip
    // a write in between our merge and save. Matches the sibling mutators, which
    // all persist while still holding the lock.
    let gallery_dir = {
        let mut cfg = state.config.lock().unwrap();
        // A changed engine selection means a different binary that may enumerate
        // devices in a different order — drop the cached list so the next probe
        // re-reads it, preserving index parity with `--backend vulkanN`. Also
        // drop the cached engine version so About cannot report the previous
        // engine's commit after a switch.
        if cfg.engine != config.engine {
            *state.gpu_devices.lock().unwrap() = None;
            *state.engine_version.lock().unwrap() = None;
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

/// Fixed path for the live-preview draft the engine overwrites during a run.
/// In the OS temp dir (tmpfs on Linux) so the tiny, constantly-rewritten file
/// is RAM-backed and cleared on reboot. Safe as a fixed (non-unique) path
/// because generation is single-flight — enforced by [`RunGuard`].
pub fn preview_path() -> PathBuf {
    std::env::temp_dir().join("muchai-preview").join("preview.png")
}

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

#[tauri::command]
pub async fn generate(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GenerationRequest,
    device_vram_mb: Option<u64>,
) -> Result<Vec<GalleryItem>, String> {
    // Claimed first, before any work: everything below assumes it is the only
    // run in flight. Released when `_run` drops on any exit path.
    let _run = RunGuard::claim(&state.generating).ok_or_else(|| {
        "A generation is already running. Wait for it to finish, or press Cancel.".to_string()
    })?;
    let cfg = state.config.lock().unwrap().clone();
    // Single source of truth: for a managed model, re-read its components from
    // model.json here — the last stop before the engine — so a stale snapshot
    // (frontend store, persisted last_request, or a hand-edited manifest) can
    // never send wrong component paths. Ad-hoc models keep their literal ref.
    let request = {
        let mut r = request;
        r.model = library::resolve_request_model(std::path::Path::new(&cfg.models_dir), &r)?;
        r
    };
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
    // LoRAs are validated here, before anything expensive happens, for the same
    // reason the component check above is: the engine's own failure mode is a
    // warning and a wrong-looking-but-successful image.
    let lora_dir =
        loras::resolve_selection(std::path::Path::new(&cfg.models_dir), &request.loras)?;
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
    let app3 = app.clone();
    let req = request.clone();
    let img = image_path.clone();
    let backend_owned = backend;
    // Decide Low-VRAM for THIS run: manual toggle forces it on; otherwise
    // auto-engage when the summed weight bytes won't fit the selected GPU's VRAM.
    // Weights are summed as *in-memory* bytes (what estimate_vram_mb expects), so
    // fp8 checkpoints — which the engine widens to f16 on load — are counted at
    // their real cost. A broken model (some file un-stat'able) yields None →
    // treated as "unknown", no auto-engage.
    let is_cpu = backend_owned.as_deref() == Some("cpu");
    let weights_bytes = models::sum_memory_bytes(&request.model.component_paths());
    let (low_vram, auto_engaged) =
        crate::fit::resolve_low_vram(cfg.low_vram, weights_bytes, device_vram_mb, is_cpu);
    if auto_engaged {
        // One-time, payload-free signal; the note text lives in the frontend.
        let _ = app.emit("generation:low_vram_auto", ());
    }
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
    // Load-time precision: only the diffusion model is ever re-quantised, so it
    // is measured on its own and the remaining components counted as fixed cost.
    let diffusion_bytes =
        crate::weights::memory_bytes(std::path::Path::new(request.model.diffusion_path()));
    let other_bytes = weights_bytes
        .zip(diffusion_bytes)
        .map(|(all, diffusion)| all.saturating_sub(diffusion));
    let weight_type = crate::fit::resolve_weight_type(
        &cfg.load_precision,
        diffusion_bytes,
        other_bytes,
        device_vram_mb,
        is_cpu,
    );
    let engine_opts = crate::command_builder::EngineOptions {
        low_vram,
        preview_path: preview.as_ref().map(|p| p.to_string_lossy().into_owned()),
        weight_type,
        lora_dir,
    };

    // Run the (blocking) engine on a worker thread so the async command yields.
    let joined = tauri::async_runtime::spawn_blocking(move || {
        engine::run_generation(
            &binary,
            &req,
            &img,
            engine::RunOptions { backend: backend_owned.as_deref(), opts: engine_opts },
            &slot,
            |p| {
                let _ = app2.emit("generation:progress", p);
            },
            |name| {
                let _ = app3.emit("generation:lora_missing", name);
            },
        )
    })
    .await;

    // The engine has exited, so no more preview writes: remove the draft file
    // regardless of outcome (success, error, cancel, or a worker-thread panic).
    // Done before the `?` below so a JoinError can't leak the draft. Best-effort.
    if let Some(p) = &preview {
        let _ = std::fs::remove_file(p);
    }

    let result = joined.map_err(|e| e.to_string())?;

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
                // Gallery items are frozen historical snapshots: their `model`
                // is the fully-resolved ref that actually generated the image.
                // Clear `model_id` so replaying one never re-resolves against a
                // since-changed (or deleted) manifest.
                req_i.model_id = None;
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
        // User pressed Cancel: not a failure. Return no images so the frontend
        // drops back to idle (keeping the current preview) instead of showing a
        // red error, ready for the next run.
        Err(GenError::Cancelled) => Ok(Vec::new()),
        Err(GenError::NonZero { oom: true, .. }) => Err(
            "Out of GPU memory. Try a smaller width/height or batch count.".to_string(),
        ),
        // Surface the engine's own stderr so failures are diagnosable instead
        // of an opaque "exited with code N".
        Err(GenError::NonZero { code, stderr_tail, .. }) => {
            let tail = stderr_tail.trim();
            if let Some(hint) = lora_shape_hint(tail, &request.loras) {
                return Err(format!("{hint}\n\n{tail}"));
            }
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
pub async fn pick_model_file(app: AppHandle, start_dir: Option<String>) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = app
        .dialog()
        .file()
        .add_filter("Models", &["safetensors", "gguf", "ckpt"]);
    // Open in the caller-supplied folder (e.g. the model's own dir when editing)
    // so the user isn't dumped in the process CWD. Ignore a missing/invalid dir.
    if let Some(dir) = start_dir {
        let p = PathBuf::from(&dir);
        if p.is_dir() {
            dialog = dialog.set_directory(&p);
        }
    }
    let file = dialog.blocking_pick_file();
    file.and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

/// Open a folder (or file) in the OS file manager / default app. Runs the
/// opener via its Rust API so it isn't subject to the JS command's path scope
/// (the gallery dir is user-chosen and trusted).
#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| e.to_string())
}

/// Open an external URL in the user's default browser (e.g. a catalog entry's
/// source page, so we honestly surface where a model is downloaded from).
/// https-only, to avoid opening arbitrary local/file schemes.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https URLs may be opened".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pick_gallery_dir(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let dir = app.dialog().file().blocking_pick_folder();
    dir.and_then(|d| d.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
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

/// Infer a model family from a diffusion filename. Falls back to the sd15/sdxl
/// name heuristic when no family keyword matches.
fn infer_single_file_family(filename: &str) -> String {
    let lower = filename.to_lowercase();
    // Separators are dropped before matching. Vendors write the same model as
    // "flux2", "flux-2" and "flux_2", and testing the raw string filed a real
    // `flux-2-klein-4b-Q8.gguf` under `flux1` — it missed the "flux2" needle
    // and hit "flux" instead. That mislabelled model then hid every flux2 LoRA.
    let squashed: String = lower.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    for (needle, family) in [
        ("flux2", "flux2"),
        ("flux", "flux1"),
        ("qwen", "qwen-image"),
        ("zimage", "z-image"),
        ("sd3", "sd3"),
    ] {
        if squashed.contains(needle) {
            return family.to_string();
        }
    }
    if lower.contains("xl") { "sdxl".into() } else { "sd15".into() }
}

/// Translate a tensor-shape abort into something actionable, when a LoRA was in
/// play and is therefore the likely cause.
///
/// The engine dies on `GGML_ASSERT(ggml_can_mul_mat(a, b))` deep in the graph
/// and prints a C source path — unreadable, and it names no LoRA. A LoRA
/// trained for a different size of the same architecture reaches this: MuchAI's
/// family granularity cannot tell FLUX.2 klein 4B from klein 9B, so the user is
/// allowed to try the combination and needs to be told what went wrong.
///
/// Pure, and deliberately silent when nothing was selected — a shape mismatch
/// with no LoRA is a different bug and must not be misattributed.
fn lora_shape_hint(stderr_tail: &str, selection: &[types::LoraSelection]) -> Option<String> {
    if selection.is_empty() || !stderr_tail.contains("ggml_can_mul_mat") {
        return None;
    }
    let names: Vec<&str> = selection.iter().map(|s| s.name.as_str()).collect();
    Some(format!(
        "This model and LoRA don't fit together — the LoRA was trained for a \
         differently-sized version of this model. Turn off {} and try again, or \
         switch to the model it was made for.",
        names.join(", ")
    ))
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
    } else if u.contains("civitai.com") || u.contains("civitai.red") {
        civitai_token.to_string()
    } else {
        String::new()
    }
}

/// Engine flags (`vae_format`/`prediction`) for a family, derived from its
/// recipe. Unknown families and recipes that declare no flags (e.g. z-image)
/// yield empty defaults.
fn flags_for_family(family: &str) -> manifest::ManifestFlags {
    recipes::recipe_for(family)
        .map(|r| manifest::ManifestFlags {
            vae_format: r.vae_format.map(str::to_string),
            prediction: r.prediction.map(str::to_string),
        })
        .unwrap_or_default()
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
    // Seed engine flags from the family recipe so vae_format/prediction are
    // applied on install (e.g. sd3, flux1/2, qwen-image). Unknown families and
    // recipes with no flags (e.g. z-image) fall back to empty defaults.
    let flags = flags_for_family(&entry.family);
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
        flags,
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
            let family = crate::recipes::detect_best(std::slice::from_ref(&name)).map(|(r, _)| r.family.to_string());
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

/// Gen defaults for a family + its diffusion filename (schnell/dev detection).
/// `None` for families without a preset (custom/unknown) — the UI hides the button.
fn recommended_for_family(family: &str, diffusion_filename: &str) -> Option<types::GenDefaults> {
    crate::recipes::family_defaults(family, Some(diffusion_filename))
}

/// Resolve a model's recommended settings: a manifest-level override wins;
/// otherwise fall back to the family default (schnell/dev detection via the
/// diffusion filename). `None` when neither applies (custom/unknown family).
fn resolve_recommended(man: &manifest::ModelManifest) -> Option<types::GenDefaults> {
    if man.recommended_settings.is_some() {
        return man.recommended_settings;
    }
    recommended_for_family(&man.family, &basename(&man.components.diffusion_model))
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
    Ok(resolve_recommended(&man))
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

/// Register a model from a diffusion file already on disk, referenced in
/// place (its absolute path is stored, never copied/moved). Creates a fresh
/// per-model folder under `models_dir` holding only `model.json`.
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
    let model_dir = safe_child_dir(&models_dir, &id).ok_or_else(|| "invalid model id".to_string())?;
    std::fs::create_dir_all(&model_dir).map_err(|e| e.to_string())?;

    let filename = basename(&diffusion_path);
    let fam = family.unwrap_or_else(|| infer_single_file_family(&filename));
    let mut components = manifest::ManifestComponents::default();
    // Referenced-local: store the ABSOLUTE path (not relativized).
    components.set_role(ComponentRole::Diffusion, diffusion_path.clone());
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

/// Save the full editable surface of a model's manifest: name, family, engine
/// flags, component paths, and the optional recommended-settings override.
/// Component paths arrive absolute from the UI and are relativized against the
/// model folder (in-folder files become relative; pooled/external stay absolute).
#[tauri::command]
pub fn edit_model(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    family: String,
    flags: manifest::ManifestFlags,
    components: manifest::ManifestComponents,
    recommended_settings: Option<types::GenDefaults>,
) -> Result<library::LibraryEntry, String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    let model_dir = models_dir.join(&id);
    let mut man = manifest::load_from(&model_dir).map_err(|e| e.to_string())?;

    // Relativize each provided path; drop empty optional roles to None.
    let opt = |o: &Option<String>| {
        o.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| manifest::relativize(&model_dir, s))
    };
    let normalized = manifest::ManifestComponents {
        diffusion_model: manifest::relativize(&model_dir, components.diffusion_model.trim()),
        vae: opt(&components.vae),
        clip_l: opt(&components.clip_l),
        clip_g: opt(&components.clip_g),
        t5xxl: opt(&components.t5xxl),
        llm: opt(&components.llm),
    };

    man.set_editable(name, family, flags, normalized, recommended_settings);
    manifest::save_to(&model_dir, &man).map_err(|e| e.to_string())?;
    Ok(library::entry_from_manifest(&model_dir, &man))
}

/// Delete a library entry: moves its per-model folder to trash. Pooled
/// `shared/<family>` components referenced by other models are left intact.
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

/// Free bytes on the filesystem holding the models directory, or `None` when
/// the probe failed. Drives the passive "Disk free" line in the Add dialog.
#[tauri::command]
pub fn disk_space(state: State<AppState>) -> Option<u64> {
    let models_dir = state.config.lock().unwrap().models_dir.clone();
    crate::diskspace::available_bytes(std::path::Path::new(&models_dir))
}

/// Pre-flight for a catalog install: what it needs vs. what is free.
/// `free_bytes` is `None` when the probe failed, in which case `ok` is `true` —
/// an unmeasurable disk must not block the user.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpaceCheck {
    pub required_bytes: u64,
    pub free_bytes: Option<u64>,
    pub ok: bool,
}

#[tauri::command]
pub fn check_catalog_space(
    app: AppHandle,
    state: State<'_, AppState>,
    catalog_id: String,
) -> Result<SpaceCheck, String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    let entry = load_bundled_catalog(&app)
        .into_iter()
        .find(|e| e.id == catalog_id)
        .ok_or_else(|| format!("unknown catalog entry {catalog_id}"))?;
    let plan = catalog::plan_entry_downloads(&entry, &models_dir);
    let required_bytes = catalog::required_bytes(&plan);
    let free_bytes = crate::diskspace::available_bytes(&models_dir);
    let ok = free_bytes.is_none_or(|free| crate::diskspace::fits(free, required_bytes));
    Ok(SpaceCheck { required_bytes, free_bytes, ok })
}

/// Installed models with their on-disk footprint — exactly what
/// `delete_model_entry` reclaims (its own folder; pooled `shared/<family>`
/// components are never removed by a model delete, so they are not counted
/// against any model). Sorted largest first.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReclaimableModel {
    pub id: String,
    pub name: String,
    pub size_bytes: u64,
}

#[tauri::command]
pub fn list_reclaimable(state: State<AppState>) -> Vec<ReclaimableModel> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    let mut out: Vec<ReclaimableModel> = library::scan_library(&models_dir)
        .into_iter()
        .map(|e| {
            let size_bytes = crate::diskspace::dir_size(&models_dir.join(&e.id));
            ReclaimableModel { id: e.id, name: e.name, size_bytes }
        })
        .collect();
    out.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    out
}

/// The trash folder that a model delete on this models directory actually
/// writes to, when it exists. Deleting a model moves it to the trash, which on
/// a same-filesystem trash frees no space until emptied — the blocked panel
/// offers to open this folder when it observes that.
#[tauri::command]
pub fn trash_dir(state: State<AppState>) -> Option<String> {
    let models_dir = state.config.lock().unwrap().models_dir.clone();
    crate::diskspace::trash_dir_for(std::path::Path::new(&models_dir))
        .map(|p| p.to_string_lossy().into_owned())
}

/// A freshly-added LoRA plus the detector's opinion of its base family.
///
/// `candidates` exists because a URL can only be inspected after it has been
/// downloaded. When `lora.family` came back empty, the detector was either
/// silent or torn between the entries in `candidates`, and the dialog asks the
/// user — seeding the dropdown from `candidates` when it isn't empty.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AddedLora {
    pub lora: loras::LoraInfo,
    pub candidates: Vec<String>,
}

/// Pick the family to store from a detector result: only an unambiguous single
/// candidate is trusted. Anything else stays empty and the user is asked.
fn settled_family(candidates: &[String]) -> String {
    match candidates {
        [only] => only.clone(),
        _ => String::new(),
    }
}

#[tauri::command]
pub fn list_loras(state: State<AppState>) -> Vec<loras::LoraInfo> {
    let models_dir = state.config.lock().unwrap().models_dir.clone();
    loras::list(std::path::Path::new(&models_dir))
}

/// Every family a LoRA can be filed under. **Not** `list_recipes` — that
/// returns multi-file download recipes, which omits `sd15` and `sdxl` (they are
/// single-file heuristics, see `commands::infer_single_file_family`) and
/// includes `custom` (not a base family). Between them those two omissions
/// cover most LoRAs published anywhere.
#[tauri::command]
pub fn list_families() -> Vec<String> {
    recipes::FAMILIES.iter().map(|s| s.to_string()).collect()
}

/// Base families a safetensors file's tensor names are consistent with. Empty
/// means "can't tell" — the caller asks the user.
#[tauri::command]
pub fn detect_lora_family(path: String) -> Vec<String> {
    crate::lora_detect::detect_family(std::path::Path::new(&path))
}

#[tauri::command]
pub async fn pick_lora_file(app: AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    let file = app
        .dialog()
        .file()
        .add_filter("LoRAs", &["safetensors"])
        .blocking_pick_file();
    file.and_then(|f| f.into_path().ok()).map(|p| p.to_string_lossy().into_owned())
}

/// Register a LoRA the user already has on disk. Symlinked into the pool, so
/// nothing is duplicated.
#[tauri::command]
pub fn add_local_lora(
    state: State<'_, AppState>,
    path: String,
    name: String,
    family: String,
) -> Result<loras::LoraInfo, String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    if models_dir.as_os_str().is_empty() {
        return Err("models directory is not set".into());
    }
    let src = PathBuf::from(&path);
    if !src.is_file() {
        return Err(format!("no such file: {path}"));
    }
    let pool = loras::pool_dir(&models_dir);
    std::fs::create_dir_all(&pool).map_err(|e| e.to_string())?;

    let filename = basename(&path);
    let label = if name.trim().is_empty() {
        loras::strip_weight_ext(&filename).to_string()
    } else {
        name.trim().to_string()
    };
    let index = loras::load_index(&models_dir);
    let pool_name = loras::unique_name(&models_dir, &index, &loras::sanitize_name(&label));
    let dest = loras::weight_path(&models_dir, &pool_name);
    loras::link_into_pool(&src, &dest).map_err(|e| e.to_string())?;

    let size_bytes = std::fs::metadata(&src).map(|m| m.len()).unwrap_or(0);
    let entry = loras::LoraEntry {
        id: loras::new_lora_id(),
        name: pool_name,
        display_name: label,
        family: family.trim().to_string(),
        // A file on disk carries no provenance — only Civitai reports one.
        base_model: String::new(),
        source: loras::LoraSource::Local { original_path: path },
        trigger_words: Vec::new(),
        size_bytes,
    };
    match loras::add(&models_dir, entry) {
        Ok(info) => Ok(info),
        Err(e) => {
            // The index is the source of truth; a weight file it doesn't list
            // would be an orphan nobody can select or delete.
            let _ = std::fs::remove_file(&dest);
            Err(e)
        }
    }
}

/// Download a LoRA from a pasted URL. Civitai links are resolved through its
/// API first, which is the only way to learn a LoRA's trigger words — and, for
/// a model-page link, the only way to learn what file to fetch at all.
#[tauri::command]
pub async fn add_url_lora(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    name: String,
) -> Result<AddedLora, String> {
    if !url.starts_with("https://") {
        return Err("URL must be https".into());
    }
    let (models_dir, hf_token, civitai_token) = {
        let cfg = state.config.lock().unwrap();
        (
            PathBuf::from(&cfg.models_dir),
            cfg.hf_token.clone().unwrap_or_default(),
            cfg.civitai_token.clone().unwrap_or_default(),
        )
    };
    if models_dir.as_os_str().is_empty() {
        return Err("models directory is not set".into());
    }

    // Resolve Civitai metadata off the async runtime — it's blocking I/O.
    let meta = match crate::civitai::parse_civitai_url(&url) {
        Some(r) => {
            let tok = civitai_token.clone();
            let v = tauri::async_runtime::spawn_blocking(move || crate::civitai::fetch_version(r, &tok))
                .await
                .map_err(|e| e.to_string())??;
            Some(v)
        }
        None => None,
    };
    let download_url = meta.as_ref().map(|m| m.download_url.clone()).unwrap_or_else(|| url.clone());
    let trigger_words = meta.as_ref().map(|m| m.trigger_words.clone()).unwrap_or_default();

    // Label priority: what the user typed, then Civitai's name, then the
    // filename the URL implies (which for a Civitai download link is a bare id).
    let label = if !name.trim().is_empty() {
        name.trim().to_string()
    } else if let Some(m) = &meta {
        m.display_name.clone()
    } else {
        loras::strip_weight_ext(&downloader::derive_filename(None, &download_url)).to_string()
    };

    let pool = loras::pool_dir(&models_dir);
    std::fs::create_dir_all(&pool).map_err(|e| e.to_string())?;
    let index = loras::load_index(&models_dir);
    let pool_name = loras::unique_name(&models_dir, &index, &loras::sanitize_name(&label));
    let dest = loras::weight_path(&models_dir, &pool_name);

    let cancel = state.download_cancel.clone();
    // The UI triggers at most one download at a time; this reset assumes that
    // single-flight invariant (concurrent downloads would share one cancel flag).
    cancel.store(false, Ordering::SeqCst);
    let app2 = app.clone();
    let dl_url = download_url.clone();
    let dest2 = dest.clone();
    let name_for_event = format!("{pool_name}.safetensors");

    let dl = tauri::async_runtime::spawn_blocking(move || -> Result<(), String> {
        let token = token_for_url(&dl_url, &hf_token, &civitai_token);
        let mut last_emit: u64 = 0;
        downloader::download_to(
            &dl_url,
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
        // Only the one file — the pool is shared, so never remove the directory.
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }

    // A Civitai link that needs a token answers HTTP 200 with a web page, so
    // the download "succeeds" and an HTML file lands as <name>.safetensors.
    // Nothing downstream would notice — the entry looks healthy and pre-flight
    // passes — until the engine fails to load it mid-run.
    if let Err(e) = loras::verify_weights_file(&dest) {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }

    let candidates = crate::lora_detect::detect_family(&dest);
    let size_bytes = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    let entry = loras::LoraEntry {
        id: loras::new_lora_id(),
        name: pool_name,
        display_name: label,
        family: settled_family(&candidates),
        base_model: meta.as_ref().map(|m| m.base_model.clone()).unwrap_or_default(),
        source: loras::LoraSource::Url { url },
        trigger_words,
        size_bytes,
    };
    match loras::add(&models_dir, entry) {
        Ok(lora) => Ok(AddedLora { lora, candidates }),
        Err(e) => {
            let _ = std::fs::remove_file(&dest);
            Err(e)
        }
    }
}

/// Change a LoRA's label and family. Its pool name — the engine tag, and the
/// key stored in every gallery item — is deliberately immutable.
#[tauri::command]
pub fn edit_lora(
    state: State<'_, AppState>,
    id: String,
    display_name: String,
    family: String,
) -> Result<loras::LoraInfo, String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    loras::rename(&models_dir, &id, &display_name, &family)
}

#[tauri::command]
pub fn delete_lora(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let models_dir = {
        let cfg = state.config.lock().unwrap();
        PathBuf::from(&cfg.models_dir)
    };
    loras::remove(&models_dir, &id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_guard_claims_a_free_slot() {
        let flag = Arc::new(AtomicBool::new(false));
        assert!(RunGuard::claim(&flag).is_some());
    }

    #[test]
    fn run_guard_refuses_a_second_concurrent_claim() {
        let flag = Arc::new(AtomicBool::new(false));
        let _held = RunGuard::claim(&flag).expect("first claim must succeed");
        assert!(RunGuard::claim(&flag).is_none(), "a second run must be turned away");
    }

    #[test]
    fn run_guard_releases_on_drop() {
        let flag = Arc::new(AtomicBool::new(false));
        drop(RunGuard::claim(&flag).expect("first claim must succeed"));
        assert!(
            RunGuard::claim(&flag).is_some(),
            "the flag must be free again once the run ends"
        );
    }

    #[test]
    fn run_guard_releases_on_unwind() {
        // Every early `?` return in `generate` drops the guard; so does a panic
        // in the command body. Both are the same Drop path, and if it ever
        // stopped running the app would wedge with Generate permanently refused.
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        let caught = std::panic::catch_unwind(move || {
            let _held = RunGuard::claim(&f).expect("first claim must succeed");
            panic!("boom");
        });
        assert!(caught.is_err());
        assert!(RunGuard::claim(&flag).is_some(), "a panic must not wedge the flag");
    }

    #[test]
    fn infers_family_from_filename() {
        assert_eq!(infer_single_file_family("flux1-schnell-Q4_K_M.gguf"), "flux1");
        assert_eq!(infer_single_file_family("sd_xl_base_1.0.safetensors"), "sdxl");
        assert_eq!(infer_single_file_family("v1-5-pruned-emaonly.safetensors"), "sd15");
        assert_eq!(infer_single_file_family("qwen-image-Q4.gguf"), "qwen-image");
    }

    #[test]
    fn a_separator_does_not_turn_flux2_into_flux1() {
        // Regression: this exact filename was filed as `flux1`, and the wrong
        // family then hid every flux2 LoRA from the picker.
        for name in ["flux-2-klein-4b-Q8.gguf", "flux_2_klein_9b.safetensors", "FLUX.2-dev.gguf"] {
            assert_eq!(infer_single_file_family(name), "flux2", "{name}");
        }
        assert_eq!(infer_single_file_family("z-image-turbo-Q2_K.gguf"), "z-image");
    }

    #[test]
    fn new_model_id_is_unique() {
        assert_ne!(new_model_id(), new_model_id());
    }

    #[test]
    fn a_shape_abort_is_blamed_on_the_lora_only_when_one_was_selected() {
        let abort = "ggml/src/ggml.c:3243: GGML_ASSERT(ggml_can_mul_mat(a, b)) failed";
        let sel = vec![types::LoraSelection { name: "klein4b-style".into(), weight: 1.0 }];
        let hint = lora_shape_hint(abort, &sel).expect("hint");
        assert!(hint.contains("klein4b-style"), "must name what to turn off: {hint}");
        // No LoRA in the run — the same abort means something else entirely.
        assert_eq!(lora_shape_hint(abort, &[]), None);
        // A LoRA is selected but the engine failed for an unrelated reason.
        assert_eq!(lora_shape_hint("failed to load model", &sel), None);
    }

    #[test]
    fn only_an_unambiguous_detection_settles_the_family() {
        let c = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(settled_family(&c(&["sdxl"])), "sdxl");
        // Torn or silent both mean "ask the user", not "guess the first one".
        assert_eq!(settled_family(&c(&["sdxl", "sd15"])), "");
        assert_eq!(settled_family(&c(&[])), "");
    }

    /// `npm run check` can't see across the Rust↔TS boundary, so the field
    /// names the frontend reads are pinned here. Renaming one without updating
    /// `src/lib/types.ts` would otherwise surface as `undefined` at runtime.
    #[test]
    fn lora_payloads_keep_the_field_names_the_frontend_reads() {
        let added = AddedLora {
            lora: loras::LoraInfo {
                id: "lora-1".into(),
                name: "film-grain".into(),
                display_name: "Film Grain".into(),
                family: "sdxl".into(),
                base_model: "SDXL 1.0".into(),
                trigger_words: vec!["film grain".into()],
                size_bytes: 42,
                broken: false,
            },
            candidates: vec!["sdxl".into()],
        };
        let v: serde_json::Value = serde_json::to_value(&added).unwrap();
        assert!(v.get("candidates").is_some());
        let l = v.get("lora").expect("lora");
        for key in ["id", "name", "display_name", "family", "trigger_words", "size_bytes", "broken"]
        {
            assert!(l.get(key).is_some(), "LoraInfo lost the `{key}` field");
        }
    }

    #[test]
    fn catalog_install_backfills_engine_flags_from_recipe() {
        // Families whose recipe declares engine flags must carry them on install,
        // so a freshly catalog-installed model generates with the right flags
        // without the user editing the manifest.
        let sd3 = flags_for_family("sd3");
        assert_eq!(sd3.vae_format.as_deref(), Some("sd3"));
        assert_eq!(sd3.prediction.as_deref(), Some("sd3_flow"));

        let flux2 = flags_for_family("flux2");
        assert_eq!(flux2.vae_format.as_deref(), Some("flux2"));
        // flux2 pins the VAE format but leaves prediction to the engine — see
        // the recipe comment; an explicit override crashed every generation.
        assert_eq!(flux2.prediction, None);

        let qwen = flags_for_family("qwen-image");
        assert_eq!(qwen.vae_format.as_deref(), Some("auto"));
        assert_eq!(qwen.prediction, None);

        // z-image intentionally declares no flags; unknown families fall back to
        // empty defaults too.
        assert_eq!(flags_for_family("z-image"), manifest::ManifestFlags::default());
        assert_eq!(flags_for_family("not-a-family"), manifest::ManifestFlags::default());
    }

    #[test]
    fn token_for_url_selects_by_host() {
        assert_eq!(token_for_url("https://huggingface.co/x/y.gguf", "HF", "CV"), "HF");
        assert_eq!(token_for_url("https://civitai.com/api/download/1", "HF", "CV"), "CV");
        assert_eq!(token_for_url("https://civitai.red/api/download/1", "HF", "CV"), "CV");
        assert_eq!(token_for_url("https://example.com/a.safetensors", "HF", "CV"), "");
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
    fn set_settings_preserves_last_request() {
        // Current backend state: has a meaningful, backend-owned last_request.
        let mut current = crate::config::default_config();
        current.last_request.prompt = "backend-owned prompt".into();

        // Incoming payload from the UI: preference fields changed, but it carries a
        // STALE (default) last_request.
        let mut incoming = crate::config::default_config();
        incoming.theme = crate::types::Theme::Light;
        incoming.low_vram = true;
        incoming.last_request = crate::types::GenerationRequest::default(); // stale

        let merged = merged_settings(&current, incoming);

        // Preference fields adopt the incoming values…
        assert_eq!(merged.theme, crate::types::Theme::Light);
        assert!(merged.low_vram);
        // …but the backend-owned last_request is preserved from `current`.
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
    fn recommended_settings_uses_manifest_family() {
        let defaults = recommended_for_family("flux1", "flux1-schnell-Q4_K_M.gguf").unwrap();
        assert_eq!(defaults.steps, 4);
        let dev = recommended_for_family("flux1", "flux1-dev.safetensors").unwrap();
        assert_eq!(dev.steps, 20);
        assert!(recommended_for_family("custom", "whatever.safetensors").is_none());
    }

    #[test]
    fn preview_path_is_under_temp_muchai_preview() {
        let p = super::preview_path();
        assert!(p.ends_with("muchai-preview/preview.png"), "got {p:?}");
        assert!(p.starts_with(std::env::temp_dir()), "must live under the OS temp dir");
    }

    /// Make an on-disk file pass [`is_runnable`]: `std::fs::write` does not set
    /// the executable bit, and `is_runnable` requires it on unix.
    #[cfg(unix)]
    fn make_executable(p: &Path) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[cfg(not(unix))]
    fn make_executable(_p: &Path) {}

    /// Build a temp tree with a fake built-in engine and one downloaded engine.
    /// Returns (root, builtin_dir, engines_root). Both binaries are executable
    /// so they pass [`is_runnable`].
    fn engine_fixture(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("muchai-resolve-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let builtin = root.join("resources/engine");
        let engines = root.join("engines");
        std::fs::create_dir_all(&builtin).unwrap();
        std::fs::create_dir_all(engines.join("master-797-5ef4a75")).unwrap();
        let builtin_bin = builtin.join(engine_binary_name());
        let dl_bin = engines.join("master-797-5ef4a75").join(engine_binary_name());
        std::fs::write(&builtin_bin, b"builtin").unwrap();
        std::fs::write(&dl_bin, b"dl").unwrap();
        make_executable(&builtin_bin);
        make_executable(&dl_bin);
        (root, builtin, engines)
    }

    #[test]
    fn resolves_builtin_downloaded_and_custom() {
        use crate::types::EngineSelection;
        let (root, builtin, engines) = engine_fixture("ok");
        let custom = root.join("mine/sd-cli");
        std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
        std::fs::write(&custom, b"mine").unwrap();
        make_executable(&custom);

        assert_eq!(
            resolve_engine_path(&EngineSelection::Builtin, Some(&builtin), &engines),
            Some(builtin.join(engine_binary_name()))
        );
        assert_eq!(
            resolve_engine_path(
                &EngineSelection::Downloaded { tag: "master-797-5ef4a75".into() },
                Some(&builtin),
                &engines
            ),
            Some(engines.join("master-797-5ef4a75").join(engine_binary_name()))
        );
        assert_eq!(
            resolve_engine_path(
                &EngineSelection::Custom { path: custom.to_string_lossy().into_owned() },
                Some(&builtin),
                &engines
            ),
            Some(custom)
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_downloaded_tag_falls_back_to_builtin() {
        use crate::types::EngineSelection;
        let (root, builtin, engines) = engine_fixture("gone");

        let got = resolve_engine_path(
            &EngineSelection::Downloaded { tag: "master-999-deadbee".into() },
            Some(&builtin),
            &engines,
        );

        assert_eq!(got, Some(builtin.join(engine_binary_name())));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_custom_path_falls_back_to_builtin() {
        use crate::types::EngineSelection;
        let (root, builtin, engines) = engine_fixture("nocustom");

        let got = resolve_engine_path(
            &EngineSelection::Custom { path: "/nonexistent/sd-cli".into() },
            Some(&builtin),
            &engines,
        );

        assert_eq!(got, Some(builtin.join(engine_binary_name())));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn custom_path_pointing_at_a_directory_falls_back_to_builtin() {
        use crate::types::EngineSelection;
        let (root, builtin, engines) = engine_fixture("customdir");

        // The user picked the containing folder instead of the binary inside
        // it in the file dialog. `exists()` would accept this; `is_runnable`
        // must not, since `Command::new` on a directory fails at spawn time.
        let dir = root.join("mine");
        std::fs::create_dir_all(&dir).unwrap();

        let got = resolve_engine_path(
            &EngineSelection::Custom { path: dir.to_string_lossy().into_owned() },
            Some(&builtin),
            &engines,
        );

        assert_eq!(got, Some(builtin.join(engine_binary_name())));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(unix)]
    fn downloaded_binary_without_exec_bit_falls_back_to_builtin() {
        use crate::types::EngineSelection;
        let (root, builtin, engines) = engine_fixture("noexec");

        // A binary that exists but lost its `+x` bit (zip extraction dropping
        // the mode bit, or a store mounted `noexec`) must fall back rather
        // than being returned and failing at spawn time.
        let tag = "master-noexec-0000000";
        let dl_dir = engines.join(tag);
        std::fs::create_dir_all(&dl_dir).unwrap();
        std::fs::write(dl_dir.join(engine_binary_name()), b"dl").unwrap();
        // Deliberately not made executable.

        let got = resolve_engine_path(
            &EngineSelection::Downloaded { tag: tag.into() },
            Some(&builtin),
            &engines,
        );

        assert_eq!(got, Some(builtin.join(engine_binary_name())));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_builtin_and_no_target_resolves_to_none() {
        use crate::types::EngineSelection;
        let engines = PathBuf::from("/nonexistent/engines");
        assert_eq!(resolve_engine_path(&EngineSelection::Builtin, None, &engines), None);
        assert_eq!(
            resolve_engine_path(
                &EngineSelection::Downloaded { tag: "master-797-5ef4a75".into() },
                None,
                &engines
            ),
            None
        );
        assert_eq!(
            resolve_engine_path(
                &EngineSelection::Custom { path: "/nonexistent/sd-cli".into() },
                None,
                &engines
            ),
            None
        );
    }

    #[test]
    fn invalid_engine_tag_falls_back_to_builtin_without_escaping_the_store() {
        use crate::types::EngineSelection;
        let (root, builtin, engines) = engine_fixture("tagvalidation");

        // `<engines>/../models` resolves to `<root>/models` (the fixture's
        // `engines` root is `<root>/engines`). Put a real, runnable binary
        // there so this tag is a live escape target: only `is_valid_engine_tag`
        // rejecting it — not the ordinary missing-target fallback — makes the
        // assertion below hold. Without the guard, this tag would resolve to
        // this binary instead of falling back.
        let escape = root.join("models").join(engine_binary_name());
        std::fs::create_dir_all(escape.parent().unwrap()).unwrap();
        std::fs::write(&escape, b"evil").unwrap();
        make_executable(&escape);

        for bad in ["../models", "/etc", ""] {
            let got = resolve_engine_path(
                &EngineSelection::Downloaded { tag: bad.into() },
                Some(&builtin),
                &engines,
            );
            assert_eq!(
                got,
                Some(builtin.join(engine_binary_name())),
                "tag {bad:?} must fall back to builtin"
            );
        }

        // A valid tag that simply doesn't exist still falls back cleanly, and a
        // valid tag that does exist resolves inside engines_root.
        let valid = resolve_engine_path(
            &EngineSelection::Downloaded { tag: "master-797-5ef4a75".into() },
            Some(&builtin),
            &engines,
        )
        .unwrap();
        assert!(valid.starts_with(&engines), "valid tag must resolve under engines_root");

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn is_valid_engine_tag_rejects_path_traversal_and_absolute_paths() {
        assert!(is_valid_engine_tag("master-797-5ef4a75"));
        assert!(is_valid_engine_tag("v1.2.3"));
        assert!(!is_valid_engine_tag(""));
        assert!(!is_valid_engine_tag("."));
        assert!(!is_valid_engine_tag(".."));
        assert!(!is_valid_engine_tag("../models"));
        assert!(!is_valid_engine_tag("/etc"));
        assert!(!is_valid_engine_tag("release/1.2"));
    }
}
