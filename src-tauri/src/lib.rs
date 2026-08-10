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
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use tauri::{Emitter, Manager};
use types::AppConfig;

/// Seconds between background update checks. Once a day is plenty for a
/// dependency that ships a few releases a week, and keeps us far below GitHub's
/// 60-requests-per-hour unauthenticated limit.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

/// How long the check waits before its request: a cold start should not be
/// competing with a network round trip for the moment the window opens.
const CHECK_DELAY_SECS: u64 = 10;

/// The one thing the background check emits. Named rather than inlined because
/// the listener is in another language in another file, with nothing but this
/// string joining them: `the_frontend_listens_for_the_event_the_backend_emits`
/// is what makes a drift here fail the build instead of silently losing the
/// badge.
const UPDATE_AVAILABLE_EVENT: &str = "engine:update-available";

/// Is a background update check due?
///
/// A `last` in the future means the clock moved backwards (skew, a restored
/// backup); we check anyway rather than letting a bad timestamp disable updates
/// until the calendar catches up.
fn should_check(last: Option<u64>, now: u64) -> bool {
    match last {
        None => true,
        Some(last) if last > now => true,
        Some(last) => now.saturating_sub(last) >= CHECK_INTERVAL_SECS,
    }
}

/// Startup housekeeping over the engine store, plus the answer to "should a
/// background update check run?".
///
/// Both live here rather than inline in the `setup` closure so they can be
/// tested at all: nothing in a Tauri setup closure runs under `cargo test`.
///
/// `supported` is a parameter rather than a direct read of
/// `commands::UPDATES_SUPPORTED` for the same reason: that constant is `true`
/// on the only target we build, so as a direct read no test could reach the
/// arm where it is false, and dropping it would go unnoticed until a Windows
/// build started asking GitHub about an engine it cannot install.
fn engine_startup(cfg: &AppConfig, engines_root: &Path, now: u64, supported: bool) -> bool {
    // A crash or kill mid-install can only leave a `.staging-*` directory
    // behind, and such a directory is worthless by definition. Cheap, and
    // nothing about it depends on the check, so it is unconditional.
    //
    // The one case this gets wrong is a second MuchAI started while the first
    // is installing: the newcomer sweeps a staging directory that is very much
    // in use, and the install fails with an error the user can just retry.
    // Nothing installed is at risk — only the atomic rename publishes a tag —
    // and two instances already share one config file without either guarding
    // the other, so this is not the seam worth hardening first.
    engine_install::sweep_staging(engines_root);

    supported
        && cfg.engine_update_check
        && should_check(cfg.engine_last_check, now)
        // A custom build has no tag to compare against upstream, so the answer
        // could only ever be discarded — don't spend the request. Resolved the
        // way the commands resolve it, which means a custom selection that no
        // longer runs is checked: what it falls back to is the built-in engine,
        // and that does have a tag.
        && commands::current_tag(cfg, engines_root).is_some()
}

/// The tag whose badge to light, if any.
///
/// The dot means "new", not "pending": a tag the user has already been shown
/// must not light it again, or the same dot comes back on every launch until
/// they give in and install. Whether a release is newer than what runs at all
/// is `record_check`'s answer, which is why this takes its result rather than
/// comparing tags a second time.
fn badge_tag(update: Option<commands::EngineUpdate>, seen: Option<&str>) -> Option<String> {
    let tag = update?.tag;
    (seen != Some(tag.as_str())).then_some(tag)
}

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

            // Engine housekeeping, and the once-a-day update check. The check
            // is deliberately timid: the user did not ask for it, so no
            // network, a rate-limited IP or a GitHub outage must never surface
            // an error. Its only output is one event that lights a badge.
            let engines_root = config::engines_dir();
            let cfg = app.state::<AppState>().config.lock().unwrap().clone();
            if engine_startup(&cfg, &engines_root, commands::now_unix(), commands::UPDATES_SUPPORTED) {
                let check_handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(CHECK_DELAY_SECS));
                    // The request is made before any lock is taken: the config
                    // mutex is on the path of every generation, and a GitHub
                    // call that hangs to its read timeout would take them with
                    // it.
                    let fetched = engine_release::fetch_latest_release();
                    let state = check_handle.state::<AppState>();
                    let Ok(update) = commands::record_check(
                        &state.config,
                        &config::config_file_path(),
                        &engines_root,
                        fetched,
                    ) else {
                        return;
                    };
                    // Re-read rather than captured before the sleep: the user
                    // may have dismissed this very badge from Preferences while
                    // the request was in flight, and having already installed
                    // this tag once and since switched back to an older engine
                    // is exactly the case `seen` exists to cover.
                    let seen = state.config.lock().unwrap().engine_seen_tag.clone();
                    if let Some(tag) = badge_tag(update, seen.as_deref()) {
                        let _ = check_handle.emit(UPDATE_AVAILABLE_EVENT, tag);
                    }
                });
            }
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
            commands::engine_status,
            commands::engine_check_update,
            commands::engine_changelog,
            commands::engine_apply_update,
            commands::engine_select,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MuchAI");
}

#[cfg(test)]
mod tests {
    use super::*;
    use types::EngineSelection;

    /// An arbitrary "now" well clear of the epoch, so `now - a day` stays positive.
    const NOW: u64 = 1_800_000_000;

    /// A scratch engines root of this test's own.
    fn scratch(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("muchai-lib-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// A staging leftover and a finished install, as a crash mid-install leaves them.
    fn populate(root: &Path) {
        std::fs::create_dir_all(root.join(".staging-master-797-5ef4a75")).unwrap();
        std::fs::write(root.join(".staging-master-797-5ef4a75/sd-cli"), b"half").unwrap();
        std::fs::create_dir_all(root.join("master-782-b290693")).unwrap();
    }

    fn an_update(tag: &str) -> commands::EngineUpdate {
        commands::EngineUpdate {
            tag: tag.to_string(),
            asset_size: 45_020_326,
            current_tag: Some(engine_release::BUILTIN_ENGINE_TAG.to_string()),
        }
    }

    #[test]
    fn checks_when_never_checked_before() {
        assert!(should_check(None, NOW));
    }

    #[test]
    fn does_not_check_twice_within_a_day() {
        assert!(!should_check(Some(NOW), NOW));
        assert!(!should_check(Some(NOW - 3600), NOW));
        // One second short of the interval is still short of it.
        assert!(!should_check(Some(NOW - (CHECK_INTERVAL_SECS - 1)), NOW));
    }

    #[test]
    fn checks_again_after_a_day() {
        assert!(should_check(Some(NOW - CHECK_INTERVAL_SECS), NOW));
        assert!(should_check(Some(NOW - CHECK_INTERVAL_SECS - 1), NOW));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_wedge_the_check() {
        // A future timestamp (clock skew, a restored backup) must not disable
        // update checks until the calendar catches up.
        assert!(should_check(Some(NOW + 999_999), NOW));
    }

    #[test]
    fn startup_sweeps_staging_leftovers_and_leaves_installs_alone() {
        let root = scratch("sweep");
        populate(&root);

        assert!(engine_startup(&config::default_config(), &root, NOW, true), "a fresh config is due");

        assert!(
            !root.join(".staging-master-797-5ef4a75").exists(),
            "a staging directory is an incomplete install and is never worth keeping"
        );
        assert!(root.join("master-782-b290693").exists(), "a finished install must survive");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn housekeeping_still_runs_when_the_check_is_switched_off() {
        let root = scratch("sweep-off");
        populate(&root);
        let mut cfg = config::default_config();
        cfg.engine_update_check = false;

        assert!(!engine_startup(&cfg, &root, NOW, true), "the user turned the daily check off");

        assert!(
            !root.join(".staging-master-797-5ef4a75").exists(),
            "the sweep is housekeeping, not part of the check: it runs either way"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_check_already_made_today_is_not_repeated_at_the_next_start() {
        let root = scratch("due");
        populate(&root);
        let mut cfg = config::default_config();
        cfg.engine_last_check = Some(NOW - 3600);
        assert!(!engine_startup(&cfg, &root, NOW, true));

        assert!(
            !root.join(".staging-master-797-5ef4a75").exists(),
            "the sweep is not on the check's schedule either: leftovers go on the very next \
             start, not up to a day later"
        );

        cfg.engine_last_check = Some(NOW - CHECK_INTERVAL_SECS);
        assert!(engine_startup(&cfg, &root, NOW, true), "a day later it is due again");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_platform_that_cannot_install_an_engine_is_never_checked() {
        let root = scratch("unsupported");
        populate(&root);

        assert!(
            !engine_startup(&config::default_config(), &root, NOW, false),
            "asset selection is written for Linux x86_64 alone, so anywhere else the answer \
             could only be an update we cannot install"
        );
        assert!(
            !root.join(".staging-master-797-5ef4a75").exists(),
            "housekeeping is still housekeeping on a platform that cannot update"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_custom_engine_is_never_checked_unless_it_has_stopped_working() {
        let root = scratch("custom");
        let mine = root.join("sd-cli-of-my-own");
        std::fs::write(&mine, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&mine, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut cfg = config::default_config();
        cfg.engine = EngineSelection::Custom { path: mine.to_string_lossy().into_owned() };

        assert!(
            !engine_startup(&cfg, &root, NOW, true),
            "a self-compiled build has no tag to compare, so the request could only be wasted"
        );

        // …but one that no longer runs falls back to the built-in engine, which
        // does have a tag, and that is what upstream gets compared against.
        cfg.engine = EngineSelection::Custom { path: root.join("gone").to_string_lossy().into_owned() };
        assert!(engine_startup(&cfg, &root, NOW, true));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn badges_a_release_the_user_has_not_seen() {
        assert_eq!(
            badge_tag(Some(an_update("master-797-5ef4a75")), None),
            Some("master-797-5ef4a75".to_string())
        );
        // Having seen an older release does not cover a newer one.
        assert_eq!(
            badge_tag(Some(an_update("master-799-abc1234")), Some("master-797-5ef4a75")),
            Some("master-799-abc1234".to_string())
        );
    }

    #[test]
    fn does_not_badge_a_tag_the_user_has_already_seen() {
        assert_eq!(
            badge_tag(Some(an_update("master-797-5ef4a75")), Some("master-797-5ef4a75")),
            None,
            "the dot means new, not pending — otherwise it returns on every launch"
        );
    }

    #[test]
    fn nothing_to_badge_when_the_check_found_no_update() {
        assert_eq!(badge_tag(None, None), None);
        assert_eq!(badge_tag(None, Some("master-797-5ef4a75")), None);
    }

    /// The event name is the entire contract between the background check and
    /// the badge, and it is written twice in two languages. There is no test
    /// runner on the frontend, so this is the only automated check that the two
    /// halves still agree — the same trick `builtin_tag_matches_fetch_script`
    /// uses to pin the fetch script.
    #[test]
    fn the_frontend_listens_for_the_event_the_backend_emits() {
        // Matched at the `listen(...)` call, not anywhere in the file: the name
        // also appears in a doc comment there, and a stale comment must not be
        // able to keep this assertion true on its own.
        assert!(
            include_str!("../../src/lib/api.ts").contains(&format!("(\"{UPDATE_AVAILABLE_EVENT}\"")),
            "the badge is the check's only output; a name that drifts silently loses it"
        );
        // …and a listener nothing calls is the same silence by another route.
        assert!(
            include_str!("../../src/routes/+page.svelte").contains("onEngineUpdate("),
            "nothing subscribes to the check's one output"
        );
    }
}
