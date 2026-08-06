//! Downloading, verifying and installing an engine release.
//!
//! The store is `~/.local/share/muchai/engines/<tag>/`, holding the extracted
//! archive exactly as `scripts/fetch-engine.sh` lays it out: `sd-cli` plus its
//! sibling `.so` files, ~113 MB.
//!
//! Install is *extract to `.staging-<tag>/` → verify → validate → `rename()`*.
//! Rename is atomic within a filesystem, so a directory named `<tag>` existing
//! is itself proof that a fully-downloaded, hash-verified, flag-checked engine
//! is inside. A crash or kill mid-install can only leave a `.staging-*`
//! directory, which is swept on next start. No partially-installed engine can
//! ever be selected, and there is no validity flag to drift.

use std::path::{Path, PathBuf};

/// Prefix marking an install still in progress. Never a valid tag, because
/// tags start with `master-`.
const STAGING_PREFIX: &str = ".staging-";

/// Where a finished install lives. Its existence is the completion proof.
pub fn install_dir(root: &Path, tag: &str) -> PathBuf {
    root.join(tag)
}

/// Where an install is assembled before the atomic rename.
// Not yet called outside this module or its tests — Task 11's `finish_install`
// and `install_release` call it.
#[allow(dead_code)]
pub fn staging_dir(root: &Path, tag: &str) -> PathBuf {
    root.join(format!("{STAGING_PREFIX}{tag}"))
}

/// Delete every `.staging-*` directory under `root`. Called once at startup: a
/// crash or kill mid-install can only leave one of these behind, and a staging
/// directory is by definition incomplete. Best-effort — a failure here just
/// wastes disk, it cannot break anything.
// Not yet called outside this module or its tests — Task 13's startup sweep
// calls it.
#[allow(dead_code)]
pub fn sweep_staging(root: &Path) {
    let Ok(rd) = std::fs::read_dir(root) else { return };
    for entry in rd.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.starts_with(STAGING_PREFIX) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// Installed engine tags, newest build first. Staging directories and anything
/// whose name is not a parseable tag are ignored.
pub fn installed_tags(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut tagged: Vec<(u32, String)> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let parsed = crate::engine_release::parse_tag(&name)?;
            Some((parsed.build, name))
        })
        .collect();
    tagged.sort_by(|a, b| b.0.cmp(&a.0));
    tagged.into_iter().map(|(_, name)| name).collect()
}

/// Keep the `keep` newest installed engines plus `protect`, delete the rest.
///
/// Two copies at ~113 MB each is a fair tax; an unbounded collection is not.
/// `protect` is the currently-selected tag: normally that is the newest and the
/// argument is redundant, but a user who deliberately went back to an older
/// build must not have it deleted out from under them. Directories that are not
/// parseable tags are never touched — they are not ours.
// Not yet called outside this module or its tests — Task 12's `engine_apply_update`
// command calls it. This allow is also what keeps `install_dir` and
// `installed_tags` (both called below) from being flagged as dead code.
#[allow(dead_code)]
pub fn prune(root: &Path, keep: usize, protect: &str) {
    for (i, tag) in installed_tags(root).into_iter().enumerate() {
        if i < keep || tag == protect {
            continue;
        }
        let _ = std::fs::remove_dir_all(install_dir(root, &tag));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("muchai-inst-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn mkdirs(root: &Path, names: &[&str]) {
        for n in names {
            std::fs::create_dir_all(root.join(n)).unwrap();
        }
    }

    fn entries(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn builds_install_and_staging_paths() {
        let root = Path::new("/data/engines");
        assert_eq!(install_dir(root, "master-797-5ef4a75"), root.join("master-797-5ef4a75"));
        assert_eq!(staging_dir(root, "master-797-5ef4a75"), root.join(".staging-master-797-5ef4a75"));
    }

    #[test]
    fn sweep_removes_only_staging_directories() {
        let root = tmp("sweep");
        mkdirs(&root, &["master-797-5ef4a75", ".staging-master-799-abc1234", ".staging-junk"]);
        std::fs::write(root.join("a-file.txt"), b"x").unwrap();

        sweep_staging(&root);

        assert_eq!(entries(&root), vec!["a-file.txt", "master-797-5ef4a75"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sweep_on_a_missing_root_is_a_noop() {
        sweep_staging(Path::new("/nonexistent/muchai/engines")); // must not panic
    }

    #[test]
    fn prune_keeps_the_highest_build_numbers() {
        let root = tmp("prune");
        mkdirs(&root, &["master-780-aaaaaaa", "master-797-5ef4a75", "master-791-b8bf676"]);

        prune(&root, 2, "master-797-5ef4a75");

        assert_eq!(entries(&root), vec!["master-791-b8bf676", "master-797-5ef4a75"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_never_deletes_the_selected_engine() {
        let root = tmp("prune-protect");
        mkdirs(&root, &["master-780-aaaaaaa", "master-797-5ef4a75", "master-791-b8bf676"]);

        // The user is deliberately running the oldest one.
        prune(&root, 1, "master-780-aaaaaaa");

        assert!(root.join("master-780-aaaaaaa").exists(), "the running engine must survive");
        assert!(root.join("master-797-5ef4a75").exists(), "the newest is the one kept");
        assert!(!root.join("master-791-b8bf676").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_leaves_unrecognised_directories_alone() {
        let root = tmp("prune-unknown");
        mkdirs(&root, &["master-797-5ef4a75", "master-791-b8bf676", "my-own-build"]);

        prune(&root, 1, "master-797-5ef4a75");

        assert_eq!(entries(&root), vec!["master-797-5ef4a75", "my-own-build"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn prune_on_a_missing_root_is_a_noop() {
        prune(Path::new("/nonexistent/muchai/engines"), 2, "master-797-5ef4a75");
    }

    #[test]
    fn lists_installed_tags_newest_first() {
        let root = tmp("list");
        mkdirs(&root, &["master-780-aaaaaaa", "master-797-5ef4a75", ".staging-master-799-abc1234", "junk"]);

        assert_eq!(installed_tags(&root), vec!["master-797-5ef4a75", "master-780-aaaaaaa"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}
