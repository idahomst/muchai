mod catalog;
mod command_builder;
mod commands;
mod config;
mod devices;
mod downloader;
mod engine;
mod fit;
mod gallery;
mod hf;
mod models;
mod progress_parser;
mod recipes;
mod sysmon;
mod types;

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
        })
        .setup(move |app| {
            // Allow the configured gallery dir for the asset protocol so saved
            // images load even when it's not the default location.
            let _ = app.asset_protocol_scope().allow_directory(&gallery_dir, true);

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
            commands::list_models,
            commands::starter_models,
            commands::delete_model,
            commands::download_model,
            commands::cancel_download,
            commands::pick_folder,
            commands::list_gpu_devices,
            commands::delete_image,
            commands::list_recipes,
            commands::detect_folder,
            commands::save_model_definition,
            commands::delete_model_definition,
            commands::multifile_catalog,
            commands::download_multifile,
            commands::broken_definitions,
            commands::list_hf_variants,
        ])
        .run(tauri::generate_context!())
        .expect("error while running fridAI");
}
