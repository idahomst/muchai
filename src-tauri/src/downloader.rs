use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug)]
pub enum DownloadError {
    Unauthorized,
    NotFound,
    Network(String),
    Io(String),
    Cancelled,
}

impl DownloadError {
    /// User-facing message for the command layer.
    pub fn message(&self) -> String {
        match self {
            DownloadError::Unauthorized => {
                "This model requires an access token. Add one and try again.".into()
            }
            DownloadError::NotFound => "No file was found at that URL.".into(),
            DownloadError::Network(e) => format!("Download failed: {e}"),
            DownloadError::Io(e) => format!("Could not save the file: {e}"),
            DownloadError::Cancelled => "Download cancelled.".into(),
        }
    }
}

/// Download `url` into `dest_dir`, choosing the filename from headers/URL and
/// de-duplicating on collision. Thin wrapper over `download_to`.
pub fn download_model<F: FnMut(u64, Option<u64>)>(
    url: &str,
    token: &str,
    dest_dir: &Path,
    on_progress: F,
    cancel: &AtomicBool,
) -> Result<PathBuf, DownloadError> {
    // We need the filename before the request to compute the unique path, but the
    // server's Content-Disposition may refine it. Derive from the URL up front for
    // the unique-path base; `download_to` streams to exactly the path we choose.
    let filename = derive_filename(None, url);
    let dest = unique_path(dest_dir, &filename);
    download_to(url, token, &dest, on_progress, cancel)?;
    Ok(dest)
}

/// Download `url` to exactly `dest_path`, streaming to a sibling `.part` file and
/// renaming on success. Calls `on_progress(downloaded, total)` as bytes arrive.
/// Aborts promptly when `cancel` flips to true, removing the partial file.
pub fn download_to<F: FnMut(u64, Option<u64>)>(
    url: &str,
    token: &str,
    dest_path: &Path,
    mut on_progress: F,
    cancel: &AtomicBool,
) -> Result<(), DownloadError> {
    if let Some(parent) = dest_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| DownloadError::Io(e.to_string()))?;
    }
    let mut req = ureq::get(url);
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    let resp = match req.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            return Err(DownloadError::Unauthorized)
        }
        Err(ureq::Error::Status(404, _)) => return Err(DownloadError::NotFound),
        Err(ureq::Error::Status(code, _)) => {
            return Err(DownloadError::Network(format!("server returned {code}")))
        }
        Err(ureq::Error::Transport(t)) => return Err(DownloadError::Network(t.to_string())),
    };

    let total: Option<u64> = resp.header("Content-Length").and_then(|s| s.parse::<u64>().ok());
    let part_path = dest_path.with_extension(format!(
        "{}part",
        dest_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{e}."))
            .unwrap_or_default()
    ));

    let mut file = File::create(&part_path).map_err(|e| DownloadError::Io(e.to_string()))?;
    let mut reader = resp.into_reader();
    let mut buf = [0u8; 65536];
    let mut downloaded: u64 = 0;

    loop {
        if cancel.load(Ordering::SeqCst) {
            drop(file);
            let _ = std::fs::remove_file(&part_path);
            return Err(DownloadError::Cancelled);
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                drop(file);
                let _ = std::fs::remove_file(&part_path);
                return Err(DownloadError::Io(e.to_string()));
            }
        };
        if let Err(e) = file.write_all(&buf[..n]) {
            drop(file);
            let _ = std::fs::remove_file(&part_path);
            return Err(DownloadError::Io(e.to_string()));
        }
        downloaded += n as u64;
        on_progress(downloaded, total);
    }

    file.flush().map_err(|e| {
        let _ = std::fs::remove_file(&part_path);
        DownloadError::Io(e.to_string())
    })?;
    drop(file);
    std::fs::rename(&part_path, dest_path).map_err(|e| {
        let _ = std::fs::remove_file(&part_path);
        DownloadError::Io(e.to_string())
    })?;
    Ok(())
}

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
            let start = idx + "filename=".len();
            // `idx` is found in the lowercased copy; Content-Disposition keys are
            // ASCII tokens so the offset is valid in the original, but guard the
            // boundary so we never slice mid-codepoint on an exotic header.
            if cd.is_char_boundary(start) {
                let raw = cd[start..]
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"')
                    .trim();
                // Use the header name whenever it produced anything (including a
                // file legitimately named "model.safetensors"); only fall back to
                // the URL when the header gave us nothing usable.
                if !raw.is_empty() {
                    return sanitize_filename(raw);
                }
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
        std::fs::write(dir.join("m (1).safetensors"), b"x").unwrap();
        assert_eq!(unique_path(&dir, "m.safetensors").file_name().unwrap(), "m (2).safetensors");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
