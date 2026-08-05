//! The flag-compatibility gate for a newly downloaded engine build.
//!
//! Before a downloaded engine can be selected, its `--help` output is parsed
//! and checked against every flag MuchAI is capable of emitting. A build that
//! has dropped one is refused with a message naming the flag, instead of
//! failing later on every generation with an error the user cannot act on.
//!
//! Everything here is pure string handling, so every case is a unit test.
//!
//! Callers capturing help output must merge stdout and stderr before passing
//! it in. `-h`/`--help` output lands on one or the other depending on the
//! build, and reading only one is a live way to end up with a help string
//! this module cannot parse — see `FlagCheck::Unparseable`.
//!
//! What this cannot do: `--help` tells you which flags *parse*, never which
//! values are *correct*. A build that accepts every flag and emits garbage
//! passes this gate. See the yellow-square note in `scripts/fetch-engine.sh`.

use std::collections::BTreeSet;

/// Every long flag MuchAI can put on an engine command line. Kept in sync with
/// the sources by `required_flags_covers_every_emitted_flag`.
///
/// `--version` is intentionally absent: the engine does not advertise it in
/// `--help`, and it is checked separately by the identity probe.
// Not yet called outside this module or its tests — later tasks (validation
// during install, Tauri commands) wire `missing_flags` in.
#[allow(dead_code)]
pub const REQUIRED_FLAGS: [&str; 22] = [
    "--backend",
    "--cfg-scale",
    "--clip_g",
    "--clip_l",
    "--diffusion-fa",
    "--diffusion-model",
    "--llm",
    "--lora-model-dir",
    "--max-vram",
    "--offload-to-cpu",
    "--prediction",
    "--preview",
    "--preview-interval",
    "--preview-path",
    "--sampling-method",
    "--steps",
    "--stream-layers",
    "--t5xxl",
    "--tensor-type-rules",
    "--vae",
    "--vae-format",
    "--vae-tiling",
];

/// Every long flag the engine's `--help` *declares*. A declaration lives at
/// the minimum indent among all lines whose trimmed form starts with `-`;
/// wrapped description continuation lines sit at a deeper, uniform indent (the
/// description column of the line above) and are excluded by that alone, with
/// no fixed column assumed. Within a declaration line, the description column
/// starts at the first run of two or more spaces, so flags named only inside
/// a description are excluded too — otherwise removing a real flag that some
/// other flag's help text still mentions would slip past the gate.
#[allow(dead_code)]
pub fn parse_help_flags(help: &str) -> BTreeSet<String> {
    let candidates: Vec<&str> = help
        .lines()
        .filter(|line| line.starts_with([' ', '\t']) && line.trim_start().starts_with('-'))
        .collect();

    let Some(min_indent) = candidates
        .iter()
        .map(|line| line.len() - line.trim_start().len())
        .min()
    else {
        return BTreeSet::new();
    };

    let mut out = BTreeSet::new();
    for line in candidates {
        let indent = line.len() - line.trim_start().len();
        if indent != min_indent {
            continue;
        }
        let trimmed = line.trim_start();
        let spec = match trimmed.find("  ") {
            Some(i) => &trimmed[..i],
            None => trimmed,
        };
        for tok in spec.split([' ', ',', '=']) {
            if tok.len() > 2 && tok.starts_with("--") {
                out.insert(tok.to_string());
            }
        }
    }
    out
}

/// The fewest flags a genuine `--help` from this engine can plausibly yield.
/// The real fixture yields 160; 10 is far below any partial-but-real read, so
/// falling under it means the help text could not be parsed at all rather
/// than that the build dropped most of its flags.
const PLAUSIBLE_FLAG_FLOOR: usize = 10;

/// The outcome of checking a candidate build's `--help` against
/// `REQUIRED_FLAGS`.
///
/// `Unparseable` exists because the two failures need different words. A
/// build that genuinely dropped `--tensor-type-rules` should say so; a build
/// whose help we could not read at all would otherwise be reported as
/// missing every flag at once — twenty-two dead ends instead of the one true
/// problem.
#[allow(dead_code)]
#[derive(Debug, PartialEq, Eq)]
pub enum FlagCheck {
    Compatible,
    Missing(Vec<&'static str>),
    /// The help text yielded implausibly few flags, so no conclusion can be
    /// drawn about any individual flag.
    Unparseable { flags_seen: usize },
}

/// Checks a candidate engine build's `--help` output against every flag
/// MuchAI can emit. See `FlagCheck` for how the result distinguishes a
/// dropped flag from help text this module could not make sense of.
#[allow(dead_code)]
pub fn missing_flags(help: &str) -> FlagCheck {
    let have = parse_help_flags(help);
    if have.len() < PLAUSIBLE_FLAG_FLOOR {
        return FlagCheck::Unparseable { flags_seen: have.len() };
    }
    let missing: Vec<&'static str> =
        REQUIRED_FLAGS.iter().copied().filter(|f| !have.contains(*f)).collect();
    if missing.is_empty() {
        FlagCheck::Compatible
    } else {
        FlagCheck::Missing(missing)
    }
}

/// Long-flag string literals in a Rust source file, excluding its test module.
///
/// The test modules contain *negative* assertions naming flags MuchAI
/// deliberately never emits (`assert!(!args.iter().any(|x| x == "--auto-fit"))`).
/// Requiring those would make the gate reject a perfectly good build that
/// dropped a flag we do not use.
///
/// The split marker is the literal boundary `mod tests` is declared at in all
/// three scanned files: a `#[cfg(test)]` line immediately followed by
/// `mod tests`. Splitting on the bare `"#[cfg(test)]"` string instead would
/// silently truncate a file that puts a `#[cfg(test)]` helper *above* its
/// test module (this file does exactly that for `scan_source_flags` itself),
/// and would break on any file whose non-test code merely contains that
/// literal as a string (this file's doc comments do). Asserting the split
/// found the boundary turns a future refactor that moves or renames the test
/// module into a red test instead of a silently under-scanned one.
#[cfg(test)]
fn scan_source_flags(src: &str) -> Vec<String> {
    const TEST_MODULE_MARKER: &str = "\n#[cfg(test)]\nmod tests";
    let mut parts = src.split(TEST_MODULE_MARKER);
    let body = parts.next().unwrap_or(src);
    assert!(
        parts.next().is_some(),
        "scan_source_flags: source did not contain the expected {TEST_MODULE_MARKER:?} boundary"
    );
    let mut out = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("\"--") {
        let after = &rest[i + 1..];
        let Some(end) = after.find('"') else { break };
        let flag = &after[..end];
        if flag.len() > 2
            && flag[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            out.push(flag.to_string());
        }
        rest = &after[(end + 1).min(after.len())..];
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static str {
        include_str!("../fixtures/sd-help.txt")
    }

    #[test]
    fn parses_the_shipped_help_text() {
        let flags = parse_help_flags(fixture());
        assert!(flags.len() > 100, "expected the full engine flag set, got {}", flags.len());
        assert!(flags.contains("--tensor-type-rules"));
        assert!(flags.contains("--preview-path"));
        assert!(flags.contains("--vae-tiling"));
    }

    #[test]
    fn ignores_flags_mentioned_only_in_a_description() {
        // `--taesd-preview-only`'s real description ends "(for use with
        // --preview tae)". A description mention must not count as a
        // declaration, or dropping a real flag that another flag's help text
        // still names would slip past the gate.
        let help = "  --taesd-preview-only          prevents taesd decode (for use with --nonsense tae)\n";
        let flags = parse_help_flags(help);
        assert!(flags.contains("--taesd-preview-only"));
        assert!(!flags.contains("--nonsense"));
    }

    #[test]
    fn parses_short_and_long_pairs() {
        let flags = parse_help_flags("  -o, --output <string>         path to write result image to\n");
        assert!(flags.contains("--output"));
    }

    #[test]
    fn ignores_a_wrapped_description_continuation_line() {
        // A real wrapped fragment: the description column of `--foo` wraps
        // onto its own line, and that fragment happens to start with a bare
        // flag name. It sits deeper than `--foo`'s declaration indent, so it
        // must not be counted as a declaration of `--bar`.
        let help = "  --foo <string>   does a thing, see\n        --bar for details\n";
        let flags = parse_help_flags(help);
        assert!(flags.contains("--foo"));
        assert!(!flags.contains("--bar"));
    }

    #[test]
    fn shipped_engine_passes_the_gate() {
        assert_eq!(missing_flags(fixture()), FlagCheck::Compatible);
    }

    #[test]
    fn doctored_help_without_tensor_type_rules_is_rejected() {
        let doctored: String = fixture()
            .lines()
            .filter(|l| !l.trim_start().starts_with("--tensor-type-rules"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(missing_flags(&doctored), FlagCheck::Missing(vec!["--tensor-type-rules"]));
    }

    #[test]
    fn unreadable_help_is_reported_as_unparseable_not_missing_everything() {
        assert_eq!(missing_flags(""), FlagCheck::Unparseable { flags_seen: 0 });
    }

    #[test]
    fn dropping_a_flag_still_named_only_in_another_flags_description_is_caught() {
        // The reviewer's scenario: delete the genuine `--max-vram <string>`
        // declaration, and rewrite the one description fragment that used to
        // mention it (on `--stream-layers`, wrapped onto its own line) so the
        // fragment no longer starts with a bare `--max-vram`. Before Fix 1,
        // the wrapped fragment `--max-vram; defaults to false)` was itself
        // miscounted as a *declaration* of `--max-vram`, so deleting the real
        // declaration didn't matter — the gate still saw `--max-vram` as
        // present and accepted the build. After Fix 1, continuation lines
        // are never counted, so with the real declaration gone the gate must
        // reject.
        let doctored: String = fixture()
            .lines()
            .map(|l| {
                if l.trim_start().starts_with("--max-vram <string>") {
                    return String::new();
                }
                l.replace("--max-vram; defaults to false)", "--max-vram defaults to false)")
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert_eq!(missing_flags(&doctored), FlagCheck::Missing(vec!["--max-vram"]));
    }

    /// Drift guard one: add a flag to the command builder without registering
    /// it here and the build goes red.
    #[test]
    fn required_flags_covers_every_emitted_flag() {
        let sources = [
            include_str!("command_builder.rs"),
            include_str!("devices.rs"),
            // Contributes no flags today; scanned defensively because it's
            // the other place an engine command line could grow one.
            include_str!("engine.rs"),
        ];
        // `--version` never appears in `--help`; the identity probe covers it.
        const EXEMPT: [&str; 1] = ["--version"];
        let mut unregistered = Vec::new();
        for src in sources {
            for flag in scan_source_flags(src) {
                if !REQUIRED_FLAGS.contains(&flag.as_str()) && !EXEMPT.contains(&flag.as_str()) {
                    unregistered.push(flag);
                }
            }
        }
        unregistered.sort();
        unregistered.dedup();
        assert!(
            unregistered.is_empty(),
            "these flags are emitted but not in REQUIRED_FLAGS: {unregistered:?}"
        );
    }

    /// Drift guard two: prove the gate accepts our own shipped engine rather
    /// than rejecting everything because a name was typed wrong.
    #[test]
    fn required_flags_is_a_subset_of_the_shipped_help() {
        let have = parse_help_flags(fixture());
        let absent: Vec<_> = REQUIRED_FLAGS.iter().filter(|f| !have.contains(**f)).collect();
        assert!(absent.is_empty(), "REQUIRED_FLAGS not accepted by the shipped engine: {absent:?}");
    }

    #[test]
    fn scan_source_flags_ignores_the_test_module() {
        let src = "fn f() { push(\"--real\"); }\n#[cfg(test)]\nmod tests { fn t() { assert_ne!(x, \"--never-emitted\"); } }\n";
        assert_eq!(scan_source_flags(src), vec!["--real".to_string()]);
    }
}
