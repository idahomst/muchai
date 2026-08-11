//! The memory budget assumed for a unified-memory (UMA) device.
//!
//! A UMA device — an integrated GPU, an AMD APU, an NVIDIA DGX Spark — has no
//! dedicated VRAM to read: it allocates out of system RAM on demand. This module
//! derives a stable ceiling from *total* RAM so a model cannot read "fits" at
//! launch and "won't fit" after a browser is opened, and so auto Low-VRAM does not
//! engage on one run but not the next with identical inputs.

/// Share of total system RAM a unified-memory device may be assumed to use.
/// 70% is roughly where the industry lands independently: Mesa's ANV reports about
/// ¾ of RAM as its device heap on integrated parts, Apple's
/// `recommendedMaxWorkingSetSize` is ~75%, and `amdgpu`'s GTT limit defaults to
/// about half.
pub const UMA_SHARE_PCT: u64 = 70;

/// Held back for the OS, the desktop and MuchAI itself. Only binds below ~13 GB of
/// RAM, where a flat percentage over-promises.
pub const HOST_RESERVE_MB: u64 = 4096;

/// Never report zero: a zero total makes `catalog::rate_entry` fall through to its
/// whole-RAM fallback, which is *more* generous than the budget it replaced.
pub const MIN_BUDGET_MB: u64 = 1024;

/// The memory budget for a unified-memory device, in MB.
///
/// `override_mb` replaces the whole computation, clamped to
/// `[MIN_BUDGET_MB, ram_total_mb]`, so a hand-edited config cannot produce a
/// nonsensical budget. `None` means auto: `min(ram * 70%, ram - 4 GB)`, floored.
pub fn uma_budget_mb(ram_total_mb: u64, override_mb: Option<u64>) -> u64 {
    if let Some(v) = override_mb {
        // max() on the upper bound so the clamp cannot invert when RAM reads
        // below the floor (sysinfo failure).
        return v.clamp(MIN_BUDGET_MB, ram_total_mb.max(MIN_BUDGET_MB));
    }
    let share = ram_total_mb * UMA_SHARE_PCT / 100;
    let after_reserve = ram_total_mb.saturating_sub(HOST_RESERVE_MB);
    let auto = share.min(after_reserve);
    auto.max(MIN_BUDGET_MB)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ladder from the design doc. `ram_mb`, `expected_mb`, and which term binds.
    #[test]
    fn auto_budget_ladder() {
        let cases: &[(u64, u64, &str)] = &[
            (4 * 1024, MIN_BUDGET_MB, "floor: RAM - 4 GB is 0"),
            (8 * 1024, 4096, "host reserve binds"),
            (16 * 1024, 11468, "70% binds"),
            (32 * 1024, 22937, "70% binds"),
            (128 * 1024, 91750, "70% binds (DGX Spark)"),
        ];
        for (ram, expected, why) in cases {
            assert_eq!(uma_budget_mb(*ram, None), *expected, "{} GB: {}", ram / 1024, why);
        }
    }

    #[test]
    fn zero_ram_yields_the_floor() {
        // sysinfo failing to read memory must not produce a 0 budget: 0 would make
        // catalog::rate_entry fall through to its whole-RAM path, which is more
        // generous than the budget it replaced.
        assert_eq!(uma_budget_mb(0, None), MIN_BUDGET_MB);
    }

    #[test]
    fn in_range_override_passes_through() {
        assert_eq!(uma_budget_mb(16 * 1024, Some(6000)), 6000);
    }

    #[test]
    fn override_is_clamped_at_both_ends() {
        // Above installed RAM → RAM.
        assert_eq!(uma_budget_mb(16 * 1024, Some(999_999)), 16 * 1024);
        // Below the floor (including 0) → the floor.
        assert_eq!(uma_budget_mb(16 * 1024, Some(0)), MIN_BUDGET_MB);
        assert_eq!(uma_budget_mb(16 * 1024, Some(10)), MIN_BUDGET_MB);
        // Degenerate machine: the upper bound never drops below the floor, so the
        // clamp cannot invert.
        assert_eq!(uma_budget_mb(0, Some(50_000)), MIN_BUDGET_MB);
    }

    #[test]
    fn no_override_is_not_the_same_as_zero_override() {
        // Some(0) means "the user typed 0" and clamps to the floor; None means
        // "auto" and computes from RAM. On a 16 GB box those differ.
        assert_ne!(uma_budget_mb(16 * 1024, None), uma_budget_mb(16 * 1024, Some(0)));
    }
}
