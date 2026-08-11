import { writable, derived } from "svelte/store";
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

/** Families whose models take a reference image, from the backend recipe list.
 *  Loaded once at mount; empty until then, which correctly hides the reference
 *  panel rather than flashing it. */
export const editFamilies = writable<string[]>([]);

/** Whether the run that would start right now takes a reference image.
 *
 *  This must mirror `resolve_ref_images` in the backend exactly, because that
 *  is what actually decides. It asks two questions, and so does this: is the
 *  selected model's family an edit family, *or* does the model carry a vision
 *  tower? The second is what covers a replayed gallery item — `ParamsPanel`
 *  loads it as ad-hoc (`model_id: null`), so it has no family at all, and a
 *  family-only test would hide the reference panel while the backend went
 *  right on demanding an image.
 *
 *  Keyed off `request.model_id` rather than `selectedModelId` for the same
 *  reason: `model_id` is the field the backend re-resolves the family from. */
export const isEditingModel = derived(
  [request, library, editFamilies],
  ([$request, $library, $editFamilies]) => {
    const id = $request.model_id;
    const family = id ? $library.find((e) => e.id === id)?.family : undefined;
    const hasVisionTower =
      $request.model.type === "multi_file" && ($request.model.llm_vision ?? "").trim() !== "";
    return (family !== undefined && $editFamilies.includes(family)) || hasVisionTower;
  },
);

/** Catalog id the New-model dialog should scroll to and highlight when it
 *  opens — set when the user asks to edit an image with no edit model
 *  installed. Cleared by the dialog once it has consumed it. */
export const catalogHighlightId = writable<string | null>(null);

/** Set when "Edit this image" wants the model picker left open, because more
 *  than one edit model exists and the automatic choice should be visible
 *  rather than silent. The picker clears it after opening. */
export const revealModelPicker = writable(false);

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

/** The last engine check or install failure, or null. A store rather than panel
 *  state for the same reason: an install that fails after Preferences closed
 *  would otherwise write its error into a component nobody can see, and the
 *  user would find only the old engine still selected, with no explanation.
 *  Cleared when the panel is destroyed unless an install is still running. */
export const engineError = writable<string | null>(null);

export type GenStatus =
  | { kind: "idle" }
  | { kind: "running"; progress: ProgressUpdate | null }
  | { kind: "error"; message: string };

export const genStatus = writable<GenStatus>({ kind: "idle" });
