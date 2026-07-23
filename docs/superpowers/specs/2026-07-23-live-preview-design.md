# Live Preview During Generation — Design

**Date:** 2026-07-23
**Status:** Approved (design)

## Problem / Use case

While an image is being generated, the user cannot see it forming. If the
composition is clearly wrong (subject in the wrong place, wrong framing), they
have no way to know until the full run finishes — wasting time and compute.
DrawThings shows a blurry image after the first few steps that sharpens as
generation proceeds, letting the user cancel early when the composition is off.

The pinned stable-diffusion.cpp engine (`b290693`) already supports this via its
`--preview` flags. This feature surfaces it in MuchAI.

## Goal

During a generation run, show a rough live draft of the image as it forms in the
main preview area. Combined with the existing Cancel button (now a clean no-op),
this gives the "see it's wrong → stop → don't waste time" loop.

## Non-goals (YAGNI)

- No `tae` or `vae` preview methods. Only `proj` (cheap linear latent→RGB
  projection, no extra model download, negligible overhead).
- No per-family verification pass at build time — `proj` is used for all
  families; rough output on any given family will be caught during normal
  testing, not in a dedicated sweep.
- No configurable preview interval or method in the UI. Fixed: `proj`, interval 2.

## Engine capability (confirmed)

`sd-cli --help` on the pinned engine exposes:

- `--preview <method>` — one of `none | proj | tae | vae` (default `none`).
- `--preview-path <file>` — file the engine overwrites with the current preview
  (default `./preview.png`).
- `--preview-interval <int>` — update every N denoising steps (default 1).

We use `--preview proj --preview-path <fixed> --preview-interval 2`.

## Architecture

### Refresh mechanism (approach A: progress-tick cache-bust)

The backend emits a single `generation:preview { path }` event immediately after
spawning the engine (only when preview is enabled). The frontend stores that
path and, on each existing `generation:progress` tick, reloads the `<img>` with
a cache-busting `?t=<step>` query. This reuses the progress heartbeat already
wired end-to-end — no file-watcher thread, no independent polling timer.

- The engine writes the preview file every 2 steps while progress events fire
  every step, so some reloads hit an unchanged file — harmless.
- Before the first frame exists (steps 1–2), the `<img onerror>` keeps the prior
  view (previous result or empty state).

Rejected alternatives: (B) backend mtime file-watcher — extra thread + lifecycle
for a file that changes ~10×/run; (C) frontend `setInterval` poll — second timing
source that keeps running if events stall.

### Preview file location + asset scope

- Fixed path: `<std::env::temp_dir()>/muchai-preview/preview.png`.
  - OS temp dir is tmpfs on the dev box → the tiny, constantly-overwritten file
    is RAM-backed (no SSD wear) and auto-cleared on reboot.
  - A fixed (non-unique) path is safe because generation is single-flight (one
    `child` slot in `AppState`).
- The `muchai-preview` directory is created and added to the Tauri
  asset-protocol scope once in `lib.rs` `setup()`
  (`app.asset_protocol_scope().allow_directory(dir, true)`), mirroring how the
  gallery dir is allowed, so `convertFileSrc` can load the preview file.
- The preview file has no `.json` sidecar, and it lives outside the gallery dir
  regardless, so it never appears in the history strip
  (`gallery::list_items` reads only `*.json` sidecars).

### Lifecycle / cleanup

- `generate` deletes the preview file when the run ends by ANY path: success,
  error, or cancel (best-effort `remove_file`, ignore "not found").
- `cancel_generation` ALSO deletes the preview file immediately after killing the
  child (belt-and-suspenders: cancel races the run's own cleanup). This is the
  explicit user requirement — pressing Cancel leaves no `preview.png` behind.
- Deleting the file is best-effort; a failure to delete never surfaces as a user
  error.

## Components / changes

### Backend (Rust)

1. **`command_builder.rs`**
   - `EngineOptions` gains `pub preview_path: Option<String>` (default `None`).
     `Some(path)` = preview enabled and write to `path`; `None` = disabled. A
     single field carries both the on/off state and the path, and keeps
     `build_args` a pure, unit-testable function (the path is injected, not
     computed inside it).
   - When `preview_path` is `Some(p)`, `build_args` appends
     `--preview proj --preview-path <p> --preview-interval 2`. When `None`, none
     of those flags are emitted.
   - A helper `preview_path()` in the command layer (not the pure builder)
     computes the fixed `<temp>/muchai-preview/preview.png`.

2. **`engine.rs`** — no structural change; `run_generation` already forwards
   `EngineOptions` to `build_args`. (Preview-file cleanup lives in the command
   layer, not the engine, so `run_generation` stays a pure spawn/stream unit.)

3. **`commands.rs`**
   - `generate`: when `cfg.live_preview` is true, set
     `EngineOptions.preview_path = Some(preview_path())`, ensure the preview dir
     exists, and emit `generation:preview { path }` after spawn; otherwise leave
     it `None`. On completion (all match arms — success/error/cancel), delete the
     preview file.
   - `cancel_generation`: after `child.kill()`, best-effort delete the preview
     file.

4. **`lib.rs`** `setup()`: create `<temp>/muchai-preview/` and
   `allow_directory` it in the asset-protocol scope.

5. **`types.rs`** `AppConfig`: add `live_preview: bool` with
   `#[serde(default = "default_true")]` so both new configs and pre-feature
   config files default to ON. Add the `default_true` helper.

### Frontend (Svelte/TS)

6. **`stores.ts`**: add `livePreview: Writable<string | null>` (null = no live
   frame; falls back to the final/empty view).

7. **`api.ts`**: add `onPreview(cb: (path: string) => void)` listening to
   `generation:preview`.

8. **`GenerateBar.svelte`**:
   - In `onMount`, subscribe to `onPreview` → store the absolute path in a local
     variable; unsubscribe on destroy.
   - On each `generation:progress` tick, if a preview path is set, set
     `livePreview = convertFileSrc(path) + "?t=" + currentStep`.
   - In `run()`, reset the local path at the start of each run and clear
     `livePreview` to `null` at completion (covers success, error, and the
     empty-array cancel result).

9. **`ImagePreview.svelte`**: display `$livePreview ?? $currentImage`. While a
   live frame shows: hide the Delete button (no gallery item yet) and render a
   small "Preview" badge. Add `<img onerror>` to keep the previous frame on a
   transient 404.

10. **`types.ts`**: mirror `live_preview: boolean` in the `AppConfig` type and
    `defaultConfig` (default `true`).

11. **`SettingsPanel.svelte`**: a "Live preview" checkbox bound to the config
    field (persist via the existing optimistic set-then-`setSettings` pattern),
    with an `InfoHint`.

12. **`helpText.ts`**: add a `livePreview` entry — plain language: shows a rough
    draft of the image as it forms so you can cancel early if the composition is
    wrong; costs almost nothing; may look rough on some models.

## Data flow (one run, preview ON)

1. User presses Generate → `generate` builds args with the `--preview proj …`
   flags, ensures the preview dir exists, spawns the engine, emits
   `generation:preview { path }`.
2. Frontend receives the path; the engine writes `preview.png` every 2 steps.
3. Each `generation:progress` tick → frontend reloads the `<img>` with
   `?t=<step>` → the evolving draft appears in the main preview area.
4. On finish → `generate` returns the gallery item(s), deletes `preview.png`;
   frontend clears `livePreview`, shows the final image.
   On cancel → child killed, `preview.png` deleted (by both cancel and the run's
   own cleanup), `generate` returns `Ok([])`, frontend clears `livePreview` and
   drops to idle with the prior view intact.

## Error handling

- Preview file missing/late: `<img onerror>` keeps the previous view; no error.
- Preview dir creation or file deletion failure: best-effort, never surfaced.
- `proj` renders noise for some family: preview looks rough but generation is
  unaffected (non-destructive); user can toggle preview off. No blocker.

## Testing

- `command_builder`: preview flags (`--preview proj`, `--preview-path`,
  `--preview-interval 2`) present when `EngineOptions.preview_path` is `Some`;
  all absent when `None`.
- `config`/`types`: round-trip test — `live_preview` defaults to `true` for a
  config JSON lacking the key, and survives serialize/deserialize.
- `npm run check` clean; existing 207 Rust tests stay green.
- Manual (user, during other tests): live draft appears and sharpens; Cancel
  leaves no `preview.png`; toggle off disables it.
