use std::path::{Path, PathBuf};

/// Strip any directory components and control chars; fall back to a default.
pub fn sanitize_filename(name: &str) -> String {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("")
        .trim()
        .replace(|c: char| c.is_control(), "");
    if base.is_empty() {
        "model.safetensors".to_string()
    } else {
        base
    }
}

/// Decide the output filename from the response headers, then the URL.
pub fn derive_filename(content_disposition: Option<&str>, url: &str) -> String {
    if let Some(cd) = content_disposition {
        if let Some(idx) = cd.to_lowercase().find("filename=") {
            let raw = cd[idx + "filename=".len()..]
                .trim()
                .trim_matches('"')
                .split(';')
                .next()
                .unwrap_or("");
            let cleaned = sanitize_filename(raw);
            if cleaned != "model.safetensors" {
                return cleaned;
            }
        }
    }
    let path = url.split('?').next().unwrap_or(url);
    sanitize_filename(path)
}

/// Return `dir/filename`, appending " (n)" before the extension if it exists.
pub fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let ext = path.extension().and_then(|s| s.to_str());
    for n in 1.. {
        let name = match ext {
            Some(e) => format!("{stem} ({n}).{e}"),
            None => format!("{stem} ({n})"),
        };
        let p = dir.join(name);
        if !p.exists() {
            return p;
        }
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filename_prefers_content_disposition() {
        let cd = Some("attachment; filename=\"sd_xl_base_1.0.safetensors\"");
        assert_eq!(derive_filename(cd, "https://x/y?dl=1"), "sd_xl_base_1.0.safetensors");
    }

    #[test]
    fn filename_falls_back_to_url_last_segment() {
        assert_eq!(
            derive_filename(None, "https://huggingface.co/a/b/resolve/main/model.safetensors?download=true"),
            "model.safetensors"
        );
    }

    #[test]
    fn filename_sanitizes_and_defaults() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename(""), "model.safetensors");
        assert_eq!(sanitize_filename("a/b\\c.safetensors"), "c.safetensors");
    }

    #[test]
    fn unique_path_suffixes_on_collision() {
        let dir = std::env::temp_dir().join(format!("fridai-uniq-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("m.safetensors"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "m.safetensors").file_name().unwrap(), "m (1).safetensors");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
