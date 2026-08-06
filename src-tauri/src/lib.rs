mod catalog;
mod civitai;
mod command_builder;
mod commands;
mod config;
mod devices;
mod diskspace;
mod downloader;
mod engine;
mod engine_flags;
mod engine_install;
mod engine_release;
mod fit;
mod gallery;
mod hf;
mod library;
mod lora_detect;
mod loras;
mod manifest;
mod models;
mod progress_parser;
mod recipes;
mod sysmon;
mod types;
mod weights;

use commands::AppState;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    config::migrate_legacy_data_dirs();
    let initial = config::load_config_from(&config::config_file_path());
    let gallery_dir = initial.gallery_dir.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(AppState {
            config: Mutex::new(initial),
            child: Arc::new(Mutex::new(None)),
            download_cancel: Arc::new(AtomicBool::new(false)),
            gpu_devices: Arc::new(Mutex::new(None)),
            engine_version: Arc::new(Mutex::new(None)),
            generating: Arc::new(AtomicBool::new(false)),
        })
        .setup(move |app| {
            // Allow the configured gallery dir for the asset protocol so saved
            // images load even when it's not the default location.
            let _ = app.asset_protocol_scope().allow_directory(&gallery_dir, true);

            // Allow the live-preview directory so convertFileSrc can load the
            // draft file the engine writes during generation.
            let preview_file = commands::preview_path();
            if let Some(dir) = preview_file.parent() {
                let _ = std::fs::create_dir_all(dir);
                let _ = app.asset_protocol_scope().allow_directory(dir, true);
            }

            // Background system-stats loop: emit "system:stats" ~every second,
            // keyed to the device the user has selected for generation.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // Many driver installs ship only the versioned NVML
                // (libnvidia-ml.so.1) without the unversioned dev symlink that
                // Nvml::init() loads by default. Try the versioned name first,
                // then fall back to the default so both layouts work.
                let nvml = nvml_wrapper::Nvml::builder()
                    .lib_path(std::ffi::OsStr::new("libnvidia-ml.so.1"))
                    .init()
                    .or_else(|_| nvml_wrapper::Nvml::init())
                    .ok();
                let providers = sysmon::default_providers(nvml);
                let mut sys = sysinfo::System::new();
                loop {
                    // Re-read the selection each tick so changing the device in the
                    // UI re-keys the monitor without restarting the thread.
                    let target = {
                        let state = handle.state::<AppState>();
                        let selection = state.config.lock().unwrap().gpu_device.clone();
                        let devices = state.gpu_devices.lock().unwrap().clone().unwrap_or_default();
                        sysmon::resolve_target(selection, &devices)
                    };
                    let stats = sysmon::gather(&mut sys, &providers, &target);
                    let _ = handle.emit("system:stats", stats);
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::set_settings,
            commands::list_history,
            commands::generate,
            commands::cancel_generation,
            commands::pick_model_file,
            commands::pick_gallery_dir,
            commands::open_path,
            commands::open_url,
            commands::list_library,
            commands::rate_library,
            commands::delete_model,
            commands::cancel_download,
            commands::pick_folder,
            commands::list_gpu_devices,
            commands::engine_version,
            commands::delete_image,
            commands::list_recipes,
            commands::detect_folder,
            commands::list_hf_variants,
            commands::recommended_settings,
            commands::catalog_entries,
            commands::add_catalog_model,
            commands::add_url_model,
            commands::add_local_model,
            commands::edit_model,
            commands::delete_model_entry,
            commands::disk_space,
            commands::check_catalog_space,
            commands::list_reclaimable,
            commands::trash_dir,
            commands::list_loras,
            commands::list_families,
            commands::detect_lora_family,
            commands::pick_lora_file,
            commands::add_local_lora,
            commands::add_url_lora,
            commands::edit_lora,
            commands::delete_lora,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MuchAI");
}
