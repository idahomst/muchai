import type { LibraryEntry, Suitability, RatingBasis, ModelRef, CatalogEntry } from "./types";

/** Bytes → compact decimal size, e.g. 6_780_000_000 → "6.8 GB". Model/catalog
 *  sizes are decimal (matching HuggingFace), so divide by 1000, not 1024. */
export function formatBytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n, i = 0;
  while (v >= 1000 && i < units.length - 1) { v /= 1000; i++; }
  const rounded = i === 0 ? v : v < 10 ? +v.toFixed(1) : Math.round(v);
  return `${rounded} ${units[i]}`;
}

/** Total download size of a catalog entry: diffusion weights + shared files. */
export function catalogTotalBytes(e: CatalogEntry): number {
  return e.diffusion.size_bytes + e.shared.reduce((sum, s) => sum + s.size_bytes, 0);
}

/** Human label for a library row: name + family badge text. */
export function entryLabel(entry: LibraryEntry): string {
  return entry.name;
}

/** Short family badge. */
export function familyBadge(entry: LibraryEntry): string {
  return entry.family;
}

/** Quantization token parsed from the diffusion filename, verbatim case, or ""
 *  when none is present.
 *  "flux-2-klein-9b-Q4_0.gguf" → "Q4_0"; "model-fp16.safetensors" → "fp16";
 *  "sdxl-base-1.0.safetensors" → "". */
export function quantBadge(entry: LibraryEntry): string {
  const path = entry.model.type === "single_file" ? entry.model.path : entry.model.diffusion_model;
  const name = (path.split(/[/\\]/).pop() ?? "");
  const m = name.match(/(Q\d+(?:_[0-9A-Z]+)*|fp16|fp8|bf16|f16)/i);
  return m ? m[1] : "";
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

/** Fit badge text + tone for a catalog row. When `basis` is "ram" (no GPU found)
 *  the copy names RAM and flags that CPU generation is slow. */
export function suitabilityBadge(
  s: Suitability,
  basis: RatingBasis = "vram",
): { text: string; tone: "good" | "warn" | "bad" | "muted" } {
  if (basis === "ram") {
    switch (s) {
      case "recommended": return { text: "Fits in RAM · CPU: slow", tone: "good" };
      case "tight": return { text: "Tight in RAM · CPU: slow", tone: "warn" };
      case "too_big": return { text: "Too big for RAM", tone: "bad" };
      default: return { text: "Unknown", tone: "muted" };
    }
  }
  switch (s) {
    case "recommended": return { text: "Recommended", tone: "good" };
    case "tight": return { text: "Tight fit", tone: "warn" };
    case "too_big": return { text: "Too big", tone: "bad" };
    default: return { text: "Unknown", tone: "muted" };
  }
}
