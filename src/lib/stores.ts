import { writable } from "svelte/store";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, GpuDevice, LibraryEntry, DownloadProgress } from "./types";
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

/** Id of the currently-selected library entry (drives recommended-settings). */
export const selectedModelId = writable<string | null>(null);

/** Reload the model library from disk. Call after any add/edit/delete. */
export async function refreshLibrary(): Promise<void> {
  library.set(await listLibrary());
}

// App-level model-download state. Held here (not in NewModelDialog) so progress
// keeps showing in the main UI even after the user closes the dialog — the
// download runs in the backend regardless of dialog lifetime.
export const downloadProgress = writable<DownloadProgress | null>(null);
export const downloadBusy = writable(false);
export const downloadError = writable<string | null>(null);

/** Run a model-download invoke as an app-level task: busy/progress/error live in
 *  stores so the UI survives the dialog closing mid-download. The `model:download:
 *  progress` event feeds `downloadProgress` (wired once at app mount). Resolves
 *  true on success. */
export async function runDownload(fn: () => Promise<unknown>): Promise<boolean> {
  downloadBusy.set(true);
  downloadError.set(null);
  downloadProgress.set(null);
  try {
    await fn();
    await refreshLibrary();
    return true;
  } catch (e) {
    downloadError.set(String(e));
    return false;
  } finally {
    downloadBusy.set(false);
    downloadProgress.set(null);
  }
}

export type GenStatus =
  | { kind: "idle" }
  | { kind: "running"; progress: ProgressUpdate | null }
  | { kind: "error"; message: string };

export const genStatus = writable<GenStatus>({ kind: "idle" });
