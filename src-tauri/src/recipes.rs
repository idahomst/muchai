use crate::types::ModelComponents;
use serde::{Deserialize, Serialize};

/// A typed slot in a split model, each wired to one engine flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentRole {
    Diffusion,
    Vae,
    ClipL,
    ClipG,
    T5xxl,
    Llm,
}

/// One role's recognition rule within a recipe.
#[derive(Debug, Clone)]
pub struct RoleSpec {
    pub role: ComponentRole,
    pub required: bool,
    /// Case-insensitive substring matches on the filename, e.g. ["t5xxl", "t5-xxl"].
    pub patterns: Vec<&'static str>,
}

/// A family-common downloadable part (VAE / encoder) reused across the family.
#[derive(Debug, Clone, Serialize)]
pub struct SharedComponent {
    pub role: ComponentRole,
    pub url: &'static str,
    pub size_bytes: u64,
    /// Stable filename in the shared pool (never unique-suffixed).
    pub filename: &'static str,
}

/// One model family and how to recognize/assemble its parts.
#[derive(Debug, Clone)]
pub struct ModelRecipe {
    pub family: &'static str,
    pub name: &'static str,
    pub roles: Vec<RoleSpec>,
    pub vae_format: Option<&'static str>,
    pub prediction: Option<&'static str>,
    pub shared: Vec<SharedComponent>,
}

impl ModelRecipe {
    /// Required roles whose slot is empty (None / blank). Gates Save + generation.
    pub fn missing_required_roles(&self, c: &ModelComponents) -> Vec<ComponentRole> {
        self.roles
            .iter()
            .filter(|r| r.required)
            .filter(|r| slot(c, r.role).map(|s| s.trim().is_empty()).unwrap_or(true))
            .map(|r| r.role)
            .collect()
    }
}

/// Read the component slot for a role. Diffusion is always present (String).
fn slot(c: &ModelComponents, role: ComponentRole) -> Option<&str> {
    match role {
        ComponentRole::Diffusion => Some(c.diffusion_model.as_str()),
        ComponentRole::Vae => c.vae.as_deref(),
        ComponentRole::ClipL => c.clip_l.as_deref(),
        ComponentRole::ClipG => c.clip_g.as_deref(),
        ComponentRole::T5xxl => c.t5xxl.as_deref(),
        ComponentRole::Llm => c.llm.as_deref(),
    }
}

/// Result of running detection: at most one matched filename per role.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DetectedComponents {
    pub assignments: Vec<(ComponentRole, String)>,
}

impl DetectedComponents {
    pub fn get(&self, role: ComponentRole) -> Option<&str> {
        self.assignments.iter().find(|(r, _)| *r == role).map(|(_, f)| f.as_str())
    }
    /// How many of the recipe's REQUIRED roles were matched (detection confidence).
    pub fn required_matched(&self, recipe: &ModelRecipe) -> usize {
        recipe
            .roles
            .iter()
            .filter(|r| r.required)
            .filter(|r| self.get(r.role).is_some())
            .count()
    }
}

/// Match filenames to this recipe's roles. Pure: no filesystem access.
/// For each role, among files matching any of its patterns (case-insensitive),
/// pick the file whose longest-matching pattern is longest (most specific).
pub fn detect(recipe: &ModelRecipe, filenames: &[String]) -> DetectedComponents {
    let mut assignments = Vec::new();
    for spec in &recipe.roles {
        let mut best: Option<(usize, &str)> = None; // (pattern length, filename)
        for name in filenames {
            let lower = name.to_lowercase();
            let score = spec
                .patterns
                .iter()
                .filter(|p| lower.contains(&p.to_lowercase()))
                .map(|p| p.len())
                .max();
            if let Some(s) = score {
                if best.map(|(bs, _)| s > bs).unwrap_or(true) {
                    best = Some((s, name.as_str()));
                }
            }
        }
        if let Some((_, name)) = best {
            assignments.push((spec.role, name.to_string()));
        }
    }
    DetectedComponents { assignments }
}

/// Pick the family that best explains this file set: most required roles matched,
/// then most total roles matched. None if no recipe matches any required role.
pub fn detect_best(filenames: &[String]) -> Option<(ModelRecipe, DetectedComponents)> {
    recipes()
        .into_iter()
        .filter(|r| r.family != "custom")
        .map(|r| {
            let d = detect(&r, filenames);
            (r, d)
        })
        .filter(|(r, d)| d.required_matched(r) > 0)
        .max_by_key(|(r, d)| (d.required_matched(r), d.assignments.len()))
}

fn role(role: ComponentRole, required: bool, patterns: &[&'static str]) -> RoleSpec {
    RoleSpec { role, required, patterns: patterns.to_vec() }
}

/// Built-in family recipes. `custom` is the manual-flow pseudo-family:
/// diffusion required, everything else optional, no patterns, no defaults.
pub fn recipes() -> Vec<ModelRecipe> {
    vec![
        ModelRecipe {
            family: "flux1",
            name: "FLUX.1 (dev / schnell / krea)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["flux1", "flux-1", "flux"]),
                role(ComponentRole::T5xxl, true, &["t5xxl", "t5-xxl", "t5"]),
                role(ComponentRole::ClipL, true, &["clip_l", "clip-l"]),
                role(ComponentRole::Vae, true, &["ae.", "vae"]),
            ],
            vae_format: Some("flux"),
            prediction: Some("flux_flow"),
            shared: vec![
                SharedComponent {
                    role: ComponentRole::T5xxl,
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/t5xxl_fp16.safetensors",
                    size_bytes: 9_787_841_024,
                    filename: "t5xxl_fp16.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::ClipL,
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/clip_l.safetensors",
                    size_bytes: 246_144_152,
                    filename: "clip_l.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/black-forest-labs/FLUX.1-schnell/resolve/main/ae.safetensors",
                    size_bytes: 335_304_388,
                    filename: "ae.safetensors",
                },
            ],
        },
        ModelRecipe {
            family: "sd3",
            name: "Stable Diffusion 3 / 3.5",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["sd3", "sd_3", "stable-diffusion-3"]),
                role(ComponentRole::ClipL, true, &["clip_l", "clip-l"]),
                role(ComponentRole::ClipG, true, &["clip_g", "clip-g"]),
                role(ComponentRole::T5xxl, false, &["t5xxl", "t5-xxl", "t5"]),
                role(ComponentRole::Vae, false, &["vae", "ae."]),
            ],
            vae_format: Some("sd3"),
            prediction: Some("sd3_flow"),
            shared: vec![],
        },
        ModelRecipe {
            family: "qwen-image",
            name: "Qwen-Image",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["qwen-image", "qwen_image", "qwen"]),
                role(ComponentRole::Llm, true, &["qwenvl", "qwen2.5", "qwen2_5", "llm"]),
                role(ComponentRole::Vae, true, &["vae", "ae."]),
            ],
            vae_format: Some("auto"),
            prediction: None,
            shared: vec![],
        },
        ModelRecipe {
            family: "flux2",
            name: "FLUX.2 (klein / dev)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["flux2", "flux-2", "flux.2"]),
                role(ComponentRole::Llm, true, &["qwen3", "qwen", "llm"]),
                role(ComponentRole::Vae, true, &["vae", "ae."]),
            ],
            vae_format: Some("flux2"),
            prediction: Some("flux2_flow"),
            shared: vec![],
        },
        ModelRecipe {
            family: "custom",
            name: "Custom (assign files manually)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &[]),
                role(ComponentRole::Vae, false, &[]),
                role(ComponentRole::ClipL, false, &[]),
                role(ComponentRole::ClipG, false, &[]),
                role(ComponentRole::T5xxl, false, &[]),
                role(ComponentRole::Llm, false, &[]),
            ],
            vae_format: None,
            prediction: None,
            shared: vec![],
        },
    ]
}

/// Look up a recipe by family id.
pub fn recipe_for(family: &str) -> Option<ModelRecipe> {
    recipes().into_iter().find(|r| r.family == family)
}

/// A recipe flattened for the frontend: roles with labels + defaulted flags.
#[derive(Debug, Clone, Serialize)]
pub struct RoleInfo {
    pub role: ComponentRole,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecipeInfo {
    pub family: String,
    pub name: String,
    pub roles: Vec<RoleInfo>,
    pub vae_format: Option<String>,
    pub prediction: Option<String>,
}

/// All recipes as frontend DTOs (drives the family picker + role slots).
pub fn recipe_infos() -> Vec<RecipeInfo> {
    recipes()
        .into_iter()
        .map(|r| RecipeInfo {
            family: r.family.to_string(),
            name: r.name.to_string(),
            roles: r.roles.iter().map(|s| RoleInfo { role: s.role, required: s.required }).collect(),
            vae_format: r.vae_format.map(|s| s.to_string()),
            prediction: r.prediction.map(|s| s.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flux() -> ModelRecipe {
        recipe_for("flux1").unwrap()
    }

    #[test]
    fn detect_matches_canonical_flux_set() {
        let files = vec![
            "flux1-schnell.safetensors".to_string(),
            "t5xxl_fp16.safetensors".to_string(),
            "clip_l.safetensors".to_string(),
            "ae.safetensors".to_string(),
        ];
        let d = detect(&flux(), &files);
        assert_eq!(d.get(ComponentRole::Diffusion), Some("flux1-schnell.safetensors"));
        assert_eq!(d.get(ComponentRole::T5xxl), Some("t5xxl_fp16.safetensors"));
        assert_eq!(d.get(ComponentRole::ClipL), Some("clip_l.safetensors"));
        assert_eq!(d.get(ComponentRole::Vae), Some("ae.safetensors"));
        assert_eq!(d.required_matched(&flux()), 4);
    }

    #[test]
    fn detect_leaves_missing_required_unmatched() {
        let files = vec!["flux1-dev.safetensors".to_string(), "clip_l.safetensors".to_string()];
        let d = detect(&flux(), &files);
        assert_eq!(d.get(ComponentRole::T5xxl), None);
        assert_eq!(d.get(ComponentRole::Vae), None);
        assert_eq!(d.required_matched(&flux()), 2);
    }

    #[test]
    fn detect_ignores_junk_filenames() {
        let files = vec!["notes.txt".to_string(), "random.bin".to_string()];
        let d = detect(&flux(), &files);
        assert!(d.assignments.is_empty());
    }

    #[test]
    fn detect_best_picks_flux_for_flux_files() {
        let files = vec![
            "flux1-schnell.safetensors".to_string(),
            "t5xxl_fp16.safetensors".to_string(),
            "clip_l.safetensors".to_string(),
            "ae.safetensors".to_string(),
        ];
        let (recipe, _d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "flux1");
    }

    #[test]
    fn recipe_table_integrity() {
        let all = recipes();
        // Family ids unique.
        let mut ids: Vec<&str> = all.iter().map(|r| r.family).collect();
        ids.sort();
        let mut deduped = ids.clone();
        deduped.dedup();
        assert_eq!(ids, deduped, "family ids must be unique");
        // Every required role in a non-custom recipe has >=1 pattern.
        for r in &all {
            if r.family == "custom" {
                continue;
            }
            for spec in &r.roles {
                if spec.required {
                    assert!(!spec.patterns.is_empty(), "{} {:?} needs a pattern", r.family, spec.role);
                }
            }
            // vae_format / prediction within the engine's known sets.
            const VAE: [&str; 4] = ["auto", "flux", "sd3", "flux2"];
            const PRED: [&str; 6] = ["eps", "v", "edm_v", "sd3_flow", "flux_flow", "flux2_flow"];
            if let Some(v) = r.vae_format {
                assert!(VAE.contains(&v), "{} bad vae_format {v}", r.family);
            }
            if let Some(p) = r.prediction {
                assert!(PRED.contains(&p), "{} bad prediction {p}", r.family);
            }
        }
    }

    #[test]
    fn flux2_recipe_is_registered_with_expected_shape() {
        let r = recipe_for("flux2").expect("flux2 recipe must exist");
        assert_eq!(r.vae_format, Some("flux2"));
        assert_eq!(r.prediction, Some("flux2_flow"));
        // Required roles: diffusion + llm + vae; no t5xxl/clip.
        let required: Vec<ComponentRole> =
            r.roles.iter().filter(|s| s.required).map(|s| s.role).collect();
        assert!(required.contains(&ComponentRole::Diffusion));
        assert!(required.contains(&ComponentRole::Llm));
        assert!(required.contains(&ComponentRole::Vae));
        assert!(!required.contains(&ComponentRole::T5xxl));
        assert!(!required.contains(&ComponentRole::ClipL));
    }

    #[test]
    fn detect_best_picks_flux2_for_flux2_file_set() {
        let files = vec![
            "flux2-klein.safetensors".to_string(),
            "qwen3-8b.safetensors".to_string(),
            "flux2-vae.safetensors".to_string(),
        ];
        let (recipe, _d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "flux2");
    }

    #[test]
    fn missing_required_roles_reports_empty_slots() {
        let c = ModelComponents {
            diffusion_model: "/m/flux1-dev.safetensors".into(),
            clip_l: Some("/m/clip_l.safetensors".into()),
            ..Default::default()
        };
        let missing = flux().missing_required_roles(&c);
        assert!(missing.contains(&ComponentRole::T5xxl));
        assert!(missing.contains(&ComponentRole::Vae));
        assert!(!missing.contains(&ComponentRole::Diffusion));
        assert!(!missing.contains(&ComponentRole::ClipL));
    }
}
