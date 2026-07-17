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
        // est(6000MB) = 6899+1500 = 8399 > 8192 → WontFit.
        // (6000*1.15 == 6899.99… in f64, truncates to 6899, not 6900.)
        assert_eq!(fit_verdict(Some(6000 * MB), Some(8192)), FitVerdict::WontFit);
    }

    #[test]
    fn verdict_boundary_fits_to_tight_at_ninety_percent() {
        // VRAM 4000 → 0.9×VRAM = 3600.0 exactly, so the Fits↔Tight edge sits at
        // est == 3600: est == 3600 → Fits (<= inclusive); est == 3601 → Tight.
        // This pins the `<=` and the 0.9 factor (interior points wouldn't).
        let fits_bytes = 1826 * MB + MB / 2; // weights 1826.5 → 1826.5*1.15=2100.47→2100 +1500 = 3600
        let tight_bytes = 1827 * MB; //         weights 1827.0 → 1827*1.15=2101.05→2101 +1500 = 3601
        assert_eq!(estimate_vram_mb(fits_bytes), 3600);
        assert_eq!(estimate_vram_mb(tight_bytes), 3601);
        assert_eq!(fit_verdict(Some(fits_bytes), Some(4000)), FitVerdict::Fits);
        assert_eq!(fit_verdict(Some(tight_bytes), Some(4000)), FitVerdict::Tight);
    }

    #[test]
    fn verdict_boundary_tight_to_wont_fit_at_full_vram() {
        // Tight↔WontFit edge sits at est == VRAM: est == VRAM → Tight (<= inclusive);
        // est == VRAM+1 → WontFit. Pins the second `<=`.
        let bytes = 2000 * MB; // weights 2000 → 2000*1.15=2300.0(f64)→2300 +1500 = 3800
        assert_eq!(estimate_vram_mb(bytes), 3800);
        assert_eq!(fit_verdict(Some(bytes), Some(3800)), FitVerdict::Tight); // est == VRAM → Tight
        assert_eq!(fit_verdict(Some(bytes), Some(3799)), FitVerdict::WontFit); // est > VRAM → WontFit
    }

    #[test]
    fn verdict_unknown_when_vram_or_size_missing() {
        assert_eq!(fit_verdict(Some(1000 * MB), None), FitVerdict::Unknown);
        assert_eq!(fit_verdict(None, Some(8192)), FitVerdict::Unknown);
    }
}
