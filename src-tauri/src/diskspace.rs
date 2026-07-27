//! Disk-space probing for the pre-download free-space check.
//!
//! One rule (`fits`) is shared by every enforcement point so a catalog
//! pre-flight and a mid-response guard can never disagree.

use std::path::{Path, PathBuf};

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
        // Round to one decimal (same as the JS `toFixed(1)` this mirrors),
        // then drop a trailing ".0" so whole numbers match the frontend,
        // which coerces the rounded string back through `Number(...)`.
        let rounded = format!("{:.1}", v);
        match rounded.strip_suffix(".0") {
            Some(whole) => format!("{} {}", whole, UNITS[i]),
            None => format!("{} {}", rounded, UNITS[i]),
        }
    } else {
        format!("{} {}", v.round() as u64, UNITS[i])
    }
}

/// Free bytes on the filesystem holding `path`, or `None` when no mount matches
/// (never expected on Linux, where `/` always does). Callers treat `None` as
/// "unknown — do not block".
pub fn available_bytes(path: &Path) -> Option<u64> {
    let existing = existing_ancestor(path)?;
    let target = existing.canonicalize().unwrap_or(existing);
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|d| target.starts_with(d.mount_point()))
        // Longest matching mount point wins: /home/x/models on a separate
        // drive must not be answered by the `/` mount.
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// First ancestor of `path` (including `path` itself) that exists on disk.
/// The models directory may not have been created yet.
fn existing_ancestor(path: &Path) -> Option<PathBuf> {
    path.ancestors().find(|p| p.exists()).map(|p| p.to_path_buf())
}

/// Recursive size of everything under `dir`. Symlinks are not followed (they
/// point at bytes we would not reclaim by deleting this folder) and unreadable
/// entries are skipped. A missing directory is 0.
pub fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total: u64 = 0;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            total = total.saturating_add(dir_size(&entry.path()));
        } else if let Ok(meta) = entry.metadata() {
            total = total.saturating_add(meta.len());
        }
    }
    total
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

    #[test]
    fn fmt_bytes_drops_a_trailing_zero_like_the_frontend_does() {
        // `formatBytes` in modelFormat.ts is `+v.toFixed(1)`, which coerces back
        // to a Number — 5.0 renders as "5". The blocked panel shows Rust-built
        // and JS-built figures side by side, so they must not disagree.
        assert_eq!(fmt_bytes(5_000_000_000), "5 GB");
        assert_eq!(fmt_bytes(3_000_000_000), "3 GB");
        assert_eq!(fmt_bytes(2_000_000), "2 MB");
        // Non-integer tenths keep their decimal.
        assert_eq!(fmt_bytes(6_780_000_000), "6.8 GB");
        assert_eq!(fmt_bytes(3_100_000_000), "3.1 GB");
    }

    use std::fs;
    use std::path::PathBuf;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("muchai-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn available_bytes_reports_space_for_an_existing_dir() {
        let dir = scratch("avail");
        let free = available_bytes(&dir).expect("temp dir lives on a mounted filesystem");
        assert!(free > 0, "a writable temp dir should report non-zero free space");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn available_bytes_walks_up_to_an_existing_ancestor() {
        let dir = scratch("avail-walk");
        let missing = dir.join("not").join("created").join("yet");
        assert!(!missing.exists());
        assert!(available_bytes(&missing).is_some(), "should probe the nearest existing ancestor");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_size_sums_files_recursively() {
        let dir = scratch("dirsize");
        fs::write(dir.join("a.safetensors"), vec![0u8; 100]).unwrap();
        fs::create_dir_all(dir.join("nested")).unwrap();
        fs::write(dir.join("nested").join("b.gguf"), vec![0u8; 250]).unwrap();
        assert_eq!(dir_size(&dir), 350);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_size_of_a_missing_dir_is_zero() {
        let gone = std::env::temp_dir().join("muchai-dirsize-does-not-exist");
        let _ = fs::remove_dir_all(&gone);
        assert_eq!(dir_size(&gone), 0);
    }
}
