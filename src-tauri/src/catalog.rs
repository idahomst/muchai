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

/// A catalog entry plus its VRAM fit verdict, for the New… dialog.
#[derive(Debug, Clone, Serialize)]
pub struct RatedCatalogEntry {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub suitability: Suitability,
}

/// Rate an entry against total VRAM (mirrors the old `rate`).
pub fn rate_entry(entry: &CatalogEntry, vram_total_mb: Option<u64>) -> Suitability {
    match vram_total_mb {
        None => Suitability::Unknown,
        Some(v) if v >= entry.recommended_vram_mb => Suitability::Recommended,
        Some(v) if v >= entry.min_vram_mb => Suitability::Tight,
        Some(_) => Suitability::TooBig,
    }
}

/// The full catalog rated against the given VRAM.
pub fn rated_catalog_entries(entries: Vec<CatalogEntry>, vram_total_mb: Option<u64>) -> Vec<RatedCatalogEntry> {
    entries
        .into_iter()
        .map(|entry| {
            let suitability = rate_entry(&entry, vram_total_mb);
            RatedCatalogEntry { entry, suitability }
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
    pub shared_dir: PathBuf,
    pub files: Vec<PlannedFile>,
}

/// Compute every file to download for `entry` and its on-disk destination.
/// Diffusion → model folder. Recipe shared components → shared/<family> (pooled).
/// Per-entry `shared` overrides → model folder (not pooled).
pub fn plan_entry_downloads(entry: &CatalogEntry, models_dir: &Path) -> EntryPlan {
    let model_dir = models_dir.join(&entry.id);
    let shared_dir = models_dir.join("shared").join(&entry.family);
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

    EntryPlan { model_dir, shared_dir, files }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
