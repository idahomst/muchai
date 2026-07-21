import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats, DownloadProgress, GpuDevice, RecipeInfo, GenDefaults, LibraryEntry, RatedCatalogEntry, ManifestFlags, ModelComponents } from "./types";

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
export const pickGalleryDir = () => invoke<string | null>("pick_gallery_dir");

/** Open a folder (or file) in the OS file manager / default app. */
export const openFolder = (path: string) => invoke<void>("open_path", { path });

export const imageSrc = (path: string) => convertFileSrc(path);

export const onProgress = (cb: (p: ProgressUpdate) => void): Promise<UnlistenFn> =>
  listen<ProgressUpdate>("generation:progress", (e) => cb(e.payload));

/** Fires once per run when the backend auto-engaged Low-VRAM mode for it. */
export const onGenNotice = (cb: () => void): Promise<UnlistenFn> =>
  listen("generation:low_vram_auto", () => cb());

export const onSystemStats = (cb: (s: SystemStats) => void): Promise<UnlistenFn> =>
  listen<SystemStats>("system:stats", (e) => cb(e.payload));

export const deleteModel = (path: string) => invoke<void>("delete_model", { path });
export const cancelDownload = () => invoke<void>("cancel_download");
export const pickFolder = () => invoke<string | null>("pick_folder");

export const onDownloadProgress = (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model:download:progress", (e) => cb(e.payload));

export const listRecipes = () => invoke<RecipeInfo[]>("list_recipes");

export const listLibrary = () => invoke<LibraryEntry[]>("list_library");

export const catalogEntries = (vramTotalMb: number | null) =>
  invoke<RatedCatalogEntry[]>("catalog_entries", { vramTotalMb });

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
