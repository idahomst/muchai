import { writable, get } from "svelte/store";
import { downloadModel, cancelDownload, onDownloadProgress } from "./api";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, ModelInfo, GpuDevice } from "./types";
import { defaultRequest } from "./types";

export const request = writable<GenerationRequest>(defaultRequest());
export const settings = writable<AppConfig | null>(null);
export const history = writable<GalleryItem[]>([]);
export const models = writable<ModelInfo[]>([]);
export const gpuDevices = writable<GpuDevice[]>([]);
export const currentImage = writable<string | null>(null); // converted asset src
export const currentItem = writable<GalleryItem | null>(null); // params behind the previewed image
export const sysStats = writable<SystemStats | null>(null);

export type GenStatus =
  | { kind: "idle" }
  | { kind: "running"; progress: ProgressUpdate | null }
  | { kind: "error"; message: string };

export const genStatus = writable<GenStatus>({ kind: "idle" });

export type DownloadStatus =
  | { kind: "idle" }
  | { kind: "active"; name: string; downloaded: number; total: number | null }
  | { kind: "done"; name: string }                    // dismissible notice
  | { kind: "error"; name: string; message: string }; // dismissible notice

export const downloadStatus = writable<DownloadStatus>({ kind: "idle" });

// Single-flight: at most one download runs at a time, so one module-level flag
// is enough. It lets a user-initiated cancel resolve to `idle` instead of
// surfacing the backend's cancellation as an error.
let cancelRequested = false;

/** Start a background download. No-op if one is already active (single-flight). */
export async function startDownload(url: string, token: string, name: string): Promise<void> {
  if (get(downloadStatus).kind === "active") return;
  cancelRequested = false;
  downloadStatus.set({ kind: "active", name, downloaded: 0, total: null });
  const unlisten = await onDownloadProgress((p) => {
    downloadStatus.update((s) =>
      s.kind === "active" ? { ...s, downloaded: p.downloaded, total: p.total } : s,
    );
  });
  try {
    await downloadModel(url, token);
    downloadStatus.set({ kind: "done", name });
  } catch (e) {
    downloadStatus.set(
      cancelRequested ? { kind: "idle" } : { kind: "error", name, message: String(e) },
    );
  } finally {
    unlisten();
  }
}

/** Cancel the active download; the backend removes the partial `.part` file. */
export function cancelActiveDownload(): void {
  cancelRequested = true;
  void cancelDownload();
}
