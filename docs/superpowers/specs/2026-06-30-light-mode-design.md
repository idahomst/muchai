# Light Mode — Design

**Date:** 2026-06-30
**Status:** Approved (brainstorm complete)

## Problem

MuchAI is dark-only. Colors are split between a tiny set of CSS variables in
`src/app.css` (`--border`, `--accent`) and ~16 hardcoded hex/rgba literals
scattered across 9 component `<style>` blocks. Users who prefer a light UI have
no option. We want a light theme the user can toggle, persisted across restarts.

## Goal

Add a manual **Dark/Light** theme toggle in the header. Persist the choice in
`AppConfig` (source of truth) and apply it via a `data-theme` attribute backed by
semantic CSS variables. Default to **dark** for new and existing users.

## Decisions (from brainstorm)

- **Behavior:** manual toggle only. No `prefers-color-scheme` / system-follow.
  The field is `theme: "dark" | "light"`.
- **Placement:** a small sun/moon icon button in the header next to the `MuchAI`
  brand. Sun ☀ shown in dark mode (click → light); moon ☽ in light (click → dark).
- **Default:** dark, for both new installs and existing config files (via serde
  default), so current users see no change until they switch.
- **Palette:** "cool neutral gray" light theme (page `#f4f4f6`, white panels,
  cool-gray borders, accent darkened to `#3a63d6` for contrast on white).
- **Approach:** semantic CSS-variable tokens swapped by `data-theme`, plus a
  `localStorage` pre-paint cache to avoid a startup flash for light-mode users.

## Architecture

All themeable colors become CSS custom properties defined in `src/app.css`.
`:root` holds the **dark** values (today's look); `:root[data-theme="light"]`
overrides only the tokens that change. Components reference tokens instead of
literals. The active theme is applied by setting
`document.documentElement.dataset.theme`; **absence of the attribute = dark**
(so the default and any failure path is the current look).

The Rust `AppConfig.theme` is the source of truth (persisted to `config.json`).
`localStorage.theme` is a write-through cache used **only** by a pre-paint inline
script so the correct theme is set before first paint.

### Token set

Core surface/text/line tokens (dark default → light):

| Token | Dark | Light | Used for |
|---|---|---|---|
| `--bg` | `#1b1b1f` | `#f4f4f6` | page background (`body`) |
| `--surface` | `#2a2a30` | `#ffffff` | buttons, inputs, raised panels |
| `--dialog-bg` | `#1e1e1e` | `#ffffff` | modal dialog background |
| `--text` | `#e8e8ea` | `#1d1d20` | primary text |
| `--text-muted` | `#9a9aa2` | `#6b6b75` | secondary/`.label` text |
| `--border` | `#333` | `#dcdce3` | panel/control borders |
| `--border-subtle` | `rgba(255,255,255,.1)` | `rgba(0,0,0,.08)` | hairline dividers |
| `--accent` | `#4f7cff` | `#3a63d6` | primary buttons, focus |
| `--accent-bright` | `#6ea8fe` | `#2a4fb0` | links/highlight text |
| `--accent-tint` | `rgba(110,168,254,.12)` | `rgba(58,99,214,.10)` | selected/hover row tint |
| `--on-accent` | `#ffffff` | `#ffffff` | text on accent/danger fills |

Status tokens — brights darkened in light for AA contrast on white:

| Token | Dark | Light | Used for |
|---|---|---|---|
| `--danger` | `#ff6b6b` | `#c8362f` | error text |
| `--danger-soft` | `#ffb4b4` | `#c8362f` | softer error text |
| `--danger-bg` | `rgba(180,40,40,.85)` | `#c8362f` | delete-confirm button fill |
| `--danger-tint` | `rgba(255,80,80,.15)` | `rgba(200,54,47,.10)` | error notice background |
| `--warn` | `#ffd9a8` | `#9a5b00` | "Running on CPU" notice text |
| `--warn-tint` | `rgba(255,180,80,.15)` | `rgba(154,91,0,.10)` | CPU notice background |
| `--success` | `#4caf83` | `#2e8b5f` | "✓ ready" download status |

Overlay tokens (image-action overlays sit on dark image pixels, so they stay
dark in both themes; only the dialog backdrop lightens slightly):

| Token | Dark | Light | Used for |
|---|---|---|---|
| `--overlay` | `rgba(0,0,0,.6)` | `rgba(0,0,0,.6)` | hover/action overlays on images |
| `--overlay-soft` | `rgba(0,0,0,.2)` | `rgba(0,0,0,.2)` | subtle image scrim |
| `--backdrop` | `rgba(0,0,0,.5)` | `rgba(0,0,0,.4)` | modal backdrop |

`color-scheme` also switches (`dark` → `light`) so native scrollbars, focus
rings, and form controls follow the theme.

**Note:** earlier color audits flagged `#eac` in several files — those are false
positives from `{#each ...}` blocks, not colors. The real literals are the ones
mapped above.

## Components / files

- **`src/app.css`** — define all tokens (`:root` dark, `:root[data-theme="light"]`
  overrides); set `color-scheme` per theme; replace the `body`/`button`/`.btn-primary`/
  `.label` literals with tokens.
- **`src/lib/theme.ts`** *(new)* — `applyTheme(theme: "dark" | "light")`:
  sets `document.documentElement.dataset.theme` (only `"light"` sets the attribute;
  anything else removes it → dark) and writes `localStorage.theme` inside `try/catch`.
- **`src/lib/components/ThemeToggle.svelte`** *(new)* — header icon button.
  Reads `$settings.theme`; on click computes `next`, calls `applyTheme(next)`
  immediately, `settings.set(nextConfig)` (optimistic), `await setSettings(nextConfig)`;
  on error reverts the store **and** re-applies the previous theme. Mirrors the
  `ParamsPanel.svelte` optimistic-persist pattern. Has `aria-label`
  ("Switch to light theme" / "Switch to dark theme").
- **`src/routes/+page.svelte`** — render `<ThemeToggle />` in the header next to
  `<h1 class="brand">`; after `getSettings()` in `onMount`, call `applyTheme(cfg.theme)`.
- **`src/app.html`** — inline pre-paint script in `<head>`:
  `try { if (localStorage.getItem('theme') === 'light') document.documentElement.dataset.theme = 'light'; } catch (e) {}`
- **Component `<style>` sweeps** — replace literals with tokens in:
  `DownloadDialog.svelte` (dialog-bg, backdrop, accent-tint), `GenerateBar.svelte`
  (border-subtle, danger/warn tints + text), `ImagePreview.svelte` (overlays,
  danger-bg, on-accent, danger-soft), `ModelLibrary.svelte` (danger-bg, danger,
  border-subtle, accent-bright, success, on-accent), `HistoryStrip.svelte`
  (overlay, on-accent), `ModelFolders.svelte` (danger), `GalleryLocation.svelte`
  (danger), `ParamsPanel.svelte` (danger), `DevicePicker.svelte` (danger).
- **`src-tauri/src/types.rs`** — `Theme { Dark, Light }` enum, `#[serde(rename_all = "snake_case")]`,
  `impl Default for Theme { Dark }`; add `#[serde(default)] pub theme: Theme` to `AppConfig`.
- **`src-tauri/src/config.rs`** — `default_config()` sets `theme: Theme::default()`.
- **`src/lib/types.ts`** — add `theme: "dark" | "light"` to the `AppConfig` interface.

## Data flow

**Load:**
1. `app.html` inline script sets `data-theme="light"` from `localStorage` before
   first paint (no flash). No entry → stays dark.
2. `onMount` → `getSettings()` → `applyTheme(cfg.theme)` reconciles authoritatively
   (corrects a stale localStorage value against the persisted config).

**Toggle:**
1. `applyTheme(next)` — instant visual swap + localStorage write.
2. `settings.set(nextConfig)` — optimistic store update.
3. `await setSettings(nextConfig)` — persist to `config.json`.
4. On error: `settings.set(prev)` + `applyTheme(prev.theme)` — full revert.

## Error handling

- **Persist failure:** revert store and re-apply the previous theme (no half-applied
  state).
- **Missing/unknown `theme`:** treated as dark — Rust serde default (`Theme::Dark`)
  for legacy/garbled config; JS guard applies light only on exact `"light"`.
- **localStorage unavailable/blocked:** `try/catch` swallows; theme still applies
  from config on mount (only cost: a brief dark flash for a light-mode user).

## Testing

- **Rust unit tests** (`config.rs` / `types.rs`):
  - `default_config().theme == Theme::Dark`.
  - Legacy config JSON without a `theme` key loads as `Theme::Dark` (serde default).
  - `AppConfig` round-trips through JSON for both `dark` and `light`; `Theme`
    serializes to `"dark"` / `"light"` (snake_case contract for the TS mirror).
- **`npm run check`** (svelte-check) — 0 errors after the new component + sweeps.
- **Manual E2E** on the dev box:
  - Toggle flips the whole UI instantly (panels, dialogs, history strip, monitor).
  - Choice persists across an app restart.
  - Light-mode launch shows no dark flash (pre-paint cache works).
  - Every status state is legible in light: CPU notice, error text/notice,
    delete-confirm button, download "✓ ready", selection ring, links.

## Scope / YAGNI

No system-preference following, no per-element color customization, no additional
themes beyond dark/light, no animated theme transitions. CPU/RAM/engine behavior
unchanged. Only `AppConfig` gains one `theme` field (additive, `#[serde(default)]`).
