//! What upstream offers: release tags, release JSON, asset selection, and the
//! commit subjects shown in the update card.
//!
//! Follows the split `hf.rs` and `civitai.rs` already use — pure `parse_*`
//! functions unit-tested against saved fixtures, with two thin `ureq` wrappers
//! at the bottom of the file. Every parsing decision is therefore testable
//! without a network.

/// Tag of the engine bundled in this MuchAI build. Kept in sync with
/// `ENGINE_TAG` in `scripts/fetch-engine.sh` by `builtin_tag_matches_fetch_script`.
///
/// The engine's `--version` banner reports only the commit, never the build
/// number, so "is the release newer than what I am running?" cannot be answered
/// from the binary alone. Without this constant the app would believe the
/// built-in engine is build 0 and offer an update it already has.
// Not yet read outside this module or its tests — later tasks (release
// comparison wiring, Tauri commands) consume it.
#[allow(dead_code)]
pub const BUILTIN_ENGINE_TAG: &str = "master-782-b290693";

/// A parsed `master-<build>-<sha>` release tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineTag {
    /// Monotonic upstream build number — the only reliable ordering key.
    pub build: u32,
    /// Short commit hash, as the `--version` banner reports it.
    pub sha: String,
}

/// Parse `master-782-b290693`. `None` on anything else: an unrecognised tag
/// must mean "no update offered", never "assume newer".
pub fn parse_tag(tag: &str) -> Option<EngineTag> {
    let rest = tag.strip_prefix("master-")?;
    let (build, sha) = rest.split_once('-')?;
    // `u32::from_str` accepts a leading `+` (e.g. "+782"), but the tag string
    // becomes a directory name under the engines root, and the directory-name
    // guard there rejects `+`. Accepting it here would let a tag parse,
    // compare as newer, and download successfully, only to be refused at
    // selection time and silently fall back to the built-in engine.
    if build.is_empty() || !build.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let build: u32 = build.parse().ok()?;
    if sha.is_empty() || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(EngineTag { build, sha: sha.to_string() })
}

/// True only when `candidate` is a strictly greater build than `running`.
/// Either tag failing to parse yields false — the safe direction.
// Not yet called outside this module or its tests — later tasks (background
// update check, Tauri commands) call it.
#[allow(dead_code)]
pub fn is_newer(candidate: &str, running: &str) -> bool {
    match (parse_tag(candidate), parse_tag(running)) {
        (Some(c), Some(r)) => c.build > r.build,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_release_tag() {
        let t = parse_tag("master-782-b290693").unwrap();
        assert_eq!(t.build, 782);
        assert_eq!(t.sha, "b290693");
    }

    #[test]
    fn rejects_malformed_tags() {
        for bad in [
            "",
            "master",
            "master-",
            "master-782",
            "master-782-",
            "v1.2.3",
            "master-abc-b290693",
            "master-782-zzz!",
            "master-+782-b290693",
        ] {
            assert!(parse_tag(bad).is_none(), "{bad} should not parse");
        }
    }

    #[test]
    fn newer_build_number_wins() {
        assert!(is_newer("master-797-5ef4a75", "master-782-b290693"));
        assert!(!is_newer("master-782-b290693", "master-797-5ef4a75"));
        assert!(!is_newer("master-782-b290693", "master-782-b290693"), "equal is not newer");
    }

    #[test]
    fn an_unparseable_tag_never_offers_an_update() {
        // The safe direction: if we cannot tell, we do not offer.
        assert!(!is_newer("nightly", "master-782-b290693"));
        assert!(!is_newer("master-797-5ef4a75", "some-local-build"));
    }

    /// `BUILTIN_ENGINE_TAG` duplicates a value that really lives in a shell
    /// script Rust cannot read at runtime. Bumping the pin without bumping the
    /// constant would make the app offer an update it already has.
    #[test]
    fn builtin_tag_matches_fetch_script() {
        let script = include_str!("../../scripts/fetch-engine.sh");
        let assignments: Vec<&str> =
            script.lines().filter_map(|l| l.trim().strip_prefix("ENGINE_TAG=")).collect();
        assert_eq!(
            assignments.len(),
            1,
            "expected exactly one ENGINE_TAG= assignment in scripts/fetch-engine.sh — \
             with more than one, bash's last-assignment-wins semantics make it ambiguous \
             which value is actually effective, found {assignments:?}"
        );
        let want = assignments[0].trim().trim_matches('"');
        assert_eq!(
            BUILTIN_ENGINE_TAG, want,
            "BUILTIN_ENGINE_TAG is stale — bump it whenever ENGINE_TAG in scripts/fetch-engine.sh changes"
        );
    }

    #[test]
    fn the_builtin_tag_itself_parses() {
        assert!(parse_tag(BUILTIN_ENGINE_TAG).is_some());
    }
}
