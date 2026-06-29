import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats, ModelInfo, RatedModel, DownloadProgress, GpuDevice } from "./types";

export const getSettings = () => invoke<AppConfig>("get_settings");
/** Enumerate Vulkan devices the engine can target (cached server-side). */
export const listGpuDevices = () => invoke<GpuDevice[]>("list_gpu_devices");
export const setSettings = (config: AppConfig) => invoke<void>("set_settings", { config });
export const listHistory = () => invoke<GalleryItem[]>("list_history");
/** Returns one item per produced image (batch_count may yield several). */
export const generate = (request: GenerationRequest) => invoke<GalleryItem[]>("generate", { request });
/** Move a generated image (and its sidecar) to the OS trash. */
export const deleteImage = (path: string) => invoke<void>("delete_image", { imagePath: path });
export const cancelGeneration = () => invoke<void>("cancel_generation");
export const pickModelFile = () => invoke<string | null>("pick_model_file");
export const pickGalleryDir = () => invoke<string | null>("pick_gallery_dir");

/** Open a folder (or file) in the OS file manager / default app. */
export const openFolder = (path: string) => invoke<void>("open_path", { path });

export const imageSrc = (path: string) => convertFileSrc(path);

export const onProgress = (cb: (p: ProgressUpdate) => void): Promise<UnlistenFn> =>
  listen<ProgressUpdate>("generation:progress", (e) => cb(e.payload));

export const onSystemStats = (cb: (s: SystemStats) => void): Promise<UnlistenFn> =>
  listen<SystemStats>("system:stats", (e) => cb(e.payload));

export const listModels = () => invoke<ModelInfo[]>("list_models");
export const starterModels = (vramTotalMb: number | null) =>
  invoke<RatedModel[]>("starter_models", { vramTotalMb });
export const deleteModel = (path: string) => invoke<void>("delete_model", { path });
export const downloadModel = (url: string, token: string) =>
  invoke<ModelInfo>("download_model", { url, token });
export const cancelDownload = () => invoke<void>("cancel_download");
export const pickFolder = () => invoke<string | null>("pick_folder");

export const onDownloadProgress = (cb: (p: DownloadProgress) => void): Promise<UnlistenFn> =>
  listen<DownloadProgress>("model:download:progress", (e) => cb(e.payload));
