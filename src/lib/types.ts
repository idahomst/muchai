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

// Wire values MUST match the Rust `ComponentRole` enum's serde snake_case form
// (src-tauri/src/recipes.rs).
export type ComponentRole = "diffusion" | "vae" | "clip_l" | "clip_g" | "t5xxl" | "llm";

export const ROLE_LABELS: Record<ComponentRole, string> = {
  diffusion: "Diffusion model",
  vae: "VAE",
  clip_l: "CLIP-L text encoder",
  clip_g: "CLIP-G text encoder",
  t5xxl: "T5-XXL text encoder",
  llm: "LLM text encoder",
};

// Engine enums, from src-tauri/fixtures/sd-help.txt. Empty = let engine auto-detect.
// Mirror the pinned stable-diffusion.cpp build's `--vae-format` / `--prediction`
// value sets (see src-tauri/fixtures/sd-help.txt, engine b290693). FLUX.2 klein
// uses `sefi_flow` (SeFi-Image FLOW mode), NOT `flux2_flow`.
export const VAE_FORMATS = ["", "auto", "flux", "sd3", "flux2", "wan"] as const;
export const PREDICTIONS = ["", "eps", "v", "edm_v", "sd3_flow", "flux_flow", "sefi_flow"] as const;

// Mirrors Rust `ModelComponents`. Optional fields are omitted or null.
export interface ModelComponents {
  diffusion_model: string;
  vae?: string | null;
  clip_l?: string | null;
  clip_g?: string | null;
  t5xxl?: string | null;
  llm?: string | null;
  vae_format?: string | null;
  prediction?: string | null;
}

// Mirrors Rust `ModelRef` (internally tagged, snake_case). The multi_file variant
// flattens ModelComponents fields alongside `type`.
export type ModelRef =
  | { type: "single_file"; path: string }
  | ({ type: "multi_file" } & ModelComponents);

export type ManifestFlags = {
  vae_format: string | null;
  prediction: string | null;
};

export type LibraryEntry = {
  id: string;
  name: string;
  family: string;
  model: ModelRef;
  flags: ManifestFlags;
  recommended_settings: GenDefaults | null;
  broken: boolean;
};

export type CatalogFile = { url: string; filename: string; size_bytes: number };
export type CatalogShared = { role: string; url: string; filename: string; size_bytes: number };
export type CatalogEntry = {
  id: string;
  name: string;
  family: string;
  license: string;
  source_url: string;
  diffusion: CatalogFile;
  shared: CatalogShared[];
  min_vram_mb: number;
  recommended_vram_mb: number;
};
export type RatedCatalogEntry = CatalogEntry & { suitability: Suitability; basis: RatingBasis };

/** True when the model is selectable/usable (path or diffusion set). */
export function modelIsSet(m: ModelRef): boolean {
  return m.type === "single_file" ? m.path.trim() !== "" : m.diffusion_model.trim() !== "";
}

/** A short label for a model reference (basename of its diffusion file). */
export function modelLabel(m: ModelRef): string {
  const path = m.type === "single_file" ? m.path : m.diffusion_model;
  return path.split(/[\\/]/).pop() || path || "model";
}

export interface GenerationRequest {
  model: ModelRef;
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

// Mirrors Rust `GenDefaults` (src-tauri/src/types.rs). Recommended per-family
// generation settings, applied only via the "Use recommended settings" button.
export interface GenDefaults {
  steps: number;
  cfg_scale: number;
  sampler: Sampler;
  width: number;
  height: number;
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
  // HuggingFace / Civitai access tokens. Plaintext in config.json. Mirrors the
  // Rust AppConfig fields (#[serde(default)] → null for old configs). null = unset.
  hf_token: string | null;
  civitai_token: string | null;
  // Low-VRAM offload mode (mirrors Rust AppConfig.low_vram, #[serde(default)] →
  // false for old configs). When on, generation pages weights from RAM.
  low_vram: boolean;
  // Show a rough live draft as the image generates (mirrors Rust
  // AppConfig.live_preview, #[serde(default = "default_true")] → true for old
  // configs).
  live_preview: boolean;
}

export const defaultRequest = (): GenerationRequest => ({
  model: { type: "single_file", path: "" }, prompt: "", negative_prompt: "",
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

// Wire values MUST match the Rust `RatingBasis` enum's serde snake_case form
// (src-tauri/src/catalog.rs). "ram" = no GPU found, rated against system RAM.
export type RatingBasis = "vram" | "ram" | "none";

// Wire values MUST match the Rust `FitVerdict` enum's serde snake_case form
// (src-tauri/src/fit.rs).
export type FitVerdict = "fits" | "tight" | "wont_fit" | "unknown";

/** Per-installed-model VRAM fit, from the `rate_library` command. Mirrors Rust
 *  `commands::LibraryFit`. `estimate_mb` is null for broken entries. */
export type LibraryFit = { id: string; estimate_mb: number | null; verdict: FitVerdict };

// Mirrors Rust `hf::RatedHfVariant`. One selectable diffusion variant + fit.
export interface RatedHfVariant {
  label: string;
  family: string | null;
  url: string;
  size_bytes: number;
  verdict: FitVerdict;
}

export interface RatedModel {
  id: string; name: string; url: string; size_bytes: number;
  kind: ModelKind; min_vram_mb: number; recommended_vram_mb: number;
  suitability: Suitability;
}

export interface DownloadProgress {
  downloaded: number;
  total: number | null;
  // Multi-file context (0-based). Absent on single-file downloads.
  file_index?: number;
  file_count?: number;
  file_name?: string;
}

export interface RoleInfo { role: ComponentRole; required: boolean; }
export interface RecipeInfo {
  family: string;
  name: string;
  roles: RoleInfo[];
  vae_format: string | null;
  prediction: string | null;
}

export interface DetectedSlot { role: ComponentRole; path: string; }
export interface DetectionResult { family: string; name: string; slots: DetectedSlot[]; }

export interface RatedMultiFile {
  id: string; name: string; family: string;
  diffusion_url: string; diffusion_size_bytes: number;
  overrides: unknown[]; min_vram_mb: number; recommended_vram_mb: number;
  suitability: Suitability;
}
