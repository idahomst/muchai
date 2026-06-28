use serde::Serialize;

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
