import type { LibraryEntry, Suitability, ModelRef } from "./types";

/** Human label for a library row: name + family badge text. */
export function entryLabel(entry: LibraryEntry): string {
  return entry.name;
}

/** Short family badge. */
export function familyBadge(entry: LibraryEntry): string {
  return entry.family;
}

/** True when two model refs point at the same diffusion weights. Used to match
 *  the persisted `request.model` to a library entry so the sidebar highlights
 *  the actually-active model on startup. */
export function sameModel(a: ModelRef, b: ModelRef): boolean {
  if (a.type !== b.type) return false;
  if (a.type === "single_file" && b.type === "single_file") return a.path === b.path;
  if (a.type === "multi_file" && b.type === "multi_file") return a.diffusion_model === b.diffusion_model;
  return false;
}

/** VRAM-fit badge text + tone for a catalog row. */
export function suitabilityBadge(s: Suitability): { text: string; tone: "good" | "warn" | "bad" | "muted" } {
  switch (s) {
    case "recommended": return { text: "Recommended", tone: "good" };
    case "tight": return { text: "Tight fit", tone: "warn" };
    case "too_big": return { text: "Too big", tone: "bad" };
    default: return { text: "Unknown", tone: "muted" };
  }
}
