import { marked } from "marked";
import { openExternal } from "./api";

import basics from "../../docs/guide-basics.md?raw";
import advanced from "../../docs/guide-advanced.md?raw";

// The screenshots are Vite assets, not files the webview can reach by path: a
// build hashes and moves them. Eager so a guide renders in one pass with no
// per-image await, keyed by bare filename to match how the markdown writes it
// ("screenshots/foo.png", relative to docs/, which is what GitHub needs too).
const shots: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob("../../docs/screenshots/*.png", {
      eager: true,
      query: "?url",
      import: "default",
    }) as Record<string, string>,
  ).map(([path, url]) => [path.split("/").pop() as string, url]),
);

export type GuideId = "basics" | "advanced";

export const GUIDES: { id: GuideId; title: string; blurb: string }[] = [
  {
    id: "basics",
    title: "Making your first picture",
    blurb: "Never used an image generator before? Start here.",
  },
  {
    id: "advanced",
    title: "Getting more out of MuchAI",
    blurb: "Model families, memory, LoRAs, editing, devices.",
  },
];

const source: Record<GuideId, string> = { basics, advanced };

// Each guide links to the other by filename, which is what GitHub renders from.
// In the app those have to become a switch of the open guide instead.
const CROSS_LINK: Record<string, GuideId> = {
  "guide-basics.md": "basics",
  "guide-advanced.md": "advanced",
};

// The input is our own markdown, shipped inside the binary — not user content —
// so there is nothing here to sanitize against.
export const render = (id: GuideId): string => marked.parse(source[id], { async: false });

/**
 * Point the rendered `<img>` tags at their bundled assets and give every link
 * somewhere sensible to go. Called on the container after `{@html …}` has been
 * written into it; the alternative — a custom `marked` renderer — pins us to an
 * API that has changed shape across major versions.
 */
export function enhance(el: HTMLElement, onNavigate: (id: GuideId) => void): void {
  for (const img of el.querySelectorAll("img")) {
    const name = img.getAttribute("src")?.split("/").pop();
    const url = name ? shots[name] : undefined;
    // An unresolved image is a broken guide, not something to paper over with a
    // placeholder: drop it and leave the surrounding prose to stand alone.
    if (url) img.src = url;
    else img.remove();
  }
  for (const a of el.querySelectorAll("a")) {
    const href = a.getAttribute("href") ?? "";
    const cross = CROSS_LINK[href];
    a.addEventListener("click", (e) => {
      // A plain <a> would navigate the webview off the app with no way back.
      // Anything that is neither a sibling guide nor an http(s) URL does nothing.
      e.preventDefault();
      if (cross) onNavigate(cross);
      else if (/^https?:\/\//.test(href)) void openExternal(href);
    });
  }
}
