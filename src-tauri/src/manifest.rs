use crate::types::GenDefaults;
use serde::{Deserialize, Serialize};

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
