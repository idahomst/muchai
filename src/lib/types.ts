export type Sampler =
  | "euler" | "euler_a" | "heun" | "dpm2"
  | "dpm++2s_a" | "dpm++2m" | "dpm++2mv2"
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
}

export interface AppConfig {
  sd_binary_path: string | null;
  default_model_path: string | null;
  gallery_dir: string;
  last_request: GenerationRequest;
}

export const defaultRequest = (): GenerationRequest => ({
  model_path: "", prompt: "", negative_prompt: "",
  steps: 20, cfg_scale: 7.0, sampler: "euler_a",
  width: 512, height: 512, seed: -1, batch_count: 1,
});

export const SAMPLERS: Sampler[] = [
  "euler_a", "euler", "heun", "dpm2",
  "dpm++2s_a", "dpm++2m", "dpm++2mv2", "ipndm", "ipndm_v", "lcm",
];
