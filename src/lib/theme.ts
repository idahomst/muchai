import type { AppConfig } from "$lib/types";

export type Theme = AppConfig["theme"]; // "dark" | "light"

/**
 * Apply a theme to the document and cache it for pre-paint on the next launch.
 * Only "light" sets the data-theme attribute; anything else (including unknown
 * or missing values) removes it, so the default and every failure path is dark.
 */
export function applyTheme(theme: Theme | string | null | undefined): void {
  const root = document.documentElement;
  if (theme === "light") {
    root.dataset.theme = "light";
  } else {
    delete root.dataset.theme;
  }
  try {
    localStorage.setItem("theme", theme === "light" ? "light" : "dark");
  } catch {
    // localStorage blocked/unavailable — pre-paint cache just won't update;
    // config still drives the theme on the next mount.
  }
}
