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

/// The files to remove for a gallery image: the image itself, plus its sibling
/// `.json` sidecar when one exists. Image is always first.
fn deletion_targets(image_path: &Path) -> Vec<PathBuf> {
    let mut targets = vec![image_path.to_path_buf()];
    let sidecar = image_path.with_extension("json");
    if sidecar.exists() {
        targets.push(sidecar);
    }
    targets
}

/// Move an image (and its sidecar, if any) to the OS trash. Recoverable from the
/// system file manager. Errors if the image itself cannot be trashed.
pub fn delete_to_trash(image_path: &Path) -> Result<(), String> {
    for target in deletion_targets(image_path) {
        trash::delete(&target).map_err(|e| e.to_string())?;
    }
    Ok(())
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
            batch_id: id.into(),
            batch_index: 0,
            batch_size: 1,
        }
    }

    #[test]
    fn writes_sidecar_and_lists_newest_first() {
        let dir = std::env::temp_dir().join(format!("muchai-gal-{}", std::process::id()));
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
    fn legacy_sidecar_without_batch_fields_loads_as_singleton() {
        // A sidecar written before batch fields existed has only the original
        // four keys. It must still deserialize, with the new fields defaulted.
        let it = item("legacy", 100);
        let mut v = serde_json::to_value(&it).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("batch_id");
        obj.remove("batch_index");
        obj.remove("batch_size");

        let back: GalleryItem = serde_json::from_value(v).unwrap();
        assert_eq!(back.batch_id, "");
        assert_eq!(back.batch_index, 0);
        assert_eq!(back.batch_size, 0); // consumers normalize 0 -> 1
    }

    #[test]
    fn deletion_targets_includes_sidecar_only_when_present() {
        let dir = std::env::temp_dir().join(format!("muchai-del-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("pic.png");
        std::fs::write(&img, b"png").unwrap();

        // No sidecar yet -> just the image.
        assert_eq!(deletion_targets(&img), vec![img.clone()]);

        // With a sidecar -> both, image first.
        let side = dir.join("pic.json");
        std::fs::write(&side, b"{}").unwrap();
        assert_eq!(deletion_targets(&img), vec![img.clone(), side.clone()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_to_trash_removes_image_and_sidecar() {
        let dir = std::env::temp_dir().join(format!("muchai-trash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("gone.png");
        let side = dir.join("gone.json");
        std::fs::write(&img, b"png").unwrap();
        std::fs::write(&side, b"{}").unwrap();

        delete_to_trash(&img).unwrap();

        assert!(!img.exists(), "image should be gone from the gallery dir");
        assert!(!side.exists(), "sidecar should be gone from the gallery dir");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_corrupt_sidecar() {
        let dir = std::env::temp_dir().join(format!("muchai-gal-corrupt-{}", std::process::id()));
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
