use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// Shared with `INSUFFICIENT_SPACE_PREFIX` in `src/lib/types.ts`. Tauri commands
/// return `Result<_, String>`, so the frontend recognises this failure by prefix
/// in order to open the reclaim panel. Keep the two in sync.
pub const INSUFFICIENT_SPACE_PREFIX: &str = "Not enough disk space";

#[derive(Debug)]
pub enum DownloadError {
    Unauthorized,
    NotFound,
    Network(String),
    Io(String),
    Cancelled,
    InsufficientSpace { needed: u64, free: u64 },
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
            DownloadError::InsufficientSpace { needed, free } => format!(
                "{INSUFFICIENT_SPACE_PREFIX}: needs {}, only {} free.",
                crate::diskspace::fmt_bytes(*needed),
                crate::diskspace::fmt_bytes(*free)
            ),
        }
    }
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

    // Refuse before creating the .part file: a 12 GB download that fills the
    // disk at 90% wastes twenty minutes and leaves the user at zero free space.
    // No Content-Length means the size is unknowable here, so we proceed.
    if let Some(total) = total {
        let dir = dest_path.parent().unwrap_or_else(|| Path::new("."));
        if let Some(free) = crate::diskspace::available_bytes(dir) {
            if !crate::diskspace::fits(free, total) {
                return Err(DownloadError::InsufficientSpace { needed: total, free });
            }
        }
    }

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
    fn insufficient_space_message_carries_the_shared_prefix() {
        let err = DownloadError::InsufficientSpace { needed: 12_400_000_000, free: 3_100_000_000 };
        let msg = err.message();
        assert!(
            msg.starts_with(INSUFFICIENT_SPACE_PREFIX),
            "frontend matches on this prefix, got: {msg}"
        );
        assert!(msg.contains("12 GB"), "should name the requirement, got: {msg}");
        assert!(msg.contains("3.1 GB"), "should name the free space, got: {msg}");
    }

    /// The prefix above only works because the frontend spells it identically,
    /// and two consumers now depend on it — the model dialog's reclaim panel
    /// and the engine panel's pointer to it. A drift is silent: the failure
    /// still reports, it just stops offering the way out. `include_str!` is how
    /// the rest of this codebase pins a contract that crosses languages.
    #[test]
    fn the_frontend_matches_on_the_same_prefix() {
        assert!(
            include_str!("../../src/lib/types.ts")
                .contains(&format!("= \"{INSUFFICIENT_SPACE_PREFIX}\"")),
            "types.ts no longer declares {INSUFFICIENT_SPACE_PREFIX}"
        );
    }
}
