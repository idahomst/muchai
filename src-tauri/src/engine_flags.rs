//! The flag-compatibility gate for a newly downloaded engine build.
//!
//! Before a downloaded engine can be selected, its `--help` output is parsed
//! and checked against every flag MuchAI is capable of emitting. A build that
//! has dropped one is refused with a message naming the flag, instead of
//! failing later on every generation with an error the user cannot act on.
//!
//! Everything here is pure string handling, so every case is a unit test.
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

/// Every long flag the engine's `--help` *declares*. Declarations live in the
/// left-hand column of an indented line; the description column starts at the
/// first run of two or more spaces. Flags named inside a description are
/// deliberately not counted — otherwise removing a real flag that some other
/// flag's help text still mentions would slip past the gate.
#[allow(dead_code)]
pub fn parse_help_flags(help: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in help.lines() {
        if !line.starts_with([' ', '\t']) {
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with('-') {
            continue;
        }
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

/// Flags MuchAI needs that this engine build does not declare. Empty means the
/// build is flag-compatible.
#[allow(dead_code)]
pub fn missing_flags(help: &str) -> Vec<&'static str> {
    let have = parse_help_flags(help);
    REQUIRED_FLAGS.iter().copied().filter(|f| !have.contains(*f)).collect()
}

/// Long-flag string literals in a Rust source file, excluding its test module.
///
/// The test modules contain *negative* assertions naming flags MuchAI
/// deliberately never emits (`assert!(!args.iter().any(|x| x == "--auto-fit"))`).
/// Requiring those would make the gate reject a perfectly good build that
/// dropped a flag we do not use.
#[cfg(test)]
fn scan_source_flags(src: &str) -> Vec<String> {
    let body = src.split("#[cfg(test)]").next().unwrap_or(src);
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
    fn shipped_engine_passes_the_gate() {
        assert_eq!(missing_flags(fixture()), Vec::<&str>::new());
    }

    #[test]
    fn doctored_help_without_tensor_type_rules_is_rejected() {
        let doctored: String = fixture()
            .lines()
            .filter(|l| !l.trim_start().starts_with("--tensor-type-rules"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(missing_flags(&doctored), vec!["--tensor-type-rules"]);
    }

    #[test]
    fn empty_help_reports_every_flag_missing() {
        assert_eq!(missing_flags("").len(), REQUIRED_FLAGS.len());
    }

    /// Drift guard one: add a flag to the command builder without registering
    /// it here and the build goes red.
    #[test]
    fn required_flags_covers_every_emitted_flag() {
        let sources = [
            include_str!("command_builder.rs"),
            include_str!("devices.rs"),
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
