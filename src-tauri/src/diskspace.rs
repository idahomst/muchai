//! Disk-space probing for the pre-download free-space check.
//!
//! One rule (`fits`) is shared by every enforcement point so a catalog
//! pre-flight and a mid-response guard can never disagree.

/// Space we refuse to consume. A download that would leave less than this free
/// is blocked: a disk at literal zero breaks config writes, `model.json` and
/// generated-image output, so "it technically fits" is not good enough.
pub const HEADROOM_BYTES: u64 = 1024 * 1024 * 1024; // 1 GiB

/// The single fit decision: does `required` fit in `free` while leaving
/// `HEADROOM_BYTES` behind? `checked_sub` (not `saturating_add` on the
/// requirement) so an absurd `required` reports "does not fit" instead of
/// wrapping into a false pass.
pub fn fits(free: u64, required: u64) -> bool {
    free.checked_sub(required).is_some_and(|left| left >= HEADROOM_BYTES)
}

/// Bytes → compact decimal size, e.g. 6_780_000_000 → "6.8 GB". Mirrors
/// `formatBytes` in `src/lib/modelFormat.ts`; model sizes are decimal
/// (matching HuggingFace), so divide by 1000, not 1024.
pub fn fmt_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", n, UNITS[i])
    } else if v < 10.0 {
        format!("{:.1} {}", v, UNITS[i])
    } else {
        format!("{} {}", v.round() as u64, UNITS[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fits_requires_headroom_beyond_the_download() {
        assert!(fits(HEADROOM_BYTES + 100, 100));
        assert!(fits(HEADROOM_BYTES, 0));
    }

    #[test]
    fn fits_rejects_a_download_that_eats_the_headroom() {
        assert!(!fits(HEADROOM_BYTES + 99, 100));
        assert!(!fits(HEADROOM_BYTES - 1, 0));
    }

    #[test]
    fn fits_rejects_rather_than_overflowing_on_huge_requirements() {
        assert!(!fits(100, u64::MAX));
        assert!(!fits(u64::MAX, u64::MAX));
    }

    #[test]
    fn fmt_bytes_matches_the_frontend_decimal_style() {
        assert_eq!(fmt_bytes(0), "0 B");
        assert_eq!(fmt_bytes(999), "999 B");
        assert_eq!(fmt_bytes(6_780_000_000), "6.8 GB");
        assert_eq!(fmt_bytes(23_400_000_000), "23 GB");
    }
}
