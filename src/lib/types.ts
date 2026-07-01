// Wire values MUST match the Rust `OutputFormat` enum's serde snake_case form
// (src-tauri/src/types.rs). Extensions ("png"/"jpg") live only in Rust.
export type OutputFormat = "png" | "jpeg";

// Wire values MUST match the Rust `Sampler` enum's serde snake_case form
// (src-tauri/src/types.rs). The `++` CLI spelling lives only in Rust's
// `cli_name()`; never send it over the wire. Display labels are in SAMPLERS.
export type Sampler =
  | "euler" | "euler_a" | "heun" | "dpm2"
  | "dpm_pp2s_a" | "dpm_pp2m" | "dpm_pp2m_v2"
  | "ipndm" | "ipndm_v" | "lcm";

export interface GenerationRequest {
  model_path: string;
  prompt: string;
  negative_prompt: string;
  steps: number;
  cfg_scale: number;
  sampler: Sampler;
  width: number;
  height: number;
  seed: number;       // -1 = random
  batch_count: number;
  output_format: OutputFormat;
}

export interface ProgressUpdate { current_step: number; total_steps: number; }

export interface GpuStats {
  name: string; utilization_pct: number;
  vram_used_mb: number; vram_total_mb: number;
}
export interface SystemStats {
  gpu: GpuStats | null; cpu_pct: number;
  ram_used_mb: number; ram_total_mb: number;
}

export interface GalleryItem {
  id: string; image_path: string;
  request: GenerationRequest; created_at_unix: number;
  batch_id: string; batch_index: number; batch_size: number;
}

// Wire values MUST match the Rust `DeviceKind` enum's serde snake_case form
// (src-tauri/src/types.rs).
export type DeviceKind = "discrete" | "integrated" | "cpu" | "other";

export interface GpuDevice {
  index: number;
  name: string;
  kind: DeviceKind;
}

/** Persisted GPU choice; `name` lets us validate the index still maps to it. */
export interface GpuSelection {
  index: number;
  name: string;
}

export interface AppConfig {
  sd_binary_path: string | null;
  default_model_path: string | null;
  gallery_dir: string;
  models_dir: string;
  extra_model_dirs: string[];
  last_request: GenerationRequest;
  gpu_device: GpuSelection | null;
  params_expanded: boolean;
  // Wire values MUST match the Rust `Theme` enum's serde snake_case form
  // (src-tauri/src/types.rs).
  theme: "dark" | "light";
  // Whether the one-time welcome dialog has been dismissed. Mirrors the Rust
  // AppConfig.onboarded (#[serde(default)] → false for old configs).
  onboarded: boolean;
}

export const defaultRequest = (): GenerationRequest => ({
  model_path: "", prompt: "", negative_prompt: "",
  steps: 20, cfg_scale: 7.0, sampler: "euler_a",
  width: 512, height: 512, seed: -1, batch_count: 1,
  output_format: "png",
});

export const SAMPLERS: { value: Sampler; label: string }[] = [
  { value: "euler_a", label: "Euler a" },
  { value: "euler", label: "Euler" },
  { value: "heun", label: "Heun" },
  { value: "dpm2", label: "DPM2" },
  { value: "dpm_pp2s_a", label: "DPM++ 2S a" },
  { value: "dpm_pp2m", label: "DPM++ 2M" },
  { value: "dpm_pp2m_v2", label: "DPM++ 2M v2" },
  { value: "ipndm", label: "iPNDM" },
  { value: "ipndm_v", label: "iPNDM v" },
  { value: "lcm", label: "LCM" },
];

export const FORMATS: { value: OutputFormat; label: string }[] = [
  { value: "png", label: "PNG" },
  { value: "jpeg", label: "JPEG" },
];

export interface ModelInfo { path: string; name: string; size_bytes: number; }

export type ModelKind = "sd15" | "sdxl";
export type Suitability = "recommended" | "tight" | "too_big" | "unknown";

export interface RatedModel {
  id: string; name: string; url: string; size_bytes: number;
  kind: ModelKind; min_vram_mb: number; recommended_vram_mb: number;
  suitability: Suitability;
}

export interface DownloadProgress { downloaded: number; total: number | null; }
