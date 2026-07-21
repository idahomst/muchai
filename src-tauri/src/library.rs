use crate::manifest::{self, ModelManifest};
use crate::types::{missing_components, ModelRef};
use serde::Serialize;
use std::path::Path;

/// One row in the model list, resolved from a `model.json` manifest.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    /// Engine-ready reference (single-file or multi-file), all paths absolute.
    pub model: ModelRef,
    /// Engine flags copied from the manifest (so the editor can pre-load them).
    pub flags: crate::manifest::ManifestFlags,
    /// True when one or more SET component files are missing on disk.
    pub broken: bool,
}

/// Scan `models_dir/*/model.json` into library entries, sorted by name
/// (case-insensitive). Folders without a valid manifest are ignored
/// (manifest-only). A missing/unreadable `models_dir` yields an empty list.
pub fn scan_library(models_dir: &Path) -> Vec<LibraryEntry> {
    let mut out: Vec<LibraryEntry> = Vec::new();
    let entries = match std::fs::read_dir(models_dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let m = match manifest::load_from(&dir) {
            Ok(m) => m,
            Err(_) => continue, // no/invalid manifest → not a model
        };
        out.push(entry_from_manifest(&dir, &m));
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

/// Build one library entry from an already-loaded manifest + its folder.
/// The single-manifest half of `scan_library`, reused by the add/edit commands.
pub fn entry_from_manifest(model_dir: &Path, m: &ModelManifest) -> LibraryEntry {
    let components = m.to_components(model_dir);
    let broken = !missing_components(&components).is_empty();
    LibraryEntry {
        id: m.id.clone(),
        name: m.name.clone(),
        family: m.family.clone(),
        model: m.to_model_ref(model_dir),
        flags: m.flags.clone(),
        broken,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ManifestComponents, ManifestFlags, ManifestSource};

    fn write_manifest(models_dir: &Path, id: &str, diffusion_rel: &str, with_file: bool) {
        let dir = models_dir.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        if with_file {
            std::fs::write(dir.join(diffusion_rel), b"x").unwrap();
        }
        let m = ModelManifest {
            schema_version: 1,
            id: id.into(),
            name: format!("Model {id}"),
            family: "sd15".into(),
            source: ManifestSource::Url { url: "https://e/x.safetensors".into() },
            components: ManifestComponents { diffusion_model: diffusion_rel.into(), ..Default::default() },
            flags: ManifestFlags::default(),
            recommended_settings: None,
        };
        manifest::save_to(&dir, &m).unwrap();
    }

    #[test]
    fn scans_manifest_folders_and_sorts_by_name() {
        let root = std::env::temp_dir().join(format!("muchai-lib-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_manifest(&root, "b-model", "b.safetensors", true);
        write_manifest(&root, "a-model", "a.safetensors", true);
        let lib = scan_library(&root);
        let names: Vec<&str> = lib.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Model a-model", "Model b-model"]);
        assert!(lib.iter().all(|e| !e.broken));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn folder_without_manifest_is_ignored() {
        let root = std::env::temp_dir().join(format!("muchai-lib2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("old-layout")).unwrap();
        std::fs::write(root.join("old-layout/loose.safetensors"), b"x").unwrap();
        write_manifest(&root, "real", "r.safetensors", true);
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].id, "real");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_component_marks_entry_broken() {
        let root = std::env::temp_dir().join(format!("muchai-lib3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_manifest(&root, "gone", "missing.safetensors", false);
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert!(lib[0].broken, "entry with a missing diffusion file must be broken");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_manifest_is_skipped() {
        let root = std::env::temp_dir().join(format!("muchai-lib4-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bad")).unwrap();
        std::fs::write(root.join("bad/model.json"), b"{ not valid json ]").unwrap();
        write_manifest(&root, "good", "g.safetensors", true);
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].id, "good");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_pool_folder_is_not_a_model() {
        let root = std::env::temp_dir().join(format!("muchai-lib5-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("shared/flux1")).unwrap();
        std::fs::write(root.join("shared/flux1/ae.safetensors"), b"x").unwrap();
        write_manifest(&root, "real", "r.safetensors", true);
        let lib = scan_library(&root);
        assert_eq!(lib.len(), 1);
        assert_eq!(lib[0].id, "real");
        let _ = std::fs::remove_dir_all(&root);
    }
}
