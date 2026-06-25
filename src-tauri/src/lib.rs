mod command_builder;
mod commands;
mod config;
mod engine;
mod gallery;
mod progress_parser;
mod sysmon;
mod types;

use commands::AppState;
use std::sync::{Arc, Mutex};
use tauri::Emitter;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial = config::load_config_from(&config::config_file_path());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            config: Mutex::new(initial),
            child: Arc::new(Mutex::new(None)),
        })
        .setup(|app| {
            // Background system-stats loop: emit "system:stats" ~every second.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let nvml = nvml_wrapper::Nvml::init().ok();
                let mut sys = sysinfo::System::new();
                loop {
                    let stats = sysmon::gather(&mut sys, nvml.as_ref());
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running fridAI");
}
