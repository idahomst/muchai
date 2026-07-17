//! Pure VRAM fit estimation for a model file. No I/O — fully unit-testable.
//! The estimate is deliberately rough and always surfaced to the user as an
//! *estimate*; `ACTIVATION_BUDGET_MB` is the single tunable constant.

use serde::Serialize;

/// Headroom (MB) for activations/working buffers on top of weight bytes.
pub const ACTIVATION_BUDGET_MB: u64 = 1500;

/// Rough peak VRAM (MB) needed to run a model whose on-GPU weights are
/// `file_size_bytes`. `weights_mb * 1.15 + activation budget`.
pub fn estimate_vram_mb(file_size_bytes: u64) -> u64 {
    let weights_mb = file_size_bytes as f64 / 1_048_576.0;
    (weights_mb * 1.15) as u64 + ACTIVATION_BUDGET_MB
}

/// Whether a model is expected to fit the selected device's VRAM.
/// Reuses the suitability vocabulary for UI consistency with `catalog::rate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FitVerdict {
    Fits,
    Tight,
    WontFit,
    Unknown,
}

/// Rate an estimated size against detected VRAM. `None` for either input →
/// `Unknown` (frontend shows size only, no verdict).
pub fn fit_verdict(file_size_bytes: Option<u64>, vram_total_mb: Option<u64>) -> FitVerdict {
    match (file_size_bytes, vram_total_mb) {
        (Some(bytes), Some(vram)) => {
            let est = estimate_vram_mb(bytes) as f64;
            if est <= 0.9 * vram as f64 {
                FitVerdict::Fits
            } else if est <= vram as f64 {
                FitVerdict::Tight
            } else {
                FitVerdict::WontFit
            }
        }
        _ => FitVerdict::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MB: u64 = 1_048_576;

    #[test]
    fn estimate_adds_overhead_and_budget() {
        // 1000 MB of weights → 1000*1.15 + 1500 = 2650.
        assert_eq!(estimate_vram_mb(1000 * MB), 2650);
    }

    #[test]
    fn verdict_fits_when_well_under_vram() {
        // est(1000MB) = 2650; 0.9 * 4096 = 3686.4 → Fits.
        assert_eq!(fit_verdict(Some(1000 * MB), Some(4096)), FitVerdict::Fits);
    }

    #[test]
    fn verdict_tight_between_ninety_percent_and_full() {
        // est(2500MB) = 2875+1500 = 4375; VRAM 4600: 0.9*4600=4140 < 4375 <= 4600 → Tight.
        assert_eq!(fit_verdict(Some(2500 * MB), Some(4600)), FitVerdict::Tight);
    }

    #[test]
    fn verdict_wont_fit_when_over_vram() {
        // est(6000MB) = 6900+1500 = 8400 > 8192 → WontFit.
        assert_eq!(fit_verdict(Some(6000 * MB), Some(8192)), FitVerdict::WontFit);
    }

    #[test]
    fn verdict_unknown_when_vram_or_size_missing() {
        assert_eq!(fit_verdict(Some(1000 * MB), None), FitVerdict::Unknown);
        assert_eq!(fit_verdict(None, Some(8192)), FitVerdict::Unknown);
    }
}
