use std::path::Path;

pub(crate) const MODEL_EXTS: [&str; 3] = ["safetensors", "ckpt", "gguf"];

pub(crate) fn is_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MODEL_EXTS.iter().any(|m| e.eq_ignore_ascii_case(m)))
        .unwrap_or(false)
}

/// Total size in BYTES the given files will occupy *once loaded by the engine*.
///
/// Returns `None` if ANY path can't be stat'd (missing/unreadable), so callers
/// treat a broken model as "size unknown" rather than silently undercounting.
/// An empty slice sums to `Some(0)`.
///
/// Each file is measured with `weights::memory_bytes`, which widens fp8
/// safetensors tensors the way the engine does. There is deliberately no
/// file-size equivalent here: summing on-disk bytes to decide a VRAM question
/// is the bug this replaced. Disk-space accounting lives in `diskspace`.
pub fn sum_memory_bytes(paths: &[String]) -> Option<u64> {
    let mut total: u64 = 0;
    for p in paths {
        total = total.saturating_add(crate::weights::memory_bytes(Path::new(p))?);
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(path: &Path, bytes: usize) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn sum_memory_bytes_totals_existing_files() {
        // Files of zero bytes aren't safetensors, so each falls back to its own
        // size: this pins the summing, not the format detection.
        let root = std::env::temp_dir().join(format!("muchai-sumsz-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("a.safetensors"), 100);
        touch(&root.join("b.safetensors"), 250);
        let paths = vec![
            root.join("a.safetensors").to_string_lossy().into_owned(),
            root.join("b.safetensors").to_string_lossy().into_owned(),
        ];
        assert_eq!(sum_memory_bytes(&paths), Some(350));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sum_memory_bytes_is_none_if_any_file_missing() {
        let root = std::env::temp_dir().join(format!("muchai-sumsz2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("a.safetensors"), 100);
        let paths = vec![
            root.join("a.safetensors").to_string_lossy().into_owned(),
            root.join("gone.safetensors").to_string_lossy().into_owned(),
        ];
        assert_eq!(sum_memory_bytes(&paths), None);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn sum_memory_bytes_empty_is_zero() {
        assert_eq!(sum_memory_bytes(&[]), Some(0));
    }
}
