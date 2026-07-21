use crate::types::{GenDefaults, ModelComponents, ModelRef};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILENAME: &str = "model.json";
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Where a model came from. Serialized with an internal `kind` tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ManifestSource {
    Catalog { catalog_id: String, url: String },
    Url { url: String },
    Local { original_path: String },
}

/// role → stored path. Relative to the model folder when the file lives inside
/// it; absolute when pooled (shared/) or referenced-in-place (local).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestComponents {
    pub diffusion_model: String,
    #[serde(default)]
    pub vae: Option<String>,
    #[serde(default)]
    pub clip_l: Option<String>,
    #[serde(default)]
    pub clip_g: Option<String>,
    #[serde(default)]
    pub t5xxl: Option<String>,
    #[serde(default)]
    pub llm: Option<String>,
}

/// Engine flags (not files).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFlags {
    #[serde(default)]
    pub vae_format: Option<String>,
    #[serde(default)]
    pub prediction: Option<String>,
}

/// The `model.json` document: the on-disk source of truth for one model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub family: String,
    pub source: ManifestSource,
    pub components: ManifestComponents,
    #[serde(default)]
    pub flags: ManifestFlags,
    #[serde(default)]
    pub recommended_settings: Option<GenDefaults>,
}

impl ManifestComponents {
    /// Store a component's path under the field for its role. `Diffusion`
    /// sets the required `String`; every other role sets its `Option<String>`.
    pub fn set_role(&mut self, role: crate::recipes::ComponentRole, stored: String) {
        use crate::recipes::ComponentRole::*;
        match role {
            Diffusion => self.diffusion_model = stored,
            Vae => self.vae = Some(stored),
            ClipL => self.clip_l = Some(stored),
            ClipG => self.clip_g = Some(stored),
            T5xxl => self.t5xxl = Some(stored),
            Llm => self.llm = Some(stored),
        }
    }
}

/// Resolve a stored component path to an absolute path. Relative paths resolve
/// against the model's own folder; absolute paths pass through unchanged.
pub fn resolve_path(model_dir: &Path, stored: &str) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        model_dir.join(p)
    }
}

/// Inverse of `resolve_path`: a path that lives directly under `model_dir`
/// becomes relative (just the tail); anything else stays absolute.
pub fn relativize(model_dir: &Path, abs: &str) -> String {
    match Path::new(abs).strip_prefix(model_dir) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => abs.to_string(),
    }
}

/// Write `<model_dir>/model.json` (pretty). Creates the folder if absent.
pub fn save_to(model_dir: &Path, manifest: &ModelManifest) -> std::io::Result<()> {
    std::fs::create_dir_all(model_dir)?;
    let s = serde_json::to_string_pretty(manifest).expect("manifest serializes");
    std::fs::write(model_dir.join(MANIFEST_FILENAME), s)
}

/// Read + parse `<model_dir>/model.json`. Errors on missing/invalid JSON.
pub fn load_from(model_dir: &Path) -> Result<ModelManifest, String> {
    let path = model_dir.join(MANIFEST_FILENAME);
    let s = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| e.to_string())
}

impl ModelManifest {
    /// True when the manifest has no companion files and no engine flags — i.e.
    /// a plain single checkpoint that the engine loads with `-m`.
    fn is_single_file(&self) -> bool {
        let c = &self.components;
        let no_companions = [&c.vae, &c.clip_l, &c.clip_g, &c.t5xxl, &c.llm]
            .into_iter()
            .all(|o| o.as_deref().map(|s| s.trim().is_empty()).unwrap_or(true));
        let no_flags = self.flags.vae_format.is_none() && self.flags.prediction.is_none();
        no_companions && no_flags
    }

    /// Resolve every set component path to absolute (relative → against
    /// `model_dir`) and carry the engine flags. Diffusion is always present.
    pub fn to_components(&self, model_dir: &Path) -> ModelComponents {
        let c = &self.components;
        let opt = |o: &Option<String>| {
            o.as_deref()
                .filter(|s| !s.trim().is_empty())
                .map(|s| resolve_path(model_dir, s).to_string_lossy().into_owned())
        };
        ModelComponents {
            diffusion_model: resolve_path(model_dir, &c.diffusion_model)
                .to_string_lossy()
                .into_owned(),
            vae: opt(&c.vae),
            clip_l: opt(&c.clip_l),
            clip_g: opt(&c.clip_g),
            t5xxl: opt(&c.t5xxl),
            llm: opt(&c.llm),
            vae_format: self.flags.vae_format.clone(),
            prediction: self.flags.prediction.clone(),
        }
    }

    /// The engine-ready reference. Single checkpoint → `SingleFile { -m path }`;
    /// any companion or flag → `MultiFile(components)`.
    pub fn to_model_ref(&self, model_dir: &Path) -> ModelRef {
        if self.is_single_file() {
            ModelRef::SingleFile {
                path: resolve_path(model_dir, &self.components.diffusion_model)
                    .to_string_lossy()
                    .into_owned(),
            }
        } else {
            ModelRef::MultiFile(self.to_components(model_dir))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ModelComponents, ModelRef};

    fn sample() -> ModelManifest {
        ModelManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            id: "flux1-schnell-def456".into(),
            name: "FLUX.1 schnell (Q4)".into(),
            family: "flux1".into(),
            source: ManifestSource::Catalog {
                catalog_id: "flux1-schnell-q4".into(),
                url: "https://example/flux1-schnell-Q4.gguf".into(),
            },
            components: ManifestComponents {
                diffusion_model: "flux1-schnell-Q4.gguf".into(),
                t5xxl: Some("/models/shared/flux1/t5xxl_fp16.safetensors".into()),
                clip_l: Some("/models/shared/flux1/clip_l.safetensors".into()),
                vae: Some("/models/shared/flux1/ae.safetensors".into()),
                ..Default::default()
            },
            flags: ManifestFlags::default(),
            recommended_settings: None,
        }
    }

    #[test]
    fn manifest_round_trips_through_json() {
        let m = sample();
        let json = serde_json::to_string(&m).unwrap();
        let back: ModelManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn source_tag_is_kind_snake_case() {
        let json = serde_json::to_string(&sample().source).unwrap();
        assert!(json.contains(r#""kind":"catalog""#), "got {json}");
    }

    #[test]
    fn optional_component_keys_default_to_none() {
        let json = r#"{"diffusion_model":"d.gguf"}"#;
        let c: ManifestComponents = serde_json::from_str(json).unwrap();
        assert_eq!(c.diffusion_model, "d.gguf");
        assert!(c.vae.is_none() && c.t5xxl.is_none() && c.clip_l.is_none());
    }

    #[test]
    fn missing_flags_and_recommended_default() {
        let json = r#"{
            "schema_version":1,"id":"x","name":"X","family":"sd15",
            "source":{"kind":"local","original_path":"/m/x.safetensors"},
            "components":{"diffusion_model":"/m/x.safetensors"}
        }"#;
        let m: ModelManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.flags, ManifestFlags::default());
        assert!(m.recommended_settings.is_none());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let json = r#"{
            "schema_version":1,"id":"x","name":"X","family":"sd15",
            "source":{"kind":"url","url":"https://e/x.safetensors"},
            "components":{"diffusion_model":"x.safetensors"},
            "tags":["anime"],"thumbnail":"t.png"
        }"#;
        let m: ModelManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.id, "x");
    }

    #[test]
    fn set_role_targets_the_right_field() {
        use crate::recipes::ComponentRole;
        let mut c = ManifestComponents::default();
        c.set_role(ComponentRole::Diffusion, "d.gguf".into());
        c.set_role(ComponentRole::T5xxl, "/pool/t5.safetensors".into());
        assert_eq!(c.diffusion_model, "d.gguf");
        assert_eq!(c.t5xxl.as_deref(), Some("/pool/t5.safetensors"));
        assert!(c.vae.is_none() && c.clip_l.is_none());
    }

    #[test]
    fn resolve_path_joins_relative_against_model_dir() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        assert_eq!(
            resolve_path(dir, "flux1-schnell-Q4.gguf"),
            std::path::PathBuf::from("/models/flux1-schnell-def456/flux1-schnell-Q4.gguf")
        );
    }

    #[test]
    fn resolve_path_passes_absolute_through() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        let abs = "/models/shared/flux1/t5xxl_fp16.safetensors";
        assert_eq!(resolve_path(dir, abs), std::path::PathBuf::from(abs));
    }

    #[test]
    fn to_model_ref_single_when_only_diffusion_and_no_flags() {
        let dir = std::path::Path::new("/models/my-sdxl");
        let m = ModelManifest {
            schema_version: 1,
            id: "my-sdxl".into(),
            name: "My SDXL".into(),
            family: "sdxl".into(),
            source: ManifestSource::Local { original_path: "/dl/sdxl.safetensors".into() },
            components: ManifestComponents {
                diffusion_model: "/dl/sdxl.safetensors".into(),
                ..Default::default()
            },
            flags: ManifestFlags::default(),
            recommended_settings: None,
        };
        assert_eq!(
            m.to_model_ref(dir),
            ModelRef::SingleFile { path: "/dl/sdxl.safetensors".into() }
        );
    }

    #[test]
    fn to_model_ref_multi_when_a_companion_is_set() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        let m = sample(); // has t5xxl/clip_l/vae companions
        match m.to_model_ref(dir) {
            ModelRef::MultiFile(c) => {
                assert_eq!(c.diffusion_model, "/models/flux1-schnell-def456/flux1-schnell-Q4.gguf");
                assert_eq!(c.t5xxl.as_deref(), Some("/models/shared/flux1/t5xxl_fp16.safetensors"));
            }
            other => panic!("expected MultiFile, got {other:?}"),
        }
    }

    #[test]
    fn to_model_ref_multi_when_only_flag_set() {
        let dir = std::path::Path::new("/models/sd3");
        let m = ModelManifest {
            schema_version: 1,
            id: "sd3".into(),
            name: "SD3".into(),
            family: "sd3".into(),
            source: ManifestSource::Url { url: "https://e/sd3.safetensors".into() },
            components: ManifestComponents { diffusion_model: "sd3.safetensors".into(), ..Default::default() },
            flags: ManifestFlags { vae_format: Some("sd3".into()), prediction: None },
            recommended_settings: None,
        };
        match m.to_model_ref(dir) {
            ModelRef::MultiFile(c) => assert_eq!(c.vae_format.as_deref(), Some("sd3")),
            other => panic!("expected MultiFile, got {other:?}"),
        }
    }

    #[test]
    fn to_components_resolves_all_paths_and_copies_flags() {
        let dir = std::path::Path::new("/models/flux1-schnell-def456");
        let c: ModelComponents = sample().to_components(dir);
        assert_eq!(c.diffusion_model, "/models/flux1-schnell-def456/flux1-schnell-Q4.gguf");
        assert_eq!(c.vae.as_deref(), Some("/models/shared/flux1/ae.safetensors"));
    }

    #[test]
    fn relativize_makes_in_folder_paths_relative() {
        let dir = std::path::Path::new("/models/abc");
        assert_eq!(relativize(dir, "/models/abc/flux1.gguf"), "flux1.gguf");
    }

    #[test]
    fn relativize_leaves_pooled_and_external_absolute() {
        let dir = std::path::Path::new("/models/abc");
        assert_eq!(
            relativize(dir, "/models/shared/flux1/ae.safetensors"),
            "/models/shared/flux1/ae.safetensors"
        );
        assert_eq!(relativize(dir, "/home/me/dl/x.safetensors"), "/home/me/dl/x.safetensors");
    }

    #[test]
    fn save_then_load_round_trips_on_disk() {
        let dir = std::env::temp_dir().join(format!("muchai-manifest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let m = sample();
        save_to(&dir, &m).unwrap();
        assert!(dir.join(MANIFEST_FILENAME).exists());
        let back = load_from(&dir).unwrap();
        assert_eq!(m, back);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
