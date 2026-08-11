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
export type ComponentRole = "diffusion" | "vae" | "clip_l" | "clip_g" | "t5xxl" | "llm" | "llm_vision";

export const ROLE_LABELS: Record<ComponentRole, string> = {
  diffusion: "Diffusion model",
  vae: "VAE",
  clip_l: "CLIP-L text encoder",
  clip_g: "CLIP-G text encoder",
  t5xxl: "T5-XXL text encoder",
  llm: "LLM text encoder",
  llm_vision: "Vision tower (mmproj)",
};

// Engine enums, from src-tauri/fixtures/sd-help.txt. Empty = let engine auto-detect.
// Mirror the pinned stable-diffusion.cpp build's `--vae-format` / `--prediction`
// value sets (see src-tauri/fixtures/sd-help.txt, engine b290693). These are the
// values the engine *accepts*; picking a wrong one for a checkpoint aborts the
// engine mid-graph. FLUX.2 in particular must be left unset — see the flux2
// recipe in src-tauri/src/recipes.rs.
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
  llm_vision?: string | null;
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

/** Mirrors Rust `loras::LoraInfo`. `name` is the pool filename stem and the
 *  engine tag — it never changes. `display_name` is the editable label.
 *  `family` is "" when the base family is unknown; it drives a compatibility
 *  hint only, never what the picker shows. `base_model` is Civitai's own label
 *  ("Flux.2 Klein"), shown verbatim so the user can judge a fit the family is
 *  too coarse to express — it is "" for a local file. */
export interface LoraInfo {
  id: string;
  name: string;
  display_name: string;
  family: string;
  base_model: string;
  trigger_words: string[];
  size_bytes: number;
  broken: boolean;
}

/** Mirrors Rust `commands::AddedLora`. When `lora.family` is "", the detector
 *  was unsure; `candidates` narrows the dropdown when it isn't empty. */
export interface AddedLora {
  lora: LoraInfo;
  candidates: string[];
}

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

/** Mirrors Rust `LoraSelection` (src-tauri/src/types.rs). `name` is the pool
 *  filename stem the engine's `<lora:NAME:WEIGHT>` tag resolves — never the
 *  display label. */
export interface LoraSelection {
  name: string;
  weight: number;
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
  // Id of the managed library model this request targets (mirrors Rust
  // GenerationRequest.model_id, #[serde(default)] → null for old configs).
  // Set → backend re-resolves components from model.json (single source of
  // truth); null → ad-hoc model, `model` used literally.
  model_id: string | null;
  // LoRAs applied to this run (mirrors Rust GenerationRequest.loras,
  // #[serde(default)] → [] for old configs and gallery sidecars).
  loras: LoraSelection[];
  // Reference images for instruction editing, absolute paths (mirrors Rust
  // GenerationRequest.ref_images, #[serde(default)] → [] for old configs and
  // gallery sidecars). Whether they reach the engine is the backend's call.
  ref_images: string[];
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
  name: string; utilization_pct: number | null;
  vram_used_mb: number | null; vram_total_mb: number | null;
  shared_used_mb: number | null; shared: boolean;
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

// Wire values MUST match the Rust `EngineSelection` enum's serde form
// (`#[serde(tag = "type", rename_all = "snake_case")]`, src-tauri/src/types.rs).
export type EngineSelection =
  | { type: "builtin" }
  | { type: "downloaded"; tag: string }
  | { type: "custom"; path: string };

/** Everything the Engine preferences section renders, in one round trip.
 *  Mirrors Rust `commands::EngineStatus`. */
export interface EngineStatus {
  /** The active selection, echoed so the UI never has to guess. */
  selection: EngineSelection;
  /** Release tag in use, or null for a custom build with unknown provenance. */
  tag: string | null;
  /** Commit from `--version`, or null when the engine won't run. */
  commit: string | null;
  /** Absolute path actually in use, after any fallback. */
  path: string | null;
  /** True when the selection couldn't be honoured and builtin was used. */
  fell_back: boolean;
  /** Downloaded engines available to select, newest first. */
  installed: string[];
  /** False off Linux x86_64, where no asset we can run is published. */
  supported: boolean;
}

/** An available upgrade. Mirrors Rust `commands::EngineUpdate`. */
export interface EngineUpdate {
  tag: string;
  asset_size: number;
  /** Tag we compared against, so the UI can say "from X to Y". */
  current_tag: string | null;
}

/** One upstream commit between the running engine and the candidate. Mirrors
 *  Rust `engine_release::ChangeEntry`. `noteworthy` is false for docs/ci/chore
 *  -style commits, which are collapsed behind a count. */
export interface ChangeEntry {
  subject: string;
  noteworthy: boolean;
}

export interface AppConfig {
  // Legacy override, superseded by `engine`. Retained on the Rust struct only
  // so an existing config can be migrated once at load; the backend never
  // writes it again and nothing consults it to decide which binary runs.
  // Mirrors Rust AppConfig.sd_binary_path. Optional because the Rust field is
  // `skip_serializing_if = "Option::is_none"` and the one-time migration into
  // `engine` clears it, so after the first load the key is absent, not null.
  sd_binary_path?: string | null;
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
  // Load-time weight precision for the diffusion model (mirrors Rust
  // AppConfig.load_precision, #[serde(default)] → "auto" for old configs).
  // "auto" re-quantises only when the model won't fit the selected GPU,
  // "original" never does, any other value is an engine weight type.
  load_precision: LoadPrecision;
  // Which engine binary the backend spawns. Mirrors Rust AppConfig.engine
  // (#[serde(default)] → { type: "builtin" } for old configs).
  engine: EngineSelection;
  // Daily update check (mirrors Rust AppConfig.engine_update_check,
  // #[serde(default = "default_true")] → true for old configs).
  engine_update_check: boolean;
  // Unix seconds of the last check, null if never. Backend-owned: `set_settings`
  // preserves it, so what the UI sends here is ignored.
  engine_last_check: number | null;
  // Newest engine tag the user has been shown, so a declined update doesn't
  // re-badge on every launch. null = nothing seen yet. Unlike the two fields
  // above this one IS taken from the payload — dismissing the badge is a UI act.
  engine_seen_tag: string | null;
}

// Values accepted by AppConfig.load_precision. The quantised entries must stay
// in sync with fit::QUANT_LADDER on the Rust side — the string is passed to the
// engine verbatim as a --tensor-type-rules target.
export type LoadPrecision = "auto" | "original" | "q8_0" | "q5_1" | "q4_K";

export const LOAD_PRECISION_OPTIONS: { value: LoadPrecision; label: string; hint: string }[] = [
  { value: "auto", label: "Auto", hint: "Reduce precision only when the model won't fit your GPU" },
  { value: "original", label: "Original", hint: "Always load the model exactly as stored" },
  { value: "q8_0", label: "8-bit", hint: "About half the memory of a 16-bit model, near-identical output" },
  { value: "q5_1", label: "5-bit", hint: "Smaller again, slight quality loss" },
  { value: "q4_K", label: "4-bit", hint: "Smallest; noticeable quality loss, but runs on modest GPUs" },
];

export const defaultRequest = (): GenerationRequest => ({
  model: { type: "single_file", path: "" }, prompt: "", negative_prompt: "",
  steps: 20, cfg_scale: 7.0, sampler: "euler_a",
  width: 512, height: 512, seed: -1, batch_count: 1,
  output_format: "png", model_id: null, loras: [], ref_images: [],
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

/** Mirrors Rust `commands::SpaceCheck`. `free_bytes` is null when the disk
 *  probe failed, in which case `ok` is true — never block on an unknown. */
export interface SpaceCheck {
  required_bytes: number;
  free_bytes: number | null;
  ok: boolean;
}

/** A chosen reference image (mirrors Rust commands::RefImageInfo). */
export interface RefImageInfo {
  path: string;
  width: number;
  height: number;
  suggested_width: number;
  suggested_height: number;
}

/** Mirrors Rust `commands::ReclaimableModel`: an installed model and the bytes
 *  deleting it would free. */
export interface ReclaimableModel {
  id: string;
  name: string;
  size_bytes: number;
}

/** Shared with Rust `downloader::INSUFFICIENT_SPACE_PREFIX`. Tauri command
 *  errors arrive as plain strings, so the disk-full failure is recognised by
 *  this prefix. Keep the two in sync. */
export const INSUFFICIENT_SPACE_PREFIX = "Not enough disk space";
