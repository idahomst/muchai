# Background Model Downloads — Design Spec

**Date:** 2026-06-30
**Status:** Approved (design)
**Roadmap item:** #3 — "Background model downloads: download one model while generating with a different one; decouple download from the active generate flow."

## Goal

Let the user start a model download, dismiss the download dialog, and keep using
the app — including generating images with a *different* model — while the
download runs in the background. Progress, completion, and errors surface inline
in the Model panel.

## Core Insight

This is a **100% frontend feature.** The backend already supports concurrency:

- `download_model` runs in `tauri::async_runtime::spawn_blocking` (a background thread).
- `generate` spawns its own `sd-cli` child process.

These never contend. The *only* thing blocking the user today is the modal
`DownloadDialog`, which traps them in a progress view (`busy` state) until the
download finishes or is cancelled. The fix is to lift download state out of the
modal into a persistent global store and render progress inline.

No Rust changes. The existing command surface is reused verbatim:
- `download_model(url, token) -> ModelInfo`
- `cancel_download()`
- event `model:download:progress` → `DownloadProgress { downloaded, total }`

## Architecture

### Global store (`src/lib/stores.ts`)

Add a discriminated-union store mirroring the existing `genStatus` pattern:

```ts
export type DownloadStatus =
  | { kind: "idle" }
  | { kind: "active"; name: string; downloaded: number; total: number | null }
  | { kind: "done"; name: string }                    // dismissible notice
  | { kind: "error"; name: string; message: string }; // dismissible notice

export const downloadStatus = writable<DownloadStatus>({ kind: "idle" });
```

Plus two lifecycle helpers (in `stores.ts`, alongside the store):

```ts
export async function startDownload(url: string, token: string, name: string): Promise<void>
export function cancelActiveDownload(): void
```

`startDownload`:
1. Single-flight guard: if current status is `active`, return immediately (no-op).
2. Set `downloadStatus = { kind: "active", name, downloaded: 0, total: null }`.
3. Clear the module-level `cancelRequested` flag.
4. Attach the `model:download:progress` listener (via `onDownloadProgress`); on
   each event, update `downloaded`/`total` only while status is still `active`.
5. `await downloadModel(url, token)`:
   - success → `downloadStatus = { kind: "done", name }`
   - error → if `cancelRequested`, set `{ kind: "idle" }`; else `{ kind: "error", name, message: String(e) }`
6. `finally`: detach the listener.

`cancelActiveDownload`:
- Set the module-level `cancelRequested = true`, call `cancelDownload()`. The
  backend aborts the stream and cleans up the `.part` file; the `startDownload`
  catch then resolves to `idle`.

The listener is attached per-download (in `startDownload`) and detached in its
`finally`, so there is exactly one live listener during an active download and
none otherwise — consistent with the single-flight model.

### Components

**`src/lib/components/DownloadDialog.svelte`**
- Remove local state: `busy`, `downloaded`, `total`, `cancelling`, `unlisten`,
  the `pct` derived, the `onMount` progress listener, and the in-dialog progress
  view.
- `start(downloadUrl, name)` calls `startDownload(downloadUrl, token.trim(), name)`
  then `onclose()` immediately — the dialog closes and progress appears inline in
  the Model panel.
- If the dialog is opened while `$downloadStatus.kind === "active"`: disable the
  starter *Get* buttons and the paste-URL *Download* button, and show a note:
  "A download is already running — see the Model panel."
- Remove the `ondownloaded` prop from the component's `$props()` (no longer used;
  the Model panel handles refresh). `onclose` remains.
- Starter downloads pass `s.name`. Paste-URL downloads derive a name from the URL
  basename (filename without extension; fall back to the trimmed URL).

**`src/lib/components/ModelLibrary.svelte`**
- Stop passing `ondownloaded`; render `<DownloadDialog onclose={…} />` only.
- Subscribe to `downloadStatus`. Render an inline status line under the existing
  `.actions` row:
  - `active`: `⬇ {name}… {fmt(downloaded)}{total ? ` / ${fmt(total)} (${pct}%)` : "…"}` + **Cancel** button (calls `cancelActiveDownload`).
  - `done`: `✓ {name} ready` + dismiss ✕ (sets `downloadStatus` to `{ kind: "idle" }`).
  - `error`: `⚠ {name}: {message}` + dismiss ✕ (sets idle).
- An `$effect` watching `downloadStatus`: when it transitions to `kind === "done"`,
  call `refresh()` (`listModels()` → `models.set`) so the new model appears in the
  dropdown. Guard against re-running for the same `done` state (track the handled
  name) so a later unrelated reactive change doesn't re-trigger a refresh.
- **`model_path` is never modified by download completion.** The active selection
  stays exactly as the user left it.
- The existing delete-error `error` state and the `orphanPath` logic are unchanged.

## Data Flow

1. User opens the Download dialog, clicks *Get* (or pastes a URL and clicks
   *Download*).
2. Dialog calls `startDownload(url, token, name)` → `downloadStatus = active`;
   dialog closes via `onclose()`.
3. Backend streams progress → store updates `downloaded`/`total` → the Model
   panel's inline bar animates.
4. The user generates with a *different* model in the meantime — entirely
   unaffected (GenerateBar has no dependency on download state).
5. Backend completes → `downloadStatus = done` → the Model panel `$effect`
   refreshes the model list (new model appears in the dropdown) and shows
   `✓ {name} ready`. The active `model_path` is unchanged.
6. The user dismisses the notice, or it is cleared when the next download starts.

**Cancel:** Model panel *Cancel* → `cancelActiveDownload()` → backend aborts and
removes the `.part` file → `downloadStatus` returns to `idle`.

## Edge Cases

- **Reopen dialog mid-download:** start buttons disabled with an explanatory note;
  no second download can begin (single-flight).
- **Network/HTTP error:** `downloadStatus = error`, shown inline in the Model
  panel, dismissible.
- **Single global progress event:** correct under single-flight (one active
  download at a time).
- **App closed mid-download:** unchanged from today (the background thread ends
  with the process; partial `.part` remains until a future download overwrites or
  the engine cleans it — out of scope).

## Testing

- **No backend change** → no new Rust tests. Confirm the existing suite still
  passes (57 tests).
- `npm run check` (svelte-check) clean: 0 errors, 0 warnings.
- **Manual E2E (dev box, `npm run tauri dev`):**
  - Start a starter download, dialog closes, inline progress appears in the Model
    panel and advances.
  - While downloading, switch to a different model and generate — generation works
    and is unaffected.
  - Download completes → the new model appears in the dropdown, the active
    selection is unchanged, and a `✓ … ready` notice shows; dismiss it.
  - Cancel mid-download → `.part` is removed and the status returns to idle.
  - Reopen the Download dialog mid-download → start buttons are disabled with the
    note.
  - Paste-URL download → the inline status shows a sensible name derived from the
    URL.

## Non-Goals (YAGNI)

- Multiple concurrent downloads (single-flight only; roadmap asks for "one model").
- A backend concurrency guard (single-flight is frontend-enforced by disabling the
  start UI; the existing `download_cancel` comment documents the assumption).
- A download queue.
- Resume / persistence of downloads across app restart.

## Files Touched

- `src/lib/stores.ts` — add `DownloadStatus` type, `downloadStatus` store,
  `startDownload`, `cancelActiveDownload`.
- `src/lib/components/DownloadDialog.svelte` — drop local progress state; start +
  close; disable when a download is active.
- `src/lib/components/ModelLibrary.svelte` — inline status line, cancel/dismiss,
  refresh-on-done effect; stop passing `ondownloaded`.

No Rust files change.
