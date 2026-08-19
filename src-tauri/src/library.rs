use crate::manifest::{self, ModelManifest};
use crate::types::{missing_components, GenerationRequest, ModelRef};
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
    /// Per-model recommended-settings override from the manifest (None = fall
    /// back to the family default). Exposed so the editor can pre-load it.
    pub recommended_settings: Option<crate::types::GenDefaults>,
    /// Per-model edit-capability override from the manifest (None = the family
    /// decides). The frontend needs it to mirror `resolve_ref_images` without
    /// re-reading model.json.
    pub edits_images: Option<bool>,
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
        recommended_settings: m.recommended_settings,
        edits_images: m.edits_images,
        broken,
    }
}

/// Resolve one library entry by its manifest `id`, re-reading from disk. This is
/// the single-source-of-truth lookup: `generate` calls it so an edited or stale
/// `model.json` is always re-read at the moment of use. `None` when no manifest
/// in `models_dir` has that id (deleted/renamed).
pub fn resolve_by_id(models_dir: &Path, id: &str) -> Option<LibraryEntry> {
    scan_library(models_dir).into_iter().find(|e| e.id == id)
}

/// The `ModelRef` to hand the engine for a request. Managed model (`model_id`
/// Some) → re-resolve components from `model.json` (single source of truth),
/// ignoring the possibly-stale `request.model` snapshot. Ad-hoc (`model_id`
/// None) → the literal `request.model`. Err when a named id no longer resolves.
pub fn resolve_request_model(models_dir: &Path, request: &GenerationRequest) -> Result<ModelRef, String> {
    match &request.model_id {
        Some(id) => resolve_by_id(models_dir, id)
            .map(|e| e.model)
            .ok_or_else(|| "Selected model is no longer in your library. Re-select a model.".to_string()),
        None => Ok(request.model.clone()),
    }
}

/// The family of the managed model this request targets, re-read from
/// `model.json` — the same single-source-of-truth rule `resolve_request_model`
/// follows, so a family edited between selection and generation is honoured.
///
/// `None` for an ad-hoc request (`model_id: None` — a manual single-file pick
/// or a replayed history item) and for a model that has since been deleted. A
/// separate function rather than a wider `resolve_request_model` return type:
/// every other caller wants only the `ModelRef`, and the two lookups are
/// cheap (one `model.json` read each) and independent.
pub fn resolve_request_family(models_dir: &Path, request: &GenerationRequest) -> Option<String> {
    let id = request.model_id.as_deref()?;
    resolve_by_id(models_dir, id).map(|e| e.family)
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
            edits_images: None,
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

    #[test]
    fn resolve_by_id_finds_the_matching_entry() {
        let root = std::env::temp_dir().join(format!("muchai-resolve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_manifest(&root, "wanted", "w.safetensors", true);
        write_manifest(&root, "other", "o.safetensors", true);
        let entry = resolve_by_id(&root, "wanted");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().id, "wanted");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_by_id_returns_none_for_unknown_id() {
        let root = std::env::temp_dir().join(format!("muchai-resolve2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        write_manifest(&root, "present", "p.safetensors", true);
        assert!(resolve_by_id(&root, "absent").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_request_model_overrides_a_stale_snapshot_with_the_manifest() {
        // THE ORIGINAL BUG: the client sends a stale `model` snapshot whose `llm`
        // points at the WRONG text-encoder, but `model_id` names a managed model.
        // resolve_request_model must return the manifest's components (correct
        // encoder), never the stale snapshot.
        use crate::manifest::{ManifestComponents, ManifestFlags, ManifestSource};
        use crate::types::{GenerationRequest, ModelComponents};

        let root = std::env::temp_dir().join(format!("muchai-req-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("qwen-image");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("diff.gguf"), b"x").unwrap();
        let m = ModelManifest {
            schema_version: 1,
            id: "qwen-image".into(),
            name: "Qwen-Image".into(),
            family: "qwen-image".into(),
            source: ManifestSource::Url { url: "https://e/x.gguf".into() },
            components: ManifestComponents {
                diffusion_model: "diff.gguf".into(),
                // The CORRECT encoder (absolute so it round-trips verbatim).
                llm: Some("/correct/Qwen2.5-VL-7B.gguf".into()),
                ..Default::default()
            },
            flags: ManifestFlags::default(),
            recommended_settings: None,
            edits_images: None,
        };
        manifest::save_to(&dir, &m).unwrap();

        let req = GenerationRequest {
            model_id: Some("qwen-image".into()),
            // Stale snapshot with the WRONG encoder — must be ignored.
            model: ModelRef::MultiFile(ModelComponents {
                diffusion_model: "/stale/diff.gguf".into(),
                llm: Some("/wrong/Qwen3-8B.gguf".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let resolved = resolve_request_model(&root, &req).unwrap();
        match resolved {
            ModelRef::MultiFile(c) => {
                assert_eq!(c.llm.as_deref(), Some("/correct/Qwen2.5-VL-7B.gguf"));
            }
            other => panic!("expected MultiFile, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_request_model_uses_literal_ref_when_no_model_id() {
        // Ad-hoc model (model_id None): the literal `model` is used even though
        // `models_dir` contains no such manifest.
        use crate::types::GenerationRequest;
        let root = std::env::temp_dir().join(format!("muchai-req2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let req = GenerationRequest {
            model: ModelRef::SingleFile { path: "/adhoc/model.safetensors".into() },
            model_id: None,
            ..Default::default()
        };
        let resolved = resolve_request_model(&root, &req).unwrap();
        assert_eq!(resolved, ModelRef::SingleFile { path: "/adhoc/model.safetensors".into() });
    }

    #[test]
    fn resolve_request_model_errors_when_id_is_gone() {
        use crate::types::GenerationRequest;
        let root = std::env::temp_dir().join(format!("muchai-req3-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let req = GenerationRequest {
            model_id: Some("deleted".into()),
            ..Default::default()
        };
        assert!(resolve_request_model(&root, &req).is_err());
    }

    #[test]
    fn a_managed_request_resolves_its_family_from_the_manifest() {
        let root = std::env::temp_dir().join(format!("muchai-req-family-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("model-a");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("diff.gguf"), b"x").unwrap();
        let m = ModelManifest {
            schema_version: 1,
            id: "model-a".into(),
            name: "Qwen Edit".into(),
            family: "qwen-image-edit".into(),
            source: ManifestSource::Local { original_path: "/x".into() },
            components: ManifestComponents { diffusion_model: "diff.gguf".into(), ..Default::default() },
            flags: ManifestFlags::default(),
            recommended_settings: None,
            edits_images: None,
        };
        manifest::save_to(&dir, &m).unwrap();

        let mut req = GenerationRequest { model_id: Some("model-a".into()), ..Default::default() };
        assert_eq!(resolve_request_family(&root, &req).as_deref(), Some("qwen-image-edit"));

        req.model_id = None;
        assert_eq!(
            resolve_request_family(&root, &req),
            None,
            "an ad-hoc model has no manifest and therefore no family"
        );

        req.model_id = Some("model-gone".into());
        assert_eq!(
            resolve_request_family(&root, &req),
            None,
            "a deleted model resolves to no family, not to a stale one"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
