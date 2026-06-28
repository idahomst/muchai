use crate::types::ModelInfo;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const MODEL_EXTS: [&str; 3] = ["safetensors", "ckpt", "gguf"];

fn is_model_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| MODEL_EXTS.iter().any(|m| e.eq_ignore_ascii_case(m)))
        .unwrap_or(false)
}

fn collect(dir: &Path, out: &mut Vec<ModelInfo>, seen: &mut HashSet<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // missing / unreadable dirs are skipped
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out, seen);
        } else if is_model_file(&path) {
            let canon = path.canonicalize().unwrap_or_else(|_| path.clone());
            if !seen.insert(canon.clone()) {
                continue; // already found via another watched dir
            }
            let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let name = canon
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            out.push(ModelInfo {
                path: canon.to_string_lossy().into_owned(),
                name,
                size_bytes,
            });
        }
    }
}

/// Scan every directory (recursively), returning unique model files sorted by
/// name. Missing/unreadable directories are skipped silently.
pub fn scan_models(dirs: &[PathBuf]) -> Vec<ModelInfo> {
    let mut out: Vec<ModelInfo> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for dir in dirs {
        collect(dir, &mut out, &mut seen);
    }
    out.sort_by_key(|m| m.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path, bytes: usize) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, vec![0u8; bytes]).unwrap();
    }

    #[test]
    fn finds_model_files_recursively_and_ignores_others() {
        let root = std::env::temp_dir().join(format!("fridai-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("a.safetensors"), 10);
        touch(&root.join("sub/b.ckpt"), 20);
        touch(&root.join("sub/c.gguf"), 30);
        touch(&root.join("notes.txt"), 5);
        touch(&root.join("image.png"), 5);

        let models = scan_models(&[root.clone()]);
        let names: Vec<&str> = models.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]); // sorted, extensions filtered
        assert_eq!(models[0].size_bytes, 10);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_directory_is_skipped_not_an_error() {
        let models = scan_models(&[PathBuf::from("/no/such/fridai/dir")]);
        assert!(models.is_empty());
    }

    #[test]
    fn deduplicates_when_a_file_is_reachable_via_two_dirs() {
        let root = std::env::temp_dir().join(format!("fridai-dedup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        touch(&root.join("models/x.safetensors"), 1);
        // Scan the parent AND the child: x is reachable from both.
        let models = scan_models(&[root.clone(), root.join("models")]);
        assert_eq!(models.len(), 1, "same file must appear once");
        let _ = fs::remove_dir_all(&root);
    }
}
