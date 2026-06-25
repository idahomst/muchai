use crate::types::{AppConfig, GenerationRequest};
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("eu", "livesport", "fridai")
}

pub fn default_gallery_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().join("gallery"))
        .unwrap_or_else(|| PathBuf::from("./gallery"))
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
        last_request: GenerationRequest::default(),
    }
}

/// Load config from a path; on missing file or parse error, return defaults.
pub fn load_config_from(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| default_config()),
        Err(_) => default_config(),
    }
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
}
