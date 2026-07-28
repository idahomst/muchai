use crate::types::{GenDefaults, ModelComponents, Sampler};
use serde::{Deserialize, Serialize};

/// Every base family a model in the library can carry, for the LoRA family
/// dropdown. Deliberately not derived from `RECIPES`: `sd15` and `sdxl` have no
/// recipe (they are single-file models, family inferred from the filename), and
/// the `custom` recipe is not a base family. Keep in sync with
/// `family_defaults` below and with `catalog::validate`.
pub const FAMILIES: &[&str] =
    &["sd15", "sdxl", "sd3", "flux1", "flux2", "qwen-image", "z-image"];

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
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/t5xxl_fp8_e4m3fn.safetensors",
                    size_bytes: 4_893_934_904,
                    filename: "t5xxl_fp8_e4m3fn.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::ClipL,
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/clip_l.safetensors",
                    size_bytes: 246_144_152,
                    filename: "clip_l.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/camenduru/FLUX.1-dev-ungated/resolve/main/ae.safetensors",
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
            shared: vec![
                SharedComponent {
                    role: ComponentRole::ClipL,
                    url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5-fp8/resolve/main/text_encoders/clip_l.safetensors",
                    size_bytes: 246_144_152,
                    filename: "clip_l.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::ClipG,
                    url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5-fp8/resolve/main/text_encoders/clip_g.safetensors",
                    size_bytes: 1_389_382_176,
                    filename: "clip_g.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::T5xxl,
                    url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5-fp8/resolve/main/text_encoders/t5xxl_fp8_e4m3fn.safetensors",
                    size_bytes: 4_893_934_904,
                    filename: "t5xxl_fp8_e4m3fn.safetensors",
                },
                SharedComponent {
                    // Ungated mirror of the SD3.5 VAE. Byte-identical to
                    // stabilityai/stable-diffusion-3.5-large's vae, but that repo
                    // is license-gated (401 even with a valid token unless the
                    // account accepted its terms), so we use a public re-upload —
                    // same precedent as the flux1 camenduru ungated AE above.
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/Shio-Koube/SD-3.5-vae/resolve/main/diffusion_pytorch_model.safetensors",
                    size_bytes: 167_666_902,
                    filename: "sd3.5_vae.safetensors",
                },
            ],
        },
        ModelRecipe {
            family: "qwen-image",
            name: "Qwen-Image",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["qwen-image", "qwen_image", "qwen"]),
                role(ComponentRole::Llm, true, &["qwenvl", "qwen2.5", "qwen2_5", "qwen_2.5", "llm"]),
                role(ComponentRole::Vae, true, &["vae", "ae."]),
            ],
            vae_format: Some("auto"),
            prediction: None,
            shared: vec![
                SharedComponent {
                    // GGUF-quantized Qwen2.5-VL text encoder. The Comfy-Org
                    // fp8_scaled encoder (9.38GB on disk) expands to ~15.8GB in
                    // RAM and OOMs the 12GB card; this Q4_K_S GGUF stays ~5.6GB.
                    // leejet blesses QuantStack diffusion + a Qwen2.5-VL GGUF
                    // encoder as the working pair — the Comfy-Org fp8 pairing
                    // rendered a degenerate cyan latent on the pinned engine.
                    role: ComponentRole::Llm,
                    url: "https://huggingface.co/mradermacher/Qwen2.5-VL-7B-Instruct-GGUF/resolve/main/Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf",
                    size_bytes: 4_457_767_936,
                    filename: "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/Comfy-Org/Qwen-Image_ComfyUI/resolve/main/split_files/vae/qwen_image_vae.safetensors",
                    size_bytes: 253_806_246,
                    filename: "qwen_image_vae.safetensors",
                },
            ],
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
            // No prediction override: the engine identifies the checkpoint itself
            // ("Version: Flux.2 klein" → "running in Flux FLOW mode") and picks the
            // right denoiser. Forcing `sefi_flow` here used to abort every FLUX.2
            // generation with `GGML_ASSERT(ggml_can_repeat(b, a))` inside
            // `Flux::modulate` — measured on engine b290693 with both a 4B GGUF and
            // a 9B fp8 klein checkpoint, which generate cleanly once the override is
            // gone. `flux2_flow` is not a value the pinned binary accepts at all.
            prediction: None,
            shared: vec![
                SharedComponent {
                    role: ComponentRole::Llm,
                    url: "https://huggingface.co/unsloth/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf",
                    size_bytes: 5_027_784_512,
                    filename: "Qwen3-8B-Q4_K_M.gguf",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/Comfy-Org/flux2-dev/resolve/main/split_files/vae/flux2-vae.safetensors",
                    size_bytes: 336_213_556,
                    filename: "flux2-vae.safetensors",
                },
            ],
        },
        ModelRecipe {
            family: "z-image",
            name: "Z-Image (Turbo)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["z_image", "z-image", "zimage"]),
                role(ComponentRole::Llm, true, &["qwen3", "qwen", "llm"]),
                role(ComponentRole::Vae, true, &["ae.", "vae"]),
            ],
            vae_format: None,
            prediction: None,
            shared: vec![
                SharedComponent {
                    role: ComponentRole::Llm,
                    url: "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
                    size_bytes: 2_497_281_120,
                    filename: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/camenduru/FLUX.1-dev-ungated/resolve/main/ae.safetensors",
                    size_bytes: 335_304_388,
                    filename: "ae.safetensors",
                },
            ],
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

/// Recommended generation settings for a model family, or `None` for families
/// without a meaningful preset (`custom`, single-file "none", or any unknown id
/// → the UI hides the "Use recommended settings" button). For `flux1`, the
/// diffusion filename selects the schnell (4-step) vs. dev/krea (20-step)
/// profile; a missing filename assumes dev. Family keys match `detect_best`
/// family ids plus the single-file heuristics `"sdxl"` / `"sd15"`.
pub fn family_defaults(family: &str, diffusion_filename: Option<&str>) -> Option<GenDefaults> {
    let d = |steps, cfg_scale, sampler, width, height| GenDefaults {
        steps,
        cfg_scale,
        sampler,
        width,
        height,
    };
    match family {
        "flux1" => {
            let is_schnell = diffusion_filename
                .map(|f| f.to_lowercase().contains("schnell"))
                .unwrap_or(false);
            let steps = if is_schnell { 4 } else { 20 };
            Some(d(steps, 1.0, Sampler::Euler, 1024, 1024))
        }
        "flux2" => Some(d(4, 1.0, Sampler::Euler, 1024, 1024)),
        "sd3" => {
            // SD3.5 Large Turbo is timestep-distilled: it bakes guidance in, so
            // CFG must be ~1.0 and steps low. Applying the non-turbo default
            // (cfg 4.5) to the distilled model produces a solid/blue image.
            let is_turbo = diffusion_filename
                .map(|f| f.to_lowercase().contains("turbo"))
                .unwrap_or(false);
            if is_turbo {
                Some(d(4, 1.0, Sampler::Euler, 1024, 1024))
            } else {
                Some(d(28, 4.5, Sampler::Euler, 1024, 1024))
            }
        }
        "qwen-image" => Some(d(20, 2.5, Sampler::Euler, 1024, 1024)),
        "z-image" => Some(d(8, 1.0, Sampler::Euler, 1024, 1024)),
        "sdxl" => Some(d(28, 7.0, Sampler::EulerA, 1024, 1024)),
        "sd15" => Some(d(20, 7.0, Sampler::EulerA, 512, 512)),
        _ => None,
    }
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
            // vae_format / prediction within the engine's known sets. These
            // MUST mirror the pinned binary's `--help` (see fixtures/sd-help.txt),
            // not any newer upstream build. Accepting a value is not the same as
            // it being correct: the engine takes `sefi_flow` happily and then
            // aborts mid-graph on a FLUX.2 checkpoint, which is why flux2 sets
            // no prediction at all.
            const VAE: [&str; 5] = ["auto", "flux", "sd3", "flux2", "wan"];
            const PRED: [&str; 6] = ["eps", "v", "edm_v", "sd3_flow", "flux_flow", "sefi_flow"];
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
        // Deliberately unset: forcing `sefi_flow` aborted the engine in
        // `Flux::modulate`. Auto-detection picks the working denoiser.
        assert_eq!(r.prediction, None);
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

    #[test]
    fn family_defaults_flux1_schnell_uses_four_steps() {
        let d = family_defaults("flux1", Some("flux1-schnell-Q4_0.gguf")).unwrap();
        assert_eq!(d.steps, 4);
        assert_eq!(d.cfg_scale, 1.0);
        assert_eq!(d.sampler, crate::types::Sampler::Euler);
        assert_eq!((d.width, d.height), (1024, 1024));
    }

    #[test]
    fn family_defaults_flux1_dev_uses_twenty_steps() {
        let d = family_defaults("flux1", Some("flux1-dev.safetensors")).unwrap();
        assert_eq!(d.steps, 20);
        assert_eq!(d.cfg_scale, 1.0);
    }

    #[test]
    fn family_defaults_flux1_without_filename_defaults_to_dev() {
        // No filename to check for "schnell" → assume the dev/krea profile.
        let d = family_defaults("flux1", None).unwrap();
        assert_eq!(d.steps, 20);
    }

    #[test]
    fn family_defaults_sd3_turbo_uses_distilled_settings() {
        // SD3.5 Large Turbo is distilled: cfg 4.5 (the non-turbo default) makes
        // it emit a solid/blue image, so a "turbo" filename must drop to cfg 1.0.
        let turbo = family_defaults("sd3", Some("sd3.5_large_turbo-Q4_0.gguf")).unwrap();
        assert_eq!((turbo.steps, turbo.cfg_scale), (4, 1.0));
        let large = family_defaults("sd3", Some("sd3.5_large-Q5_0.gguf")).unwrap();
        assert_eq!((large.steps, large.cfg_scale), (28, 4.5));
    }

    #[test]
    fn family_defaults_cover_each_family() {
        let flux2 = family_defaults("flux2", None).unwrap();
        assert_eq!(flux2.steps, 4);
        assert_eq!(flux2.sampler, crate::types::Sampler::Euler);
        assert_eq!((flux2.width, flux2.height), (1024, 1024));
        let sd3 = family_defaults("sd3", None).unwrap();
        assert_eq!((sd3.steps, sd3.cfg_scale), (28, 4.5));
        assert_eq!(sd3.sampler, crate::types::Sampler::Euler);
        assert_eq!((sd3.width, sd3.height), (1024, 1024));
        let qwen = family_defaults("qwen-image", None).unwrap();
        assert_eq!((qwen.steps, qwen.cfg_scale), (20, 2.5));
        assert_eq!(qwen.sampler, crate::types::Sampler::Euler);
        assert_eq!((qwen.width, qwen.height), (1024, 1024));
        let sdxl = family_defaults("sdxl", None).unwrap();
        assert_eq!((sdxl.steps, sdxl.sampler, (sdxl.width, sdxl.height)),
                   (28, crate::types::Sampler::EulerA, (1024, 1024)));
        let sd15 = family_defaults("sd15", None).unwrap();
        assert_eq!((sd15.steps, sd15.sampler, (sd15.width, sd15.height)),
                   (20, crate::types::Sampler::EulerA, (512, 512)));
    }

    #[test]
    fn family_defaults_unknown_and_custom_are_none() {
        assert!(family_defaults("custom", None).is_none());
        assert!(family_defaults("totally-unknown", None).is_none());
    }

    #[test]
    fn flux1_pool_uses_fp8_t5_and_ungated_ae() {
        let r = recipe_for("flux1").unwrap();
        let t5 = r.shared.iter().find(|s| s.role == ComponentRole::T5xxl).unwrap();
        assert_eq!(t5.filename, "t5xxl_fp8_e4m3fn.safetensors");
        assert_eq!(t5.size_bytes, 4_893_934_904);
        let ae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert!(ae.url.contains("camenduru/FLUX.1-dev-ungated"), "AE must be the ungated mirror");
        assert_eq!(ae.size_bytes, 335_304_388);
    }

    #[test]
    fn sd3_pool_has_encoders_and_ungated_vae() {
        let r = recipe_for("sd3").unwrap();
        let roles: Vec<ComponentRole> = r.shared.iter().map(|s| s.role).collect();
        assert!(roles.contains(&ComponentRole::ClipL));
        assert!(roles.contains(&ComponentRole::ClipG));
        assert!(roles.contains(&ComponentRole::T5xxl));
        let vae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.filename, "sd3.5_vae.safetensors");
        // Must be an ungated source: the stabilityai repo is license-gated and
        // 401s even with a valid token. The mirror is byte-identical.
        assert!(!vae.url.contains("stabilityai"), "sd3 vae must not use the gated stabilityai repo");
        assert!(vae.url.starts_with("https://huggingface.co/"));
        assert_eq!(vae.size_bytes, 167_666_902);
        let cg = r.shared.iter().find(|s| s.role == ComponentRole::ClipG).unwrap();
        assert_eq!(cg.size_bytes, 1_389_382_176);
    }

    #[test]
    fn qwen_image_pool_has_llm_and_vae_and_detects_llm() {
        let r = recipe_for("qwen-image").unwrap();
        let llm = r.shared.iter().find(|s| s.role == ComponentRole::Llm).unwrap();
        // GGUF encoder, not the Comfy-Org fp8_scaled (which OOMs the 12GB card).
        assert_eq!(llm.filename, "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf");
        assert_eq!(llm.size_bytes, 4_457_767_936);
        assert!(r.shared.iter().any(|s| s.role == ComponentRole::Vae));
        let files = vec![
            "Qwen_Image-Q3_K_S.gguf".to_string(),
            "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf".to_string(),
            "qwen_image_vae.safetensors".to_string(),
        ];
        let d = detect(&r, &files);
        assert_eq!(d.get(ComponentRole::Llm), Some("Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf"));
        assert_eq!(d.get(ComponentRole::Diffusion), Some("Qwen_Image-Q3_K_S.gguf"));
    }

    #[test]
    fn flux2_pool_has_qwen3_llm_and_vae() {
        let r = recipe_for("flux2").unwrap();
        let llm = r.shared.iter().find(|s| s.role == ComponentRole::Llm).unwrap();
        assert_eq!(llm.filename, "Qwen3-8B-Q4_K_M.gguf");
        assert_eq!(llm.size_bytes, 5_027_784_512);
        let vae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.filename, "flux2-vae.safetensors");
        assert_eq!(vae.size_bytes, 336_213_556);
    }

    #[test]
    fn z_image_recipe_registered() {
        let r = recipe_for("z-image").expect("z-image recipe must exist");
        assert_eq!(r.vae_format, None);
        assert_eq!(r.prediction, None);
        let required: Vec<ComponentRole> =
            r.roles.iter().filter(|s| s.required).map(|s| s.role).collect();
        assert!(required.contains(&ComponentRole::Diffusion));
        assert!(required.contains(&ComponentRole::Llm));
        assert!(required.contains(&ComponentRole::Vae));
        let llm = r.shared.iter().find(|s| s.role == ComponentRole::Llm).unwrap();
        assert_eq!(llm.filename, "Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        assert_eq!(llm.size_bytes, 2_497_281_120);
        let vae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.size_bytes, 335_304_388);
    }

    #[test]
    fn detect_best_picks_z_image_for_z_image_files() {
        let files = vec![
            "z_image_turbo-Q4_K.gguf".to_string(),
            "Qwen3-4B-Instruct-2507-Q4_K_M.gguf".to_string(),
            "ae.safetensors".to_string(),
        ];
        let (recipe, _d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "z-image");
    }

    #[test]
    fn family_defaults_z_image_uses_eight_steps() {
        let d = family_defaults("z-image", None).unwrap();
        assert_eq!(d.steps, 8);
        assert_eq!(d.cfg_scale, 1.0);
        assert_eq!(d.sampler, crate::types::Sampler::Euler);
        assert_eq!((d.width, d.height), (1024, 1024));
    }

    #[test]
    fn every_recipe_family_except_custom_is_in_the_family_list() {
        for info in recipe_infos() {
            if info.family == "custom" {
                continue;
            }
            assert!(
                FAMILIES.contains(&info.family.as_str()),
                "recipe family {} is missing from FAMILIES",
                info.family
            );
        }
        // The two that have no recipe at all — the reason this list exists.
        assert!(FAMILIES.contains(&"sd15"));
        assert!(FAMILIES.contains(&"sdxl"));
    }
}
