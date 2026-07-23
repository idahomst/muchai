# About Dialog + README + MIT License Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an in-app About dialog (credits + version + license), a rewritten README, and an MIT LICENSE, so the repo can be made public with the project's thanks visible in-app and in the repo.

**Architecture:** Frontend-only. A single canonical content module (`src/lib/about.ts`) feeds the About dialog; the README prose mirrors the same credits. The dialog opens from the existing `v{version}` status-bar label, mirrors the `WelcomeDialog` modal pattern, and opens external links through the existing https-only `open_url` Rust command. No backend changes.

**Tech Stack:** Svelte 5 (runes), TypeScript, Tauri v2. Spec: `docs/superpowers/specs/2026-07-23-about-dialog-design.md`.

**Testing note (read first):** This project has **no frontend unit-test runner** — the only frontend gate is `svelte-check` (`npm run check`), plus manual verification. Classic red-green TDD does not apply to these Svelte/TS changes, so each task's "test" step is a **`npm run check` clean** gate (0 errors / 0 warnings) and, where useful, a targeted reasoning/grep check. The backend is untouched, so `cargo test` must stay at **211 passing**. Do NOT introduce vitest/jest (YAGNI, per spec).

**Commit convention:** every commit body ends with:
`Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`

---

### Task 1: Credits content module

**Files:**
- Create: `src/lib/about.ts`

- [ ] **Step 1: Create the content module**

Create `src/lib/about.ts` with the exact content below. All `url` values MUST be `https://` (the backend `open_url` rejects any other scheme). Models are credited collectively (no per-author list). Neural-Pixel intentionally has no `url` (no confirmed official link).

```ts
export const APP_TAGLINE = "Make images from text, right on your own machine.";

export interface CreditItem {
  label: string;
  url?: string;
  note?: string;
}

export interface CreditSection {
  heading: string;
  items: CreditItem[];
}

export const CREDITS: CreditSection[] = [
  {
    heading: "Image engine",
    items: [
      {
        label: "stable-diffusion.cpp",
        url: "https://github.com/leejet/stable-diffusion.cpp",
        note: "by leejet & contributors — the Vulkan/ggml engine that renders every image.",
      },
    ],
  },
  {
    heading: "Built with",
    items: [
      { label: "Tauri", url: "https://tauri.app" },
      { label: "Svelte", url: "https://svelte.dev" },
      { label: "Vite", url: "https://vite.dev" },
      { label: "Rust", url: "https://www.rust-lang.org" },
    ],
  },
  {
    heading: "Models",
    items: [
      {
        label: "Hugging Face and its community",
        url: "https://huggingface.co",
        note: "for the open models MuchAI can download and run.",
      },
    ],
  },
  {
    heading: "Inspired by",
    items: [
      { label: "Draw Things", url: "https://drawthings.ai" },
      { label: "Neural-Pixel" },
    ],
  },
  {
    heading: "Developed by",
    items: [
      {
        label: "Claude Opus (Anthropic)",
        note: "designed & built with Claude, for Martin Stepanek.",
      },
    ],
  },
];
```

- [ ] **Step 2: Type-check**

Run: `npm run check`
Expected: `0 ERRORS 0 WARNINGS` (the module has no consumers yet; this just confirms it type-checks).

- [ ] **Step 3: Commit**

```bash
git add src/lib/about.ts
git commit -m "feat(about): add credits content module

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `openUrl` API wrapper + AboutDialog component

**Files:**
- Modify: `src/lib/api.ts`
- Create: `src/lib/components/AboutDialog.svelte`

- [ ] **Step 1: Add the `openUrl` wrapper**

In `src/lib/api.ts`, add this export directly below the existing `openFolder` line (`export const openFolder = (path: string) => invoke<void>("open_path", { path });`). The `invoke` import already exists in this file.

```ts
export const openUrl = (url: string) => invoke<void>("open_url", { url });
```

- [ ] **Step 2: Create the AboutDialog component**

Create `src/lib/components/AboutDialog.svelte` with the exact content below. It mirrors `WelcomeDialog.svelte` (backdrop, `role="dialog" aria-modal`, Escape + backdrop-click close, focus-on-open, `onclose` prop, theme tokens). The version is imported from `package.json` exactly as `ResourceMonitor.svelte` does (`../../../package.json`).

```svelte
<script lang="ts">
  import { version } from "../../../package.json";
  import { APP_TAGLINE, CREDITS } from "../about";
  import { openUrl } from "../api";

  let { onclose }: { onclose: () => void } = $props();
  let closeBtn = $state<HTMLButtonElement>();

  $effect(() => { closeBtn?.focus(); });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  // Best-effort: a dead/unreachable link must never crash or alert (mirrors
  // openFolder's fire-and-forget spirit).
  function open(url: string | undefined) {
    if (url) openUrl(url).catch(() => {});
  }
</script>

<svelte:window {onkeydown} />

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-labelledby="about-title">
    <h2 id="about-title">About MuchAI <span class="ver">v{version}</span></h2>
    <p class="tagline">{APP_TAGLINE}</p>

    {#each CREDITS as section}
      <div class="section">
        <h3>{section.heading}</h3>
        <ul>
          {#each section.items as item}
            <li>
              {#if item.url}
                <button class="link" onclick={() => open(item.url)}>{item.label}</button>
              {:else}
                <span class="name">{item.label}</span>
              {/if}
              {#if item.note}<span class="note"> — {item.note}</span>{/if}
            </li>
          {/each}
        </ul>
      </div>
    {/each}

    <p class="footer">© 2026 Martin Stepanek · MIT License</p>

    <div class="row">
      <button class="btn-primary" bind:this={closeBtn} onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:var(--backdrop); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--dialog-bg); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(460px, 92vw); max-height:88vh; overflow-y:auto;
    display:flex; flex-direction:column; gap:.6rem; }
  h2 { margin:0; font-size:1.1rem; display:flex; align-items:baseline; gap:.5rem; }
  .ver { font-size:.8rem; opacity:.55; font-weight:normal; }
  .tagline { margin:0; font-size:.85rem; opacity:.85; }
  .section { display:flex; flex-direction:column; gap:.2rem; }
  .section h3 { margin:.3rem 0 0; font-size:.72rem; text-transform:uppercase;
    letter-spacing:.04em; opacity:.6; }
  ul { margin:0; padding-left:1.1rem; display:flex; flex-direction:column; gap:.15rem;
    font-size:.82rem; line-height:1.4; }
  .link { font:inherit; padding:0; background:none; border:none; cursor:pointer;
    color:var(--accent-bright); text-decoration:underline; }
  .note { opacity:.75; }
  .footer { margin:.4rem 0 0; font-size:.75rem; opacity:.6; }
  .row { display:flex; justify-content:flex-end; margin-top:.3rem; }
  .btn-primary { font:inherit; font-size:.85rem; padding:.4rem .9rem; cursor:pointer; }
</style>
```

- [ ] **Step 3: Type-check**

Run: `npm run check`
Expected: `0 ERRORS 0 WARNINGS`.

- [ ] **Step 4: Commit**

```bash
git add src/lib/api.ts src/lib/components/AboutDialog.svelte
git commit -m "feat(about): AboutDialog component + openUrl api wrapper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Wire the trigger (version label → dialog)

**Files:**
- Modify: `src/lib/components/ResourceMonitor.svelte`
- Modify: `src/routes/+page.svelte`

- [ ] **Step 1: Make the version label a button in ResourceMonitor**

In `src/lib/components/ResourceMonitor.svelte`:

1a. Add a props declaration at the top of the `<script>` block (immediately after the existing imports, before `const gb = ...`). The component currently takes no props:

```svelte
  let { onAbout }: { onAbout: () => void } = $props();
```

1b. Replace the version `<span>`:

```svelte
  <span class="ver" title="App version">v{version}</span>
```

with a button:

```svelte
  <button class="ver" title="About MuchAI" onclick={onAbout}>v{version}</button>
```

1c. Replace the `.ver` CSS rule:

```css
  .ver { margin-left:auto; opacity:.55; padding-left:1rem; }
```

with a button-chrome-reset version that keeps the same look and signals it is clickable:

```css
  .ver { margin-left:auto; opacity:.55; padding-left:1rem; font:inherit;
    background:none; border:none; cursor:pointer; color:inherit; }
  .ver:hover { opacity:.9; }
```

- [ ] **Step 2: Render the dialog from +page.svelte**

In `src/routes/+page.svelte`:

2a. Add the import alongside the other dialog imports (near the `WelcomeDialog` / `PreferencesDialog` imports):

```svelte
  import AboutDialog from "$lib/components/AboutDialog.svelte";
```

2b. Add state next to the other dialog flags (near `let showPrefs = $state(false);`):

```svelte
  let showAbout = $state(false);
```

2c. Pass the callback to the ResourceMonitor. Replace `<ResourceMonitor />` with:

```svelte
<ResourceMonitor onAbout={() => (showAbout = true)} />
```

2d. Render the dialog alongside the others (after the `{#if showPrefs}…{/if}` block):

```svelte
{#if showAbout}
  <AboutDialog onclose={() => (showAbout = false)} />
{/if}
```

- [ ] **Step 3: Type-check the whole app**

Run: `npm run check`
Expected: `0 ERRORS 0 WARNINGS`. This confirms the prop threads correctly (ResourceMonitor now requires `onAbout`; the only call site supplies it) and the dialog compiles into the app.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/ResourceMonitor.svelte src/routes/+page.svelte
git commit -m "feat(about): open About dialog from the version label

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: MIT LICENSE

**Files:**
- Create: `LICENSE` (repo root)

- [ ] **Step 1: Create the LICENSE file**

Create `LICENSE` at the repository root with the exact standard MIT text below (verbatim — do not reword; this is the canonical OSI MIT template with the year/holder filled in):

```
MIT License

Copyright (c) 2026 Martin Stepanek

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

- [ ] **Step 2: Verify**

Run: `head -3 LICENSE`
Expected: first line `MIT License`, third line `Copyright (c) 2026 Martin Stepanek`.

- [ ] **Step 3: Commit**

```bash
git add LICENSE
git commit -m "docs: add MIT LICENSE

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Rewrite README

**Files:**
- Modify (full rewrite): `README.md`

- [ ] **Step 1: Verify the build scripts referenced below exist**

Run: `ls scripts/fetch-engine.sh scripts/build-appimage.sh`
Expected: both paths exist. (They are referenced in the README's build steps; confirm the names before writing.)

- [ ] **Step 2: Replace README.md**

Overwrite `README.md` entirely with the content below. The Acknowledgements section mirrors `src/lib/about.ts` — if you changed a credit there, keep them in sync.

````markdown
# MuchAI

**Make images from text, right on your own machine.**

MuchAI is a local, offline text-to-image desktop app. It wraps
[stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp) in a
simple GUI: pick a model, type a prompt, press Generate. Nothing leaves your
computer.

<!-- screenshot: docs/screenshot.png -->

## Features

- **Text-to-image generation**, fully local and offline.
- **Model library with curated downloads** — hardware-aware starter models rated
  for your VRAM, plus paste-a-URL for your own.
- **Live preview** — watch a rough draft form as it generates, so you can cancel
  early when the composition is wrong.
- **Runs on GPU or CPU** — Vulkan backend across NVIDIA / AMD / Intel, with a CPU
  fallback.
- **Live resource monitor** — GPU / VRAM / CPU / RAM usage while you work.
- **Dark and light themes.**

## Requirements

- Linux x86_64.
- A Vulkan-capable GPU (NVIDIA / AMD / Intel) — or run on CPU (slower).
- glibc 2.38 or newer.

## Install

### Download (recommended)

Grab the latest `MuchAI-*.AppImage` from the
[Releases](https://github.com/idahomst/muchai/releases) page, make it
executable, and run it:

```bash
chmod +x MuchAI-*.AppImage
./MuchAI-*.AppImage
```

### Build from source

Prerequisites: a [Rust toolchain](https://www.rust-lang.org/tools/install),
[Node.js](https://nodejs.org) (18+), and the system libraries Tauri needs
(WebKitGTK etc. — see the [Tauri Linux setup guide](https://tauri.app/start/prerequisites/)).

```bash
npm install
bash scripts/fetch-engine.sh   # downloads the pinned stable-diffusion.cpp engine
npm run tauri dev              # run in development
# or build a release AppImage:
bash scripts/build-appimage.sh
```

## A note on models

MuchAI runs open model weights that each carry **their own license** — some are
restricted to non-commercial use. MuchAI does not grant you any rights to the
models; respecting each model's license is your responsibility.

## Acknowledgements

- **Image engine:** [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp)
  by leejet & contributors — the Vulkan/ggml engine that renders every image.
- **Built with:** [Tauri](https://tauri.app), [Svelte](https://svelte.dev),
  [Vite](https://vite.dev), and [Rust](https://www.rust-lang.org).
- **Models:** thanks to [Hugging Face](https://huggingface.co) and its community
  for the open models MuchAI can download and run.
- **Inspired by:** [Draw Things](https://drawthings.ai) and Neural-Pixel.
- **Developed by:** Claude Opus (Anthropic) — designed & built with Claude, for
  Martin Stepanek.

## License

[MIT](LICENSE) © 2026 Martin Stepanek.
````

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: rewrite README for public release

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Full-suite verification

**Files:** none (verification only).

- [ ] **Step 1: Frontend type-check**

Run: `npm run check`
Expected: `0 ERRORS 0 WARNINGS`.

- [ ] **Step 2: Backend unchanged**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: `test result: ok. 211 passed` (no backend files were touched by this feature).

- [ ] **Step 3: Manual checklist (hand off to the owner — cannot be automated)**

Launch the app (`npm run tauri dev`) and confirm:
- Clicking the `v{version}` label in the bottom status bar opens the About dialog.
- Each credit link opens in the system browser (engine, Tauri/Svelte/Vite/Rust, Hugging Face, Draw Things).
- Neural-Pixel shows as plain text (no link) and the "Developed by" note shows as plain text.
- Escape, clicking the backdrop, and the Close button all dismiss the dialog.
- The dialog is readable in BOTH dark and light themes.
- The footer shows "© 2026 Martin Stepanek · MIT License".

- [ ] **Step 4: Confirm no stray changes**

Run: `git status` and `git log --oneline main..HEAD`
Expected: working tree clean; six commits (Tasks 1–5, one each; Task 6 adds none).

---

## Post-implementation

After all tasks pass and the final code review is done, use
**superpowers:finishing-a-development-branch** to merge `feat/about-readme-license`
to `main` and push (matching the project's established pattern).

**Then (manual, owner):** switch the `idahomst/muchai` repository to **public** —
the README, LICENSE, and in-app About dialog are the gate for that step.

