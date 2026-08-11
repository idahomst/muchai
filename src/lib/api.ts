import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats, DownloadProgress, GpuDevice, RecipeInfo, GenDefaults, LibraryEntry, RatedCatalogEntry, ManifestFlags, ModelComponents, LibraryFit, SpaceCheck, ReclaimableModel, LoraInfo, AddedLora, EngineSelection, EngineStatus, EngineUpdate, ChangeEntry, RefImageInfo } from "./types";

export const getSettings = () => invoke<AppConfig>("get_settings");
/** Enumerate Vulkan devices the engine can target (cached server-side). */
export const listGpuDevices = () => invoke<GpuDevice[]>("list_gpu_devices");
export const setSettings = (config: AppConfig) => invoke<void>("set_settings", { config });
export const listHistory = () => invoke<GalleryItem[]>("list_history");
/** Returns one item per produced image (batch_count may yield several).
 *  `deviceVramMb` is the selected GPU's total VRAM (from sysStats) so the
 *  backend can auto-engage Low-VRAM; null when unknown / running on CPU. */
export const generate = (request: GenerationRequest, deviceVramMb: number | null = null) =>
  invoke<GalleryItem[]>("generate", { request, deviceVramMb });
/** Move a generated image (and its sidecar) to the OS trash. */
export const deleteImage = (path: string) => invoke<void>("delete_image", { imagePath: path });
export const cancelGeneration = () => invoke<void>("cancel_generation");
export const pickModelFile = (startDir?: string) =>
  invoke<string | null>("pick_model_file", { startDir: startDir ?? null });
export const pickRefImageDialog = (startDir?: string) =>
  invoke<string | null>("pick_ref_image_dialog", { startDir: startDir ?? null });
export const pickRefImage = (path: string) =>
  invoke<RefImageInfo>("pick_ref_image", { path });
export const pickGalleryDir = () => invoke<string | null>("pick_gallery_dir");

/** Open a folder (or file) in the OS file manager / default app. */
export const openFolder = (path: string) => invoke<void>("open_path", { path });

/** Open an external https URL in the user's default browser. */
export const openExternal = (url: string) => invoke<void>("open_url", { url });

export const imageSrc = (path: string) => convertFileSrc(path);

export const onProgress = (cb: (p: ProgressUpdate) => void): Promise<UnlistenFn> =>
  listen<ProgressUpdate>("generation:progress", (e) => cb(e.payload));

/** Fires once per run (only when live preview is enabled) with the absolute
 *  path the engine will overwrite with the draft image. */
export const onPreview = (cb: (path: string) => void): Promise<UnlistenFn> =>
  listen<string>("generation:preview", (e) => cb(e.payload));

/** Fires once per run when the backend auto-engaged Low-VRAM mode for it. */
export const onGenNotice = (cb: () => void): Promise<UnlistenFn> =>
  listen("generation:low_vram_auto", () => cb());

/** Fires when the engine reported it could not find a selected LoRA, carrying
 *  that LoRA's name. The run still SUCCEEDS — with a silently unmodified image
 *  — so this event is the only way the user learns the LoRA did nothing. */
export const onLoraMissing = (cb: (name: string) => void): Promise<UnlistenFn> =>
  listen<string>("generation:lora_missing", (e) => cb(e.payload));

export const onSystemStats = (cb: (s: SystemStats) => void): Promise<UnlistenFn> =>
  listen<SystemStats>("system:stats", (e) => cb(e.payload));

export const deleteModel = (path: string) => invoke<void>("delete_model", { path });
export const cancelDownload = () => invoke<void>("cancel_download");
export const pickFolder = () => invoke<string | null>("pick_folder");

export const onDownloadProgress = (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model:download:progress", (e) => cb(e.payload));

export const listRecipes = () => invoke<RecipeInfo[]>("list_recipes");

export const listLibrary = () => invoke<LibraryEntry[]>("list_library");

export const rateLibrary = (vramTotalMb: number | null) =>
  invoke<LibraryFit[]>("rate_library", { vramTotalMb });

export const catalogEntries = (vramTotalMb: number | null, ramTotalMb: number | null) =>
  invoke<RatedCatalogEntry[]>("catalog_entries", { vramTotalMb, ramTotalMb });

export const addCatalogModel = (catalogId: string) =>
  invoke<LibraryEntry>("add_catalog_model", { catalogId });

export const addUrlModel = (url: string, name: string) =>
  invoke<LibraryEntry>("add_url_model", { url, name });

export const addLocalModel = (diffusionPath: string, name: string, family: string | null) =>
  invoke<LibraryEntry>("add_local_model", { diffusionPath, name, family });

export const editModel = (
  id: string,
  name: string,
  family: string,
  flags: ManifestFlags,
  components: ModelComponents,
  recommendedSettings: GenDefaults | null,
) => invoke<LibraryEntry>("edit_model", { id, name, family, flags, components, recommendedSettings });

export const deleteModelEntry = (id: string) =>
  invoke<void>("delete_model_entry", { id });

export const recommendedSettings = (id: string) =>
  invoke<GenDefaults | null>("recommended_settings", { id });

/** Free bytes where models are stored; null when the probe failed. */
export const diskSpace = () => invoke<number | null>("disk_space");

/** Memory budget assumed for a shared-memory GPU, before any override — the
 *  Preferences placeholder. */
export const autoUmaBudgetMb = () => invoke<number>("auto_uma_budget_mb");

/** Pre-flight a catalog install against free disk space. */
export const checkCatalogSpace = (catalogId: string) =>
  invoke<SpaceCheck>("check_catalog_space", { catalogId });

/** Installed models with on-disk sizes, largest first. */
export const listReclaimable = () => invoke<ReclaimableModel[]>("list_reclaimable");

/** XDG trash folder, when it exists. */
export const trashDir = () => invoke<string | null>("trash_dir");

export const listLoras = () => invoke<LoraInfo[]>("list_loras");

/** Every base family a LoRA can be filed under. Not the same as listRecipes() —
 *  that omits sd15/sdxl and includes "custom". */
export const listFamilies = () => invoke<string[]>("list_families");

/** Families whose models take a reference image, from the backend recipe list. */
export const listEditFamilies = () => invoke<string[]>("list_edit_families");

/** Base families a safetensors file's tensor names match. Empty = ask the user. */
export const detectLoraFamily = (path: string) =>
  invoke<string[]>("detect_lora_family", { path });

export const pickLoraFile = () => invoke<string | null>("pick_lora_file");

export const addLocalLora = (path: string, name: string, family: string) =>
  invoke<LoraInfo>("add_local_lora", { path, name, family });

export const addUrlLora = (url: string, name: string) =>
  invoke<AddedLora>("add_url_lora", { url, name });

export const editLora = (
  id: string,
  displayName: string,
  family: string,
  baseModel: string,
) => invoke<LoraInfo>("edit_lora", { id, displayName, family, baseModel });

export const deleteLora = (id: string) => invoke<void>("delete_lora", { id });

/** Which engine is running, plus what else is installed. */
export const engineStatus = () => invoke<EngineStatus>("engine_status");

/** Ask GitHub for the newest release. null = already current, unsupported
 *  platform, or the running engine is a custom build we can't compare. Throws
 *  on a network failure — callers the user didn't trigger must swallow it. */
export const engineCheckUpdate = () => invoke<EngineUpdate | null>("engine_check_update");

/** Upstream commits between the running engine and `toTag`. */
export const engineChangelog = (toTag: string) =>
  invoke<ChangeEntry[]>("engine_changelog", { toTag });

/** Download, verify and install a release, then select it. Returns its commit. */
export const engineApplyUpdate = (tag: string) => invoke<string>("engine_apply_update", { tag });

export const engineSelect = (selection: EngineSelection) =>
  invoke<void>("engine_select", { selection });

/** Fires at most once a launch when a newer engine exists. Lights the badge;
 *  never shows a dialog. Both event names below are pinned against the Rust
 *  constants by tests in `lib.rs` that look for this exact `listen<...>("…"`
 *  form — spell the literal out inline rather than hoisting it to a variable,
 *  or the pin stops matching and silently protects nothing. */
export const onEngineUpdate = (cb: (tag: string) => void): Promise<UnlistenFn> =>
  listen<string>("engine:update-available", (e) => cb(e.payload));

/** Byte progress while an engine is downloading. Separate event from model
 *  downloads so the two progress bars can't cross-talk — only the *cancel*
 *  flag is shared. */
export const onEngineDownloadProgress = (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
  listen<DownloadProgress>("engine:download:progress", (e) => cb(e.payload));
