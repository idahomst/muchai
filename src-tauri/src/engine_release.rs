//! What upstream offers: release tags, release JSON, asset selection, and the
//! commit subjects shown in the update card.
//!
//! Follows the split `hf.rs` and `civitai.rs` already use — pure `parse_*`
//! functions unit-tested against saved fixtures, with two thin `ureq` wrappers
//! at the bottom of the file. Every parsing decision is therefore testable
//! without a network.

use serde::{Deserialize, Serialize};

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

/// Token identifying the backend build MuchAI ships. The one thing to change
/// if MuchAI ever ships ROCm instead of Vulkan.
pub const BACKEND_TOKEN: &str = "vulkan";

/// One downloadable file attached to a release.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseAsset {
    pub name: String,
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256 from GitHub's per-asset `digest` field, with the
    /// `sha256:` prefix stripped. GitHub publishing this is why the updater
    /// needs no hardcoded hash, unlike `scripts/fetch-engine.sh`.
    pub sha256: Option<String>,
}

/// A release reduced to what the updater needs: its tag and its one asset.
// Not yet constructed outside its own tests — Task 7's HTTP wrapper builds
// one from `parse_release_json` + `select_asset`.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub struct EngineRelease {
    pub tag: String,
    pub asset: ReleaseAsset,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    size: u64,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

/// Parse a GitHub release object into its tag and assets.
// Not yet called outside this module or its tests — Task 7's HTTP wrapper
// calls it. Keeps `RawRelease`/`RawAsset` reachable too.
#[allow(dead_code)]
pub fn parse_release_json(body: &str) -> Result<(String, Vec<ReleaseAsset>), String> {
    let raw: RawRelease = serde_json::from_str(body)
        .map_err(|_| "Couldn't read the latest release from GitHub (unexpected response).".to_string())?;
    let assets = raw
        .assets
        .into_iter()
        .map(|a| ReleaseAsset {
            name: a.name,
            url: a.browser_download_url,
            size: a.size,
            sha256: a
                .digest
                .as_deref()
                .and_then(|d| d.strip_prefix("sha256:"))
                .map(str::to_ascii_lowercase),
        })
        .collect();
    Ok((raw.tag_name, assets))
}

/// The single Linux/x86_64/<backend> zip in a release.
///
/// Matched by tokens, never by the literal filename: the real name embeds the
/// CI runner's distro version (`…-Ubuntu-24.04-…`) and will change without
/// warning. Zero matches or more than one returns `None` and is reported as
/// "no update" — installing a guessed archive is worse than doing nothing.
// Not yet called outside this module or its tests — Task 7's HTTP wrapper
// calls it. Keeps `BACKEND_TOKEN` reachable too.
#[allow(dead_code)]
pub fn select_asset(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset> {
    let mut it = assets.iter().filter(|a| {
        a.name.contains("Linux")
            && a.name.contains("x86_64")
            && a.name.contains(BACKEND_TOKEN)
            && a.name.ends_with(".zip")
    });
    let first = it.next()?;
    it.next().is_none().then_some(first)
}

/// Conventional-commit prefixes the update card summarises as a count rather
/// than listing (Task 14 filters on `noteworthy`, then reports the remainder as
/// "plus N documentation and maintenance commits" — the "Show all" button
/// toggles truncation of the *noteworthy* list, not these).
/// Anything else — including an unprefixed subject — is listed.
const NOISE_PREFIXES: [&str; 6] = ["docs:", "ci:", "chore:", "style:", "test:", "refactor:"];

/// One upstream commit, as shown in the update card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeEntry {
    pub subject: String,
    /// False only for a recognised noise prefix. An unprefixed subject counts
    /// as noteworthy: hiding a real fix because someone forgot the prefix is
    /// worse than showing one line too many.
    pub noteworthy: bool,
}

/// Whether a commit subject belongs above the fold.
pub fn is_noteworthy(subject: &str) -> bool {
    let lower = subject.trim_start().to_ascii_lowercase();
    !NOISE_PREFIXES.iter().any(|p| lower.starts_with(p))
}

fn entry(message: &str) -> ChangeEntry {
    let subject = message.lines().next().unwrap_or("").trim().to_string();
    ChangeEntry { noteworthy: is_noteworthy(&subject), subject }
}

#[derive(Deserialize)]
struct RawCommitBody {
    message: String,
}

#[derive(Deserialize)]
struct RawCommit {
    commit: RawCommitBody,
}

#[derive(Deserialize)]
struct RawCompare {
    #[serde(default)]
    commits: Vec<RawCommit>,
}

/// Parse `GET /compare/<base>...<head>` into a **newest-first** changelog.
/// GitHub returns `commits` oldest-first, so the headline the release page
/// shows is the last element; reversing here means the card can just take
/// element 0 and every consumer agrees on the order.
// Not yet called outside this module or its tests — Task 7's HTTP wrapper
// calls it. Keeps `ChangeEntry`, `is_noteworthy`, `entry`, and the `Raw*`
// compare types reachable too.
#[allow(dead_code)]
pub fn parse_compare_json(body: &str) -> Result<Vec<ChangeEntry>, String> {
    let raw: RawCompare = serde_json::from_str(body)
        .map_err(|_| "Couldn't read the change list from GitHub (unexpected response).".to_string())?;
    Ok(raw
        .commits
        .iter()
        .rev()
        .map(|c| entry(&c.commit.message))
        // An empty subject carries no information and would render as a blank bullet.
        .filter(|e| !e.subject.is_empty())
        .collect())
}

/// Parse `GET /commits/<sha>` — the fallback when `/compare` 404s because
/// GitHub does not know the revision the user is running (a self-compiled
/// `Custom` engine). Yields the headline alone; showing no list is better than
/// showing a wrong one.
// Not yet called outside this module or its tests — Task 7's HTTP wrapper
// calls it.
#[allow(dead_code)]
pub fn parse_commit_json(body: &str) -> Result<ChangeEntry, String> {
    let raw: RawCommit = serde_json::from_str(body)
        .map_err(|_| "Couldn't read the change list from GitHub (unexpected response).".to_string())?;
    Ok(entry(&raw.commit.message))
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

    fn release_fixture() -> &'static str {
        include_str!("../fixtures/gh-release-latest.json")
    }

    fn asset(name: &str) -> ReleaseAsset {
        ReleaseAsset {
            name: name.to_string(),
            url: format!("https://example.invalid/{name}"),
            size: 1,
            sha256: None,
        }
    }

    #[test]
    fn parses_the_release_fixture() {
        let (tag, assets) = parse_release_json(release_fixture()).unwrap();
        assert_eq!(tag, "master-797-5ef4a75");
        assert_eq!(assets.len(), 9);
    }

    #[test]
    fn selects_the_linux_vulkan_asset_and_its_digest() {
        let (_, assets) = parse_release_json(release_fixture()).unwrap();
        let a = select_asset(&assets).expect("one Linux/x86_64/vulkan zip");
        assert_eq!(a.name, "sd-master-5ef4a75-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip");
        assert_eq!(a.size, 45020326);
        assert_eq!(
            a.sha256.as_deref(),
            Some("d365b1ffe73d6a4ece7367e6d7e0368fde53b8b18d508e3100bb5141c0cf26de"),
            "the sha256: prefix must be stripped"
        );
        assert!(a.url.ends_with("-x86_64-vulkan.zip"));
    }

    #[test]
    fn each_selection_clause_rejects_a_near_miss() {
        // One name per clause: matches every clause but the one named. Without
        // these, `Linux` and `x86_64` mask each other in the real-Windows-asset
        // test below and neither is exercised.
        for (why, name) in [
            ("not Linux", "sd-master-x-bin-win-vulkan-x86_64.zip"),
            ("not x86_64", "sd-master-x-bin-Linux-Ubuntu-24.04-aarch64-vulkan.zip"),
            ("not vulkan", "sd-master-x-bin-Linux-Ubuntu-24.04-x86_64-rocm-7.14.0.zip"),
            ("not a zip", "sd-master-x-bin-Linux-Ubuntu-24.04-x86_64-vulkan.tar.gz"),
        ] {
            assert!(select_asset(&[asset(name)]).is_none(), "{why}: {name}");
        }
    }

    #[test]
    fn windows_vulkan_asset_is_not_mistaken_for_the_linux_one() {
        // `sd-…-bin-win-vulkan-x64.zip` contains "vulkan" but lacks both
        // "Linux" and "x86_64" ("x64", not "x86_64"); the per-clause cases
        // live in `each_selection_clause_rejects_a_near_miss` above.
        let assets = vec![asset("sd-master-5ef4a75-bin-win-vulkan-x64.zip")];
        assert!(select_asset(&assets).is_none());
    }

    #[test]
    fn zero_matches_is_no_update() {
        assert!(select_asset(&[]).is_none());
        assert!(select_asset(&[asset("sd-bin-Linux-x86_64-rocm-7.14.0.zip")]).is_none());
    }

    #[test]
    fn two_matches_is_no_update_rather_than_a_guess() {
        let assets = vec![
            asset("sd-master-x-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip"),
            asset("sd-master-x-bin-Linux-Ubuntu-26.04-x86_64-vulkan.zip"),
        ];
        assert!(select_asset(&assets).is_none(), "ambiguity must not be resolved by guessing");
    }

    #[test]
    fn a_future_ubuntu_bump_still_matches() {
        let assets = vec![asset("sd-master-x-bin-Linux-Ubuntu-26.04-x86_64-vulkan.zip")];
        assert!(select_asset(&assets).is_some(), "selection must not depend on the distro version");
    }

    #[test]
    fn an_asset_without_a_digest_parses_with_none() {
        let json = r#"{"tag_name":"master-1-aaaaaaa","assets":[{"name":"x.zip","size":2,"browser_download_url":"https://e.invalid/x.zip"}]}"#;
        let (_, assets) = parse_release_json(json).unwrap();
        assert_eq!(assets[0].sha256, None);
    }

    #[test]
    fn an_uppercase_digest_is_lowercased() {
        // GitHub does not guarantee casing; the release fixture happens to be
        // lowercase already, so this is the only test that would catch a
        // dropped `to_ascii_lowercase`.
        let json = r#"{"tag_name":"master-1-aaaaaaa","assets":[{"name":"x.zip","size":2,"browser_download_url":"https://e.invalid/x.zip","digest":"sha256:D365B1FF"}]}"#;
        let (_, assets) = parse_release_json(json).unwrap();
        assert_eq!(assets[0].sha256.as_deref(), Some("d365b1ff"));
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse_release_json("{ nope ]").is_err());
    }

    fn compare_fixture() -> &'static str {
        include_str!("../fixtures/gh-compare.json")
    }

    #[test]
    fn parses_the_compare_fixture_newest_first() {
        let log = parse_compare_json(compare_fixture()).unwrap();
        assert_eq!(log.len(), 15);
        assert_eq!(
            log[0].subject,
            "feat: expose IP-Adapter in server request schema and capabilities (#1824)",
            "GitHub returns oldest-first; the headline is the last element"
        );
        assert_eq!(log[14].subject, "feat: add hunyuan video 1.5 support (#1795)");
    }

    #[test]
    fn counts_noteworthy_and_noise_in_the_real_range() {
        let log = parse_compare_json(compare_fixture()).unwrap();
        let noteworthy = log.iter().filter(|c| c.noteworthy).count();
        assert_eq!(noteworthy, 11, "7 fix: + 4 feat:");
        assert_eq!(log.len() - noteworthy, 4, "2 docs: + 1 ci: + 1 chore:");
    }

    #[test]
    fn classifies_conventional_prefixes() {
        assert!(is_noteworthy("fix: correct dangling pointer (#1813)"));
        assert!(is_noteworthy("feat: add Mage-Flow support (#1808)"));
        assert!(is_noteworthy("perf: halve VAE decode time"));
        assert!(!is_noteworthy("docs: fix links to sd 1.5 (#1798)"));
        assert!(!is_noteworthy("ci: update ROCm releases to 7.14.0 (#1802)"));
        assert!(!is_noteworthy("chore: add missing override declarations (#1800)"));
        assert!(!is_noteworthy("refactor: split the sampler table"));
        assert!(!is_noteworthy("test: cover the Qwen3-VL path"));
        assert!(!is_noteworthy("style: clang-format the tree"));
        assert!(!is_noteworthy("DOCS: shout"), "casing must not defeat the noise filter");
        assert!(!is_noteworthy("  docs: leading whitespace"), "leading space must not defeat it either");
    }

    #[test]
    fn an_unprefixed_subject_is_noteworthy() {
        // Showing too much is the safe direction; silently hiding a real fix
        // because someone forgot the prefix is not.
        assert!(is_noteworthy("Fix the thing that was broken"));
        assert!(is_noteworthy("Merge pull request #1830 from foo/bar"));
    }

    #[test]
    fn only_the_first_line_of_a_message_is_used() {
        let json = r#"{"commits":[{"commit":{"message":"fix: one thing\n\nA long body that must not appear in the card."}}]}"#;
        let log = parse_compare_json(json).unwrap();
        assert_eq!(log[0].subject, "fix: one thing");
    }

    #[test]
    fn trailing_whitespace_on_the_first_line_is_trimmed() {
        // `str::lines()` already strips a `\r` that precedes a `\n`, so a
        // `\r\n`-terminated first line does not exercise `entry`'s `.trim()`.
        // Plain trailing spaces before the newline do: without `.trim()` this
        // would carry a trailing `"   "` into the card.
        let json = r#"{"commits":[{"commit":{"message":"fix: one thing   \n\nbody"}}]}"#;
        let log = parse_compare_json(json).unwrap();
        assert_eq!(log[0].subject, "fix: one thing");
    }

    #[test]
    fn an_empty_compare_yields_an_empty_log() {
        assert_eq!(parse_compare_json(r#"{"commits":[]}"#).unwrap(), vec![]);
    }

    #[test]
    fn a_commit_with_an_empty_subject_is_dropped() {
        // An empty subject carries no information and would render as a blank
        // bullet in the update card; it must not survive the parse.
        let json = r#"{"commits":[{"commit":{"message":""}},{"commit":{"message":"fix: real change"}}]}"#;
        let log = parse_compare_json(json).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].subject, "fix: real change");
    }

    #[test]
    fn parses_a_single_commit_object() {
        // The /commits/<sha> fallback, used when /compare 404s on a rev GitHub
        // does not know (a self-compiled Custom engine).
        let json = r#"{"commit":{"message":"feat: expose IP-Adapter (#1824)\n\nbody"}}"#;
        assert_eq!(
            parse_commit_json(json).unwrap().subject,
            "feat: expose IP-Adapter (#1824)"
        );
    }

    #[test]
    fn malformed_compare_json_is_an_error() {
        assert!(parse_compare_json("nope").is_err());
        assert!(parse_commit_json("nope").is_err());
        // A body with no `commits` key at all (e.g. `{"message":"Not Found"}`)
        // deliberately parses to `Ok([])`: Task 7's HTTP wrapper returns `Err`
        // on a 404 status before this function ever sees the body, and an
        // empty list only ever renders as no bullets, never a wrong one.
        assert!(parse_compare_json(r#"{"commits":{}}"#).is_err(), "a wrong-typed commits field is an error");
    }
}
