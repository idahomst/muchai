//! Civitai lookup: URL classification and version-metadata parsing. Everything
//! is pure and unit-tested except `fetch_version`, a thin `ureq` wrapper —
//! the same split `hf.rs` uses.
//!
//! Civitai is the only source for a LoRA's trigger words, and a pasted Civitai
//! link is usually a model *page*, not a download link, so resolving one takes
//! a lookup rather than a string rewrite.

use serde::Deserialize;

/// What a pasted Civitai URL points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CivitaiRef {
    /// A specific version — directly downloadable.
    Version(u64),
    /// A model page with no version pinned; its newest version is used.
    Model(u64),
}

/// Classify a pasted string. Pure. `None` when it isn't a recognisable Civitai
/// model or download URL.
pub fn parse_civitai_url(url: &str) -> Option<CivitaiRef> {
    let rest = url.trim();
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))
        .unwrap_or(rest);
    let rest = rest
        .strip_prefix("civitai.com/")
        .or_else(|| rest.strip_prefix("civitai.red/"))?;
    let (path, query) = match rest.split_once('?') {
        Some((p, q)) => (p, q),
        None => (rest, ""),
    };
    let path = path.split('#').next().unwrap_or(path);
    let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

    // A pinned version always wins over the page's model id.
    for pair in query.split('&') {
        if let Some(v) = pair.strip_prefix("modelVersionId=") {
            if let Ok(id) = v.parse::<u64>() {
                return Some(CivitaiRef::Version(id));
            }
        }
    }
    match segs.as_slice() {
        ["api", "download", "models", id, ..] => id.parse().ok().map(CivitaiRef::Version),
        ["models", id, ..] => id.parse().ok().map(CivitaiRef::Model),
        _ => None,
    }
}

/// Everything MuchAI needs from a Civitai version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CivitaiVersion {
    pub version_id: u64,
    /// "<model name> (<version name>)", or just the model name when unnamed.
    pub display_name: String,
    /// Civitai's own base-model label, verbatim — "Flux.2 Klein", "SDXL 1.0",
    /// "Qwen Image". Kept as free text rather than mapped onto MuchAI's family
    /// list, because it is finer-grained than a family and only the user can
    /// judge the difference (a Klein 4B LoRA on a Klein 9B model is `flux2`
    /// either way, and aborts the engine mid-graph).
    pub base_model: String,
    pub trigger_words: Vec<String>,
    pub download_url: String,
    pub size_bytes: u64,
}

#[derive(Deserialize)]
struct RawFile {
    #[serde(default)]
    name: String,
    #[serde(rename = "sizeKB", default)]
    size_kb: f64,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    primary: bool,
    #[serde(rename = "downloadUrl", default)]
    download_url: String,
}

#[derive(Deserialize)]
struct RawModel {
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawVersion {
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(rename = "baseModel", default)]
    base_model: String,
    #[serde(rename = "trainedWords", default)]
    trained_words: Vec<String>,
    #[serde(default)]
    model: Option<RawModel>,
    #[serde(default)]
    files: Vec<RawFile>,
}

/// Parse a `GET /api/v1/model-versions/{id}` response. Pure.
pub fn parse_version_json(body: &str) -> Result<CivitaiVersion, String> {
    let raw: RawVersion = serde_json::from_str(body)
        .map_err(|_| "Couldn't read the LoRA details from Civitai (unexpected response).".to_string())?;
    // Civitai attaches training data, configs and preview archives to a version,
    // so picking "the first file" downloads the wrong thing surprisingly often.
    // `primary` is its own marker for the real weights; type "Model" is the
    // fallback for older entries that don't set it.
    let file = raw
        .files
        .iter()
        .find(|f| f.primary)
        .or_else(|| raw.files.iter().find(|f| f.kind == "Model"))
        .or_else(|| raw.files.first())
        .ok_or_else(|| "This Civitai version has no downloadable file.".to_string())?;
    if file.download_url.is_empty() {
        return Err("This Civitai version has no download link.".to_string());
    }
    let model_name = raw.model.as_ref().map(|m| m.name.clone()).unwrap_or_default();
    let base = if model_name.is_empty() { file.name.clone() } else { model_name };
    let display_name =
        if raw.name.is_empty() { base } else { format!("{base} ({})", raw.name) };
    Ok(CivitaiVersion {
        version_id: raw.id,
        display_name,
        base_model: raw.base_model.clone(),
        trigger_words: raw.trained_words.clone(),
        download_url: file.download_url.clone(),
        size_bytes: (file.size_kb * 1024.0).round() as u64,
    })
}

/// Read the newest version id from a `GET /api/v1/models/{id}` response. Pure.
/// Civitai orders `modelVersions` newest-first.
pub fn parse_first_version_id(body: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    value.get("modelVersions")?.as_array()?.first()?.get("id")?.as_u64()
}

fn get(url: &str, token: &str) -> Result<String, String> {
    // Bounded timeouts so a hung Civitai connection can't stall the dialog.
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(10))
        .timeout_read(std::time::Duration::from_secs(30))
        .build();
    let mut req = agent.get(url);
    if !token.is_empty() {
        req = req.set("Authorization", &format!("Bearer {token}"));
    }
    match req.call() {
        Ok(resp) => resp
            .into_string()
            .map_err(|_| "Couldn't read the response from Civitai.".to_string()),
        Err(ureq::Error::Status(401, _)) | Err(ureq::Error::Status(403, _)) => {
            Err("Civitai refused the request — add a Civitai token in Preferences (⚙).".into())
        }
        Err(ureq::Error::Status(404, _)) => Err("Not found on Civitai.".into()),
        Err(ureq::Error::Status(code, _)) => Err(format!("Civitai returned HTTP {code}.")),
        Err(ureq::Error::Transport(t)) => Err(format!("Network error reaching Civitai: {t}")),
    }
}

/// Resolve a pasted Civitai reference to its version metadata. A model-page
/// reference costs a second request: the page id has to be resolved to a
/// version id before the version can be read.
pub fn fetch_version(r: CivitaiRef, token: &str) -> Result<CivitaiVersion, String> {
    let version_id = match r {
        CivitaiRef::Version(id) => id,
        CivitaiRef::Model(id) => {
            let body = get(&format!("https://civitai.com/api/v1/models/{id}"), token)?;
            parse_first_version_id(&body)
                .ok_or_else(|| "That Civitai model has no downloadable versions.".to_string())?
        }
    };
    let body = get(&format!("https://civitai.com/api/v1/model-versions/{version_id}"), token)?;
    parse_version_json(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_direct_download_url() {
        assert_eq!(
            parse_civitai_url("https://civitai.com/api/download/models/128713"),
            Some(CivitaiRef::Version(128713))
        );
    }

    #[test]
    fn parses_a_model_page_with_an_explicit_version() {
        assert_eq!(
            parse_civitai_url("https://civitai.com/models/9019?modelVersionId=128713"),
            Some(CivitaiRef::Version(128713))
        );
    }

    #[test]
    fn parses_a_bare_model_page() {
        assert_eq!(
            parse_civitai_url("https://civitai.com/models/9019/film-grain"),
            Some(CivitaiRef::Model(9019))
        );
    }

    #[test]
    fn accepts_the_civitai_red_mirror() {
        assert_eq!(
            parse_civitai_url("https://civitai.red/api/download/models/42"),
            Some(CivitaiRef::Version(42))
        );
    }

    #[test]
    fn rejects_non_civitai_urls() {
        assert_eq!(parse_civitai_url("https://huggingface.co/org/repo"), None);
        assert_eq!(parse_civitai_url("not a url"), None);
        assert_eq!(parse_civitai_url("https://civitai.com/user/someone"), None);
    }

    #[test]
    fn parses_a_version_response() {
        let json = include_str!("../fixtures/civitai-version.json");
        let v = parse_version_json(json).unwrap();
        assert_eq!(v.version_id, 128713);
        assert_eq!(v.display_name, "Film Grain (v1.0)");
        assert_eq!(v.trigger_words, vec!["film grain".to_string(), "35mm photo".to_string()]);
        // Kept verbatim: it is shown to the user, not matched against anything.
        assert_eq!(v.base_model, "Flux.1 D");
        // The primary file, NOT the first one — Civitai lists training data and
        // preview archives alongside the weights.
        assert_eq!(v.download_url, "https://civitai.com/api/download/models/128713");
        assert_eq!(v.size_bytes, 168_034_304);
    }

    #[test]
    fn a_version_response_with_no_files_is_an_error() {
        let json = r#"{"id":1,"name":"v1","model":{"name":"X"},"files":[]}"#;
        assert!(parse_version_json(json).is_err());
    }

    #[test]
    fn unparseable_json_is_an_error() {
        assert!(parse_version_json("<html>404</html>").is_err());
    }

    #[test]
    fn reads_the_first_version_id_from_a_model_response() {
        let json = r#"{"id":9019,"name":"Film Grain","modelVersions":[{"id":128713},{"id":99}]}"#;
        assert_eq!(parse_first_version_id(json), Some(128713));
        assert_eq!(parse_first_version_id(r#"{"modelVersions":[]}"#), None);
        assert_eq!(parse_first_version_id("nope"), None);
    }
}
