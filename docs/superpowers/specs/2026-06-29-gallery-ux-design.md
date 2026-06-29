# Gallery UX: delete, selection highlight, batch grouping — Design Spec

**Date:** 2026-06-29
**Status:** Approved (brainstorming complete)
**Branch:** `feat/gallery-ux`

## Goal

Three cohesive improvements to fridAI's gallery/thumbnail area:

1. **Delete** a generated image from within the app (to the OS trash, with confirmation).
2. **Selection highlight** so it's clear which thumbnail the big preview + metadata panel describe.
3. **Batch grouping** so images produced by one batch generation are visually tied together.

All three live on the same surface (the big preview, the params panel, the
thumbnail strip), so they're designed and built together.

## Background / current state

- `GalleryItem { id, image_path, request, created_at_unix }` (Rust
  `src-tauri/src/types.rs`, mirrored in `src/lib/types.ts`).
- Each generated image has a sibling sidecar `<stem>.json` holding its
  `GalleryItem`. `gallery.rs` has `write_sidecar` and `list_items` (reads every
  `*.json`, sorts newest-first by `created_at_unix`).
- Batch images already encode grouping implicitly: a single generation gets
  `id = <uuid>`; a batch of N gets `id = <uuid>_0`, `<uuid>_1`, … and the engine
  writes `<uuid>_0.png`, etc. (`commands.rs::generate`).
- `currentItem` / `currentImage` stores drive the big preview (`ImagePreview`)
  and the metadata panel (`ParamsPanel`). They are set after a generation (to the
  first produced image) and on thumbnail click (`HistoryStrip.open`). The
  `history` store is re-fetched via `listHistory()` after each generation.
- **Problem:** the active thumbnail is not visually marked, so the user can't tell
  which thumbnail the preview/metadata belong to; and batch members look identical
  to unrelated single images.

## Architecture

### 1. Data model — explicit batch identity

Add three `#[serde(default)]` fields to `GalleryItem` (Rust + TS mirror):

- `batch_id: String` — shared key for all images of one generation run.
- `batch_index: u32` — 0-based position within the batch.
- `batch_size: u32` — total images in the batch.

`commands.rs::generate` sets them per produced image:

- `batch_id` = the run's base `<uuid>` (the same value used as the file-name stem
  base today), for **both** single and batch runs.
- `batch_index` = `i` (the produced-image index).
- `batch_size` = `produced.len()`.

Singles therefore get `batch_size = 1`, `batch_index = 0`, `batch_id = <uuid>`.

**Backward compatibility:** old sidecars lack these fields. `#[serde(default)]`
yields `batch_id = ""`, `batch_index = 0`, `batch_size = 0`. Consumers normalize:
effective size = `batch_size.max(1)`; an empty `batch_id` falls back to the
item's own `id`. So pre-existing images load as singletons — no migration, no
breakage. (This is the same backward-compatible `#[serde(default)]` pattern used
by `AppConfig.gpu_device`.)

`batch_id` is chosen over parsing the `_N` suffix of `id` because it makes each
sidecar self-describing — grouping, ordering, and the index tags need no string
parsing or sibling-counting.

### 2. Delete — backend

New function in `gallery.rs`:

```
pub fn delete_to_trash(image_path: &Path) -> Result<(), String>
```

- Moves the image file to the OS trash via the `trash` crate (new dependency,
  cross-platform — also covers the future macOS port).
- Also trashes the sibling `<stem>.json` sidecar **if it exists** (best-effort:
  a missing sidecar is not an error).
- Returns `Err(String)` if the image itself can't be trashed (surfaced to the UI
  like other command errors).

New Tauri command in `commands.rs`:

```
#[tauri::command]
pub fn delete_image(image_path: String) -> Result<(), String>
```

Registered in `lib.rs`'s `invoke_handler`. Frontend API: `deleteImage(path)` in
`src/lib/api.ts`.

### 3. Delete — frontend

- A delete button on the **big preview** (`ImagePreview.svelte`), shown only when
  `$currentItem` is set.
- Click → a confirm prompt ("Move this image to the trash?") → on confirm call
  `deleteImage($currentItem.image_path)`.
- On success: re-fetch `history` via `listHistory()`. Then move the selection to
  the nearest remaining item — the next item by recency in the refreshed list, or
  the previous if the deleted item was last; if the gallery is now empty, clear
  `currentImage` / `currentItem`.
- Per-image only: deleting one member of a batch leaves the others intact.
- On error: surface the message (reuse the existing error-display style; no crash).

### 4. Selection highlight (metadata ↔ thumbnail)

In `HistoryStrip.svelte`, a thumbnail is marked **selected** when
`item.id === $currentItem?.id`, rendered as an amber ring
(`outline: 2px solid var(--accent)` style, matching the app accent). No new
state — `currentItem` already updates on generation and on click. This visually
ties the strip to the big preview and the metadata in `ParamsPanel`.

### 5. Batch grouping (chosen visual: ring + colored underline bar)

In `HistoryStrip.svelte`:

- Group the flat `history` list by effective `batch_id`. Preserve overall
  newest-first ordering by keying each group off its earliest-encountered position
  in the sorted list (a batch's members share a timestamp, so the group stays
  contiguous).
- Within a multi-image group, sort members by `batch_index` (disk read order from
  `read_dir` is not guaranteed, so explicit sort is required).
- A multi-image group renders its thumbnails together under a colored underline
  bar (the app accent), each thumbnail tagged with `batch_index + 1` (1, 2, 3…) in
  a small corner label.
- Single-image groups (`batch_size <= 1`) render exactly as today (no bar, no tag).
- The strip remains horizontally scrollable (`overflow-x: auto`), so large batches
  simply extend and scroll — no collapsing.
- The selection ring (section 4) composes with grouping: a selected batch member
  shows both the ring and its index tag.

### Data flow

```
generate (batch of N)
  └─ commands::generate writes N sidecars, each with
       batch_id=<uuid>, batch_index=i, batch_size=N
  └─ frontend: currentItem = items[0]; history = listHistory()

render strip
  └─ group history by batch_id → ordered groups
       └─ multi-image group: underline bar + index tags, sorted by batch_index
       └─ thumbnail selected when item.id === currentItem.id  → ring

click thumbnail
  └─ currentImage/currentItem/request = that item  → ring + preview + metadata update

delete (from preview)
  └─ confirm → deleteImage(currentItem.image_path)
       └─ gallery::delete_to_trash (png + sidecar → OS trash)
            └─ history = listHistory(); selection moves to nearest remaining (or clears)
```

## Error handling

- **Trash failure** (image can't be moved): `delete_to_trash` returns `Err`;
  the UI shows the message and leaves selection/history unchanged.
- **Missing sidecar on delete:** ignored (best-effort); the image is still trashed.
- **Old sidecars without batch fields:** load as singletons via `#[serde(default)]`
  + `batch_size.max(1)` normalization.
- **Delete the last/only image:** preview and metadata clear gracefully.

## Wire contract (TS mirrors Rust serde)

- `GalleryItem` gains `batch_id: string; batch_index: number; batch_size: number;`
  (snake_case on the wire, matching the field names).
- `deleteImage(path: string): Promise<void>` added to `src/lib/api.ts`.

## Testing

- **Rust (`gallery.rs`):**
  - `delete_to_trash` removes both the PNG and its sidecar from the directory
    (assert neither remains); tolerates a missing sidecar without error.
  - A sidecar written without the batch fields deserializes with
    `batch_size == 0` (normalized to 1 by consumers) — i.e. `#[serde(default)]`
    round-trip for the new fields, including the legacy-singleton case.
  - Existing `list_items` newest-first / corrupt-sidecar tests still pass.
- **Frontend:** `svelte-check` clean.
- **Manual E2E (dev box):**
  - Generate a batch (count ≥ 3): thumbnails are grouped under the bar, tagged
    1..N in order.
  - Click thumbnails: the ring follows the click and the metadata/preview match
    the ringed thumbnail.
  - Delete from the preview: confirm prompt appears; on confirm the image leaves
    the strip, the file is in the OS trash, and selection moves to a neighbor.
  - Delete one batch member: the rest of the batch remains, still grouped.
  - Restart the app: grouping and metadata persist (rebuilt from sidecars).

## Out of scope (explicit, deferred to backlog)

- General app-settings/preferences menu, and any "don't ask before delete" toggle
  (the confirm prompt is always on for now).
- Background model downloads (separate future feature).
- Multi-select / bulk delete; deleting an entire batch in one action.
- In-app trash/restore UI (the OS trash is the recovery path).
