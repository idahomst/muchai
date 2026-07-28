use crate::recipes::ComponentRole;
use serde::{Deserialize, Serialize};

/// Typed component files of a split model, each wired to a specific engine flag.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelComponents {
    pub diffusion_model: String, // --diffusion-model (required)
    #[serde(default)]
    pub vae: Option<String>, // --vae
    #[serde(default)]
    pub clip_l: Option<String>, // --clip_l
    #[serde(default)]
    pub clip_g: Option<String>, // --clip_g
    #[serde(default)]
    pub t5xxl: Option<String>, // --t5xxl
    #[serde(default)]
    pub llm: Option<String>, // --llm
    #[serde(default)]
    pub vae_format: Option<String>, // --vae-format
    #[serde(default)]
    pub prediction: Option<String>, // --prediction
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampler {
    Euler,
    #[default]
    EulerA,
    Heun,
    Dpm2,
    DpmPp2sA,
    DpmPp2m,
    DpmPp2mV2,
    Ipndm,
    IpndmV,
    Lcm,
}

impl Sampler {
    /// Exact token stable-diffusion.cpp expects after its sampling-method flag.
    pub fn cli_name(self) -> &'static str {
        match self {
            Sampler::Euler => "euler",
            Sampler::EulerA => "euler_a",
            Sampler::Heun => "heun",
            Sampler::Dpm2 => "dpm2",
            Sampler::DpmPp2sA => "dpm++2s_a",
            Sampler::DpmPp2m => "dpm++2m",
            Sampler::DpmPp2mV2 => "dpm++2mv2",
            Sampler::Ipndm => "ipndm",
            Sampler::IpndmV => "ipndm_v",
            Sampler::Lcm => "lcm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    #[default]
    Png,
    Jpeg,
}

impl OutputFormat {
    /// File extension (no dot) used to drive the engine's `-o` format inference.
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
        }
    }
}

/// UI color theme. Persisted in `AppConfig`. Defaults to Dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// A model reference: single all-in-one file, or split components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelRef {
    /// -> `-m <path>`
    SingleFile { path: String },
    /// -> `--diffusion-model` + friends
    MultiFile(ModelComponents),
}

impl Default for ModelRef {
    fn default() -> Self {
        ModelRef::SingleFile { path: String::new() }
    }
}

impl ModelRef {
    /// The weight files that make up this model, for size/estimation purposes.
    /// Single-file → just its path. Multi-file → diffusion model plus every SET,
    /// non-blank optional component (`vae`/`clip_l`/`clip_g`/`t5xxl`/`llm`).
    /// The `vae_format` / `prediction` fields are engine flags, not files, and
    /// are excluded. Order: diffusion, vae, clip_l, clip_g, t5xxl, llm.
    pub fn component_paths(&self) -> Vec<String> {
        match self {
            ModelRef::SingleFile { path } => vec![path.clone()],
            ModelRef::MultiFile(c) => {
                let mut paths = vec![c.diffusion_model.clone()];
                for p in [&c.vae, &c.clip_l, &c.clip_g, &c.t5xxl, &c.llm].into_iter().flatten() {
                    if !p.trim().is_empty() {
                        paths.push(p.clone());
                    }
                }
                paths
            }
        }
    }

    /// Path of the diffusion weights alone. For a single-file checkpoint that
    /// is the whole file. Used by the load-time quantisation decision, which
    /// only ever re-quantises the diffusion model.
    pub fn diffusion_path(&self) -> &str {
        match self {
            ModelRef::SingleFile { path } => path,
            ModelRef::MultiFile(c) => &c.diffusion_model,
        }
    }
}

/// Component slots whose file no longer exists on disk. Empty = all good.
/// Only *set* slots are checked; a `None` optional slot is never reported.
pub fn missing_components(c: &ModelComponents) -> Vec<(ComponentRole, String)> {
    let checks: [(ComponentRole, Option<&String>); 6] = [
        (ComponentRole::Diffusion, Some(&c.diffusion_model)),
        (ComponentRole::Vae, c.vae.as_ref()),
        (ComponentRole::ClipL, c.clip_l.as_ref()),
        (ComponentRole::ClipG, c.clip_g.as_ref()),
        (ComponentRole::T5xxl, c.t5xxl.as_ref()),
        (ComponentRole::Llm, c.llm.as_ref()),
    ];
    let mut out = Vec::new();
    for (role, path) in checks {
        if let Some(p) = path {
            if !p.trim().is_empty() && !std::path::Path::new(p).exists() {
                out.push((role, p.clone()));
            }
        }
    }
    out
}

/// One LoRA applied to a generation.
///
/// `name` is the pool filename stem, which is literally the `NAME` in the
/// engine's `<lora:NAME:WEIGHT>` prompt tag — not a display label and not an
/// id, because the engine resolves LoRAs by filename and nothing else.
/// `PartialEq` (not `Eq`) because `weight` is `f32`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoraSelection {
    pub name: String,
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub model: ModelRef,
    pub prompt: String,
    pub negative_prompt: String,
    pub steps: u32,
    pub cfg_scale: f32,
    pub sampler: Sampler,
    pub width: u32,
    pub height: u32,
    pub seed: i64, // -1 = random
    pub batch_count: u32,
    /// Output image format. Defaults to PNG; pre-feature sidecars/configs lack
    /// this key and deserialize as PNG.
    #[serde(default)]
    pub output_format: OutputFormat,
    /// Id of the managed library model this request targets, when it came from
    /// the model selector. `Some(id)` → the backend re-resolves component paths
    /// from `model.json` at generate time (single source of truth); the `model`
    /// field is then only a display/fallback snapshot. `None` → ad-hoc model
    /// (manual single-file pick or a replayed history item); `model` is literal.
    /// `#[serde(default)]` so pre-feature configs/sidecars load as `None`.
    #[serde(default)]
    pub model_id: Option<String>,
    /// LoRAs to apply to this run, in order. `#[serde(default)]` so every
    /// pre-feature config and gallery sidecar still loads. Empty means the
    /// engine command line is byte-identical to a pre-LoRA run.
    #[serde(default)]
    pub loras: Vec<LoraSelection>,
}

impl Default for GenerationRequest {
    fn default() -> Self {
        Self {
            model: ModelRef::default(),
            prompt: String::new(),
            negative_prompt: String::new(),
            steps: 20,
            cfg_scale: 7.0,
            sampler: Sampler::default(),
            width: 512,
            height: 512,
            seed: -1,
            batch_count: 1,
            output_format: OutputFormat::default(),
            model_id: None,
            loras: Vec::new(),
        }
    }
}

/// Recommended generation settings for a model family. Applied only on explicit
/// user action (the "Use recommended settings" button) — never auto-applied.
/// `PartialEq` (not `Eq`) because `cfg_scale` is `f32`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GenDefaults {
    pub steps: u32,
    pub cfg_scale: f32,
    pub sampler: Sampler,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub current_step: u32,
    pub total_steps: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Discrete,
    Integrated,
    Cpu,
    Other,
}

/// A Vulkan device as reported by the engine. `index` is the `vulkanN` index
/// used in `--backend vulkan{index}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuDevice {
    pub index: u32,
    pub name: String,
    pub kind: DeviceKind,
}

/// The user's persisted device choice. `name` is stored alongside `index` so a
/// stale selection (hardware/driver changed) can be detected and ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuSelection {
    pub index: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuStats {
    pub name: String,
    pub utilization_pct: u32,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemStats {
    pub gpu: Option<GpuStats>,
    pub cpu_pct: f32,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GalleryItem {
    pub id: String,
    pub image_path: String,
    pub request: GenerationRequest,
    pub created_at_unix: u64,
    /// Shared key for all images produced by one generation run. Empty on
    /// pre-batch-field sidecars; consumers fall back to `id`.
    #[serde(default)]
    pub batch_id: String,
    /// 0-based position within the batch.
    #[serde(default)]
    pub batch_index: u32,
    /// Total images in the batch. 0 on legacy sidecars; normalize with `.max(1)`.
    #[serde(default)]
    pub batch_size: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
    /// Multi-file context (0-based). Absent/None on single-file downloads.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
}

/// serde default for `AppConfig.live_preview`: ON. Pre-feature config files
/// lack the key and must default to true (the feature is enabled by default).
fn default_true() -> bool {
    true
}

/// Which engine binary MuchAI runs.
///
/// Replaces the overloaded `AppConfig.sd_binary_path`, which could not tell
/// "the user pointed at a self-compiled build" apart from "the updater
/// installed this" — so an auto-update would have silently stomped a
/// deliberate choice. The updater only ever moves between `Builtin` and
/// `Downloaded`; it refuses to touch `Custom`.
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineSelection {
    /// The engine bundled with this MuchAI build. Always present, never
    /// pruned, so it is always available as the revert target.
    #[default]
    Builtin,
    /// A release the updater downloaded, living in `engines/<tag>/`.
    Downloaded { tag: String },
    /// A path the user chose by hand.
    Custom { path: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub sd_binary_path: Option<String>, // None => use bundled sidecar
    pub default_model_path: Option<String>,
    pub gallery_dir: String,
    /// Primary managed models folder; downloads land here.
    #[serde(default)]
    pub models_dir: String,
    /// Additional folders MuchAI scans and merges into the model list.
    #[serde(default)]
    pub extra_model_dirs: Vec<String>,
    /// Chosen Vulkan device. `None` = engine default (auto-picks best device).
    #[serde(default)]
    pub gpu_device: Option<GpuSelection>,
    /// Whether the params panel under the preview is expanded. Defaults to
    /// `false` (collapsed) for new and pre-feature config files.
    #[serde(default)]
    pub params_expanded: bool,
    /// UI color theme. Defaults to Dark; pre-feature config files lack this key
    /// and deserialize as Dark.
    #[serde(default)]
    pub theme: Theme,
    /// Whether the user has dismissed the one-time welcome dialog. Defaults to
    /// `false`; pre-feature config files lack this key and deserialize as false.
    #[serde(default)]
    pub onboarded: bool,
    /// HuggingFace access token for gated/large downloads. Plaintext; pre-feature
    /// configs lack this key and deserialize as None.
    #[serde(default)]
    pub hf_token: Option<String>,
    /// Civitai access token. Plaintext; stored now, consumed by the multi-file
    /// download rework. Pre-feature configs deserialize as None.
    #[serde(default)]
    pub civitai_token: Option<String>,
    /// Low-VRAM offload mode: page weights from RAM + tiled VAE + flash attention
    /// so models larger than VRAM can run (slower). Old configs default to false.
    #[serde(default)]
    pub low_vram: bool,
    /// Show a rough live draft of the image as it generates (engine
    /// `--preview proj`). Default ON; pre-feature configs lack the key and
    /// default to true via `default_true`.
    #[serde(default = "default_true")]
    pub live_preview: bool,
    /// Load-time weight precision for the diffusion model. `auto` (default)
    /// re-quantises only when the model won't otherwise fit the selected GPU;
    /// `original` never does; an explicit engine type (`q8_0`, `q5_1`, `q4_K`)
    /// always does. Old configs lack the key and get `auto`.
    #[serde(default = "default_load_precision")]
    pub load_precision: String,
    /// Which engine binary to run. `#[serde(default)]` → `Builtin` for configs
    /// written before this field existed; `load_config_from` then migrates any
    /// legacy `sd_binary_path` into `Custom`.
    #[serde(default)]
    pub engine: EngineSelection,
    /// Ask GitHub for a newer engine release at most once a day. Default ON,
    /// following the `live_preview` precedent; turning it off suppresses the
    /// outbound request entirely.
    #[serde(default = "default_true")]
    pub engine_update_check: bool,
    /// Unix seconds of the last update check — the once-a-day rate limit.
    /// `None` means "never checked", which is deliberately distinct from a
    /// check that happened at the Unix epoch.
    #[serde(default)]
    pub engine_last_check: Option<u64>,
    /// Newest tag the user has already been shown, so the badge does not come
    /// back on every launch until they install.
    #[serde(default)]
    pub engine_seen_tag: Option<String>,
    pub last_request: GenerationRequest,
}

/// serde default for `AppConfig.load_precision`.
fn default_load_precision() -> String {
    LOAD_PRECISION_AUTO.to_string()
}

/// `load_precision` value meaning "decide from the VRAM fit estimate".
pub const LOAD_PRECISION_AUTO: &str = "auto";
/// `load_precision` value meaning "never re-quantise".
pub const LOAD_PRECISION_ORIGINAL: &str = "original";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_request_round_trips_through_json() {
        let req = GenerationRequest {
            prompt: "a lovely cat".into(),
            sampler: Sampler::DpmPp2m,
            seed: 1234,
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: GenerationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(req, back);
    }

    #[test]
    fn sampler_serializes_snake_case() {
        let json = serde_json::to_string(&Sampler::EulerA).unwrap();
        assert_eq!(json, "\"euler_a\"");
    }

    /// Pins the exact JSON wire form for every sampler. The TS `Sampler` union
    /// and `SAMPLERS` (src/lib/types.ts) MUST mirror these literals — if this
    /// list changes, update the frontend in lockstep. Guards against the
    /// TS-spelling-vs-serde-snake_case drift that broke the dpm++ samplers.
    #[test]
    fn sampler_wire_form_matches_frontend_contract() {
        let cases = [
            (Sampler::Euler, "euler"),
            (Sampler::EulerA, "euler_a"),
            (Sampler::Heun, "heun"),
            (Sampler::Dpm2, "dpm2"),
            (Sampler::DpmPp2sA, "dpm_pp2s_a"),
            (Sampler::DpmPp2m, "dpm_pp2m"),
            (Sampler::DpmPp2mV2, "dpm_pp2m_v2"),
            (Sampler::Ipndm, "ipndm"),
            (Sampler::IpndmV, "ipndm_v"),
            (Sampler::Lcm, "lcm"),
        ];
        for (variant, wire) in cases {
            // serialize: variant -> exact wire string
            assert_eq!(serde_json::to_string(&variant).unwrap(), format!("\"{wire}\""));
            // deserialize: wire string the frontend sends -> variant
            let back: Sampler = serde_json::from_str(&format!("\"{wire}\"")).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn gpu_device_serializes_snake_case_kind() {
        let d = GpuDevice { index: 1, name: "NVIDIA GeForce RTX 3060".into(), kind: DeviceKind::Discrete };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"kind\":\"discrete\""), "got {json}");
        let back: GpuDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn app_config_defaults_gpu_device_to_none_when_absent() {
        // Config JSON missing the gpu_device key must still deserialize.
        let json = r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","models_dir":"/tmp/m","extra_model_dirs":[],"last_request":{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.gpu_device.is_none());
    }

    #[test]
    fn output_format_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&OutputFormat::Png).unwrap(), "\"png\"");
        assert_eq!(serde_json::to_string(&OutputFormat::Jpeg).unwrap(), "\"jpeg\"");
        let back: OutputFormat = serde_json::from_str("\"jpeg\"").unwrap();
        assert_eq!(back, OutputFormat::Jpeg);
    }

    #[test]
    fn output_format_extension_maps_correctly() {
        assert_eq!(OutputFormat::Png.extension(), "png");
        assert_eq!(OutputFormat::Jpeg.extension(), "jpg");
    }

    #[test]
    fn output_format_defaults_to_png() {
        assert_eq!(OutputFormat::default(), OutputFormat::Png);
    }

    #[test]
    fn generation_request_without_output_format_defaults_to_png() {
        // A pre-feature request/sidecar lacks the output_format key.
        let json = r#"{"model":{"type":"single_file","path":""},"prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}"#;
        let req: GenerationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.output_format, OutputFormat::Png);
    }

    #[test]
    fn generation_request_without_model_id_deserializes_to_none() {
        // Old configs / gallery sidecars written before this feature have no
        // `model_id` key; they must load as an ad-hoc request (model_id None).
        let json = r#"{
            "model": {"type":"single_file","path":"/m/x.safetensors"},
            "prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,
            "sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1
        }"#;
        let req: GenerationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.model_id, None);
    }

    #[test]
    fn generation_request_model_id_round_trips() {
        let req = GenerationRequest {
            model_id: Some("flux2-klein-9b-q4".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: GenerationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model_id.as_deref(), Some("flux2-klein-9b-q4"));
    }

    #[test]
    fn theme_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Theme::Dark).unwrap(), "\"dark\"");
        assert_eq!(serde_json::to_string(&Theme::Light).unwrap(), "\"light\"");
    }

    #[test]
    fn theme_defaults_to_dark() {
        assert_eq!(Theme::default(), Theme::Dark);
    }

    #[test]
    fn model_ref_single_file_wire_form() {
        let m = ModelRef::SingleFile { path: "/m/x.safetensors".into() };
        let json = serde_json::to_string(&m).unwrap();
        assert_eq!(json, r#"{"type":"single_file","path":"/m/x.safetensors"}"#);
        let back: ModelRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn model_ref_multi_file_flattens_components() {
        let m = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/flux1-dev.safetensors".into(),
            t5xxl: Some("/m/t5xxl.safetensors".into()),
            clip_l: Some("/m/clip_l.safetensors".into()),
            vae: Some("/m/ae.safetensors".into()),
            vae_format: Some("flux".into()),
            prediction: Some("flux_flow".into()),
            ..Default::default()
        });
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains(r#""type":"multi_file""#), "got {json}");
        assert!(json.contains(r#""diffusion_model":"/m/flux1-dev.safetensors""#), "got {json}");
        let back: ModelRef = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
    }

    #[test]
    fn model_components_omitted_options_default_to_none() {
        let json = r#"{"diffusion_model":"/m/d.safetensors"}"#;
        let c: ModelComponents = serde_json::from_str(json).unwrap();
        assert_eq!(c.diffusion_model, "/m/d.safetensors");
        assert!(c.vae.is_none() && c.t5xxl.is_none() && c.clip_l.is_none());
    }

    #[test]
    fn missing_components_reports_only_set_but_absent_paths() {
        let dir = std::env::temp_dir().join(format!("muchai-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let present = dir.join("d.safetensors");
        std::fs::write(&present, b"x").unwrap();

        let c = ModelComponents {
            diffusion_model: present.to_string_lossy().into_owned(), // exists
            t5xxl: Some(dir.join("gone.safetensors").to_string_lossy().into_owned()), // set, absent
            clip_l: None, // optional, unset -> not reported
            ..Default::default()
        };
        let missing = missing_components(&c);
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].0, ComponentRole::T5xxl);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn app_config_round_trips_tokens() {
        // A config JSON that includes both tokens deserializes with them set,
        // and re-serializing preserves them.
        let json = r#"{
            "sd_binary_path": null,
            "default_model_path": null,
            "gallery_dir": "/g",
            "models_dir": "/m",
            "extra_model_dirs": [],
            "gpu_device": null,
            "params_expanded": false,
            "theme": "dark",
            "onboarded": false,
            "hf_token": "hf_abc123",
            "civitai_token": "civ_xyz",
            "last_request": {
                "model": { "type": "single_file", "path": "" },
                "prompt": "",
                "negative_prompt": "",
                "steps": 20,
                "cfg_scale": 7.0,
                "sampler": "euler_a",
                "width": 512,
                "height": 512,
                "seed": -1,
                "batch_count": 1
            }
        }"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.hf_token.as_deref(), Some("hf_abc123"));
        assert_eq!(cfg.civitai_token.as_deref(), Some("civ_xyz"));

        let round: AppConfig =
            serde_json::from_str(&serde_json::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(round.hf_token.as_deref(), Some("hf_abc123"));
        assert_eq!(round.civitai_token.as_deref(), Some("civ_xyz"));
    }

    #[test]
    fn app_config_defaults_tokens_to_none_for_old_config() {
        // A pre-feature config JSON lacking the token keys must deserialize with
        // both tokens as None (serde default), not error.
        let json = r#"{
            "sd_binary_path": null,
            "default_model_path": null,
            "gallery_dir": "/g",
            "models_dir": "/m",
            "extra_model_dirs": [],
            "gpu_device": null,
            "params_expanded": false,
            "theme": "dark",
            "onboarded": false,
            "last_request": {
                "model": { "type": "single_file", "path": "" },
                "prompt": "",
                "negative_prompt": "",
                "steps": 20,
                "cfg_scale": 7.0,
                "sampler": "euler_a",
                "width": 512,
                "height": 512,
                "seed": -1,
                "batch_count": 1
            }
        }"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.hf_token.is_none());
        assert!(cfg.civitai_token.is_none());
    }

    #[test]
    fn app_config_live_preview_defaults_to_true_and_round_trips() {
        // A config JSON written before this feature lacks the key: it must
        // deserialize with live_preview = true (feature default ON).
        let legacy = r#"{
            "sd_binary_path": null,
            "default_model_path": null,
            "gallery_dir": "/g",
            "last_request": {
                "model": {"type": "single_file", "path": ""},
                "prompt": "", "negative_prompt": "",
                "steps": 20, "cfg_scale": 7.0, "sampler": "euler_a",
                "width": 512, "height": 512, "seed": -1, "batch_count": 1
            }
        }"#;
        let cfg: AppConfig = serde_json::from_str(legacy).unwrap();
        assert!(cfg.live_preview, "missing key must default to true");

        // And an explicit false survives a serialize/deserialize round-trip.
        let mut off = cfg.clone();
        off.live_preview = false;
        let json = serde_json::to_string(&off).unwrap();
        let back: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(!back.live_preview);
    }

    #[test]
    fn component_paths_single_file_is_just_the_path() {
        let m = ModelRef::SingleFile { path: "/m/model.safetensors".into() };
        assert_eq!(m.component_paths(), vec!["/m/model.safetensors".to_string()]);
    }

    #[test]
    fn component_paths_multi_file_lists_diffusion_plus_set_components() {
        let m = ModelRef::MultiFile(ModelComponents {
            diffusion_model: "/m/flux1-dev.safetensors".into(),
            t5xxl: Some("/m/t5xxl.safetensors".into()),
            clip_l: Some("/m/clip_l.safetensors".into()),
            vae: Some("/m/ae.safetensors".into()),
            clip_g: None,                 // unset → excluded
            llm: Some("   ".into()),      // blank → excluded
            vae_format: Some("flux".into()), // NOT a file path → excluded
            prediction: Some("flux_flow".into()), // NOT a file path → excluded
        });
        assert_eq!(
            m.component_paths(),
            vec![
                "/m/flux1-dev.safetensors".to_string(),
                "/m/ae.safetensors".to_string(),
                "/m/clip_l.safetensors".to_string(),
                "/m/t5xxl.safetensors".to_string(),
            ]
        );
    }

    #[test]
    fn request_without_a_loras_key_deserializes_to_an_empty_selection() {
        // Every config.json and gallery sidecar written before LoRA support
        // lacks this key. Losing them to a parse error is not acceptable.
        let json = r#"{
            "model":{"type":"single_file","path":"/m/x.safetensors"},
            "prompt":"a cat","negative_prompt":"","steps":20,"cfg_scale":7.0,
            "sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1
        }"#;
        let req: GenerationRequest = serde_json::from_str(json).unwrap();
        assert!(req.loras.is_empty());
    }

    #[test]
    fn lora_selection_round_trips() {
        let req = GenerationRequest {
            loras: vec![
                LoraSelection { name: "film-grain".into(), weight: 0.8 },
                LoraSelection { name: "detail-tweaker".into(), weight: 1.0 },
            ],
            ..Default::default()
        };
        let back: GenerationRequest = serde_json::from_str(&serde_json::to_string(&req).unwrap()).unwrap();
        assert_eq!(back.loras, req.loras);
    }
}
