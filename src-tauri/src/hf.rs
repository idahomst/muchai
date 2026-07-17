//! HuggingFace model discovery: URL classification, tree-API parse, quant-label
//! extraction, and variant grouping. All grouping/parsing logic is pure and
//! unit-tested; only `fetch_tree` performs I/O (thin `ureq` wrapper).

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
}
