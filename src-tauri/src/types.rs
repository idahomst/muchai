use serde::{Deserialize, Serialize};

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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressUpdate {
    pub current_step: u32,
    pub total_steps: u32,
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub sd_binary_path: Option<String>, // None => use bundled sidecar
    pub default_model_path: Option<String>,
    pub gallery_dir: String,
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
}
