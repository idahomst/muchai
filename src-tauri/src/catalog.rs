use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Suitability {
    Recommended,
    Tight,
    TooBig,
    Unknown,
}

/// What the fit verdict was computed against. `Ram` means no usable GPU was
/// found, so the entry was rated against system RAM (CPU/iGPU path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RatingBasis {
    Vram,
    Ram,
    None,
}

/// One diffusion file in a catalog entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogFile {
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub size_bytes: u64,
}

/// A per-entry component override (ships its own copy instead of the pool).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogShared {
    pub role: crate::recipes::ComponentRole,
    pub url: String,
    pub filename: String,
    #[serde(default)]
    pub size_bytes: u64,
}

/// A curated catalog entry (single- or multi-file; multi if the family recipe
/// has a non-empty shared list, or the entry lists its own `shared`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    pub license: String,
    pub source_url: String,
    pub diffusion: CatalogFile,
    #[serde(default)]
    pub shared: Vec<CatalogShared>,
    pub min_vram_mb: u64,
    pub recommended_vram_mb: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct CatalogDoc {
    #[allow(dead_code)]
    schema_version: u32,
    entries: Vec<CatalogEntry>,
}

/// Parse a catalog document, returning its entries. `None` on malformed JSON.
pub fn parse_catalog(json: &str) -> Option<Vec<CatalogEntry>> {
    serde_json::from_str::<CatalogDoc>(json).ok().map(|d| d.entries)
}

/// Validate a catalog entry: https urls, known family, sane VRAM ordering,
/// non-blank id/name. Accepts `.gguf` and `.safetensors`/`.ckpt` diffusion files.
pub fn validate_entry(e: &CatalogEntry) -> Result<(), String> {
    if e.id.trim().is_empty() || e.name.trim().is_empty() {
        return Err("entry id/name must be non-empty".into());
    }
    if crate::recipes::recipe_for(&e.family).is_none() && e.family != "sdxl" && e.family != "sd15" {
        return Err(format!("unknown family {}", e.family));
    }
    if !e.diffusion.url.starts_with("https://") {
        return Err("diffusion url must be https".into());
    }
    for s in &e.shared {
        if !s.url.starts_with("https://") {
            return Err("shared url must be https".into());
        }
    }
    if e.recommended_vram_mb < e.min_vram_mb {
        return Err("recommended_vram_mb < min_vram_mb".into());
    }
    Ok(())
}

/// Parse, or return an empty catalog (never panics). Used by the loader. Drops
/// (and logs) any entry that fails `validate_entry` rather than surfacing it.
pub fn load_catalog_from_str(json: &str) -> Vec<CatalogEntry> {
    let entries = match parse_catalog(json) {
        Some(e) => e,
        None => {
            eprintln!("catalog.json is malformed; using an empty catalog");
            return Vec::new();
        }
    };
    entries
        .into_iter()
        .filter(|e| match validate_entry(e) {
            Ok(()) => true,
            Err(why) => {
                eprintln!("catalog entry {} dropped: {why}", e.id);
                false
            }
        })
        .collect()
}

/// A catalog entry plus its fit verdict + what it was rated against, for the New… dialog.
#[derive(Debug, Clone, Serialize)]
pub struct RatedCatalogEntry {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub suitability: Suitability,
    pub basis: RatingBasis,
}

/// Rate a memory total (MB) against an entry's thresholds.
fn rate_value(entry: &CatalogEntry, total_mb: u64) -> Suitability {
    if total_mb >= entry.recommended_vram_mb {
        Suitability::Recommended
    } else if total_mb >= entry.min_vram_mb {
        Suitability::Tight
    } else {
        Suitability::TooBig
    }
}

/// Rate an entry against total VRAM, falling back to system RAM when no usable
/// GPU is present (0 or unknown VRAM). Returns the verdict and which memory it
/// was computed against.
pub fn rate_entry(
    entry: &CatalogEntry,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> (Suitability, RatingBasis) {
    if let Some(v) = vram_total_mb.filter(|v| *v > 0) {
        (rate_value(entry, v), RatingBasis::Vram)
    } else if let Some(r) = ram_total_mb.filter(|r| *r > 0) {
        (rate_value(entry, r), RatingBasis::Ram)
    } else {
        (Suitability::Unknown, RatingBasis::None)
    }
}

/// The full catalog rated against VRAM (RAM fallback). See `rate_entry`.
pub fn rated_catalog_entries(
    entries: Vec<CatalogEntry>,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> Vec<RatedCatalogEntry> {
    entries
        .into_iter()
        .map(|entry| {
            let (suitability, basis) = rate_entry(&entry, vram_total_mb, ram_total_mb);
            RatedCatalogEntry { entry, suitability, basis }
        })
        .collect()
}

/// One file to fetch and where to write it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    pub url: String,
    pub dest: PathBuf,
    pub role: crate::recipes::ComponentRole,
    pub size_bytes: u64,
    /// true if pooled under shared/<family> (skip if already present).
    pub shared: bool,
}

/// The full plan for materializing a catalog entry on disk.
#[derive(Debug, Clone)]
pub struct EntryPlan {
    pub model_dir: PathBuf,
    pub files: Vec<PlannedFile>,
}

/// Compute every file to download for `entry` and its on-disk destination.
/// Diffusion → model folder. Recipe shared components → shared/<family> (pooled).
/// Per-entry `shared` overrides → model folder (not pooled).
pub fn plan_entry_downloads(entry: &CatalogEntry, models_dir: &Path) -> EntryPlan {
    let model_dir = models_dir.join(&entry.id);
    // Shared parts pool per *recipe*, not per entry family: an edit family
    // reuses its base family's encoder and VAE rather than re-downloading them.
    let pool = crate::recipes::recipes()
        .into_iter()
        .find(|r| r.family == entry.family)
        .map(|r| r.pool_family.to_string())
        .unwrap_or_else(|| entry.family.clone());
    let shared_dir = models_dir.join("shared").join(&pool);
    let mut files = Vec::new();

    files.push(PlannedFile {
        url: entry.diffusion.url.clone(),
        dest: model_dir.join(&entry.diffusion.filename),
        role: crate::recipes::ComponentRole::Diffusion,
        size_bytes: entry.diffusion.size_bytes,
        shared: false,
    });

    let override_roles: std::collections::HashSet<_> =
        entry.shared.iter().map(|s| s.role).collect();
    for s in &entry.shared {
        files.push(PlannedFile {
            url: s.url.clone(),
            dest: model_dir.join(&s.filename),
            role: s.role,
            size_bytes: s.size_bytes,
            shared: false,
        });
    }

    if let Some(recipe) = crate::recipes::recipe_for(&entry.family) {
        for comp in &recipe.shared {
            if override_roles.contains(&comp.role) {
                continue;
            }
            files.push(PlannedFile {
                url: comp.url.to_string(),
                dest: shared_dir.join(comp.filename),
                role: comp.role,
                size_bytes: comp.size_bytes,
                shared: true,
            });
        }
    }

    EntryPlan { model_dir, files }
}

/// Bytes this plan will actually pull down: every planned file except pooled
/// shared components already present on disk. Mirrors the skip rule in
/// `commands::add_catalog_model`'s download loop — the two must agree or the
/// pre-flight over-reports a reused encoder.
pub fn required_bytes(plan: &EntryPlan) -> u64 {
    plan.files
        .iter()
        .filter(|f| !(f.shared && f.dest.exists()))
        .fold(0u64, |acc, f| acc.saturating_add(f.size_bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_bytes_skips_pooled_components_already_on_disk() {
        use crate::recipes::ComponentRole;
        let root = std::env::temp_dir().join(format!("muchai-required-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let shared_dir = root.join("shared").join("flux1");
        std::fs::create_dir_all(&shared_dir).unwrap();
        std::fs::write(shared_dir.join("t5xxl.safetensors"), b"x").unwrap();

        let plan = EntryPlan {
            model_dir: root.join("m"),
            files: vec![
                PlannedFile {
                    url: "https://h/diffusion.gguf".into(),
                    dest: root.join("m").join("diffusion.gguf"),
                    role: ComponentRole::Diffusion,
                    size_bytes: 7_000_000_000,
                    shared: false,
                },
                PlannedFile {
                    url: "https://h/t5xxl.safetensors".into(),
                    dest: shared_dir.join("t5xxl.safetensors"),
                    role: ComponentRole::T5xxl,
                    size_bytes: 9_700_000_000,
                    shared: true,
                },
                PlannedFile {
                    url: "https://h/clip_l.safetensors".into(),
                    dest: shared_dir.join("clip_l.safetensors"),
                    role: ComponentRole::ClipL,
                    size_bytes: 250_000_000,
                    shared: true,
                },
            ],
        };

        // t5xxl is pooled AND present → free. clip_l is pooled but missing → counted.
        assert_eq!(required_bytes(&plan), 7_000_000_000 + 250_000_000);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn required_bytes_counts_non_shared_files_even_when_present() {
        use crate::recipes::ComponentRole;
        let root = std::env::temp_dir().join(format!("muchai-required2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let model_dir = root.join("m");
        std::fs::create_dir_all(&model_dir).unwrap();
        std::fs::write(model_dir.join("diffusion.gguf"), b"x").unwrap();

        let plan = EntryPlan {
            model_dir: model_dir.clone(),
            files: vec![PlannedFile {
                url: "https://h/diffusion.gguf".into(),
                dest: model_dir.join("diffusion.gguf"),
                role: ComponentRole::Diffusion,
                size_bytes: 7_000_000_000,
                shared: false,
            }],
        };

        // A stale same-named file in the model folder is overwritten, not reused.
        assert_eq!(required_bytes(&plan), 7_000_000_000);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_unified_catalog_json() {
        let json = r#"{
          "schema_version": 1,
          "entries": [
            {"id":"sd15","name":"SD 1.5","family":"sd15","license":"OpenRAIL",
             "source_url":"https://h/sd15",
             "diffusion":{"url":"https://h/sd15.safetensors","filename":"sd15.safetensors","size_bytes":10},
             "shared":[],"min_vram_mb":2048,"recommended_vram_mb":4096}
          ]
        }"#;
        let cat = parse_catalog(json).expect("valid catalog parses");
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].id, "sd15");
        assert_eq!(cat[0].license, "OpenRAIL");
        assert_eq!(cat[0].diffusion.filename, "sd15.safetensors");
    }

    #[test]
    fn malformed_catalog_degrades_to_empty() {
        assert!(parse_catalog("{ not json ]").is_none());
        let empty = load_catalog_from_str("{ not json ]");
        assert!(empty.is_empty());
    }

    #[test]
    fn accepts_gguf_diffusion_entry() {
        let json = r#"{"schema_version":1,"entries":[
          {"id":"flux1-schnell-q4","name":"FLUX.1 schnell Q4","family":"flux1","license":"Apache-2.0",
           "source_url":"https://h/flux","diffusion":{"url":"https://h/flux1-schnell-Q4_K_M.gguf","filename":"flux1-schnell-Q4_K_M.gguf","size_bytes":0},
           "shared":[],"min_vram_mb":8192,"recommended_vram_mb":12288}
        ]}"#;
        let cat = parse_catalog(json).unwrap();
        assert!(cat[0].diffusion.filename.ends_with(".gguf"));
    }

    #[test]
    fn validate_entry_requires_https_and_known_family() {
        let ok = CatalogEntry {
            id: "e".into(), name: "E".into(), family: "flux1".into(),
            license: "Apache-2.0".into(), source_url: "https://h/e".into(),
            diffusion: CatalogFile { url: "https://h/e.gguf".into(), filename: "e.gguf".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 8192, recommended_vram_mb: 12288,
        };
        assert!(validate_entry(&ok).is_ok());

        let mut bad_url = ok.clone();
        bad_url.diffusion.url = "http://insecure/e.gguf".into();
        assert!(validate_entry(&bad_url).is_err(), "non-https diffusion url rejected");

        let mut bad_fam = ok.clone();
        bad_fam.family = "no-such-family".into();
        assert!(validate_entry(&bad_fam).is_err(), "unknown family rejected");

        let mut bad_vram = ok.clone();
        bad_vram.recommended_vram_mb = 1; // < min
        assert!(validate_entry(&bad_vram).is_err(), "recommended < min rejected");
    }

    #[test]
    fn load_catalog_from_str_drops_invalid_entries() {
        let json = r#"{"schema_version":1,"entries":[
          {"id":"good","name":"Good","family":"flux1","license":"Apache-2.0","source_url":"https://h/g",
           "diffusion":{"url":"https://h/g.gguf","filename":"g.gguf","size_bytes":0},"shared":[],
           "min_vram_mb":8192,"recommended_vram_mb":12288},
          {"id":"bad","name":"Bad","family":"flux1","license":"Apache-2.0","source_url":"https://h/b",
           "diffusion":{"url":"http://insecure/b.gguf","filename":"b.gguf","size_bytes":0},"shared":[],
           "min_vram_mb":8192,"recommended_vram_mb":12288}
        ]}"#;
        let cat = load_catalog_from_str(json);
        assert_eq!(cat.len(), 1);
        assert_eq!(cat[0].id, "good");
    }

    #[test]
    fn plan_entry_downloads_pools_shared_and_folds_diffusion() {
        let entry = CatalogEntry {
            id: "flux1-schnell".into(), name: "FLUX.1 schnell".into(), family: "flux1".into(),
            license: "Apache-2.0".into(), source_url: "https://h/flux".into(),
            diffusion: CatalogFile { url: "https://h/flux1-schnell.gguf".into(), filename: "flux1-schnell.gguf".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 8192, recommended_vram_mb: 12288,
        };
        let models_dir = std::path::Path::new("/models");
        let plan = plan_entry_downloads(&entry, models_dir);
        assert_eq!(plan.model_dir, models_dir.join("flux1-schnell"));
        assert!(plan.files.iter().any(|f| f.dest == plan.model_dir.join("flux1-schnell.gguf")));
        let shared_dir = models_dir.join("shared").join("flux1");
        assert!(plan.files.iter().any(|f| f.dest.starts_with(&shared_dir)), "family shared components pooled under shared/<family>");
    }

    #[test]
    fn plan_entry_downloads_single_file_family_has_no_shared() {
        let entry = CatalogEntry {
            id: "sd15".into(), name: "SD 1.5".into(), family: "sd15".into(),
            license: "OpenRAIL".into(), source_url: "https://h/sd15".into(),
            diffusion: CatalogFile { url: "https://h/sd15.safetensors".into(), filename: "sd15.safetensors".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 2048, recommended_vram_mb: 4096,
        };
        let plan = plan_entry_downloads(&entry, std::path::Path::new("/models"));
        assert_eq!(plan.files.len(), 1, "single-file family downloads only the diffusion weight");
    }

    fn sample_entry() -> CatalogEntry {
        CatalogEntry {
            id: "e".into(), name: "E".into(), family: "flux1".into(),
            license: "Apache-2.0".into(), source_url: "https://h/e".into(),
            diffusion: CatalogFile { url: "https://h/e.gguf".into(), filename: "e.gguf".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 8192, recommended_vram_mb: 12288,
        }
    }

    #[test]
    fn rate_entry_prefers_vram_basis() {
        let e = sample_entry();
        assert_eq!(rate_entry(&e, Some(16384), Some(4096)), (Suitability::Recommended, RatingBasis::Vram));
        assert_eq!(rate_entry(&e, Some(10240), Some(65536)), (Suitability::Tight, RatingBasis::Vram));
        assert_eq!(rate_entry(&e, Some(4096), Some(65536)), (Suitability::TooBig, RatingBasis::Vram));
    }

    #[test]
    fn rate_entry_falls_back_to_ram_when_no_vram() {
        let e = sample_entry();
        assert_eq!(rate_entry(&e, None, Some(16384)), (Suitability::Recommended, RatingBasis::Ram));
        assert_eq!(rate_entry(&e, Some(0), Some(10240)), (Suitability::Tight, RatingBasis::Ram));
        assert_eq!(rate_entry(&e, None, Some(4096)), (Suitability::TooBig, RatingBasis::Ram));
    }

    #[test]
    fn rate_entry_unknown_when_neither_known() {
        let e = sample_entry();
        assert_eq!(rate_entry(&e, None, None), (Suitability::Unknown, RatingBasis::None));
        assert_eq!(rate_entry(&e, Some(0), Some(0)), (Suitability::Unknown, RatingBasis::None));
    }

    #[test]
    fn rated_entries_carry_basis() {
        let rated = rated_catalog_entries(vec![sample_entry()], None, Some(16384));
        assert_eq!(rated.len(), 1);
        assert_eq!(rated[0].suitability, Suitability::Recommended);
        assert_eq!(rated[0].basis, RatingBasis::Ram);
    }

    #[test]
    fn bundled_catalog_file_is_valid() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/catalog.json");
        let s = std::fs::read_to_string(path).expect("bundled catalog exists");
        let entries = parse_catalog(&s).expect("bundled catalog parses");
        assert!(!entries.is_empty(), "seed at least one entry");
        for e in &entries {
            validate_entry(e).unwrap_or_else(|why| panic!("{} invalid: {why}", e.id));
        }
    }

    #[test]
    fn bundled_catalog_spans_tiers_and_families() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/catalog.json");
        let s = std::fs::read_to_string(path).unwrap();
        let entries = parse_catalog(&s).unwrap();
        // Tier floor + ceiling present.
        assert!(entries.iter().any(|e| e.recommended_vram_mb <= 4096), "need an ultra-light entry");
        assert!(entries.iter().any(|e| e.recommended_vram_mb > 16000), "need a 24GB-tier entry");
        // Every engine family represented. `sd3` is intentionally excluded: the
        // only ungated SD3.5 GGUFs (city96 lineage) decode to a degenerate
        // constant latent ("blue square") under the pinned engine, so we ship no
        // SD3.5 catalog entry. The sd3 recipe stays for manual multi-file adds.
        for fam in ["sd15", "sdxl", "flux1", "flux2", "qwen-image", "z-image"] {
            assert!(entries.iter().any(|e| e.family == fam), "family {fam} missing from catalog");
        }
    }

    #[test]
    fn an_edit_entry_pools_into_the_base_familys_directory() {
        let entry = CatalogEntry {
            id: "qwen-image-edit-2511-q3ks".into(),
            name: "Qwen-Image-Edit 2511".into(),
            family: "qwen-image-edit".into(),
            license: "Apache-2.0".into(),
            source_url: "https://huggingface.co/unsloth/Qwen-Image-Edit-2511-GGUF".into(),
            diffusion: CatalogFile {
                url: "https://h/qwen-image-edit-2511-Q3_K_S.gguf".into(),
                filename: "qwen-image-edit-2511-Q3_K_S.gguf".into(),
                size_bytes: 9_218_914_912,
            },
            shared: vec![],
            min_vram_mb: 12288,
            recommended_vram_mb: 24576,
        };
        let plan = plan_entry_downloads(&entry, Path::new("/models"));
        let pooled: Vec<&Path> =
            plan.files.iter().filter(|f| f.shared).map(|f| f.dest.as_path()).collect();
        assert!(!pooled.is_empty(), "the edit recipe has shared components");
        for dest in pooled {
            assert!(
                dest.starts_with("/models/shared/qwen-image"),
                "{dest:?} must reuse the qwen-image pool, not create a new one"
            );
            assert!(
                !dest.starts_with("/models/shared/qwen-image-edit"),
                "{dest:?} re-downloads what the user already has"
            );
        }
    }
}
