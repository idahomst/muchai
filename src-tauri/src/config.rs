use crate::types::{AppConfig, GenerationRequest, Theme};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

fn project_dirs() -> Option<ProjectDirs> {
    // On Linux the qualifier/organization are ignored — XDG paths use only the
    // app name ("muchai"), so ~/.config/muchai and ~/.local/share/muchai are
    // stable regardless of the qualifier/org values passed here.
    ProjectDirs::from("cz", "mst", "muchai")
}

pub fn default_gallery_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().join("gallery"))
        .unwrap_or_else(|| PathBuf::from("./gallery"))
}

pub fn default_models_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().join("models"))
        .unwrap_or_else(|| PathBuf::from("./models"))
}

pub fn config_file_path() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().join("config.json"))
        .unwrap_or_else(|| PathBuf::from("./muchai-config.json"))
}

pub fn default_config() -> AppConfig {
    AppConfig {
        sd_binary_path: None,
        default_model_path: None,
        gallery_dir: default_gallery_dir().to_string_lossy().into_owned(),
        models_dir: default_models_dir().to_string_lossy().into_owned(),
        extra_model_dirs: Vec::new(),
        gpu_device: None,
        params_expanded: false,
        theme: Theme::Dark,
        onboarded: false,
        model_definitions: Vec::new(),
        hf_token: None,
        civitai_token: None,
        low_vram: false,
        last_request: GenerationRequest::default(),
    }
}

/// Load config from a path; on missing file or parse error, return defaults.
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

/// Save config to a path, creating parent directories as needed.
pub fn save_config_to(path: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let s = serde_json::to_string_pretty(cfg).expect("config serializes");
    std::fs::write(path, s)
}

/// Move `legacy` to `new` if `new` doesn't exist yet and `legacy` does.
/// Best-effort and idempotent: returns Ok(false) (no-op) if `new` already
/// exists or `legacy` is absent. Never inspects directory contents.
///
/// Note: `std::fs::rename` will not cross filesystems, so on `EXDEV` the move
/// is skipped (best-effort) rather than falling back to a copy; the legacy data
/// is left intact in that case.
fn migrate_dir(legacy: &Path, new: &Path) -> std::io::Result<bool> {
    if new.exists() || !legacy.exists() {
        return Ok(false);
    }
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(legacy, new)?;
    Ok(true)
}

/// Rehome an absolute path that lives under `legacy` onto `new`, preserving the
/// relative tail. Returns None if the path isn't under `legacy`. A path exactly
/// equal to `legacy` maps to `new` with no trailing separator.
fn rewrite_prefix(p: &str, legacy: &Path, new: &Path) -> Option<String> {
    let rel = Path::new(p).strip_prefix(legacy).ok()?;
    if rel.as_os_str().is_empty() {
        return Some(new.to_string_lossy().into_owned());
    }
    Some(new.join(rel).to_string_lossy().into_owned())
}

/// Fix up stored gallery/models paths that pointed under the old data dir so
/// they follow the renamed directory. User-chosen paths outside the old data
/// dir are left untouched. Returns true iff a field was rewritten.
pub fn rewrite_data_paths(cfg: &mut AppConfig, legacy_data: &Path, new_data: &Path) -> bool {
    let mut changed = false;
    if let Some(g) = rewrite_prefix(&cfg.gallery_dir, legacy_data, new_data) {
        cfg.gallery_dir = g;
        changed = true;
    }
    if let Some(m) = rewrite_prefix(&cfg.models_dir, legacy_data, new_data) {
        cfg.models_dir = m;
        changed = true;
    }
    changed
}

/// One-time rebrand migration: move ~/.config/fridai → ~/.config/muchai and
/// ~/.local/share/fridai → ~/.local/share/muchai, then rehome the absolute
/// gallery/models paths inside the migrated config. Safe to call on every
/// startup: it no-ops once migration is complete. Does not log config contents.
pub fn migrate_legacy_data_dirs() {
    let (Some(legacy), Some(current)) = (
        ProjectDirs::from("cz", "mst", "fridai"),
        ProjectDirs::from("cz", "mst", "muchai"),
    ) else {
        return;
    };
    migrate_data_dirs_between(
        legacy.config_dir(),
        current.config_dir(),
        legacy.data_dir(),
        current.data_dir(),
    );
}

/// Move the config and data dirs from their legacy to their new locations and
/// rehome stored paths. Extracted from `migrate_legacy_data_dirs` so the
/// orchestration (including the partial-migration edge case) is unit-testable.
fn migrate_data_dirs_between(
    legacy_config: &Path,
    new_config: &Path,
    legacy_data: &Path,
    new_data: &Path,
) {
    let _ = migrate_dir(legacy_config, new_config);
    let _ = migrate_dir(legacy_data, new_data);

    // Only rehome stored paths once the new data dir is actually present;
    // otherwise a partial (e.g. cross-filesystem) migration would leave config
    // pointing at an empty dir while the real data stays under the legacy path.
    // Also only re-save when something actually changed, so we don't rewrite
    // config.json on every startup.
    if new_data.exists() {
        let cfg_path = new_config.join("config.json");
        if cfg_path.exists() {
            let mut cfg = load_config_from(&cfg_path);
            if rewrite_data_paths(&mut cfg, legacy_data, new_data) {
                let _ = save_config_to(&cfg_path, &cfg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_defaults() {
        let cfg = load_config_from(Path::new("/nonexistent/fridai/none.json"));
        assert!(cfg.sd_binary_path.is_none());
        assert_eq!(cfg.last_request.steps, 20);
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.default_model_path = Some("/m/x.safetensors".into());
        cfg.last_request.prompt = "hello".into();
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-corrupt-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "{ this is not valid json ]").unwrap();
        let cfg = load_config_from(&path);
        assert_eq!(cfg, default_config());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_config_has_models_dir() {
        let cfg = default_config();
        assert!(!cfg.models_dir.is_empty());
        assert!(cfg.extra_model_dirs.is_empty());
    }

    #[test]
    fn old_config_without_params_expanded_defaults_to_collapsed() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-pe-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no params_expanded key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.params_expanded, "missing params_expanded must default to false (collapsed)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn params_expanded_round_trips() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-pe2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.params_expanded = true;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert!(back.params_expanded);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_config_without_theme_defaults_to_dark() {
        use crate::types::Theme;
        let dir = std::env::temp_dir().join(format!("fridai-cfg-theme-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no theme key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert_eq!(cfg.theme, Theme::Dark, "missing theme must default to Dark");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_round_trips() {
        use crate::types::Theme;
        let dir = std::env::temp_dir().join(format!("fridai-cfg-theme2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.theme = Theme::Light;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert_eq!(back.theme, Theme::Light);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_config_without_onboarded_defaults_to_false() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-onb-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no onboarded key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.onboarded, "missing onboarded must default to false");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn onboarded_round_trips() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-onb2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.onboarded = true;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert!(back.onboarded);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_config_without_model_fields_loads_and_backfills_models_dir() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-old-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no models_dir / extra_model_dirs keys.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.models_dir.is_empty(), "empty models_dir must be backfilled to default");
        assert!(cfg.extra_model_dirs.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn old_config_without_low_vram_defaults_to_false() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-lv-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no low_vram key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.low_vram, "missing low_vram must default to false");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn low_vram_round_trips() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-lv2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.low_vram = true;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert!(back.low_vram);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrate_dir_moves_when_new_absent_and_legacy_present() {
        let base = std::env::temp_dir().join(format!("muchai-mig-{}", std::process::id()));
        let legacy = base.join("share/fridai");
        let new = base.join("share/muchai");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(legacy.join("marker.txt"), b"keep me").unwrap();

        let moved = migrate_dir(&legacy, &new).unwrap();

        assert!(moved);
        assert!(!legacy.exists());
        assert_eq!(std::fs::read(new.join("marker.txt")).unwrap(), b"keep me");
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn migrate_dir_is_noop_when_new_already_exists() {
        let base = std::env::temp_dir().join(format!("muchai-mig2-{}", std::process::id()));
        let legacy = base.join("fridai");
        let new = base.join("muchai");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(legacy.join("old.txt"), b"x").unwrap();

        let moved = migrate_dir(&legacy, &new).unwrap();

        assert!(!moved);
        assert!(legacy.exists()); // untouched
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn migrate_dir_is_noop_when_legacy_absent() {
        let base = std::env::temp_dir().join(format!("muchai-mig3-{}", std::process::id()));
        let legacy = base.join("fridai");
        let new = base.join("muchai");
        let moved = migrate_dir(&legacy, &new).unwrap();
        assert!(!moved);
        assert!(!new.exists());
    }

    #[test]
    fn rewrite_data_paths_rehomes_paths_under_legacy_and_leaves_others() {
        let legacy = Path::new("/home/u/.local/share/fridai");
        let new = Path::new("/home/u/.local/share/muchai");
        let mut cfg = default_config();
        cfg.gallery_dir = "/home/u/.local/share/fridai/gallery".into();
        cfg.models_dir = "/mnt/big/models".into(); // custom, outside legacy prefix

        let changed = rewrite_data_paths(&mut cfg, legacy, new);

        assert!(changed);
        assert_eq!(cfg.gallery_dir, "/home/u/.local/share/muchai/gallery");
        assert_eq!(cfg.models_dir, "/mnt/big/models"); // unchanged
    }

    #[test]
    fn rewrite_data_paths_returns_false_when_nothing_matches() {
        let legacy = Path::new("/home/u/.local/share/fridai");
        let new = Path::new("/home/u/.local/share/muchai");
        let mut cfg = default_config();
        cfg.gallery_dir = "/mnt/a/gallery".into();
        cfg.models_dir = "/mnt/b/models".into();

        let changed = rewrite_data_paths(&mut cfg, legacy, new);

        assert!(!changed);
        assert_eq!(cfg.gallery_dir, "/mnt/a/gallery");
        assert_eq!(cfg.models_dir, "/mnt/b/models");
    }

    #[test]
    fn rewrite_data_paths_rehomes_models_dir_under_legacy() {
        let legacy = Path::new("/home/u/.local/share/fridai");
        let new = Path::new("/home/u/.local/share/muchai");
        let mut cfg = default_config();
        cfg.gallery_dir = "/mnt/a/gallery".into(); // outside legacy prefix
        cfg.models_dir = "/home/u/.local/share/fridai/models".into();

        let changed = rewrite_data_paths(&mut cfg, legacy, new);

        assert!(changed);
        assert_eq!(cfg.models_dir, "/home/u/.local/share/muchai/models");
        assert_eq!(cfg.gallery_dir, "/mnt/a/gallery"); // unchanged
    }

    #[test]
    fn rewrite_data_paths_handles_path_equal_to_prefix() {
        let legacy = Path::new("/home/u/.local/share/fridai");
        let new = Path::new("/home/u/.local/share/muchai");
        let mut cfg = default_config();
        cfg.gallery_dir = "/home/u/.local/share/fridai".into();

        let changed = rewrite_data_paths(&mut cfg, legacy, new);

        assert!(changed);
        // No trailing separator artifact from new.join("").
        assert_eq!(cfg.gallery_dir, "/home/u/.local/share/muchai");
    }

    #[test]
    fn migrate_data_dirs_between_skips_rewrite_when_data_dir_absent() {
        let base = std::env::temp_dir().join(format!("muchai-mig4-{}", std::process::id()));
        let legacy_config = base.join("config/fridai");
        let new_config = base.join("config/muchai");
        let legacy_data = base.join("share/fridai");
        let new_data = base.join("share/muchai");

        // new_config dir with a config.json pointing under legacy_data.
        std::fs::create_dir_all(&new_config).unwrap();
        let legacy_gallery = legacy_data.join("gallery");
        let mut cfg = default_config();
        cfg.gallery_dir = legacy_gallery.to_string_lossy().into_owned();
        save_config_to(&new_config.join("config.json"), &cfg).unwrap();

        // Deliberately do NOT create legacy_data or new_data — simulates a
        // partial (cross-filesystem) migration where the data move didn't land.
        assert!(!new_data.exists());

        migrate_data_dirs_between(&legacy_config, &new_config, &legacy_data, &new_data);

        let back = load_config_from(&new_config.join("config.json"));
        assert_eq!(
            back.gallery_dir,
            legacy_gallery.to_string_lossy(),
            "rewrite must be skipped while new_data is absent"
        );
        std::fs::remove_dir_all(&base).ok();
    }
}
