use crate::types::GalleryItem;
use std::path::{Path, PathBuf};

/// Write the parameter sidecar JSON next to an image (same stem, ".json").
pub fn write_sidecar(image_path: &Path, item: &GalleryItem) -> std::io::Result<PathBuf> {
    let sidecar = image_path.with_extension("json");
    let s = serde_json::to_string_pretty(item).expect("gallery item serializes");
    std::fs::write(&sidecar, s)?;
    Ok(sidecar)
}

/// List gallery items in a directory by reading every "*.json" sidecar,
/// newest first.
pub fn list_items(dir: &Path) -> Vec<GalleryItem> {
    let mut items: Vec<GalleryItem> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(s) = std::fs::read_to_string(&path) {
                    if let Ok(item) = serde_json::from_str::<GalleryItem>(&s) {
                        items.push(item);
                    }
                }
            }
        }
    }
    items.sort_by(|a, b| b.created_at_unix.cmp(&a.created_at_unix));
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::GenerationRequest;

    fn item(id: &str, ts: u64) -> GalleryItem {
        GalleryItem {
            id: id.into(),
            image_path: format!("/g/{id}.png"),
            request: GenerationRequest::default(),
            created_at_unix: ts,
        }
    }

    #[test]
    fn writes_sidecar_and_lists_newest_first() {
        let dir = std::env::temp_dir().join(format!("fridai-gal-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        write_sidecar(&dir.join("older.png"), &item("older", 100)).unwrap();
        write_sidecar(&dir.join("newer.png"), &item("newer", 200)).unwrap();

        let listed = list_items(&dir);
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "newer"); // newest first
        assert_eq!(listed[1].id, "older");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_corrupt_sidecar() {
        let dir = std::env::temp_dir().join(format!("fridai-gal-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        write_sidecar(&dir.join("good.png"), &item("good", 100)).unwrap();
        // A non-JSON ".json" file must be skipped, never panic the listing.
        std::fs::write(dir.join("bad.json"), b"not valid json {{{").unwrap();

        let listed = list_items(&dir);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, "good");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
