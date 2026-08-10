//! Pixel dimensions of an image, read from its header — never by decoding it.
//!
//! Only the reference-image picker needs this, and it needs one number pair,
//! not pixels. Decoding a user-supplied file to learn its width is how image
//! handling acquires a CVE list; parsing 30 bytes of header is not.
//!
//! Every read is bounds-checked through `get(..)?`, and only the first
//! `MAX_HEADER` bytes are ever consulted, so a 400 MB TIFF costs the same as a
//! 4 KB PNG. Unsupported or malformed input is `None`, never a panic —
//! `never_panics_on_a_truncated_header` walks every prefix of every fixture.

/// Bytes of prefix worth searching. A JPEG's first SOF marker lives well
/// inside this on anything a camera or an editor produces; a file that hides
/// it beyond 1 MB is one we decline rather than one we chase.
const MAX_HEADER: usize = 1024 * 1024;

/// Pixel dimensions of `bytes`, or `None` if it is not a PNG/JPEG/WebP or the
/// header is malformed or truncated.
pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    let b = &bytes[..bytes.len().min(MAX_HEADER)];
    png(b).or_else(|| jpeg(b)).or_else(|| webp(b))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn le32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 2)?;
    Some(u16::from_be_bytes([s[0], s[1]]) as u32)
}

fn le24(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 3)?;
    Some(u32::from(s[0]) | (u32::from(s[1]) << 8) | (u32::from(s[2]) << 16))
}

/// PNG: 8-byte signature, then the IHDR chunk whose width/height are the two
/// big-endian u32s at offsets 16 and 20. IHDR is required to be first.
fn png(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..8)? != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    if b.get(12..16)? != b"IHDR" {
        return None;
    }
    let (w, h) = (be32(b, 16)?, be32(b, 20)?);
    (w > 0 && h > 0).then_some((w, h))
}

/// JPEG: SOI, then a chain of `FF <marker> <u16 length>` segments. Height and
/// width are the two u16s at +5 and +7 of the first Start-Of-Frame marker.
///
/// C4 (Huffman tables), C8 (JPEG extensions) and CC (arithmetic conditioning)
/// share the C0..CF range without being frame headers, so they are skipped
/// like any other segment.
fn jpeg(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut i = 2usize;
    loop {
        // Fill bytes: any number of 0xFF may pad before a marker.
        while *b.get(i)? == 0xFF && *b.get(i + 1)? == 0xFF {
            i += 1;
        }
        if *b.get(i)? != 0xFF {
            return None; // desynchronised — not a marker boundary
        }
        let marker = *b.get(i + 1)?;
        // Standalone markers carry no length payload.
        if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        if marker == 0xD9 || marker == 0xDA {
            return None; // end of image / start of scan — no SOF was found
        }
        let len = be16(b, i + 2)? as usize;
        if len < 2 {
            return None; // a segment shorter than its own length field
        }
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let h = be16(b, i + 5)?;
            let w = be16(b, i + 7)?;
            return (w > 0 && h > 0).then_some((w, h));
        }
        // `len >= 2` above is what guarantees progress: i strictly increases,
        // so a marker chain that loops forever cannot hang the caller.
        i += 2 + len;
    }
}

/// WebP: `RIFF <u32 size> WEBP` then a sub-chunk that says which flavour it is.
///
/// - `VP8 ` (lossy): 14-bit width/height as little-endian u16s at +26/+28,
///   masked to drop the 2-bit scale field.
/// - `VP8L` (lossless): 14-bit width-1/height-1 packed into a 32-bit LE word
///   at +21.
/// - `VP8X` (extended): 24-bit width-1/height-1 at +24 and +27.
fn webp(b: &[u8]) -> Option<(u32, u32)> {
    if b.get(..4)? != b"RIFF" || b.get(8..12)? != b"WEBP" {
        return None;
    }
    match b.get(12..16)? {
        b"VP8 " => {
            let le16 = |at: usize| -> Option<u32> {
                let s = b.get(at..at + 2)?;
                Some(u16::from_le_bytes([s[0], s[1]]) as u32)
            };
            let w = le16(26)? & 0x3FFF;
            let h = le16(28)? & 0x3FFF;
            (w > 0 && h > 0).then_some((w, h))
        }
        b"VP8L" => {
            if *b.get(20)? != 0x2F {
                return None; // lossless signature byte
            }
            let bits = le32(b, 21)?;
            let w = (bits & 0x3FFF) + 1;
            let h = ((bits >> 14) & 0x3FFF) + 1;
            Some((w, h))
        }
        b"VP8X" => {
            let w = le24(b, 24)? + 1;
            let h = le24(b, 27)? + 1;
            Some((w, h))
        }
        _ => None,
    }
}

/// Target pixel count for an edit. Qwen-Image-Edit is trained around 1 MP;
/// meaningfully above it costs VRAM and coherence, below it costs detail.
const TARGET_PIXELS: f64 = 1_048_576.0;

/// Output size for editing a `src_w` × `src_h` reference: the source's aspect
/// ratio, scaled to about one megapixel, snapped to the multiple of 16 the
/// engine's latent grid wants, and clamped to a sane range.
///
/// Clamping happens *after* rounding and independently per axis, so an extreme
/// aspect ratio (a 10000×3 panorama) yields a legal 2048×256 rather than a
/// height of zero. That distorts the aspect ratio — correctly: a zero-height
/// image is not a compromise, it is a failed run.
///
/// A degenerate source (a zero dimension, which `dimensions` never returns but
/// a caller could still pass) falls back to 1024×1024.
pub fn suggest_size(src_w: u32, src_h: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (1024, 1024);
    }
    let scale = (TARGET_PIXELS / (f64::from(src_w) * f64::from(src_h))).sqrt();
    let snap = |v: u32| -> u32 {
        let scaled = (f64::from(v) * scale / 16.0).round() * 16.0;
        (scaled as u32).clamp(256, 2048)
    };
    (snap(src_w), snap(src_h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Vec<u8> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/refimg/");
        std::fs::read(format!("{path}{name}")).expect("fixture exists")
    }

    #[test]
    fn reads_png() {
        assert_eq!(dimensions(&fixture("png_3000x2000.png")), Some((3000, 2000)));
    }

    #[test]
    fn reads_jpeg() {
        assert_eq!(dimensions(&fixture("jpeg_800x600.jpg")), Some((800, 600)));
    }

    #[test]
    fn reads_all_three_webp_flavours() {
        assert_eq!(dimensions(&fixture("webp_lossy_640x480.webp")), Some((640, 480)));
        assert_eq!(dimensions(&fixture("webp_lossless_320x240.webp")), Some((320, 240)));
        assert_eq!(dimensions(&fixture("webp_extended_1920x1080.webp")), Some((1920, 1080)));
    }

    #[test]
    fn rejects_what_is_not_an_image() {
        assert_eq!(dimensions(&fixture("not_an_image.txt")), None);
        // 20 bytes: signature, IHDR's tag and width, and then the file stops
        // mid-height. A prefix long enough to hold the whole IHDR is *not*
        // rejected — reading it is the point of a header parser.
        assert_eq!(dimensions(&fixture("truncated.png")), None);
        assert_eq!(dimensions(&[]), None);
    }

    #[test]
    fn never_panics_on_a_truncated_header() {
        // Every prefix of every fixture. A slice index anywhere in this module
        // is a panic in the generate path, reachable by any file a user picks.
        for name in [
            "png_3000x2000.png",
            "jpeg_800x600.jpg",
            "webp_lossy_640x480.webp",
            "webp_lossless_320x240.webp",
            "webp_extended_1920x1080.webp",
        ] {
            let bytes = fixture(name);
            for n in 0..bytes.len().min(2048) {
                let _ = dimensions(&bytes[..n]);
            }
        }
    }

    #[test]
    fn a_jpeg_whose_marker_chain_never_ends_terminates() {
        // 0xFF 0xD8 then a segment claiming length 2 forever: a naive walker
        // advances by (len - 2) = 0 and spins.
        let mut evil = vec![0xFF, 0xD8];
        for _ in 0..500 {
            evil.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x02]);
        }
        assert_eq!(dimensions(&evil), None);
    }

    #[test]
    fn suggests_a_megapixel_at_the_source_aspect_ratio() {
        // The spec's worked example.
        assert_eq!(suggest_size(3000, 2000), (1248, 832));
        // Square in, square out, exactly at the budget.
        assert_eq!(suggest_size(1024, 1024), (1024, 1024));
        // Already small: upscaled toward the budget, not left alone.
        assert_eq!(suggest_size(512, 512), (1024, 1024));
    }

    #[test]
    fn every_suggestion_is_a_legal_engine_size() {
        for (w, h) in [
            (3000, 2000), (1024, 1024), (512, 512), (1, 1), (10000, 3),
            (3, 10000), (7680, 4320), (99, 100), (1920, 1080),
        ] {
            let (sw, sh) = suggest_size(w, h);
            assert_eq!(sw % 16, 0, "{w}x{h} → width {sw} is not a multiple of 16");
            assert_eq!(sh % 16, 0, "{w}x{h} → height {sh} is not a multiple of 16");
            assert!((256..=2048).contains(&sw), "{w}x{h} → width {sw} out of range");
            assert!((256..=2048).contains(&sh), "{w}x{h} → height {sh} out of range");
        }
    }

    #[test]
    fn a_degenerate_size_does_not_divide_by_zero() {
        assert_eq!(suggest_size(0, 0), (1024, 1024));
        assert_eq!(suggest_size(1000, 0), (1024, 1024));
    }
}
