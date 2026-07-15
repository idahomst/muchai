use crate::recipes::{ComponentRole, ModelRecipe, SharedComponent};
use crate::types::ModelComponents;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Sd15,
    Sdxl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Suitability {
    Recommended,
    Tight,
    TooBig,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub url: String,
    /// Approximate download size, for display only; the real size comes from
    /// the server's Content-Length at download time.
    pub size_bytes: u64,
    pub kind: ModelKind,
    pub min_vram_mb: u64,
    pub recommended_vram_mb: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RatedModel {
    #[serde(flatten)]
    pub model: CatalogModel,
    pub suitability: Suitability,
}

fn m(id: &str, name: &str, url: &str, size_bytes: u64, kind: ModelKind, min: u64, rec: u64) -> CatalogModel {
    CatalogModel {
        id: id.into(),
        name: name.into(),
        url: url.into(),
        size_bytes,
        kind,
        min_vram_mb: min,
        recommended_vram_mb: rec,
    }
}

/// The built-in single-file starter models. Public, free-to-download checkpoints.
pub fn starter_catalog() -> Vec<CatalogModel> {
    vec![
        m(
            "sd15",
            "Stable Diffusion 1.5",
            "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors",
            4_265_146_304,
            ModelKind::Sd15,
            2048,
            4096,
        ),
        m(
            "sdxl-base",
            "SDXL Base 1.0",
            "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors",
            6_938_040_706,
            ModelKind::Sdxl,
            6144,
            8192,
        ),
    ]
}

/// Rate a model against the first GPU's total VRAM (None = no GPU detected).
pub fn rate(model: &CatalogModel, vram_total_mb: Option<u64>) -> Suitability {
    match vram_total_mb {
        None => Suitability::Unknown,
        Some(v) if v >= model.recommended_vram_mb => Suitability::Recommended,
        Some(v) if v >= model.min_vram_mb => Suitability::Tight,
        Some(_) => Suitability::TooBig,
    }
}

/// The catalog rated against the given VRAM, ready to return to the UI.
pub fn rated_catalog(vram_total_mb: Option<u64>) -> Vec<RatedModel> {
    starter_catalog()
        .into_iter()
        .map(|model| {
            let suitability = rate(&model, vram_total_mb);
            RatedModel { model, suitability }
        })
        .collect()
}

/// A curated multi-file catalog entry — one downloadable split model.
#[derive(Debug, Clone, Serialize)]
pub struct MultiFileCatalogEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    pub diffusion_url: String,
    pub diffusion_size_bytes: u64,
    /// Roles this model ships its OWN copy of (downloaded into the model folder,
    /// not the shared pool). Usually empty.
    pub overrides: Vec<SharedComponent>,
    pub min_vram_mb: u64,
    pub recommended_vram_mb: u64,
}

/// Built-in curated multi-file models.
pub fn multi_file_catalog() -> Vec<MultiFileCatalogEntry> {
    vec![MultiFileCatalogEntry {
        id: "flux1-schnell".into(),
        name: "FLUX.1 schnell".into(),
        family: "flux1".into(),
        diffusion_url:
            "https://huggingface.co/black-forest-labs/FLUX.1-schnell/resolve/main/flux1-schnell.safetensors"
                .into(),
        diffusion_size_bytes: 23_782_506_688,
        overrides: vec![],
        min_vram_mb: 8192,
        recommended_vram_mb: 16384,
    }]
}

/// One file to fetch during multi-file download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedDownload {
    pub url: String,
    pub dest: PathBuf,
    pub role: ComponentRole,
}

/// Resolve an entry + its recipe into the exact files to download.
/// - Diffusion → always the entry's URL → `models_dir/<id>/<filename>`.
/// - Each family-shared role → into `models_dir/shared/<family>/<filename>`,
///   skipped when `exists(dest)` (already pooled). If the entry OVERRIDES the
///   role, download the override into the model folder instead.
pub fn plan_downloads(
    entry: &MultiFileCatalogEntry,
    recipe: &ModelRecipe,
    models_dir: &Path,
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<PlannedDownload> {
    let model_dir = models_dir.join(&entry.id);
    let shared_dir = models_dir.join("shared").join(&entry.family);
    let mut plan = Vec::new();

    // Diffusion: always fetched into the per-model folder.
    let diff_name = crate::downloader::derive_filename(None, &entry.diffusion_url);
    plan.push(PlannedDownload {
        url: entry.diffusion_url.clone(),
        dest: model_dir.join(diff_name),
        role: ComponentRole::Diffusion,
    });

    // NOTE: the shared-path joins below MUST stay in lockstep with
    // `assemble_components` (same model_dir/shared_dir + override resolution).
    for shared in &recipe.shared {
        if shared.role == ComponentRole::Diffusion {
            // Diffusion is always the per-model file fetched above; never
            // double-fetch it even if a recipe lists it under `shared`.
            continue;
        }
        if let Some(ov) = entry.overrides.iter().find(|o| o.role == shared.role) {
            // Model ships its own copy → per-model folder, always fetched.
            plan.push(PlannedDownload {
                url: ov.url.to_string(),
                dest: model_dir.join(ov.filename),
                role: ov.role,
            });
        } else {
            let dest = shared_dir.join(shared.filename);
            if !exists(&dest) {
                plan.push(PlannedDownload {
                    url: shared.url.to_string(),
                    dest,
                    role: shared.role,
                });
            }
        }
    }
    plan
}

/// Final absolute component paths for a fully-downloaded entry (independent of
/// which files actually needed fetching), plus the recipe's format defaults.
///
/// INVARIANT: the per-file destinations here MUST match `plan_downloads` above
/// (same `model_dir`/`shared_dir` joins + `derive_filename(None, diffusion_url)`).
/// If you change path derivation in one, change it in the other — otherwise a
/// model can download successfully yet its saved definition points at wrong paths.
pub fn assemble_components(
    entry: &MultiFileCatalogEntry,
    recipe: &ModelRecipe,
    models_dir: &Path,
) -> ModelComponents {
    let model_dir = models_dir.join(&entry.id);
    let shared_dir = models_dir.join("shared").join(&entry.family);
    let diff_name = crate::downloader::derive_filename(None, &entry.diffusion_url);

    let mut c = ModelComponents {
        diffusion_model: model_dir.join(diff_name).to_string_lossy().into_owned(),
        vae_format: recipe.vae_format.map(|s| s.to_string()),
        prediction: recipe.prediction.map(|s| s.to_string()),
        ..Default::default()
    };
    for shared in &recipe.shared {
        let path = if let Some(ov) = entry.overrides.iter().find(|o| o.role == shared.role) {
            model_dir.join(ov.filename)
        } else {
            shared_dir.join(shared.filename)
        }
        .to_string_lossy()
        .into_owned();
        match shared.role {
            ComponentRole::Vae => c.vae = Some(path),
            ComponentRole::ClipL => c.clip_l = Some(path),
            ComponentRole::ClipG => c.clip_g = Some(path),
            ComponentRole::T5xxl => c.t5xxl = Some(path),
            ComponentRole::Llm => c.llm = Some(path),
            ComponentRole::Diffusion => {}
        }
    }
    c
}

#[derive(Debug, Clone, Serialize)]
pub struct RatedMultiFile {
    #[serde(flatten)]
    pub entry: MultiFileCatalogEntry,
    pub suitability: Suitability,
}

/// The multi-file catalog rated against the given VRAM.
pub fn rated_multi_file_catalog(vram_total_mb: Option<u64>) -> Vec<RatedMultiFile> {
    multi_file_catalog()
        .into_iter()
        .map(|entry| {
            let suitability = match vram_total_mb {
                None => Suitability::Unknown,
                Some(v) if v >= entry.recommended_vram_mb => Suitability::Recommended,
                Some(v) if v >= entry.min_vram_mb => Suitability::Tight,
                Some(_) => Suitability::TooBig,
            };
            RatedMultiFile { entry, suitability }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn catalog_is_non_empty_and_well_formed() {
        let c = starter_catalog();
        assert!(!c.is_empty());
        for m in &c {
            assert!(m.url.starts_with("https://"), "{} must be https", m.id);
            assert!(m.recommended_vram_mb >= m.min_vram_mb);
        }
    }

    #[test]
    fn rate_handles_all_branches() {
        let sdxl = starter_catalog()
            .into_iter()
            .find(|m| matches!(m.kind, ModelKind::Sdxl))
            .unwrap();
        assert_eq!(rate(&sdxl, None), Suitability::Unknown);
        assert_eq!(rate(&sdxl, Some(sdxl.recommended_vram_mb)), Suitability::Recommended);
        assert_eq!(rate(&sdxl, Some(sdxl.recommended_vram_mb + 4096)), Suitability::Recommended);
        assert_eq!(rate(&sdxl, Some(sdxl.min_vram_mb)), Suitability::Tight);
        assert_eq!(rate(&sdxl, Some(sdxl.min_vram_mb - 1)), Suitability::TooBig);
    }

    #[test]
    fn plan_includes_diffusion_and_all_shared_when_pool_empty() {
        let entry = &multi_file_catalog()[0];
        let recipe = crate::recipes::recipe_for("flux1").unwrap();
        let root = Path::new("/models");
        let plan = plan_downloads(entry, &recipe, root, &|_| false);
        // diffusion + 3 shared (t5xxl, clip_l, vae)
        assert_eq!(plan.len(), 4);
        let diff = plan.iter().find(|p| p.role == ComponentRole::Diffusion).unwrap();
        assert_eq!(diff.dest, root.join("flux1-schnell").join("flux1-schnell.safetensors"));
        let t5 = plan.iter().find(|p| p.role == ComponentRole::T5xxl).unwrap();
        assert_eq!(t5.dest, root.join("shared").join("flux1").join("t5xxl_fp16.safetensors"));
    }

    #[test]
    fn plan_skips_shared_files_already_in_pool() {
        let entry = &multi_file_catalog()[0];
        let recipe = crate::recipes::recipe_for("flux1").unwrap();
        let root = Path::new("/models");
        // Pretend everything under shared/ already exists → only diffusion planned.
        let plan = plan_downloads(entry, &recipe, root, &|p| p.starts_with(root.join("shared")));
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].role, ComponentRole::Diffusion);
    }

    #[test]
    fn plan_puts_overrides_in_model_folder() {
        let mut entry = multi_file_catalog()[0].clone();
        entry.overrides = vec![SharedComponent {
            role: ComponentRole::Vae,
            url: "https://example/ae-custom.safetensors",
            size_bytes: 1,
            filename: "ae-custom.safetensors",
        }];
        let recipe = crate::recipes::recipe_for("flux1").unwrap();
        let root = Path::new("/models");
        let plan = plan_downloads(&entry, &recipe, root, &|_| false);
        let vae = plan.iter().find(|p| p.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.dest, root.join("flux1-schnell").join("ae-custom.safetensors"));
    }
}
