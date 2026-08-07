import { writable } from "svelte/store";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, GpuDevice, LibraryEntry, DownloadProgress, LoraInfo } from "./types";
import { defaultRequest } from "./types";
import { listLibrary, listLoras } from "./api";

export const request = writable<GenerationRequest>(defaultRequest());
export const settings = writable<AppConfig | null>(null);
export const history = writable<GalleryItem[]>([]);
export const gpuDevices = writable<GpuDevice[]>([]);
export const currentImage = writable<string | null>(null); // converted asset src
// Live-preview frame during a run: a convertFileSrc()'d asset URL with a
// cache-busting ?t=<step> query, or null when no run is showing a draft.
// Takes visual precedence over currentImage while set (see ImagePreview).
export const livePreview = writable<string | null>(null);
export const currentItem = writable<GalleryItem | null>(null); // params behind the previewed image
export const sysStats = writable<SystemStats | null>(null);

export const library = writable<LibraryEntry[]>([]);

/** Id of the currently-selected library entry (drives recommended-settings). */
export const selectedModelId = writable<string | null>(null);

/** Transient banner text shown when the persisted model can't be resolved
 *  (e.g. it was deleted since last launch). Cleared on dismiss or on select. */
export const modelNotice = writable<string | null>(null);

/** Reload the model library from disk. Call after any add/edit/delete. */
export async function refreshLibrary(): Promise<void> {
  library.set(await listLibrary());
}

/** Every registered LoRA, sorted by label. */
export const loras = writable<LoraInfo[]>([]);

/** Reload the LoRA pool from disk. Call after any add/edit/delete. */
export async function refreshLoras(): Promise<void> {
  loras.set(await listLoras());
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
 *  to what `fn` returned, or `null` if it failed — callers must test against
 *  `null`, not truthiness, since a task may legitimately resolve to nothing. */
export async function runDownload<T>(fn: () => Promise<T>): Promise<T | null> {
  downloadBusy.set(true);
  downloadError.set(null);
  downloadProgress.set(null);
  try {
    const result = await fn();
    await refreshLibrary();
    return result;
  } catch (e) {
    downloadError.set(String(e));
    return null;
  } finally {
    downloadBusy.set(false);
    downloadProgress.set(null);
  }
}

/** Tag of an engine update waiting to be noticed, or null. Set by the once-a-day
 *  background check at launch — never by a manual check, which would light a dot
 *  on the section the user is already looking at. Cleared once they have opened
 *  the Engine section. Drives the dot on the gear icon. */
export const engineUpdateTag = writable<string | null>(null);

/** An engine install in flight, and its percentage (null until the first
 *  progress event). App-level for the same reason `downloadBusy` is: the
 *  install runs in the backend regardless of the dialog's lifetime, and it
 *  holds the generation slot while it does, so closing Preferences must not be
 *  what makes it invisible and uncancellable. */
export const engineInstalling = writable(false);
export const enginePct = writable<number | null>(null);

export type GenStatus =
  | { kind: "idle" }
  | { kind: "running"; progress: ProgressUpdate | null }
  | { kind: "error"; message: string };

export const genStatus = writable<GenStatus>({ kind: "idle" });
