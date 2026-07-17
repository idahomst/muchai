import { writable, get } from "svelte/store";
import { downloadModel, downloadMultifile, cancelDownload, onDownloadProgress } from "./api";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, ModelInfo, GpuDevice, ModelDefinition } from "./types";
import { defaultRequest } from "./types";

export const request = writable<GenerationRequest>(defaultRequest());
export const settings = writable<AppConfig | null>(null);
export const history = writable<GalleryItem[]>([]);
export const models = writable<ModelInfo[]>([]);
export const definitions = writable<ModelDefinition[]>([]);
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
  | {
      kind: "active";
      name: string;
      downloaded: number;
      total: number | null;
      fileIndex?: number;
      fileCount?: number;
      fileName?: string;
    }
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
      s.kind === "active"
        ? { ...s, downloaded: p.downloaded, total: p.total, fileIndex: p.file_index, fileCount: p.file_count, fileName: p.file_name }
        : s,
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

/**
 * Download a single model file for the assembly flow and return its ModelInfo
 * so the caller can fill a slot. Single-flight like startDownload (sets
 * downloadStatus active, wires progress, enforces one-at-a-time via the shared
 * backend cancel flag). Returns null if a download is already active or on error.
 */
export async function startFileDownload(url: string, token: string, name: string): Promise<ModelInfo | null> {
  if (get(downloadStatus).kind === "active") return null;
  cancelRequested = false;
  downloadStatus.set({ kind: "active", name, downloaded: 0, total: null });
  const unlisten = await onDownloadProgress((p) => {
    downloadStatus.update((s) =>
      s.kind === "active"
        ? { ...s, downloaded: p.downloaded, total: p.total, fileIndex: p.file_index, fileCount: p.file_count, fileName: p.file_name }
        : s,
    );
  });
  try {
    const info = await downloadModel(url, token);
    downloadStatus.set({ kind: "done", name });
    return info;
  } catch (e) {
    downloadStatus.set(cancelRequested ? { kind: "idle" } : { kind: "error", name, message: String(e) });
    return null;
  } finally {
    unlisten();
  }
}

/** Cancel the active download; the backend removes the partial `.part` file. */
export function cancelActiveDownload(): void {
  cancelRequested = true;
  void cancelDownload();
}

/**
 * Start a curated multi-file download in the background (single-flight, like
 * startDownload). Upserts the returned definition into the `definitions` store
 * and returns it (the caller selects it). No-op if a download is already active.
 */
export async function startMultiFileDownload(entryId: string, token: string, name: string): Promise<ModelDefinition | null> {
  if (get(downloadStatus).kind === "active") return null;
  cancelRequested = false;
  downloadStatus.set({ kind: "active", name, downloaded: 0, total: null, fileIndex: 0 });
  const unlisten = await onDownloadProgress((p) => {
    downloadStatus.update((s) =>
      s.kind === "active"
        ? { ...s, downloaded: p.downloaded, total: p.total, fileIndex: p.file_index, fileCount: p.file_count, fileName: p.file_name }
        : s,
    );
  });
  try {
    const def = await downloadMultifile(entryId, token);
    definitions.update((d) => {
      // Upsert in place so an existing definition keeps its list position.
      const i = d.findIndex((x) => x.id === def.id);
      if (i === -1) return [...d, def];
      const next = d.slice();
      next[i] = def;
      return next;
    });
    downloadStatus.set({ kind: "done", name });
    return def;
  } catch (e) {
    downloadStatus.set(cancelRequested ? { kind: "idle" } : { kind: "error", name, message: String(e) });
    return null;
  } finally {
    unlisten();
  }
}
