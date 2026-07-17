use crate::types::{AppConfig, GenerationRequest, Theme};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

fn project_dirs() -> Option<ProjectDirs> {
    // On Linux the qualifier/organization are ignored — XDG paths use only the
    // app name ("fridai"), so ~/.config/fridai and ~/.local/share/fridai are
    // stable regardless of the qualifier/org values passed here.
    ProjectDirs::from("cz", "mst", "fridai")
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
        .unwrap_or_else(|| PathBuf::from("./fridai-config.json"))
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
}
