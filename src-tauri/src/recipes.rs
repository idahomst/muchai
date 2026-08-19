use crate::types::{GenDefaults, Sampler};
use serde::{Deserialize, Serialize};

/// Every base family a model in the library can carry, for the LoRA family
/// dropdown. Deliberately not derived from `RECIPES`: `sd15` and `sdxl` have no
/// recipe (they are single-file models, family inferred from the filename), and
/// the `custom` recipe is not a base family. Keep in sync with
/// `family_defaults` below and with `catalog::validate`.
pub const FAMILIES: &[&str] = &[
    "sd15", "sdxl", "sd3", "flux1", "flux1-kontext", "flux2", "qwen-image", "qwen-image-edit",
    "z-image",
];

/// True when models of this family are instruction editors — read off the
/// family's own recipe. `sd15` and `sdxl` have no recipe and are not editors,
/// which is the correct answer for both.
pub fn is_edit_family(family: &str) -> bool {
    recipe_for(family).map(|r| r.edits_images).unwrap_or(false)
}

/// Every family whose models take a reference image. Derived from `recipes()`,
/// so adding an edit family is one edit, not two.
pub fn edit_families() -> Vec<String> {
    recipes()
        .into_iter()
        .filter(|r| r.edits_images)
        .map(|r| r.family.to_string())
        .collect()
}

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
    /// Vision tower (mmproj) of a vision-language text encoder — `--llm_vision`.
    /// Belongs to the encoder, not to the diffusion model, which is why it
    /// pools alongside the encoder rather than living in the model folder.
    LlmVision,
}

/// One role's recognition rule within a recipe.
#[derive(Debug, Clone)]
pub struct RoleSpec {
    pub role: ComponentRole,
    pub required: bool,
    /// Case-insensitive substring matches on the filename, e.g. ["t5xxl", "t5-xxl"].
    pub patterns: Vec<&'static str>,
    /// Case-insensitive substrings that disqualify a filename from this role
    /// even when a pattern matches. `detect` resolves each role independently,
    /// so without this one file can win two roles: `qwen_image_vae.safetensors`
    /// matches the Qwen diffusion pattern `"qwen_image"` (10 chars) far more
    /// strongly than the VAE pattern `"vae"` (3), and the mmproj matches the
    /// encoder's `"qwen2.5"` exactly as well as the encoder itself does. Both
    /// end with a component pointed at the wrong file and no missing-component
    /// warning, because every path exists.
    pub exclude: Vec<&'static str>,
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
    /// Directory under `models_dir/shared/` this family's shared components
    /// pool into. Equal to `family` for every family that owns its parts.
    /// `qwen-image-edit` sets `"qwen-image"`: it uses the identical Qwen2.5-VL
    /// encoder and VAE, and pooling under its own id would re-download 4.7 GB
    /// the user already has. Pinned by `only_the_edit_family_pools_somewhere_else`.
    pub pool_family: &'static str,
    pub name: &'static str,
    /// True when models of this family take a reference image and an
    /// instruction rather than a from-scratch prompt. On the recipe rather than
    /// in a side list because a family that cannot answer this question has not
    /// finished being defined.
    pub edits_images: bool,
    pub roles: Vec<RoleSpec>,
    pub vae_format: Option<&'static str>,
    pub prediction: Option<&'static str>,
    pub shared: Vec<SharedComponent>,
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
            if spec.exclude.iter().any(|x| lower.contains(&x.to_lowercase())) {
                continue;
            }
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
///
/// `max_by_key` returns the *last* maximum, so a tie is settled by recipe order
/// — which is not a meaningful ranking. Two recipes must therefore never be
/// able to tie on the same file set; that is enforced in the recipes, by
/// keeping each family's diffusion patterns narrow enough not to match another
/// family's files. See the comment on `qwen-image-edit`'s Diffusion role.
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
    RoleSpec { role, required, patterns: patterns.to_vec(), exclude: Vec::new() }
}

/// A role whose patterns are broad enough to reach a file that belongs to a
/// different slot. See `RoleSpec::exclude`.
fn role_not(
    role: ComponentRole,
    required: bool,
    patterns: &[&'static str],
    exclude: &[&'static str],
) -> RoleSpec {
    RoleSpec { role, required, patterns: patterns.to_vec(), exclude: exclude.to_vec() }
}

/// Built-in family recipes. `custom` is the manual-flow pseudo-family:
/// diffusion required, everything else optional, no patterns, no defaults.
pub fn recipes() -> Vec<ModelRecipe> {
    vec![
        ModelRecipe {
            family: "flux1",
            pool_family: "flux1",
            name: "FLUX.1 (dev / schnell / krea)",
            edits_images: false,
            roles: vec![
                // A Kontext checkpoint matches "flux1" just as well as a dev
                // checkpoint does, and would then tie with the flux1-kontext
                // recipe on required roles — a tie `detect_best` settles by
                // list position. Excluding it here decides the match on the
                // filename instead, wherever either recipe sits.
                role_not(ComponentRole::Diffusion, true, &["flux1", "flux-1", "flux"], &["kontext"]),
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
            family: "flux1-kontext",
            // Kontext dev is the FLUX.1 dev stack with a different transformer:
            // same T5-XXL, same CLIP-L, and the stock FLUX.1 autoencoder (the
            // GGUF repos ship no VAE of their own, which is why the pool's
            // `ae.safetensors` is the file it is meant to run with). Pooling
            // under flux1 reuses what a FLUX.1 install already has rather than
            // fetching it twice — the same reason qwen-image-edit pools under
            // qwen-image.
            pool_family: "flux1",
            name: "FLUX.1 Kontext (edits images)",
            edits_images: true,
            roles: vec![
                // Excluding the VAE is not optional: a Kontext VAE ships as
                // `flux1-kontext-dev-vae.safetensors`, which matches the
                // "flux1-kontext" pattern exactly as strongly as the diffusion
                // checkpoint does. `detect` keeps the first file at a given
                // score, so without this the winner is whichever `read_dir`
                // returned first — and the real checkpoint goes unassigned.
                role_not(
                    ComponentRole::Diffusion,
                    true,
                    &["flux1-kontext", "flux-kontext", "kontext"],
                    &["vae", "ae."],
                ),
                role(ComponentRole::T5xxl, true, &["t5xxl", "t5-xxl", "t5"]),
                role(ComponentRole::ClipL, true, &["clip_l", "clip-l"]),
                role(ComponentRole::Vae, true, &["ae.", "vae"]),
            ],
            vae_format: Some("flux"),
            prediction: Some("flux_flow"),
            shared: vec![
                // Byte-identical to the flux1 entries above and pooled to the
                // same paths, so an existing FLUX.1 install is reused rather
                // than re-fetched. Keep the two in sync.
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
            pool_family: "sd3",
            name: "Stable Diffusion 3 / 3.5",
            edits_images: false,
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
            pool_family: "qwen-image",
            name: "Qwen-Image",
            edits_images: false,
            roles: vec![
                // `"qwen_image"` matches `qwen_image_vae.safetensors` more
                // strongly than `"vae"` does, so without the exclusion the VAE
                // becomes the diffusion model on any folder that holds both.
                role_not(ComponentRole::Diffusion, true, &["qwen-image", "qwen_image", "qwen"], &["vae"]),
                // The mmproj shares the encoder's name up to a suffix. This
                // family has no vision slot, but the shared pool it reads from
                // holds one for qwen-image-edit.
                role_not(
                    ComponentRole::Llm,
                    true,
                    &["qwenvl", "qwen2.5", "qwen2_5", "qwen_2.5", "llm"],
                    &["mmproj", "llm_vision", "qwen2vl_vision"],
                ),
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
            family: "qwen-image-edit",
            // Same Qwen2.5-VL encoder, same VAE as Qwen-Image — pool with it.
            pool_family: "qwen-image",
            name: "Qwen-Image-Edit",
            edits_images: true,
            roles: vec![
                // Deliberately narrow: NOT "qwen-image"/"qwen". The plain
                // Qwen-Image family already claims those, and since
                // "qwen-image" is a prefix of "qwen-image-edit", a broad
                // pattern here makes an ordinary text-to-image Qwen file set
                // tie with — and then out-rank — its own family. Only a
                // filename that actually says "edit" is an edit model.
                role_not(
                    ComponentRole::Diffusion,
                    true,
                    &["qwen-image-edit", "qwen_image_edit", "qwen-edit", "qwen_edit"],
                    &["vae"],
                ),
                // Excluding the vision tower is what keeps `--llm` and
                // `--llm_vision` from being handed the same file: the mmproj
                // matches `"qwen2.5"` exactly as well as the encoder does, so
                // the winner would otherwise be whichever `read_dir` returned
                // first — and the real encoder would go unassigned.
                role_not(
                    ComponentRole::Llm,
                    true,
                    &["qwenvl", "qwen2.5", "qwen2_5", "qwen_2.5", "llm"],
                    &["mmproj", "llm_vision", "qwen2vl_vision"],
                ),
                // Pattern order is irrelevant — `detect` takes the *longest*
                // match, not the first. What matters is that the mmproj's own
                // filename says so: the plain `"vision"` catch-all is here for
                // repos that name the file that way, and would otherwise be all
                // this role has to go on.
                role(ComponentRole::LlmVision, true, &["mmproj", "llm_vision", "qwen2vl_vision", "vision"]),
                role(ComponentRole::Vae, true, &["vae", "ae."]),
            ],
            vae_format: Some("auto"),
            // No `flow_shift` here on purpose. leejet's docs pass
            // `--flow-shift 3`, but an A/B at a fixed seed on engine
            // `master-813-bfbef5b` (2026-08-10) produced visually
            // indistinguishable images with and without it — `auto` already
            // resolves to something equivalent for this family. Adding the
            // field would mean a new `ModelComponents` slot, a flag-gate entry
            // and a wider absent-component test, all to say nothing.
            prediction: None,
            shared: vec![
                SharedComponent {
                    // Byte-identical to the qwen-image entry above, and pooled
                    // to the same path, so an existing install is reused rather
                    // than re-fetched. Keep the two in sync.
                    role: ComponentRole::Llm,
                    url: "https://huggingface.co/mradermacher/Qwen2.5-VL-7B-Instruct-GGUF/resolve/main/Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf",
                    size_bytes: 4_457_767_936,
                    filename: "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf",
                },
                SharedComponent {
                    // The vision tower the plain qwen-image family has no use
                    // for. BF16 because mmproj is small and quantising the
                    // vision tower is where edit fidelity goes to die.
                    role: ComponentRole::LlmVision,
                    url: "https://huggingface.co/QuantStack/Qwen-Image-Edit-GGUF/resolve/main/mmproj/Qwen2.5-VL-7B-Instruct-mmproj-BF16.gguf",
                    size_bytes: 1_354_163_040,
                    filename: "Qwen2.5-VL-7B-Instruct-mmproj-BF16.gguf",
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
            pool_family: "flux2",
            name: "FLUX.2 (klein / dev)",
            edits_images: false,
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
            pool_family: "z-image",
            name: "Z-Image (Turbo)",
            edits_images: false,
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
            pool_family: "custom",
            name: "Custom (assign files manually)",
            edits_images: false,
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
        // The dev profile: Kontext is not distilled, and there is no schnell
        // variant to branch on. 1024×1024 is only the fallback — an edit run
        // takes its size from the reference image (see `imagedim::suggest_size`).
        "flux1-kontext" => Some(d(20, 1.0, Sampler::Euler, 1024, 1024)),
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
        // Same sampler/steps/CFG as the base family. 1024×1024 is only the
        // fallback: an edit run overrides width/height from the reference
        // image's aspect ratio (see `imagedim::suggest_size`).
        "qwen-image-edit" => Some(d(20, 2.5, Sampler::Euler, 1024, 1024)),
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
        let edit = family_defaults("qwen-image-edit", None).unwrap();
        assert_eq!((edit.steps, edit.cfg_scale), (20, 2.5));
        assert_eq!(edit.sampler, crate::types::Sampler::Euler);
        assert_eq!((edit.width, edit.height), (1024, 1024));
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

    /// The whole point of moving the flag onto the recipe: there is no second
    /// place to update, so the predicate and the recipes cannot disagree.
    #[test]
    fn edit_capability_is_read_from_the_recipe_not_a_side_list() {
        for r in recipes() {
            assert_eq!(
                is_edit_family(r.family),
                r.edits_images,
                "{} disagrees with its own recipe",
                r.family
            );
        }
        assert!(edit_families().contains(&"qwen-image-edit".to_string()));
    }

    #[test]
    fn the_edit_family_list_matches_the_predicate() {
        let listed = edit_families();
        assert!(!listed.is_empty(), "some family must be able to edit");
        for f in listed {
            assert!(is_edit_family(&f), "{f} is listed but the predicate rejects it");
            assert!(FAMILIES.contains(&f.as_str()), "{f} must also be a real family");
        }
    }

    /// A Kontext file set matches flux1's roles as well as flux1-kontext's, so a
    /// tie would be settled by recipe order — `detect_best` takes `max_by_key`,
    /// which yields the *last* maximum. flux1's diffusion role excludes
    /// "kontext" instead, so the decision is made by the filenames themselves
    /// and survives either recipe being moved.
    #[test]
    fn kontext_outranks_plain_flux_regardless_of_recipe_order() {
        let files: Vec<String> = [
            "flux1-kontext-dev-Q5_K_M.gguf",
            "t5xxl_fp8_e4m3fn.safetensors",
            "clip_l.safetensors",
            "ae.safetensors",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let (recipe, d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "flux1-kontext");
        assert_eq!(d.get(ComponentRole::Diffusion), Some("flux1-kontext-dev-Q5_K_M.gguf"));
        assert_eq!(d.required_matched(&recipe), 4);

        // flux1 must not merely lose the tie — it must fail to claim the file at
        // all, which is what makes the outcome order-independent.
        let plain = detect(&recipe_for("flux1").unwrap(), &files);
        assert_eq!(plain.get(ComponentRole::Diffusion), None);
    }

    /// The exclusion must not cost the base family its own files.
    #[test]
    fn a_plain_flux_set_still_detects_as_flux1() {
        let files: Vec<String> = [
            "flux1-dev-Q4_K_S.gguf",
            "t5xxl_fp8_e4m3fn.safetensors",
            "clip_l.safetensors",
            "ae.safetensors",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let (recipe, _d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "flux1");
    }

    /// The Kontext VAE ships under a name that also carries "kontext", so the
    /// diffusion role would otherwise claim it — whichever of the two the
    /// caller happened to list first wins a same-length pattern match.
    #[test]
    fn the_kontext_vae_does_not_win_the_diffusion_slot() {
        let files: Vec<String> = [
            "flux1-kontext-dev-vae.safetensors",
            "flux1-kontext-dev-Q5_K_M.gguf",
            "t5xxl_fp8_e4m3fn.safetensors",
            "clip_l.safetensors",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let d = detect(&recipe_for("flux1-kontext").unwrap(), &files);
        assert_eq!(d.get(ComponentRole::Diffusion), Some("flux1-kontext-dev-Q5_K_M.gguf"));
        assert_eq!(d.get(ComponentRole::Vae), Some("flux1-kontext-dev-vae.safetensors"));
    }

    #[test]
    fn kontext_pools_its_shared_parts_with_flux1() {
        let r = recipe_for("flux1-kontext").unwrap();
        assert_eq!(r.pool_family, "flux1");
        assert!(r.edits_images);
        let flux = recipe_for("flux1").unwrap();
        for c in &r.shared {
            let same = flux.shared.iter().find(|s| s.role == c.role).unwrap();
            assert_eq!(c.url, same.url, "{:?} must reuse the flux1 download", c.role);
            assert_eq!(c.filename, same.filename, "{:?} must pool to the same path", c.role);
        }
    }

    #[test]
    fn family_defaults_kontext_matches_flux1_dev() {
        let d = family_defaults("flux1-kontext", None).unwrap();
        assert_eq!(d.steps, 20);
        assert_eq!(d.cfg_scale, 1.0);
        assert_eq!(d.sampler, crate::types::Sampler::Euler);
    }

    #[test]
    fn only_the_edit_families_can_take_a_reference_image() {
        assert!(is_edit_family("qwen-image-edit"));
        assert!(is_edit_family("flux1-kontext"));
        for f in ["sd15", "sdxl", "sd3", "flux1", "flux2", "qwen-image", "z-image", "custom", ""] {
            assert!(!is_edit_family(f), "{f} is not an editing family");
        }
    }

    /// Pooling elsewhere is deliberate and rare: it means "this family's shared
    /// parts are byte-identical to another's". Anything not on this list pooling
    /// somewhere other than its own name is a typo, not a decision.
    #[test]
    fn only_the_derived_families_pool_somewhere_else() {
        for r in recipes() {
            let expected = match r.family {
                "qwen-image-edit" => "qwen-image",
                "flux1-kontext" => "flux1",
                f => f,
            };
            assert_eq!(
                r.pool_family, expected,
                "{} pools its shared components under the wrong directory",
                r.family
            );
        }
    }

    /// `qwen-image` is a prefix of `qwen-image-edit`, so the two recipes are one
    /// careless pattern away from claiming each other's files. A plain
    /// text-to-image Qwen set misdetected as an editor would demand an mmproj it
    /// has no use for; an edit set misdetected as plain Qwen would drop the
    /// vision tower and silently generate an unrelated picture. Pin both ways.
    #[test]
    fn qwen_edit_and_plain_qwen_do_not_claim_each_others_files() {
        let plain = [
            "qwen-image-2512-Q2_K.gguf".to_string(),
            "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf".to_string(),
            "qwen_image_vae.safetensors".to_string(),
        ];
        let (r, _) = detect_best(&plain).expect("a plain Qwen set is detectable");
        assert_eq!(r.family, "qwen-image", "no mmproj in sight — this is not an editor");

        let edit = [
            "qwen-image-edit-2511-Q3_K_S.gguf".to_string(),
            "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf".to_string(),
            "Qwen2.5-VL-7B-Instruct-mmproj-BF16.gguf".to_string(),
            "qwen_image_vae.safetensors".to_string(),
        ];
        let (r, d) = detect_best(&edit).expect("an edit set is detectable");
        assert_eq!(r.family, "qwen-image-edit");
        assert_eq!(
            d.get(ComponentRole::LlmVision),
            Some("Qwen2.5-VL-7B-Instruct-mmproj-BF16.gguf"),
            "the mmproj must land in the vision slot, not the encoder's"
        );
    }

    /// The mmproj matches the encoder's `"qwen2.5"` exactly as well as the
    /// encoder itself does, and `detect` keeps the first file on a tie — so
    /// before the exclusion this passed only because the encoder happened to be
    /// listed first. `read_dir` gives no such guarantee. List the mmproj first.
    #[test]
    fn the_vision_tower_never_takes_the_encoder_slot_whatever_the_file_order() {
        let edit = [
            "Qwen2.5-VL-7B-Instruct-mmproj-BF16.gguf".to_string(),
            "qwen-image-edit-2511-Q3_K_S.gguf".to_string(),
            "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf".to_string(),
            "qwen_image_vae.safetensors".to_string(),
        ];
        let (r, d) = detect_best(&edit).expect("an edit set is detectable");
        assert_eq!(r.family, "qwen-image-edit");
        assert_eq!(d.get(ComponentRole::Llm), Some("Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf"));
        assert_eq!(d.get(ComponentRole::LlmVision), Some("Qwen2.5-VL-7B-Instruct-mmproj-BF16.gguf"));
    }

    /// `qwen_image_vae.safetensors` matches the diffusion pattern `"qwen_image"`
    /// (10 characters) far more strongly than the VAE's `"vae"` (3), so the VAE
    /// used to win the diffusion slot outright — both flags pointed at a 0.25 GB
    /// VAE and nothing reported a missing component, because every path existed.
    #[test]
    fn the_vae_never_takes_the_diffusion_slot() {
        let plain = [
            "qwen_image_vae.safetensors".to_string(),
            "qwen-image-2512-Q2_K.gguf".to_string(),
            "Qwen2.5-VL-7B-Instruct.Q4_K_S.gguf".to_string(),
        ];
        let (r, d) = detect_best(&plain).expect("a plain Qwen set is detectable");
        assert_eq!(r.family, "qwen-image");
        assert_eq!(d.get(ComponentRole::Diffusion), Some("qwen-image-2512-Q2_K.gguf"));
        assert_eq!(d.get(ComponentRole::Vae), Some("qwen_image_vae.safetensors"));
    }

    /// A file can only be one component. `detect` resolves each role
    /// independently, so nothing structural stops two roles from landing on the
    /// same file — the generation then points two flags at one path while the
    /// real component for the losing role goes unassigned, and no
    /// missing-component warning fires because every path exists. Feed each
    /// recipe one plausible filename per pattern and assert the assignment stays
    /// one-to-one.
    #[test]
    fn no_recipe_gives_one_file_to_two_roles() {
        for r in recipes() {
            let files: Vec<String> = r
                .roles
                .iter()
                .flat_map(|s| s.patterns.iter())
                .map(|p| format!("{p}.gguf"))
                .collect();
            let d = detect(&r, &files);
            let mut taken: Vec<&str> = Vec::new();
            for (role, name) in &d.assignments {
                assert!(
                    !taken.contains(&name.as_str()),
                    "{}: {name} fills {role:?} and an earlier role at the same time",
                    r.family
                );
                taken.push(name);
            }
        }
    }
}
