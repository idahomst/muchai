use serde::{Deserialize, Serialize};

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

/// Parse, or return an empty catalog (never panics). Used by the loader.
pub fn load_catalog_from_str(json: &str) -> Vec<CatalogEntry> {
    match parse_catalog(json) {
        Some(entries) => entries,
        None => {
            eprintln!("catalog.json is malformed; using an empty catalog");
            Vec::new()
        }
    }
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
}
