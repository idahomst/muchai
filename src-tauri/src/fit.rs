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

/// Load-time quantisation ladder for the diffusion model, best quality first,
/// as `(engine type name, bits per weight)`. The bit widths come from ggml's
/// block layouts: q8_0 is 34 bytes per 32 weights (8.5 bpw), q5_1 is 24 per 32
/// (6.0), q4_K is 144 per 256 (4.5).
///
/// Verified against engine `b290693` on a 17,316 MB (f16-resident) FLUX.2 klein
/// diffusion model: predicted q8_0 9,199 MB vs 9,291 MB measured, predicted
/// q4_K 4,870 MB vs 5,011 MB measured — both within 3%.
pub const QUANT_LADDER: [(&str, f64); 3] = [("q8_0", 8.5), ("q5_1", 6.0), ("q4_K", 4.5)];

/// Resident bytes of `memory_bytes` worth of f16 weights after re-quantising to
/// `bits_per_weight`. The f16 baseline (2 bytes/weight) is what the engine
/// actually holds before quantisation, including widened fp8 tensors.
fn quantized_bytes(memory_bytes: u64, bits_per_weight: f64) -> u64 {
    let weights = memory_bytes as f64 / 2.0;
    (weights * bits_per_weight / 8.0) as u64
}

/// Pick a load-time weight type for the diffusion model, or `None` to load it
/// unchanged.
///
/// `diffusion_bytes` and `other_bytes` are *in-memory* bytes (see
/// `weights::memory_bytes`); only the diffusion model is quantised, so
/// `other_bytes` — text encoders, VAE — is added untouched at every rung.
///
/// Returns `None` when VRAM is unknown or the model already fits: quantising a
/// model that fits would cost quality for nothing. When nothing on the ladder
/// fits, the bottom rung is returned anyway — a degraded model that runs beats
/// one that cannot load.
///
/// `diffusion_is_quantized` (a GGUF — see `weights::is_quantized`) declines the
/// whole ladder, because for such a file every rung is an *upcast*. The
/// arithmetic above starts from f16, but a GGUF is already below that, and
/// `weights::memory_bytes` reports its resident size directly rather than an
/// f16 figure to shrink. Measured on `qwen-image-edit-2511-Q3_K_S.gguf` (~3.4
/// bpw, 8,792 MB resident): q4_K took it to 10,993 MB (+25%) and q5_1 to 14,644
/// MB (+67%) — after 255 s of conversion, having predicted a shrink both times.
pub fn choose_weight_type(
    diffusion_bytes: u64,
    other_bytes: u64,
    vram_mb: Option<u64>,
    diffusion_is_quantized: bool,
) -> Option<&'static str> {
    let vram = vram_mb?;
    if diffusion_is_quantized {
        return None;
    }
    let as_is = diffusion_bytes.saturating_add(other_bytes);
    if estimate_vram_mb(as_is) <= vram {
        return None;
    }
    let mut last = None;
    for (name, bpw) in QUANT_LADDER {
        let total = quantized_bytes(diffusion_bytes, bpw).saturating_add(other_bytes);
        if estimate_vram_mb(total) <= vram {
            return Some(name);
        }
        last = Some(name);
    }
    last
}

/// Resolve the user's `load_precision` preference into an engine weight type
/// for one run, or `None` to load the model unchanged.
///
/// `auto` defers to `choose_weight_type`; `original` always declines; anything
/// else is taken as an explicit engine type and passed through — including on an
/// already-quantised model, where only `auto` steps back: an explicit choice is
/// the user's to make and to undo.
///
/// Quantisation is pointless on CPU (there is no VRAM ceiling to duck under) and
/// is skipped there.
pub fn resolve_weight_type(
    preference: &str,
    diffusion_bytes: Option<u64>,
    other_bytes: Option<u64>,
    device_vram_mb: Option<u64>,
    is_cpu_device: bool,
    diffusion_is_quantized: bool,
) -> Option<String> {
    if is_cpu_device || preference == crate::types::LOAD_PRECISION_ORIGINAL {
        return None;
    }
    if preference != crate::types::LOAD_PRECISION_AUTO {
        return Some(preference.to_string());
    }
    choose_weight_type(diffusion_bytes?, other_bytes?, device_vram_mb, diffusion_is_quantized)
        .map(str::to_string)
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
    fn no_quantisation_when_the_model_already_fits() {
        // est(1 GB) ≈ 2677 MB < 12000 → loading it degraded would be pure loss.
        assert_eq!(choose_weight_type(1024 * MB, 0, Some(12000), false), None);
    }

    #[test]
    fn no_quantisation_when_vram_unknown() {
        // Can't tell whether it's needed; never silently degrade quality.
        assert_eq!(choose_weight_type(64 * 1024 * MB, 0, None, false), None);
    }

    #[test]
    fn picks_the_highest_quality_rung_that_fits() {
        // Diffusion 17,316 MB resident (the measured fp8 klein 9B, widened to
        // f16) on a 12 GB card: as-is est ≈ 19,913 > 12,000, so it must degrade.
        // q8_0 → 9,199 MB + 4,816 MB encoder = 14,015; est ≈ 17,617 > 12,000 → no.
        // q5_1 → 6,493 + 4,816 = 11,309; est ≈ 14,505 > 12,000 → no.
        // q4_K → 4,870 + 4,816 = 9,686; est ≈ 12,639 > 12,000 → also no,
        // so the bottom rung is returned as best effort.
        let diffusion = 17_316 * MB;
        let other = 4_816 * MB;
        assert_eq!(choose_weight_type(diffusion, other, Some(12_000), false), Some("q4_K"));
    }

    #[test]
    fn stops_at_q8_0_when_that_is_enough() {
        // 17,316 MB alone on a 24 GB card: as-is est ≈ 19,913+1500 > 24,000? No —
        // est = 17316*1.15 + 1500 = 21,413 <= 24,000, so it fits and returns None.
        assert_eq!(choose_weight_type(17_316 * MB, 0, Some(24_000), false), None);
        // Drop the budget to 20,000: as-is 21,413 doesn't fit, q8_0 (9,199 MB →
        // est 12,079) does, and it's the top rung.
        assert_eq!(choose_weight_type(17_316 * MB, 0, Some(20_000), false), Some("q8_0"));
    }

    #[test]
    fn falls_through_to_the_bottom_rung_when_nothing_fits() {
        // A model so large no rung helps still gets the smallest type rather
        // than None — refusing to quantise would guarantee a load failure.
        assert_eq!(choose_weight_type(400 * 1024 * MB, 0, Some(8000), false), Some("q4_K"));
    }

    #[test]
    fn untouched_components_are_counted_at_every_rung() {
        // Encoder + VAE are never quantised, so a budget they alone exceed can
        // never be satisfied: the ladder bottoms out instead of claiming a fit.
        assert_eq!(choose_weight_type(1024 * MB, 40 * 1024 * MB, Some(8000), false), Some("q4_K"));
    }

    #[test]
    fn precision_original_never_quantises_however_bad_the_fit() {
        let huge = 400 * 1024 * MB;
        assert_eq!(resolve_weight_type("original", Some(huge), Some(0), Some(8000), false, false), None);
    }

    #[test]
    fn precision_auto_follows_the_ladder() {
        // Same inputs as picks_the_highest_quality_rung_that_fits.
        assert_eq!(
            resolve_weight_type("auto", Some(17_316 * MB), Some(4_816 * MB), Some(12_000), false, false),
            Some("q4_K".to_string())
        );
        // …and stays out of the way when the model already fits.
        assert_eq!(
            resolve_weight_type("auto", Some(1024 * MB), Some(0), Some(12_000), false, false),
            None
        );
    }

    #[test]
    fn explicit_precision_is_passed_through_even_when_the_model_fits() {
        // The user asked for 8-bit; honour it rather than second-guessing.
        assert_eq!(
            resolve_weight_type("q8_0", Some(1024 * MB), Some(0), Some(24_000), false, false),
            Some("q8_0".to_string())
        );
    }

    #[test]
    fn explicit_precision_does_not_need_measurable_sizes() {
        // Unknown bytes must not silently downgrade an explicit choice to "off".
        assert_eq!(
            resolve_weight_type("q4_K", None, None, None, false, false),
            Some("q4_K".to_string())
        );
    }

    #[test]
    fn auto_precision_declines_when_sizes_are_unknown() {
        assert_eq!(resolve_weight_type("auto", None, Some(0), Some(8000), false, false), None);
        assert_eq!(resolve_weight_type("auto", Some(1024 * MB), None, Some(8000), false, false), None);
    }

    #[test]
    fn cpu_device_never_quantises() {
        // No VRAM ceiling to duck under, so quantising would only lose quality —
        // and this holds even for an explicit request.
        let huge = 400 * 1024 * MB;
        assert_eq!(resolve_weight_type("auto", Some(huge), Some(0), Some(8000), true, false), None);
        assert_eq!(resolve_weight_type("q4_K", Some(huge), Some(0), Some(8000), true, false), None);
    }

    #[test]
    fn an_already_quantised_model_is_left_alone_however_bad_the_fit() {
        // The real case: Qwen-Image-Edit 2511 Q3_K_S, 8,792 MB resident at ~3.4
        // bpw plus a 7,139 MB encoder stack, on a 12 GB card. The f16 arithmetic
        // claims q5_1 would shrink the diffusion model to 3,297 MB; the engine
        // measured 14,644 MB, because every rung of the ladder is an upcast from
        // 3.4 bpw. Declining costs nothing and saves 255 s of conversion.
        let diffusion = 8_792 * MB;
        let other = 7_139 * MB;
        assert_eq!(choose_weight_type(diffusion, other, Some(12_000), true), None);
        // Same numbers unquantised: it *would* have degraded the model.
        assert!(choose_weight_type(diffusion, other, Some(12_000), false).is_some());
    }

    #[test]
    fn an_explicit_precision_still_reaches_an_already_quantised_model() {
        // Only `auto` steps back. A user who types q4_K has chosen to re-quantise
        // and can choose otherwise; silently ignoring them would be worse.
        assert_eq!(
            resolve_weight_type("q4_K", Some(8_792 * MB), Some(0), Some(12_000), false, true),
            Some("q4_K".to_string())
        );
    }

    #[test]
    fn ladder_is_ordered_best_quality_first() {
        // choose_weight_type returns the FIRST fitting rung, so a mis-ordered
        // ladder would silently hand back a worse type than necessary.
        let bpws: Vec<f64> = QUANT_LADDER.iter().map(|(_, b)| *b).collect();
        assert!(bpws.windows(2).all(|w| w[0] > w[1]), "ladder must descend: {bpws:?}");
    }

    #[test]
    fn low_vram_off_when_vram_or_weights_unknown() {
        // Can't decide a fit → don't auto-engage (manual toggle still available).
        assert_eq!(resolve_low_vram(false, None, Some(12000), false), (false, false));
        assert_eq!(resolve_low_vram(false, Some(1024 * MB), None, false), (false, false));
    }
}
