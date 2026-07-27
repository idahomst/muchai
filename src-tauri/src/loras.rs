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

/// One pool entry as the frontend sees it: the stored row plus liveness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoraInfo {
    pub id: String,
    pub name: String,
    pub display_name: String,
    pub family: String,
    pub trigger_words: Vec<String>,
    pub size_bytes: u64,
    /// The weight file is missing, or is a symlink whose target is gone. Same
    /// `broken` concept `library::LibraryEntry` exposes, so the UI can treat a
    /// dangling LoRA exactly like a dangling model.
    pub broken: bool,
}

impl LoraInfo {
    fn from_entry(models_dir: &Path, e: &LoraEntry) -> LoraInfo {
        LoraInfo {
            id: e.id.clone(),
            name: e.name.clone(),
            display_name: e.display_name.clone(),
            family: e.family.clone(),
            trigger_words: e.trigger_words.clone(),
            size_bytes: e.size_bytes,
            broken: is_broken(models_dir, e),
        }
    }
}

/// True when the entry's weight file can't be opened. `Path::exists` follows
/// symlinks, so a link whose target the user deleted reports as broken — which
/// is the outcome we want.
pub fn is_broken(models_dir: &Path, entry: &LoraEntry) -> bool {
    !weight_path(models_dir, &entry.name).exists()
}

/// Every pool entry, sorted by display name (case-insensitive) so the picker
/// order doesn't depend on install order.
pub fn list(models_dir: &Path) -> Vec<LoraInfo> {
    let mut out: Vec<LoraInfo> = load_index(models_dir)
        .loras
        .iter()
        .map(|e| LoraInfo::from_entry(models_dir, e))
        .collect();
    out.sort_by_key(|l| l.display_name.to_lowercase());
    out
}

/// Append `entry` to the index. The caller has already put the weight file in
/// the pool and picked a `name` through `unique_name`.
pub fn add(models_dir: &Path, entry: LoraEntry) -> Result<LoraInfo, String> {
    let mut index = load_index(models_dir);
    index.schema_version = INDEX_SCHEMA_VERSION;
    let info = LoraInfo::from_entry(models_dir, &entry);
    index.loras.push(entry);
    save_index(models_dir, &index).map_err(|e| e.to_string())?;
    Ok(info)
}

/// Drop the entry and delete its weight file. For a local registration that
/// file is a symlink, and `remove_file` removes the link, not the user's
/// original.
pub fn remove(models_dir: &Path, id: &str) -> Result<(), String> {
    let mut index = load_index(models_dir);
    let pos = index
        .loras
        .iter()
        .position(|e| e.id == id)
        .ok_or_else(|| format!("unknown LoRA {id}"))?;
    let removed = index.loras.remove(pos);
    index.schema_version = INDEX_SCHEMA_VERSION;
    save_index(models_dir, &index).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(weight_path(models_dir, &removed.name));
    Ok(())
}

/// Change the label and family. `name` — the engine tag, and therefore the key
/// every gallery item and the persisted `last_request` refer to — never moves.
pub fn rename(
    models_dir: &Path,
    id: &str,
    display_name: &str,
    family: &str,
) -> Result<LoraInfo, String> {
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err("Give the LoRA a name.".into());
    }
    let mut index = load_index(models_dir);
    let updated = {
        let e = index
            .loras
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("unknown LoRA {id}"))?;
        e.display_name = display_name.to_string();
        e.family = family.trim().to_string();
        e.clone()
    };
    index.schema_version = INDEX_SCHEMA_VERSION;
    save_index(models_dir, &index).map_err(|e| e.to_string())?;
    Ok(LoraInfo::from_entry(models_dir, &updated))
}

/// Validate a run's LoRA selection and return the pool directory to pass to the
/// engine, or `None` when nothing is selected.
///
/// Every selection is resolved against the index and its file checked before
/// the engine is spawned. This is not belt-and-braces: a LoRA the engine cannot
/// find is only a `[WARN] can not found lora` line followed by a successful
/// exit and a silently unmodified image, so a missing LoRA that reaches the
/// engine is a failure the user never sees.
pub fn resolve_selection(
    models_dir: &Path,
    selections: &[crate::types::LoraSelection],
) -> Result<Option<String>, String> {
    if selections.is_empty() {
        return Ok(None);
    }
    let index = load_index(models_dir);
    for s in selections {
        let entry = index.loras.iter().find(|e| e.name == s.name).ok_or_else(|| {
            format!(
                "The LoRA \"{}\" is no longer in your library. Remove it from the selection and try again.",
                s.name
            )
        })?;
        if is_broken(models_dir, entry) {
            return Err(format!(
                "The LoRA \"{}\" is missing its file. Re-add it, or remove it from the selection.",
                entry.display_name
            ));
        }
    }
    Ok(Some(pool_dir(models_dir).to_string_lossy().into_owned()))
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

    /// Create the pool with a real weight file for each named entry.
    fn pool_with(dir: &Path, names: &[&str]) -> LoraIndex {
        std::fs::create_dir_all(pool_dir(dir)).unwrap();
        let mut index = LoraIndex { schema_version: INDEX_SCHEMA_VERSION, loras: Vec::new() };
        for n in names {
            std::fs::write(weight_path(dir, n), b"weights").unwrap();
            index.loras.push(entry(n));
        }
        save_index(dir, &index).unwrap();
        index
    }

    #[test]
    fn list_sorts_by_display_name_case_insensitively() {
        let dir = tmp("list-sort");
        let mut index = pool_with(&dir, &["zebra", "apple", "Mango"]);
        index.loras[0].display_name = "zebra".into();
        index.loras[1].display_name = "apple".into();
        index.loras[2].display_name = "Mango".into();
        save_index(&dir, &index).unwrap();
        let names: Vec<String> = list(&dir).into_iter().map(|l| l.display_name).collect();
        assert_eq!(names, vec!["apple", "Mango", "zebra"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_weight_file_marks_the_entry_broken() {
        let dir = tmp("broken-missing");
        pool_with(&dir, &["present", "vanished"]);
        std::fs::remove_file(weight_path(&dir, "vanished")).unwrap();
        let got = list(&dir);
        assert!(!got.iter().find(|l| l.name == "present").unwrap().broken);
        assert!(got.iter().find(|l| l.name == "vanished").unwrap().broken);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn dead_symlink_marks_the_entry_broken() {
        // A local LoRA is a symlink into the user's own folder; if they move or
        // delete the original, the link survives but resolves to nothing.
        let dir = tmp("broken-symlink");
        std::fs::create_dir_all(pool_dir(&dir)).unwrap();
        let target = dir.join("elsewhere.safetensors");
        std::fs::write(&target, b"weights").unwrap();
        std::os::unix::fs::symlink(&target, weight_path(&dir, "linked")).unwrap();
        let index = LoraIndex { schema_version: INDEX_SCHEMA_VERSION, loras: vec![entry("linked")] };
        save_index(&dir, &index).unwrap();
        assert!(!list(&dir)[0].broken);
        std::fs::remove_file(&target).unwrap();
        assert!(list(&dir)[0].broken);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn add_persists_and_remove_deletes_the_weight_file() {
        let dir = tmp("add-remove");
        std::fs::create_dir_all(pool_dir(&dir)).unwrap();
        std::fs::write(weight_path(&dir, "film-grain"), b"weights").unwrap();
        let added = add(&dir, entry("film-grain")).unwrap();
        assert_eq!(added.name, "film-grain");
        assert_eq!(load_index(&dir).loras.len(), 1);

        remove(&dir, "lora-film-grain").unwrap();
        assert!(load_index(&dir).loras.is_empty());
        assert!(!weight_path(&dir, "film-grain").exists(), "the weight file is freed too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_of_an_unknown_id_errors() {
        let dir = tmp("remove-unknown");
        pool_with(&dir, &["a"]);
        assert!(remove(&dir, "lora-nope").is_err());
        assert_eq!(load_index(&dir).loras.len(), 1, "nothing is dropped on a bad id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_changes_the_label_and_family_but_never_the_pool_filename() {
        // `name` is the engine tag. Renaming the file would invalidate every
        // saved gallery item and the persisted last_request.
        let dir = tmp("rename");
        pool_with(&dir, &["film-grain"]);
        let updated = rename(&dir, "lora-film-grain", "Film Grain v2", "flux1").unwrap();
        assert_eq!(updated.display_name, "Film Grain v2");
        assert_eq!(updated.family, "flux1");
        assert_eq!(updated.name, "film-grain");
        assert!(weight_path(&dir, "film-grain").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_rejects_an_empty_display_name() {
        let dir = tmp("rename-empty");
        pool_with(&dir, &["film-grain"]);
        assert!(rename(&dir, "lora-film-grain", "   ", "sdxl").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn sel(name: &str) -> crate::types::LoraSelection {
        crate::types::LoraSelection { name: name.to_string(), weight: 1.0 }
    }

    #[test]
    fn no_selection_resolves_to_no_pool_directory() {
        let dir = tmp("resolve-none");
        assert_eq!(resolve_selection(&dir, &[]).unwrap(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_valid_selection_resolves_to_the_pool_directory() {
        let dir = tmp("resolve-ok");
        pool_with(&dir, &["film-grain"]);
        let got = resolve_selection(&dir, &[sel("film-grain")]).unwrap();
        assert_eq!(got, Some(pool_dir(&dir).to_string_lossy().into_owned()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_selection_naming_an_unknown_lora_is_rejected_by_name() {
        // The engine would only WARN and produce an unmodified image, so this
        // has to fail loudly here or the user never learns anything went wrong.
        let dir = tmp("resolve-unknown");
        pool_with(&dir, &["film-grain"]);
        let err = resolve_selection(&dir, &[sel("ghost")]).unwrap_err();
        assert!(err.contains("ghost"), "message must name the LoRA, got: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_selection_whose_file_vanished_is_rejected_by_label() {
        let dir = tmp("resolve-broken");
        let mut index = pool_with(&dir, &["film-grain"]);
        index.loras[0].display_name = "Film Grain v2".into();
        save_index(&dir, &index).unwrap();
        std::fs::remove_file(weight_path(&dir, "film-grain")).unwrap();
        let err = resolve_selection(&dir, &[sel("film-grain")]).unwrap_err();
        assert!(err.contains("Film Grain v2"), "message must name the LoRA, got: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
