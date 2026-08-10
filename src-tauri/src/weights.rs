//! Format-aware *in-memory* weight size.
//!
//! The fit estimator used to assume on-disk bytes ≈ in-RAM/VRAM bytes. That
//! holds for GGUF (quantised blocks load verbatim) but is badly wrong for
//! safetensors carrying fp8 tensors: the pinned engine has no fp8 compute
//! kernels — only converters (`f8_e4m3_to_f16_vec` in `libstable-diffusion.so`)
//! — so every fp8 tensor is widened to f16 at load time.
//!
//! Measured on engine `b290693` with a ComfyUI-exported scaled-fp8 FLUX.2 klein
//! checkpoint (8.72 GB of `F8_E4M3` + 0.71 GB `BF16`, 9.43 GB of tensor payload):
//!
//! ```text
//! total params memory size = 22296.90MB ... diffusion_model 17316.04MB(RAM)
//! ```
//!
//! 9.43 GB on disk → 17.3 GB resident. Predicting the file size there under-reports
//! by ~8 GB, which is the difference between "tight" and "cannot possibly run".
//!
//! Known limitation: GGUF is modelled as 1× because its blocks are stored in
//! their final form. In practice the engine still re-packs some quantised types
//! on load (a 3.28 GB `Q2_K` text encoder was measured at 4.82 GB resident), so
//! GGUF remains a mild under-estimate. That factor is not modelled here because
//! it depends on engine-internal type promotion rather than anything readable
//! from the file.

use std::fs;
use std::io::Read;
use std::path::Path;

/// Largest safetensors JSON header we will read. Real headers are well under a
/// megabyte (the measured klein checkpoint has a 90,768-byte header); this only
/// exists so a non-safetensors file whose first 8 bytes happen to look like a
/// huge length can't make us allocate wildly.
const MAX_HEADER_BYTES: u64 = 64 * 1024 * 1024;

/// Bytes-in-memory per byte-on-disk for one safetensors dtype.
///
/// Only the fp8 types expand: the engine widens them to f16 on load. Everything
/// else (F32/F16/BF16/integer/bool) is consumed in its stored width.
pub fn dtype_expansion(dtype: &str) -> u64 {
    match dtype {
        "F8_E4M3" | "F8_E5M2" => 2,
        _ => 1,
    }
}

/// In-memory bytes implied by a safetensors JSON header.
///
/// `None` when the header isn't a JSON object or carries no tensors — callers
/// fall back to the file size. `__metadata__` is skipped: it is the only
/// reserved key and holds no tensor.
pub fn memory_bytes_from_header(header_json: &str) -> Option<u64> {
    let parsed: serde_json::Value = serde_json::from_str(header_json).ok()?;
    let obj = parsed.as_object()?;
    let mut total: u64 = 0;
    let mut seen = false;
    for (name, entry) in obj {
        if name == "__metadata__" {
            continue;
        }
        let Some(dtype) = entry.get("dtype").and_then(|d| d.as_str()) else {
            continue;
        };
        let Some(offsets) = entry.get("data_offsets").and_then(|o| o.as_array()) else {
            continue;
        };
        let (Some(start), Some(end)) = (
            offsets.first().and_then(|v| v.as_u64()),
            offsets.get(1).and_then(|v| v.as_u64()),
        ) else {
            continue;
        };
        // A malformed pair (end < start) contributes nothing rather than wrapping.
        let stored = end.saturating_sub(start);
        seen = true;
        total = total.saturating_add(stored.saturating_mul(dtype_expansion(dtype)));
    }
    seen.then_some(total)
}

/// The JSON header of a safetensors file, verbatim.
///
/// `None` for anything that isn't a readable safetensors container: GGUF, a
/// file shorter than its own claimed header, a garbage length prefix, or
/// non-UTF-8 bytes. Callers read that as "this file tells us nothing".
///
/// Shared by `memory_bytes` (which sums tensor payload sizes) and
/// `lora_detect` (which reads tensor names), so the `MAX_HEADER_BYTES`
/// allocation guard exists in exactly one place.
///
/// Note the file is identified by its *contents*, not its extension: weights
/// downloaded from Civitai routinely arrive with no extension at all.
pub fn read_header(path: &Path) -> Option<String> {
    let file_len = fs::metadata(path).ok()?.len();
    let mut file = fs::File::open(path).ok()?;
    let mut prefix = [0u8; 8];
    file.read_exact(&mut prefix).ok()?;
    if &prefix[0..4] == b"GGUF" {
        return None;
    }
    // safetensors: little-endian u64 header length, then that many bytes of JSON.
    let header_len = u64::from_le_bytes(prefix);
    if header_len == 0 || header_len > MAX_HEADER_BYTES || header_len.saturating_add(8) > file_len {
        return None;
    }
    let mut buf = vec![0u8; header_len as usize];
    file.read_exact(&mut buf).ok()?;
    String::from_utf8(buf).ok()
}

/// Bytes this weight file will occupy once loaded by the engine.
///
/// Falls back to the plain file size for GGUF, for unreadable headers, and for
/// anything that isn't recognisably safetensors — so an unknown format is never
/// worse than the previous behaviour. `None` only when the file can't be stat'ed.
pub fn memory_bytes(path: &Path) -> Option<u64> {
    let file_len = fs::metadata(path).ok()?.len();
    let from_header = read_header(path).and_then(|h| memory_bytes_from_header(&h));
    Some(from_header.unwrap_or(file_len))
}

/// Whether this file already stores its weights quantised, i.e. whether it is a
/// GGUF. Unreadable or non-GGUF files answer `false`.
///
/// The distinction matters to the load-time precision ladder: `fit`'s arithmetic
/// starts from an f16 baseline, which a GGUF has already left behind. See
/// `fit::choose_weight_type`.
pub fn is_quantized(path: &Path) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).is_ok() && &magic == b"GGUF"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Build a safetensors file: 8-byte LE header length, JSON header, payload.
    fn write_safetensors(path: &Path, header: &str, payload_len: usize) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        let mut f = fs::File::create(path).unwrap();
        f.write_all(&(header.len() as u64).to_le_bytes()).unwrap();
        f.write_all(header.as_bytes()).unwrap();
        f.write_all(&vec![0u8; payload_len]).unwrap();
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("muchai-weights-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn fp8_dtypes_double_everything_else_is_unchanged() {
        assert_eq!(dtype_expansion("F8_E4M3"), 2);
        assert_eq!(dtype_expansion("F8_E5M2"), 2);
        for d in ["F32", "F16", "BF16", "I64", "U8", "BOOL", "F64"] {
            assert_eq!(dtype_expansion(d), 1, "{d} must not expand");
        }
    }

    #[test]
    fn unknown_dtype_is_treated_as_unexpanded() {
        // Forward compatibility: a dtype this build has never heard of is counted
        // at its stored width rather than skipped, so it still contributes.
        assert_eq!(dtype_expansion("FP4_SOMETHING"), 1);
        let header = r#"{"a":{"dtype":"FP4_SOMETHING","shape":[8],"data_offsets":[0,400]}}"#;
        assert_eq!(memory_bytes_from_header(header), Some(400));
    }

    #[test]
    fn header_sums_stored_bytes_with_per_dtype_expansion() {
        // 1000 bytes of fp8 → 2000 resident; 500 bytes of bf16 → 500.
        let header = r#"{
            "w":{"dtype":"F8_E4M3","shape":[10,100],"data_offsets":[0,1000]},
            "n":{"dtype":"BF16","shape":[500],"data_offsets":[1000,1500]}
        }"#;
        assert_eq!(memory_bytes_from_header(header), Some(2500));
    }

    #[test]
    fn header_ignores_metadata_key() {
        // __metadata__ is a string map, not a tensor — counting it would panic
        // on the missing data_offsets or inflate the total.
        let header = r#"{
            "__metadata__":{"format":"pt","prompt":"irrelevant"},
            "w":{"dtype":"F32","shape":[4],"data_offsets":[0,16]}
        }"#;
        assert_eq!(memory_bytes_from_header(header), Some(16));
    }

    #[test]
    fn header_without_tensors_is_none() {
        // Metadata only → no basis for an estimate, caller uses the file size.
        assert_eq!(memory_bytes_from_header(r#"{"__metadata__":{"a":"b"}}"#), None);
        assert_eq!(memory_bytes_from_header("[1,2,3]"), None);
        assert_eq!(memory_bytes_from_header("not json"), None);
    }

    #[test]
    fn header_skips_malformed_entries_but_keeps_good_ones() {
        let header = r#"{
            "ok":{"dtype":"F16","shape":[2],"data_offsets":[0,4]},
            "no_dtype":{"shape":[2],"data_offsets":[4,8]},
            "no_offsets":{"dtype":"F16","shape":[2]},
            "reversed":{"dtype":"F16","shape":[2],"data_offsets":[20,8]}
        }"#;
        // Only "ok" contributes 4; "reversed" saturates to 0 instead of wrapping.
        assert_eq!(memory_bytes_from_header(header), Some(4));
    }

    #[test]
    fn fp8_file_reports_double_its_tensor_payload() {
        let dir = tmp("fp8");
        let p = dir.join("model-with-no-extension");
        let header = r#"{"w":{"dtype":"F8_E4M3","shape":[64],"data_offsets":[0,64]}}"#;
        write_safetensors(&p, header, 64);
        // Payload is 64 fp8 bytes → 128 resident, even though the file is larger
        // than that once the header is counted.
        assert_eq!(memory_bytes(&p), Some(128));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn extensionless_file_is_identified_by_contents_not_name() {
        // Civitai downloads land as a bare numeric id with no extension; the
        // estimate must not depend on the filename.
        let dir = tmp("noext");
        let p = dir.join("2973304");
        let header = r#"{"w":{"dtype":"F8_E4M3","shape":[16],"data_offsets":[0,32]}}"#;
        write_safetensors(&p, header, 32);
        assert_eq!(memory_bytes(&p), Some(64));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn gguf_falls_back_to_file_size() {
        let dir = tmp("gguf");
        let p = dir.join("encoder.gguf");
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(b"GGUF").unwrap();
        f.write_all(&[0u8; 996]).unwrap();
        drop(f);
        assert_eq!(memory_bytes(&p), Some(1000));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_a_gguf_counts_as_already_quantised() {
        // The magic decides, not the extension: a `.gguf` name on a safetensors
        // file would otherwise talk the precision ladder out of a real fit.
        let dir = tmp("isquant");
        let g = dir.join("model.gguf");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&g, b"GGUF\0\0\0\0").unwrap();
        assert!(is_quantized(&g));

        let st = dir.join("model.safetensors");
        write_safetensors(&st, r#"{"w":{"dtype":"F16","data_offsets":[0,64]}}"#, 64);
        assert!(!is_quantized(&st));

        // Too short to hold the magic, and absent entirely.
        let tiny = dir.join("tiny.gguf");
        fs::write(&tiny, b"GG").unwrap();
        assert!(!is_quantized(&tiny));
        assert!(!is_quantized(&dir.join("nope.gguf")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_file_falls_back_to_file_size() {
        // An absurd leading u64 must not drive a huge allocation, and a file too
        // short to hold its own claimed header is rejected.
        let dir = tmp("garbage");
        let p = dir.join("junk.safetensors");
        let mut f = fs::File::create(&p).unwrap();
        f.write_all(&u64::MAX.to_le_bytes()).unwrap();
        f.write_all(&[0u8; 92]).unwrap();
        drop(f);
        assert_eq!(memory_bytes(&p), Some(100));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn tiny_file_falls_back_to_file_size() {
        // Shorter than the 8-byte length prefix: read_exact fails, no panic.
        let dir = tmp("tiny");
        let p = dir.join("truncated.safetensors");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(memory_bytes(&p), Some(3));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_is_none() {
        assert_eq!(memory_bytes(Path::new("/nonexistent/muchai/model.safetensors")), None);
    }

    #[test]
    fn read_header_returns_the_json_for_a_safetensors_file() {
        let dir = tmp("readhdr");
        let p = dir.join("x.safetensors");
        let header = r#"{"w":{"dtype":"F16","shape":[2],"data_offsets":[0,4]}}"#;
        write_safetensors(&p, header, 4);
        assert_eq!(read_header(&p).as_deref(), Some(header));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_header_is_none_for_non_safetensors() {
        let dir = tmp("readhdr-none");
        // GGUF magic, a file too short for its 8-byte prefix, and an absurd
        // claimed header length all mean "not a readable safetensors header".
        let gguf = dir.join("a.gguf");
        fs::write(&gguf, b"GGUF\0\0\0\0padding").unwrap();
        let tiny = dir.join("b.safetensors");
        fs::write(&tiny, b"abc").unwrap();
        let garbage = dir.join("c.safetensors");
        let mut f = fs::File::create(&garbage).unwrap();
        f.write_all(&u64::MAX.to_le_bytes()).unwrap();
        f.write_all(&[0u8; 92]).unwrap();
        drop(f);
        for p in [&gguf, &tiny, &garbage] {
            assert_eq!(read_header(p), None, "{}", p.display());
        }
        assert_eq!(read_header(Path::new("/nonexistent/muchai/x.safetensors")), None);
        let _ = fs::remove_dir_all(&dir);
    }
}
