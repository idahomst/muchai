import type { LibraryEntry, Suitability } from "./types";

/** Human label for a library row: name + family badge text. */
export function entryLabel(entry: LibraryEntry): string {
  return entry.name;
}

/** Short family badge. */
export function familyBadge(entry: LibraryEntry): string {
  return entry.family;
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
