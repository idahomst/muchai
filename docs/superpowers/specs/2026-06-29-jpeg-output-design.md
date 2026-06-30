# JPEG output format — Design Spec

**Date:** 2026-06-29
**Status:** Approved (brainstorming complete)
**Branch:** `feat/jpeg-output` (to be created)

## Goal

Let the user choose the output image format — **PNG (default) or JPEG** — per
generation, via a dropdown alongside the other generation parameters. The choice
is recorded per image, embedded metadata travels in the JPEG too, and the
selection is sticky across restarts.

## Background / current state

- The engine (`stable-diffusion.cpp` `sd-cli`) infers the output format **from the
  `-o` output-path extension** (`.png` → PNG, `.jpg` → JPEG). There is **no
  JPEG-quality flag**; quality is the library default (stb_image_write). The
  engine also does not support WebP.
- `command_builder.rs::build_args(req, output_path, backend)` already receives the
  full `output_path` and emits it after `-o`. It does **not** need to change — the
  extension is decided by the caller.
- `commands.rs::generate` hardcodes `.png` in three places: the initial
  `image_path` (`{id}.png`), the batch-discovery loop (`{id}_{i}.png`), and it
  relies on `image_path` for the single-file fallback.
- Metadata embedding is **on by default**: `generate`/`build_args` never pass
  `--disable-image-metadata`, so the engine embeds the full params into the image
  (PNG tEXt / JPEG EXIF-XMP — confirmed). No change needed to keep this.
- Everything downstream of the produced file is **extension-agnostic**:
  - `gallery::write_sidecar` derives the sidecar via `image_path.with_extension("json")`.
  - `gallery::list_items` reads `*.json` sidecars and takes the real path from
    `item.image_path`.
  - `gallery::deletion_targets` / `delete_to_trash` derive the sidecar via
    `with_extension("json")` and trash the image by its stored path.
  - The frontend displays via the asset protocol using the stored `image_path`.
  So JPEGs list, display, and delete identically to PNGs with no change.
- `Sampler` is the existing precedent for an enum gen-param: `#[serde(rename_all =
  "snake_case")]`, a `cli_name()` helper, a `Default` impl, mirrored in TS as a
  string union plus a `SAMPLERS` array used to build a `<select>`. `OutputFormat`
  follows this pattern exactly.
- `SettingsPanel.svelte` binds the generation-params grid directly to the
  `$request` store; it is where the new dropdown lives. `generate` already saves
  the whole request to `AppConfig.last_request`, so a `$request` field is sticky
  for free.

## Architecture

### Data model

Add an `OutputFormat` enum and one field to `GenerationRequest`
(`src-tauri/src/types.rs`):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    Png,
    Jpeg,
}

impl OutputFormat {
    /// File extension (no dot) used to drive the engine's `-o` format inference.
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Png => "png",
            OutputFormat::Jpeg => "jpg",
        }
    }
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Png
    }
}
```

- `GenerationRequest` gains `#[serde(default)] pub output_format: OutputFormat`.
  `#[serde(default)]` makes pre-feature sidecars and `last_request` deserialize as
  `Png` — existing history and the saved request keep working with no migration.
- `GenerationRequest::default()` sets `output_format: OutputFormat::Png`.

Wire form: `"png"` / `"jpeg"`. Extensions: `png` / `jpg`.

### TypeScript mirror (`src/lib/types.ts`)

- `GenerationRequest` interface gains `output_format: "png" | "jpeg";`.
- `defaultRequest()` sets `output_format: "png"`.
- Add a `FORMATS` const mirroring `SAMPLERS`:
  `export const FORMATS = [{ value: "png", label: "PNG" }, { value: "jpeg", label: "JPEG" }] as const;`
  (value strings must match the serde wire form exactly).

### Engine & file flow (`commands.rs::generate`)

- Compute once: `let ext = request.output_format.extension();`
- Initial path: `gallery_dir.join(format!("{id}.{ext}"))` (was `{id}.png`).
- Batch discovery loop: `gallery_dir.join(format!("{id}_{i}.{ext}"))` (was
  `{id}_{i}.png`). The engine inserts the batch index into the stem and preserves
  the extension, yielding `{id}_0.jpg`, `{id}_1.jpg`, …
- Single-file fallback already uses `image_path` (which now carries `ext`) — no
  literal to change there.
- `build_args` is unchanged; it forwards whatever path it is handed.

### UI

`SettingsPanel.svelte` — add one control to the existing 2-column grid, next to
Sampler/Batch, using the same markup as the Sampler dropdown:

```svelte
<label class="label">Format
  <select bind:value={$request.output_format}>
    {#each FORMATS as f}<option value={f.value}>{f.label}</option>{/each}
  </select>
</label>
```

(`FORMATS` imported from `../types` alongside `SAMPLERS`.) No new styles.

`ParamsPanel.svelte` — add one row to the **expanded** grid so a produced image
shows its encoding:

```svelte
<span class="k">Format</span><span class="v">{r.output_format.toUpperCase()}</span>
```

The collapsed one-line summary is unchanged (format is not essential there).

### Data flow

```
SettingsPanel <select> → $request.output_format ("png" | "jpeg")
  └─ generate(request)
       ├─ ext = output_format.extension()  ("png" | "jpg")
       ├─ image_path = {gallery}/{id}.{ext}
       ├─ build_args(..., image_path, ...)  → engine writes that format (metadata embedded)
       ├─ batch discovery: {id}_{i}.{ext}
       ├─ write_sidecar (with_extension("json"))  ← stores request incl. output_format
       └─ last_request = request  (persists format for next launch + reuse)
```

## Error handling

- **Old sidecar/config without `output_format`:** `#[serde(default)]` → `Png`. No
  error, no migration.
- **Invalid value:** impossible — the UI is a fixed two-option `<select>`.
- **Engine fails to produce a file:** handled by the existing "no image file was
  found" / stderr-tail error paths in `generate`. No new handling.

## Testing

- **Rust (`types.rs` / `command_builder.rs` tests):**
  - `OutputFormat` serializes to `"png"`/`"jpeg"` and round-trips.
  - `OutputFormat::extension()` returns `"png"` / `"jpg"`.
  - A `GenerationRequest` JSON omitting `output_format` deserializes with
    `output_format == OutputFormat::Png`.
  - `build_args` passes a `.jpg` output path through verbatim after `-o`
    (extension-agnostic).
- **Frontend:** `svelte-check` clean.
- **Manual E2E (dev box):**
  - Generate with Format = JPEG → a `.jpg` lands in the gallery, displays in the
    preview, and its sidecar + embedded metadata are intact.
  - Batch count > 1 with JPEG → multiple `.jpg` files, all listed and selectable.
  - Switch back to PNG → produces `.png` again.
  - Format choice survives an app restart (persisted via `last_request`).
  - Deleting a JPEG trashes both the image and its sidecar.

## Out of scope (deferred)

- JPEG quality / compression control (no engine flag exists).
- WebP or any format beyond PNG/JPEG (engine doesn't support it).
- Converting or re-encoding existing PNG images.
- A global "default format" preference separate from the per-request value
  (the per-request value already persists via `last_request`).
