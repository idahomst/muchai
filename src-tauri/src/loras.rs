//! The LoRA pool: `models_dir/loras/` plus its `index.json`.
//!
//! Deliberately not part of `library.rs`. The engine's `--lora-model-dir` takes
//! one flat directory and does not recurse, so the manifest-per-folder layout
//! models use cannot apply here; forcing LoRAs into `ModelManifest` would mean
//! making `diffusion_model` optional — weakening a type every existing consumer
//! relies on — and still building a flat mirror afterwards.
//!
//! A stray `index.json` inside the pool is inert: the engine only opens the
//! paths it is explicitly asked for by name.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const POOL_DIRNAME: &str = "loras";
pub const INDEX_FILENAME: &str = "index.json";
pub const INDEX_BACKUP_FILENAME: &str = "index.json.bad";
pub const INDEX_SCHEMA_VERSION: u32 = 1;

/// Where a LoRA came from. Mirrors `manifest::ManifestSource`'s tagged shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoraSource {
    Url { url: String },
    Local { original_path: String },
}

/// One row of `index.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoraEntry {
    pub id: String,
    /// Filename stem in the pool — and therefore the `NAME` the engine's
    /// `<lora:NAME:WEIGHT>` tag resolves. Unique within the pool.
    pub name: String,
    /// Shown in the UI. Freely renameable without touching the file.
    pub display_name: String,
    /// One of the `family` strings `recipes.rs` uses (`sd15`, `sdxl`, `flux1`,
    /// `flux2`, `qwen-image`, `z-image`). Empty means "unknown", which disables
    /// family filtering for this entry rather than hiding it.
    pub family: String,
    pub source: LoraSource,
    #[serde(default)]
    pub trigger_words: Vec<String>,
    #[serde(default)]
    pub size_bytes: u64,
}

/// The `index.json` document.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LoraIndex {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub loras: Vec<LoraEntry>,
}

pub fn pool_dir(models_dir: &Path) -> PathBuf {
    models_dir.join(POOL_DIRNAME)
}

pub fn index_path(models_dir: &Path) -> PathBuf {
    pool_dir(models_dir).join(INDEX_FILENAME)
}

/// Absolute path of a pool entry's weight file. Always `<name>.safetensors`:
/// the engine appends that extension itself when the tag carries none, so
/// storing the file under any other name would make every tag miss.
pub fn weight_path(models_dir: &Path, name: &str) -> PathBuf {
    pool_dir(models_dir).join(format!("{name}.safetensors"))
}

/// Read `index.json`. Absent → empty (a fresh install has no pool).
///
/// Unparseable → the file is moved to `index.json.bad` and an empty index is
/// returned. It is never silently overwritten: it may be the user's only record
/// of what they had installed, and the next save would destroy it.
pub fn load_index(models_dir: &Path) -> LoraIndex {
    let path = index_path(models_dir);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return LoraIndex::default();
    };
    match serde_json::from_str(&text) {
        Ok(index) => index,
        Err(_) => {
            let backup = pool_dir(models_dir).join(INDEX_BACKUP_FILENAME);
            let _ = std::fs::rename(&path, &backup);
            LoraIndex::default()
        }
    }
}

/// Write `index.json` (pretty). Creates the pool directory if absent.
pub fn save_index(models_dir: &Path, index: &LoraIndex) -> std::io::Result<()> {
    std::fs::create_dir_all(pool_dir(models_dir))?;
    let s = serde_json::to_string_pretty(index).expect("lora index serializes");
    std::fs::write(index_path(models_dir), s)
}

/// Reduce arbitrary text to a pool stem.
///
/// Only `[A-Za-z0-9_-]` survives; every other run of characters — including
/// `.`, which the engine would read as a file extension — collapses to a single
/// `-`. Leading/trailing `-` are trimmed and an empty result becomes `lora`.
pub fn sanitize_name(raw: &str) -> String {
    let stem = raw
        .strip_suffix(".safetensors")
        .or_else(|| raw.strip_suffix(".ckpt"))
        .or_else(|| raw.strip_suffix(".pt"))
        .unwrap_or(raw);
    let mut out = String::with_capacity(stem.len());
    let mut pending_sep = false;
    for c in stem.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            if pending_sep && !out.is_empty() {
                out.push('-');
            }
            pending_sep = false;
            out.push(c);
        } else {
            pending_sep = true;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "lora".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `base`, or `base-2`, `base-3`, … until nothing in the index and nothing on
/// disk claims it. Both are checked: a weight file with no index row still
/// occupies its name, because the engine resolves tags by filename alone.
pub fn unique_name(models_dir: &Path, index: &LoraIndex, base: &str) -> String {
    let taken = |candidate: &str| {
        index.loras.iter().any(|e| e.name == candidate)
            || weight_path(models_dir, candidate).exists()
    };
    if !taken(base) {
        return base.to_string();
    }
    for n in 2..1000 {
        let candidate = format!("{base}-{n}");
        if !taken(&candidate) {
            return candidate;
        }
    }
    format!("{base}-{}", uuid::Uuid::new_v4())
}

/// A filesystem-safe unique LoRA id. Mirrors `commands::new_model_id`.
pub fn new_lora_id() -> String {
    format!("lora-{}", uuid::Uuid::new_v4())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("muchai-loras-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn entry(name: &str) -> LoraEntry {
        LoraEntry {
            id: format!("lora-{name}"),
            name: name.to_string(),
            display_name: name.to_string(),
            family: "sdxl".into(),
            source: LoraSource::Url { url: "https://example.test/x.safetensors".into() },
            trigger_words: vec!["film grain".into()],
            size_bytes: 42,
        }
    }

    #[test]
    fn index_round_trips_through_disk() {
        let dir = tmp("roundtrip");
        let index = LoraIndex { schema_version: INDEX_SCHEMA_VERSION, loras: vec![entry("film-grain")] };
        save_index(&dir, &index).unwrap();
        assert_eq!(load_index(&dir), index);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_index_loads_as_empty() {
        let dir = tmp("missing");
        assert_eq!(load_index(&dir), LoraIndex::default());
        // Reading must not create anything — a fresh install has no pool yet.
        assert!(!index_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_keys_are_ignored_and_absent_optionals_default() {
        let dir = tmp("forward-compat");
        std::fs::create_dir_all(pool_dir(&dir)).unwrap();
        std::fs::write(
            index_path(&dir),
            r#"{"schema_version":1,"future_field":true,"loras":[
                 {"id":"lora-1","name":"a","display_name":"A","family":"sd15",
                  "source":{"kind":"local","original_path":"/home/u/a.safetensors"},
                  "unexpected":42}]}"#,
        )
        .unwrap();
        let index = load_index(&dir);
        assert_eq!(index.loras.len(), 1);
        assert!(index.loras[0].trigger_words.is_empty());
        assert_eq!(index.loras[0].size_bytes, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_index_recovers_to_empty_and_preserves_the_original() {
        let dir = tmp("corrupt");
        std::fs::create_dir_all(pool_dir(&dir)).unwrap();
        std::fs::write(index_path(&dir), "{ this is not json").unwrap();
        assert_eq!(load_index(&dir), LoraIndex::default());
        // The unreadable file is moved aside, never silently overwritten: it may
        // be the only record of what the user had installed.
        let backup = pool_dir(&dir).join("index.json.bad");
        assert!(backup.exists());
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{ this is not json");
        assert!(!index_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_name_keeps_only_tag_safe_characters() {
        // The stem goes straight into `<lora:NAME:WEIGHT>` and is resolved as
        // `<dir>/NAME.safetensors`, so anything that could break the tag or be
        // mistaken for an extension is collapsed to '-'.
        assert_eq!(sanitize_name("Film Grain v2"), "Film-Grain-v2");
        assert_eq!(sanitize_name("film-grain.safetensors"), "film-grain");
        assert_eq!(sanitize_name("detail.tweaker.v1.5"), "detail-tweaker-v1-5");
        assert_eq!(sanitize_name("<lora:evil:1.0>"), "lora-evil-1-0");
        assert_eq!(sanitize_name("  ///  "), "lora");
        assert_eq!(sanitize_name(""), "lora");
        assert_eq!(sanitize_name("2973304"), "2973304");
    }

    #[test]
    fn unique_name_suffixes_on_collision_with_index_or_pool() {
        let dir = tmp("unique");
        std::fs::create_dir_all(pool_dir(&dir)).unwrap();
        let index = LoraIndex { schema_version: INDEX_SCHEMA_VERSION, loras: vec![entry("film-grain")] };
        assert_eq!(unique_name(&dir, &index, "film-grain"), "film-grain-2");
        assert_eq!(unique_name(&dir, &index, "other"), "other");
        // A stray file with no index entry still occupies the name: the engine
        // resolves by filename, so reusing it would apply the wrong weights.
        std::fs::write(weight_path(&dir, "orphan"), b"x").unwrap();
        assert_eq!(unique_name(&dir, &index, "orphan"), "orphan-2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn weight_path_always_appends_the_safetensors_extension() {
        let p = weight_path(Path::new("/models"), "film-grain");
        assert_eq!(p, Path::new("/models/loras/film-grain.safetensors"));
    }
}
