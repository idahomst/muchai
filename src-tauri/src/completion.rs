//! What a model still needs before it can run, and where those parts would come
//! from. Pure except for one question — is a file that fills this role already
//! in the shared pool — which is what makes the answer worth showing before
//! anything downloads.

use crate::manifest::ManifestComponents;
use crate::recipes::{self, ComponentRole};
use serde::Serialize;
use std::path::Path;

/// One required role a model has not filled, and how it could be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompletionRow {
    pub role: ComponentRole,
    /// Filename in the shared pool this role would be filled from — the file
    /// already there when `have`, otherwise the one that would be downloaded.
    /// `None` when the recipe declares no download for it (in practice, the
    /// diffusion model itself), which is also when `fillable` is false.
    pub filename: Option<String>,
    /// A file that fills this role is already in the pool: no download.
    pub have: bool,
    /// Declared size of that download, `0` when the role cannot be filled.
    pub size_bytes: u64,
    /// False when nothing can supply this role automatically — the user picks
    /// the file in the model editor. Reported rather than guessed at.
    pub fillable: bool,
}

/// Every required role `components` is missing for `family`, with its source.
/// Empty when the family has no recipe or nothing is missing.
pub fn plan_completion(
    family: &str,
    components: &ManifestComponents,
    models_dir: &Path,
) -> Vec<CompletionRow> {
    let Some(recipe) = recipes::recipe_for(family) else {
        return Vec::new();
    };
    // `pool_family`, not `family`: Kontext's parts live in shared/flux1 and
    // Qwen-Edit's in shared/qwen-image, which is what makes an existing install
    // complete them for free.
    let pool = models_dir.join("shared").join(recipe.pool_family);
    // The pool holds whatever the user downloaded, which need not be the exact
    // file this recipe declares — an older FLUX.1 install pooled the fp16 T5
    // rather than the fp8 one. Running the recipe's own role patterns over the
    // pool recognises those, so they count instead of being re-downloaded.
    let pooled = recipes::detect(&recipe, &pool_filenames(&pool));
    recipes::missing_required_roles(family, components)
        .into_iter()
        .map(|role| match recipe.shared.iter().find(|s| s.role == role) {
            // Only a role the recipe declares a download for is looked for in
            // the pool: that is what the pool is, and it keeps a stray file
            // there from being read as, say, a diffusion model.
            Some(c) => {
                let found = if pool.join(c.filename).exists() {
                    Some(c.filename.to_string())
                } else {
                    pooled.get(role).map(str::to_string)
                };
                CompletionRow {
                    role,
                    have: found.is_some(),
                    filename: found.or_else(|| Some(c.filename.to_string())),
                    size_bytes: c.size_bytes,
                    fillable: true,
                }
            }
            None => {
                CompletionRow { role, filename: None, have: false, size_bytes: 0, fillable: false }
            }
        })
        .collect()
}

/// Plain filenames in the shared pool. Empty when it does not exist yet, which
/// is the ordinary state before the first download.
fn pool_filenames(pool: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(pool) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ManifestComponents;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d =
            std::env::temp_dir().join(format!("muchai-completion-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_pooled_file_already_on_disk_costs_no_download() {
        let root = scratch("pooled");
        let pool = root.join("shared").join("flux1");
        std::fs::create_dir_all(&pool).unwrap();
        std::fs::write(pool.join("clip_l.safetensors"), b"x").unwrap();

        let c = ManifestComponents { diffusion_model: "k.gguf".into(), ..Default::default() };
        let rows = plan_completion("flux1-kontext", &c, &root);

        let clip = rows.iter().find(|r| r.role == ComponentRole::ClipL).unwrap();
        assert!(clip.have, "the pooled file is right there");
        assert!(clip.fillable);
        assert_eq!(clip.filename.as_deref(), Some("clip_l.safetensors"));

        let t5 = rows.iter().find(|r| r.role == ComponentRole::T5xxl).unwrap();
        assert!(!t5.have, "not pooled yet");
        assert_eq!(t5.size_bytes, 4_893_934_904);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Kontext pools under flux1, so an existing FLUX.1 install completes it for
    /// free. This is the whole reason `pool_family` exists.
    #[test]
    fn kontext_reads_the_flux1_pool_not_a_pool_of_its_own() {
        let root = scratch("poolfamily");
        std::fs::create_dir_all(root.join("shared").join("flux1-kontext")).unwrap();
        let flux_pool = root.join("shared").join("flux1");
        std::fs::create_dir_all(&flux_pool).unwrap();
        for f in ["clip_l.safetensors", "t5xxl_fp8_e4m3fn.safetensors", "ae.safetensors"] {
            std::fs::write(flux_pool.join(f), b"x").unwrap();
        }
        let c = ManifestComponents { diffusion_model: "k.gguf".into(), ..Default::default() };
        let rows = plan_completion("flux1-kontext", &c, &root);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.have), "everything is already in shared/flux1");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The pool holds what the user actually downloaded, which is not always the
    /// file the recipe would fetch: a FLUX.1 install made before the encoder
    /// pick was pinned holds `t5xxl_fp16.safetensors`, not the declared fp8. It
    /// is the same encoder in another precision and the engine takes it, so
    /// matching by the role's own patterns — not by exact filename — is what
    /// turns a 4.9 GB download into a free completion.
    #[test]
    fn a_pooled_file_the_role_recognises_counts_under_another_name() {
        let root = scratch("othername");
        let pool = root.join("shared").join("flux1");
        std::fs::create_dir_all(&pool).unwrap();
        std::fs::write(pool.join("t5xxl_fp16.safetensors"), b"x").unwrap();

        let c = ManifestComponents { diffusion_model: "k.gguf".into(), ..Default::default() };
        let rows = plan_completion("flux1-kontext", &c, &root);
        let t5 = rows.iter().find(|r| r.role == ComponentRole::T5xxl).unwrap();
        assert!(t5.have, "an fp16 T5-XXL fills the T5 role as well as the fp8 one");
        assert_eq!(t5.filename.as_deref(), Some("t5xxl_fp16.safetensors"), "the file it found");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A role the recipe declares no download for is reported, not guessed at.
    #[test]
    fn an_unfillable_role_is_reported_rather_than_invented() {
        let root = scratch("unfillable");
        let c = ManifestComponents { diffusion_model: "  ".into(), ..Default::default() };
        let rows = plan_completion("flux1", &c, &root);
        let diff = rows.iter().find(|r| r.role == ComponentRole::Diffusion).unwrap();
        assert!(!diff.fillable, "no shared component can supply a diffusion model");
        assert_eq!(diff.filename, None);
        assert_eq!(diff.size_bytes, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_family_with_no_recipe_has_nothing_to_complete() {
        let root = scratch("norecipe");
        let c =
            ManifestComponents { diffusion_model: "sd15.safetensors".into(), ..Default::default() };
        assert!(plan_completion("sd15", &c, &root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
