use serde::{Deserialize, Serialize};

/// Typed component files of a split model, each wired to a specific engine flag.
///
/// NOTE: this is a minimal stub landed by Task 1 (recipe table + filename
/// detection) solely so `recipes.rs`'s tests compile — `recipes::detect`
/// takes filenames only and `ModelRecipe::missing_required_roles` needs this
/// shape. Task 2 owns this type; it adds `ModelRef`, `ModelDefinition`, and
/// `missing_components` alongside it and should treat this struct as already
/// in place rather than redefining it.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sampler {
    Euler,
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

impl Default for Sampler {
    fn default() -> Self {
        Sampler::EulerA
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
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

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Png
    }
}

/// UI color theme. Persisted in `AppConfig`. Defaults to Dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Dark,
    Light,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GenerationRequest {
    pub model_path: String,
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
}

impl Default for GenerationRequest {
    fn default() -> Self {
        Self {
            model_path: String::new(),
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
        }
    }
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
pub struct ModelInfo {
    /// Absolute path passed to the engine via `-m`.
    pub path: String,
    /// File stem, shown in the UI.
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub sd_binary_path: Option<String>, // None => use bundled sidecar
    pub default_model_path: Option<String>,
    pub gallery_dir: String,
    /// Primary managed models folder; downloads land here.
    #[serde(default)]
    pub models_dir: String,
    /// Additional folders fridAI scans and merges into the model list.
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
    pub last_request: GenerationRequest,
}

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
        let json = r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","models_dir":"/tmp/m","extra_model_dirs":[],"last_request":{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#;
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
        let json = r#"{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}"#;
        let req: GenerationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.output_format, OutputFormat::Png);
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
}
