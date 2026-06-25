import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats } from "./types";

export const getSettings = () => invoke<AppConfig>("get_settings");
export const setSettings = (config: AppConfig) => invoke<void>("set_settings", { config });
export const listHistory = () => invoke<GalleryItem[]>("list_history");
export const generate = (request: GenerationRequest) => invoke<GalleryItem>("generate", { request });
export const cancelGeneration = () => invoke<void>("cancel_generation");
export const pickModelFile = () => invoke<string | null>("pick_model_file");

export const imageSrc = (path: string) => convertFileSrc(path);

export const onProgress = (cb: (p: ProgressUpdate) => void): Promise<UnlistenFn> =>
  listen<ProgressUpdate>("generation:progress", (e) => cb(e.payload));

export const onSystemStats = (cb: (s: SystemStats) => void): Promise<UnlistenFn> =>
  listen<SystemStats>("system:stats", (e) => cb(e.payload));
