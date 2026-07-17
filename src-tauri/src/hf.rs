//! HuggingFace model discovery: URL classification, tree-API parse, quant-label
//! extraction, and variant grouping. All grouping/parsing logic is pure and
//! unit-tested; only `fetch_tree` performs I/O (thin `ureq` wrapper).

use crate::recipes::{self, ComponentRole};
use serde::{Deserialize, Serialize};

/// A repo coordinate on huggingface.co.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfRepoRef {
    pub org: String,
    pub repo: String,
    pub revision: String,
}

/// What a pasted HuggingFace URL points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HfUrl {
    /// A repo (or tree) URL → enumerate its files.
    Repo(HfRepoRef),
    /// A direct file URL (blob/resolve) → skip enumeration; this IS the file.
    File { repo: HfRepoRef, path: String },
}

/// The last path segment (filename) of a repo-relative path.
pub fn basename(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// Filename without its final extension.
pub fn stem(path: &str) -> String {
    let b = basename(path);
    match b.rfind('.') {
        Some(i) => b[..i].to_string(),
        None => b,
    }
}

/// Classify a pasted string. Pure. `None` when it isn't a huggingface.co URL.
pub fn parse_hf_url(url: &str) -> Option<HfUrl> {
    let rest = url.trim();
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))
        .unwrap_or(rest);
    let rest = rest.strip_prefix("huggingface.co/")?;
    let rest = rest.split(['?', '#']).next().unwrap_or(rest);
    let segs: Vec<&str> = rest.split('/').filter(|s| !s.is_empty()).collect();
    if segs.len() < 2 {
        return None;
    }
    let org = segs[0].to_string();
    let repo = segs[1].to_string();
    let main = || "main".to_string();
    if segs.len() == 2 {
        return Some(HfUrl::Repo(HfRepoRef { org, repo, revision: main() }));
    }
    match segs[2] {
        "tree" => {
            // A tree URL may point at a subdirectory (segs[4..]); we intentionally
            // classify it as the whole repo and enumerate from the root — subtree
            // scoping is not supported. (blob/resolve DO preserve their file path.)
            let revision = segs.get(3).map(|s| s.to_string()).unwrap_or_else(main);
            Some(HfUrl::Repo(HfRepoRef { org, repo, revision }))
        }
        "blob" | "resolve" => {
            let revision = segs.get(3).map(|s| s.to_string()).unwrap_or_else(main);
            let path = segs.get(4..).map(|s| s.join("/")).unwrap_or_default();
            if path.is_empty() {
                Some(HfUrl::Repo(HfRepoRef { org, repo, revision }))
            } else {
                Some(HfUrl::File { repo: HfRepoRef { org, repo, revision }, path })
            }
        }
        _ => Some(HfUrl::Repo(HfRepoRef { org, repo, revision: main() })),
    }
}

/// Absolute download URL for a repo-relative file path.
pub fn resolve_url(repo: &HfRepoRef, path: &str) -> String {
    format!(
        "https://huggingface.co/{}/{}/resolve/{}/{}",
        repo.org, repo.repo, repo.revision, path
    )
}

/// Canonical quant/precision tokens, most-specific first so `q4_k_m` matches
/// before `q4` and `iq4_xs` before `q4`. Returned lowercased for stable display.
/// Substring match, so any token that CONTAINS a shorter token must precede it.
const PRECISION_TOKENS: &[&str] = &[
    // float + bitsandbytes (the safetensors low-VRAM variants this feature targets)
    "fp8_e4m3fn", "fp8_e5m2", "fp32", "bf16", "fp16", "fp8", "nf4", "fp4", "int8", "int4",
    // GGUF importance-matrix (IQ) quants — MUST precede the bare q* tokens below,
    // since e.g. "iq4_xs" contains "q4".
    "iq4_xs", "iq4_nl", "iq3_xxs", "iq3_s", "iq3_m", "iq2_xxs", "iq2_xs", "iq2_s", "iq2_m", "iq1_s", "iq1_m",
    // GGUF K-quants + legacy, most-specific first
    "q8_0", "q6_k", "q5_k_m", "q5_k", "q4_k_m", "q4_k", "q4_0", "q3_k", "q2_k",
    "q8", "q6", "q5", "q4", "q3", "q2",
];

/// Extract a quant/precision label from a filename, or `None` if none is found.
pub fn precision_label(filename: &str) -> Option<String> {
    let lower = filename.to_lowercase();
    PRECISION_TOKENS
        .iter()
        .find(|t| lower.contains(*t))
        .map(|t| t.to_string())
}

/// One file from the HF tree API, size already normalized (lfs.size ?? size).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HfTreeEntry {
    pub path: String,
    pub size_bytes: u64,
}

#[derive(Deserialize)]
struct RawTreeEntry {
    #[serde(rename = "type")]
    kind: String,
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<RawLfs>,
}

#[derive(Deserialize)]
struct RawLfs {
    #[serde(default)]
    size: u64,
}

/// Parse the tree-API JSON array into normalized file entries (dirs dropped).
pub fn parse_tree_json(body: &str) -> Result<Vec<HfTreeEntry>, String> {
    let raw: Vec<RawTreeEntry> = serde_json::from_str(body).map_err(|e| e.to_string())?;
    Ok(raw
        .into_iter()
        .filter(|e| e.kind == "file")
        .map(|e| {
            let size_bytes = match &e.lfs {
                Some(l) if l.size > 0 => l.size,
                _ => e.size,
            };
            HfTreeEntry { path: e.path, size_bytes }
        })
        .collect())
}

/// One selectable diffusion variant within a repo.
#[derive(Debug, Clone, Serialize)]
pub struct HfVariant {
    /// Quant label (e.g. "fp8") or, if none, the filename stem.
    pub label: String,
    /// Detected family (None when no recipe matched the file set).
    pub family: Option<String>,
    /// Repo-relative path of the diffusion file.
    pub path: String,
    pub size_bytes: u64,
}

/// True if `lower` (a lowercased filename) contains any of `patterns`.
fn matches_any(patterns: &[&str], lower: &str) -> bool {
    patterns.iter().any(|p| lower.contains(&p.to_lowercase()))
}

/// Group the tree's `.safetensors` files into the diffusion-variant list.
/// The detected family (via `recipes::detect_best`) determines which files are
/// diffusion (kept) vs companion encoders/VAE (excluded). With no family match,
/// every `.safetensors` is offered as a variant.
pub fn classify_variants(entries: &[HfTreeEntry]) -> Vec<HfVariant> {
    let files: Vec<&HfTreeEntry> = entries
        .iter()
        .filter(|e| e.path.to_lowercase().ends_with(".safetensors"))
        .collect();
    if files.is_empty() {
        return Vec::new();
    }
    let names: Vec<String> = files.iter().map(|e| basename(&e.path)).collect();
    let detected = recipes::detect_best(&names);
    let family = detected.as_ref().map(|(r, _)| r.family.to_string());

    // Split the recipe's role patterns into diffusion vs companion sets.
    let (diffusion_patterns, companion_patterns): (Vec<&str>, Vec<&str>) = match &detected {
        Some((recipe, _)) => {
            let mut diff = Vec::new();
            let mut comp = Vec::new();
            for spec in &recipe.roles {
                if spec.role == ComponentRole::Diffusion {
                    diff.extend(spec.patterns.iter().copied());
                } else {
                    comp.extend(spec.patterns.iter().copied());
                }
            }
            (diff, comp)
        }
        None => (Vec::new(), Vec::new()),
    };

    files
        .iter()
        .filter(|e| {
            let lower = basename(&e.path).to_lowercase();
            match &detected {
                // Family known: diffusion file = matches a diffusion pattern and
                // no companion pattern (so a VAE/encoder is never a "variant").
                Some(_) => matches_any(&diffusion_patterns, &lower) && !matches_any(&companion_patterns, &lower),
                // No family: offer every safetensors.
                None => true,
            }
        })
        .map(|e| HfVariant {
            label: precision_label(&basename(&e.path)).unwrap_or_else(|| stem(&e.path)),
            family: family.clone(),
            path: e.path.clone(),
            size_bytes: e.size_bytes,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_repo_url() {
        let u = parse_hf_url("https://huggingface.co/black-forest-labs/FLUX.1-dev").unwrap();
        assert_eq!(
            u,
            HfUrl::Repo(HfRepoRef {
                org: "black-forest-labs".into(),
                repo: "FLUX.1-dev".into(),
                revision: "main".into(),
            })
        );
    }

    #[test]
    fn parses_tree_url_with_revision() {
        let u = parse_hf_url("https://huggingface.co/org/repo/tree/refs%2Fpr%2F1/sub").unwrap();
        assert_eq!(u, HfUrl::Repo(HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "refs%2Fpr%2F1".into() }));
    }

    #[test]
    fn parses_resolve_file_url() {
        let u = parse_hf_url("https://huggingface.co/org/repo/resolve/main/flux1-dev.safetensors").unwrap();
        assert_eq!(
            u,
            HfUrl::File {
                repo: HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "main".into() },
                path: "flux1-dev.safetensors".into(),
            }
        );
    }

    #[test]
    fn parses_blob_file_url() {
        let u = parse_hf_url("https://huggingface.co/org/repo/blob/main/model.safetensors").unwrap();
        assert_eq!(
            u,
            HfUrl::File {
                repo: HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "main".into() },
                path: "model.safetensors".into(),
            }
        );
    }

    #[test]
    fn strips_query_and_fragment() {
        let u = parse_hf_url("https://huggingface.co/org/repo?download=true#section").unwrap();
        assert_eq!(u, HfUrl::Repo(HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "main".into() }));
    }

    #[test]
    fn rejects_non_hf_url() {
        assert!(parse_hf_url("https://example.com/foo/bar").is_none());
        assert!(parse_hf_url("not a url").is_none());
        assert!(parse_hf_url("https://huggingface.co/onlyorg").is_none());
    }

    #[test]
    fn resolve_url_builds_download_link() {
        let r = HfRepoRef { org: "org".into(), repo: "repo".into(), revision: "main".into() };
        assert_eq!(resolve_url(&r, "a/b.safetensors"), "https://huggingface.co/org/repo/resolve/main/a/b.safetensors");
    }

    #[test]
    fn basename_and_stem() {
        assert_eq!(basename("a/b/c.safetensors"), "c.safetensors");
        assert_eq!(stem("a/b/c.safetensors"), "c");
        assert_eq!(stem("noext"), "noext");
    }

    #[test]
    fn precision_label_extracts_known_tokens() {
        assert_eq!(precision_label("flux1-dev-fp8_e4m3fn.safetensors"), Some("fp8_e4m3fn".into()));
        assert_eq!(precision_label("model-fp16.safetensors"), Some("fp16".into()));
        assert_eq!(precision_label("model-bf16.safetensors"), Some("bf16".into()));
        assert_eq!(precision_label("t5-Q4_K_M.safetensors"), Some("q4_k_m".into()));
        assert_eq!(precision_label("x-q8_0.safetensors"), Some("q8_0".into()));
    }

    #[test]
    fn precision_label_none_when_absent() {
        assert_eq!(precision_label("flux1-dev.safetensors"), None);
    }

    #[test]
    fn precision_label_recognizes_bnb_low_vram_tokens() {
        assert_eq!(precision_label("flux1-dev-bnb-nf4.safetensors"), Some("nf4".into()));
        assert_eq!(precision_label("model-fp4.safetensors"), Some("fp4".into()));
        assert_eq!(precision_label("model-int4.safetensors"), Some("int4".into()));
    }

    #[test]
    fn precision_label_iq_quants_not_mislabeled_as_q() {
        // Regression: "iq4_xs" contains "q4"; the IQ token must win.
        assert_eq!(precision_label("mixtral-IQ4_XS.gguf"), Some("iq4_xs".into()));
        assert_eq!(precision_label("model-IQ2_XXS.gguf"), Some("iq2_xxs".into()));
    }

    #[test]
    fn parse_tree_json_normalizes_lfs_size_and_drops_dirs() {
        let body = include_str!("../fixtures/hf-tree-flux1.json");
        let entries = parse_tree_json(body).unwrap();
        // Directory dropped; 4 files kept.
        assert_eq!(entries.len(), 4);
        let diff = entries.iter().find(|e| e.path == "flux1-dev.safetensors").unwrap();
        assert_eq!(diff.size_bytes, 23802932552); // from lfs.size, not the 52-byte pointer
        let idx = entries.iter().find(|e| e.path == "model_index.json").unwrap();
        assert_eq!(idx.size_bytes, 320); // no lfs → plain size
    }

    #[test]
    fn classify_variants_lists_diffusion_files_only() {
        let body = include_str!("../fixtures/hf-tree-flux1.json");
        let entries = parse_tree_json(body).unwrap();
        let variants = classify_variants(&entries);
        // ae.safetensors is a VAE companion → excluded; model_index.json isn't
        // safetensors → excluded. Two diffusion variants remain.
        assert_eq!(variants.len(), 2);
        assert!(variants.iter().all(|v| v.family.as_deref() == Some("flux1")));
        let labels: Vec<&str> = variants.iter().map(|v| v.label.as_str()).collect();
        assert!(labels.contains(&"flux1-dev")); // no precision token → stem
        assert!(labels.contains(&"fp8"));
        let fp8 = variants.iter().find(|v| v.label == "fp8").unwrap();
        assert_eq!(fp8.path, "flux1-dev-fp8.safetensors");
        assert_eq!(fp8.size_bytes, 11901466276);
    }

    #[test]
    fn classify_variants_falls_back_to_all_safetensors_when_family_unknown() {
        let entries = vec![
            HfTreeEntry { path: "mystery-model.safetensors".into(), size_bytes: 100 },
            HfTreeEntry { path: "readme.md".into(), size_bytes: 5 },
        ];
        let variants = classify_variants(&entries);
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].family, None);
        assert_eq!(variants[0].path, "mystery-model.safetensors");
    }

    #[test]
    fn classify_variants_excludes_file_matching_both_diffusion_and_companion() {
        // Regression pin for the `&& !matches_any(companion)` clause. In the
        // qwen-image family the LLM file matches the diffusion substring "qwen"
        // AND the companion pattern "qwen2.5"; it must be excluded, leaving only
        // the true diffusion transformer. Without the clause this returns 2.
        let entries = vec![
            HfTreeEntry { path: "qwen-image.safetensors".into(), size_bytes: 20_000_000_000 },
            HfTreeEntry { path: "qwen2.5-vl-7b.safetensors".into(), size_bytes: 15_000_000_000 },
            HfTreeEntry { path: "vae.safetensors".into(), size_bytes: 300_000_000 },
        ];
        let variants = classify_variants(&entries);
        assert_eq!(variants.len(), 1);
        assert_eq!(variants[0].path, "qwen-image.safetensors");
        assert_eq!(variants[0].family.as_deref(), Some("qwen-image"));
    }

    #[test]
    fn parse_tree_json_falls_back_to_plain_size_when_lfs_size_zero() {
        // lfs present but reporting 0 must fall back to the top-level size.
        let body = r#"[{"type":"file","path":"x.bin","size":99,"lfs":{"size":0}}]"#;
        let entries = parse_tree_json(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size_bytes, 99);
    }
}
