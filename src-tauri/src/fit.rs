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

/// Convenience for the library rating command: the displayed estimate (MB) and
/// the fit verdict for one model, from its summed component bytes. `None` bytes
/// → `(None, Unknown)`.
pub fn estimate_and_verdict(
    file_size_bytes: Option<u64>,
    vram_total_mb: Option<u64>,
) -> (Option<u64>, FitVerdict) {
    (
        file_size_bytes.map(estimate_vram_mb),
        fit_verdict(file_size_bytes, vram_total_mb),
    )
}

/// Decide whether to run the engine in Low-VRAM offload mode for one generation.
/// Returns `(low_vram_enabled, auto_engaged)`. `auto_engaged` is true only when
/// the fit estimate turned it on (so the caller can surface a one-time notice);
/// a manual toggle forces it on but is never reported as "auto".
///
/// `weights_bytes` is the summed size of the model's weight files in BYTES (fed
/// straight to `estimate_vram_mb`, which expects bytes). `device_vram_mb` is the
/// selected GPU's total VRAM in MB. `is_cpu_device` short-circuits to off since
/// the offload flags only relieve GPU-VRAM pressure.
pub fn resolve_low_vram(
    manual_toggle: bool,
    weights_bytes: Option<u64>,
    device_vram_mb: Option<u64>,
    is_cpu_device: bool,
) -> (bool, bool) {
    if manual_toggle {
        return (true, false);
    }
    if is_cpu_device {
        return (false, false);
    }
    match (weights_bytes, device_vram_mb) {
        (Some(w), Some(v)) if estimate_vram_mb(w) > v => (true, true),
        _ => (false, false),
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

    #[test]
    fn estimate_and_verdict_pairs_estimate_with_verdict() {
        let (est, verdict) = estimate_and_verdict(Some(1000 * MB), Some(4096));
        assert_eq!(est, Some(estimate_vram_mb(1000 * MB)));
        assert_eq!(verdict, FitVerdict::Fits);
    }

    #[test]
    fn estimate_and_verdict_unknown_when_size_missing() {
        assert_eq!(estimate_and_verdict(None, Some(8192)), (None, FitVerdict::Unknown));
    }

    #[test]
    fn low_vram_manual_toggle_forces_on_without_auto_flag() {
        // Manual on wins regardless of fit, and is never reported as "auto".
        assert_eq!(resolve_low_vram(true, Some(500 * MB), Some(24000), false), (true, false));
        assert_eq!(resolve_low_vram(true, None, None, false), (true, false));
    }

    #[test]
    fn low_vram_auto_engages_when_weights_exceed_vram() {
        // est(20 GB) ≈ 20480*1.15 + 1500 ≈ 25052 MB > 12000 MB VRAM → auto on.
        let twenty_gb = 20u64 * 1024 * MB;
        assert_eq!(resolve_low_vram(false, Some(twenty_gb), Some(12000), false), (true, true));
    }

    #[test]
    fn low_vram_stays_off_when_model_fits() {
        // est(1 GB) = 1024*1.15 + 1500 ≈ 2677 MB < 12000 MB → off.
        let one_gb = 1024 * MB;
        assert_eq!(resolve_low_vram(false, Some(one_gb), Some(12000), false), (false, false));
    }

    #[test]
    fn low_vram_off_on_cpu_device_even_if_weights_huge() {
        // CPU has no GPU VRAM to overflow; offload flags don't apply.
        let twenty_gb = 20u64 * 1024 * MB;
        assert_eq!(resolve_low_vram(false, Some(twenty_gb), Some(12000), true), (false, false));
    }

    #[test]
    fn low_vram_off_when_vram_or_weights_unknown() {
        // Can't decide a fit → don't auto-engage (manual toggle still available).
        assert_eq!(resolve_low_vram(false, None, Some(12000), false), (false, false));
        assert_eq!(resolve_low_vram(false, Some(1024 * MB), None, false), (false, false));
    }
}
