import { writable } from "svelte/store";
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate } from "./types";
import { defaultRequest } from "./types";

export const request = writable<GenerationRequest>(defaultRequest());
export const settings = writable<AppConfig | null>(null);
export const history = writable<GalleryItem[]>([]);
export const currentImage = writable<string | null>(null); // converted asset src
export const sysStats = writable<SystemStats | null>(null);

export type GenStatus =
  | { kind: "idle" }
  | { kind: "running"; progress: ProgressUpdate | null }
  | { kind: "error"; message: string };

export const genStatus = writable<GenStatus>({ kind: "idle" });
