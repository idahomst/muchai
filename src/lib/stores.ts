import { writable } from "svelte/store";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, GpuDevice, LibraryEntry } from "./types";
import { defaultRequest } from "./types";
import { listLibrary } from "./api";

export const request = writable<GenerationRequest>(defaultRequest());
export const settings = writable<AppConfig | null>(null);
export const history = writable<GalleryItem[]>([]);
export const gpuDevices = writable<GpuDevice[]>([]);
export const currentImage = writable<string | null>(null); // converted asset src
export const currentItem = writable<GalleryItem | null>(null); // params behind the previewed image
export const sysStats = writable<SystemStats | null>(null);

export const library = writable<LibraryEntry[]>([]);

/** Reload the model library from disk. Call after any add/edit/delete. */
export async function refreshLibrary(): Promise<void> {
  library.set(await listLibrary());
}

export type GenStatus =
  | { kind: "idle" }
  | { kind: "running"; progress: ProgressUpdate | null }
  | { kind: "error"; message: string };

export const genStatus = writable<GenStatus>({ kind: "idle" });
