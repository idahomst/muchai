# About Dialog + README + MIT License — Design

**Date:** 2026-07-23
**Status:** Approved (design)

## Problem / Use case

MuchAI is about to switch from a private to a **public** repository. Two gaps
block that:

1. There is no in-app way to see what MuchAI is, who built it, and — importantly
   — **to whom and for what the project is grateful** (the open-source engine,
   frameworks, model community, and the tools that inspired it).
2. `README.md` is still the default Tauri/SvelteKit template stub, and the repo
   has **no license** at all. A public repo with no license defaults to
   "all rights reserved," which contradicts the intent of sharing the tool.

This feature closes all three: an **About dialog**, a rewritten **README**, and
an **MIT `LICENSE`** — a single credits list feeding the first two.

## Goal

Ship an in-app About dialog (credits + version + license), a proper README, and
an MIT license, so the repository can be made public with the project's thanks
visible both in the app and in the repo.

## Non-goals (YAGNI)

- No per-model / per-author credit list. Models are credited collectively as
  "Hugging Face and its community" (user decision). Individual model orgs are
  intentionally NOT enumerated (avoids an ever-growing, hard-to-maintain list).
- No new frontend test runner. The project has only `svelte-check` for the
  frontend; verification is `npm run check` + manual. Do not add vitest/jest.
- No Rust/backend changes. The needed `open_url` command already exists.
- No screenshot asset authored here (README uses a placeholder line the owner
  can fill in later); no project web page (separate, unscheduled idea).
- No settings/config for the About dialog — it is stateless and read-only.

## Decisions locked in during brainstorming

- **Trigger:** clicking the existing `v{version}` label in the ResourceMonitor
  status bar opens the dialog (not a new header button).
- **Credits depth:** curated by category; models → "Hugging Face and its
  community."
- **Inspirations:** must credit **DrawThings** and **Neural-Pixel**.
- **Developer credit:** the app was developed by **Claude Opus** — credited in
  a friendly tone, for the owner Martin Stepanek.
- **License:** **MIT** (matches stable-diffusion.cpp; simplest permissive
  license). Add a `LICENSE` file and reference it in About + README.
- **README:** written in the same effort, sharing the credits list.

## Architecture

A single canonical content module `src/lib/about.ts` holds the tagline and the
ordered credits sections. The About dialog renders from that module; the README
prose mirrors the same content (there is no runtime import between a Svelte
component and a Markdown file — "shared" means one canonical source of truth that
both are written from). The feature is **frontend-only** and reuses:

- the existing Rust `open_url` command (https-only, already registered) for
  external links,
- the `WelcomeDialog.svelte` modal pattern (backdrop, `role="dialog"
  aria-modal`, Escape + backdrop-click close, focus-on-open, `onclose` prop,
  theme tokens),
- the existing `v{version}` label in `ResourceMonitor.svelte`.

### Content module — `src/lib/about.ts` (new)

Exports:

- `APP_TAGLINE: string` — one line, e.g.
  "Make images from text, right on your own machine."
- `CREDITS: CreditSection[]` where
  `CreditSection = { heading: string; items: CreditItem[] }` and
  `CreditItem = { label: string; url?: string; note?: string }`.

Sections, in order:

1. **Image engine** — `stable-diffusion.cpp` by leejet & contributors,
   `https://github.com/leejet/stable-diffusion.cpp`, note "the Vulkan/ggml engine
   that renders every image."
2. **Built with** — Tauri (`https://tauri.app`), Svelte
   (`https://svelte.dev`), Vite (`https://vite.dev`), Rust
   (`https://www.rust-lang.org`).
3. **Models** — "Hugging Face and its community" (`https://huggingface.co`),
   note "for the open models MuchAI can download and run."
4. **Inspired by** — DrawThings (`https://drawthings.ai`), Neural-Pixel.
5. **Developed by** — "Claude Opus (Anthropic)", note
   "designed & built with Claude, for Martin Stepanek."

All `url` values MUST be `https://` (the backend `open_url` rejects anything
else).

### Component — `src/lib/components/AboutDialog.svelte` (new)

Props: `{ onclose: () => void }`. Structure mirrors `WelcomeDialog.svelte`:

- `<svelte:window onkeydown>` closing on `Escape`.
- `.backdrop` (closes on click-outside) → `.dialog`
  (`role="dialog" aria-modal="true" aria-labelledby`).
- Content: `<h2 id="about-title">About MuchAI</h2>` with the version
  `v{version}` (imported from `../../../package.json`, same as ResourceMonitor);
  the tagline; then the `CREDITS` sections. Each item renders its `label`
  (as a link-styled `<button>` calling `openUrl(url)` when it has a `url`,
  otherwise plain text) followed by its `note`.
- Footer line: "© 2026 Martin Stepanek · MIT License".
- A single `Close` button, focused on open (`$effect(() => closeBtn?.focus())`).
- Styling uses the same theme tokens as WelcomeDialog (`--dialog-bg`,
  `--border`, `--backdrop`, `--accent*`, `--text`) so dark AND light both read
  correctly. The dialog is a top-level `position:fixed` overlay and contains NO
  InfoHint tooltips, so the onboarding tooltip portal/stacking issue does not
  apply.

### API wrapper — `src/lib/api.ts` (modify)

Add next to `openFolder`:

```ts
export const openUrl = (url: string) => invoke<void>("open_url", { url });
```

### Trigger — `src/lib/components/ResourceMonitor.svelte` (modify)

- Add a prop `let { onAbout }: { onAbout: () => void } = $props();`
  (component currently takes no props; introduce the runes `$props()` for it).
- Change the `<span class="ver">` into a `<button class="ver">` with
  `title="About MuchAI"`, `onclick={onAbout}`, keeping the `.ver` styling
  (reset default button chrome: transparent background, no border, inherit
  font/color) and adding `cursor:pointer`; hover raises opacity to signal it is
  clickable.

### Wiring — `src/routes/+page.svelte` (modify)

- Import `AboutDialog`.
- Add `let showAbout = $state(false);`.
- Pass `onAbout={() => (showAbout = true)}` to `<ResourceMonitor />`.
- Render alongside the other dialogs:
  `{#if showAbout}<AboutDialog onclose={() => (showAbout = false)} />{/if}`.

### `LICENSE` (new, repo root)

Standard MIT license text, "Copyright (c) 2026 Martin Stepanek".

### `README.md` (rewrite, replacing the template stub)

Sections:

- **Title + tagline** and a screenshot placeholder
  (`<!-- screenshot: docs/screenshot.png -->`).
- **What it is / Features** — local (offline) text-to-image; model library with
  curated hardware-aware downloads; live preview during generation; dark/light
  themes; runs on GPU (Vulkan, multi-vendor) or CPU; live resource monitor.
- **Requirements** — Linux x86_64; a Vulkan-capable GPU (NVIDIA/AMD/Intel) or
  CPU fallback; glibc ≥ 2.38.
- **Install** — (a) download the AppImage from Releases; (b) build from source:
  `bash scripts/fetch-engine.sh` → `npm install` → `npm run tauri dev` (dev) or
  `bash scripts/build-appimage.sh` (release).
- **Acknowledgements** — the same credits as the About dialog (engine, built
  with, Hugging Face community, inspired by DrawThings + Neural-Pixel, developed
  with Claude Opus).
- **A note on models** — model weights carry their own licenses (some
  non-commercial); respecting each model's license is the user's responsibility.
- **License** — MIT, linking the `LICENSE` file.

## Data flow

1. User clicks `v{version}` in the status bar → `onAbout()` → `showAbout = true`.
2. `AboutDialog` renders from `about.ts` (`CREDITS` + tagline + version + footer).
3. User clicks a credit link → `openUrl(url)` → Rust `open_url` (https-only) →
   the URL opens in the system browser.
4. Escape / backdrop click / Close → `onclose()` → `showAbout = false`.

## Error handling

- **Link open failure:** `openUrl` is best-effort. The click handler catches and
  ignores any rejection (a dead or unreachable link never crashes the app or
  surfaces an error), matching the spirit of `openFolder`.
- The dialog is stateless and read-only; there is no persistence to fail.

## Testing

- **Backend:** unchanged → `cargo test` stays at 211 passing.
- **Frontend:** `npm run check` must be clean (0 errors / 0 warnings). No unit
  test runner is introduced.
- **Manual (owner):** the version label opens the dialog; each credit link opens
  in the browser; Escape, backdrop click, and Close all dismiss it; the dialog is
  readable in BOTH dark and light themes; the footer shows the MIT line.
- **README/LICENSE:** proofread; verify the build-from-source commands match the
  actual scripts (`scripts/fetch-engine.sh`, `scripts/build-appimage.sh`).

## Out of scope / follow-ups

- Switching the repository to public (a manual step the owner takes AFTER this
  lands).
- A screenshot asset for the README (owner adds later).
- UX polish pass and pre-download free-space check remain separate post-beta
  items.
