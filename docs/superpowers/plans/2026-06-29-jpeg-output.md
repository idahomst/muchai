# JPEG Output Format Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user pick PNG (default) or JPEG output per generation via a dropdown; the choice drives the engine's output extension, is recorded per image, and sticks across restarts.

**Architecture:** Add an `OutputFormat` enum to `GenerationRequest` (mirrors the existing `Sampler` pattern: snake_case serde + helper + `#[serde(default)]` for backward compatibility). `commands.rs::generate` derives the file extension from it (the engine infers format from the `-o` path). The frontend adds a `<select>` bound to `$request.output_format`, which already persists via `last_request`.

**Tech Stack:** Tauri v2 (Rust), SvelteKit + Svelte 5 runes, serde JSON, stable-diffusion.cpp `sd-cli`.

**Reference:** Design spec at `docs/superpowers/specs/2026-06-29-jpeg-output-design.md`.

---

## File structure

- `src-tauri/src/types.rs` — `OutputFormat` enum + `output_format` field on `GenerationRequest` + tests.
- `src-tauri/src/command_builder.rs` — one test confirming a `.jpg` output path passes through verbatim (no production change).
- `src-tauri/src/commands.rs` — `generate` derives the extension from `output_format`.
- `src/lib/types.ts` — mirror the field, `defaultRequest`, and a `FORMATS` const.
- `src/lib/components/SettingsPanel.svelte` — Format dropdown.
- `src/lib/components/ParamsPanel.svelte` — Format row in the expanded grid.

---

### Task 1: `OutputFormat` data model (Rust)

**Files:**
- Modify: `src-tauri/src/types.rs` (add enum near `Sampler`; add field to `GenerationRequest`; add tests)

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/types.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn output_format_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&OutputFormat::Png).unwrap(), "\"png\"");
        assert_eq!(serde_json::to_string(&OutputFormat::Jpeg).unwrap(), "\"jpeg\"");
        let back: OutputFormat = serde_json::from_str("\"jpeg\"").unwrap();
        assert_eq!(back, OutputFormat::Jpeg);
    }

    #[test]
    fn output_format_extension_maps_correctly() {
        assert_eq!(OutputFormat::Png.extension(), "png");
        assert_eq!(OutputFormat::Jpeg.extension(), "jpg");
    }

    #[test]
    fn output_format_defaults_to_png() {
        assert_eq!(OutputFormat::default(), OutputFormat::Png);
    }

    #[test]
    fn generation_request_without_output_format_defaults_to_png() {
        // A pre-feature request/sidecar lacks the output_format key.
        let json = r#"{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}"#;
        let req: GenerationRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.output_format, OutputFormat::Png);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd /home/idaho/g/mst/muchai/src-tauri && cargo test --lib output_format`
Expected: FAIL to compile — `OutputFormat` and `output_format` do not exist yet.

- [ ] **Step 3: Add the `OutputFormat` enum**

In `src-tauri/src/types.rs`, add this directly **after** the `impl Default for Sampler { ... }` block (around line 40, before `GenerationRequest`):

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

- [ ] **Step 4: Add the field to `GenerationRequest`**

In `src-tauri/src/types.rs`, the `GenerationRequest` struct currently ends with `batch_count`. Replace:

```rust
    pub seed: i64, // -1 = random
    pub batch_count: u32,
}
```

with:

```rust
    pub seed: i64, // -1 = random
    pub batch_count: u32,
    /// Output image format. Defaults to PNG; pre-feature sidecars/configs lack
    /// this key and deserialize as PNG.
    #[serde(default)]
    pub output_format: OutputFormat,
}
```

- [ ] **Step 5: Set the field in `GenerationRequest::default()`**

In the same file, the `Default` impl for `GenerationRequest` ends with `batch_count: 1,`. Replace:

```rust
            seed: -1,
            batch_count: 1,
        }
    }
}
```

with:

```rust
            seed: -1,
            batch_count: 1,
            output_format: OutputFormat::default(),
        }
    }
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cd /home/idaho/g/mst/muchai/src-tauri && cargo test --lib`
Expected: all tests PASS (the 4 new ones plus all existing). The existing `generation_request_round_trips_through_json` test still passes because `..Default::default()` now includes `output_format`.

- [ ] **Step 7: Commit**

```bash
cd /home/idaho/g/mst/muchai && git add src-tauri/src/types.rs && git commit -m "feat(jpeg-output): add OutputFormat to GenerationRequest

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Engine extension wiring (Rust)

**Files:**
- Modify: `src-tauri/src/command_builder.rs` (add one passthrough test)
- Modify: `src-tauri/src/commands.rs` (`generate` — derive extension)

- [ ] **Step 1: Add a `.jpg` passthrough test for `build_args`**

In `src-tauri/src/command_builder.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn output_path_extension_passes_through_verbatim() {
        // build_args is format-agnostic: whatever extension the caller chose on
        // the -o path is forwarded unchanged (the engine infers format from it).
        let args = build_args(&sample(), "/out/x.jpg", None);
        assert_eq!(val_after(&args, "-o"), Some("/out/x.jpg"));
    }
```

- [ ] **Step 2: Run it to confirm it passes (no production change needed)**

Run: `cd /home/idaho/g/mst/muchai/src-tauri && cargo test --lib output_path_extension_passes_through_verbatim`
Expected: PASS. (`build_args` already forwards the `-o` path verbatim; this test pins that contract so the JPEG path is safe.)

- [ ] **Step 3: Derive the extension in `generate`**

In `src-tauri/src/commands.rs`, find this block inside `pub async fn generate`:

```rust
    let id = uuid::Uuid::new_v4().to_string();
    let image_path = gallery_dir.join(format!("{id}.png"));
```

Replace it with:

```rust
    let id = uuid::Uuid::new_v4().to_string();
    let ext = request.output_format.extension();
    let image_path = gallery_dir.join(format!("{id}.{ext}"));
```

- [ ] **Step 4: Use the extension in batch discovery**

In the same function, find the batch-discovery loop:

```rust
            for i in 0..batch {
                let p = gallery_dir.join(format!("{id}_{i}.png"));
                if p.exists() {
                    produced.push((i, p));
                }
            }
```

Replace the `format!` line so it reads:

```rust
            for i in 0..batch {
                let p = gallery_dir.join(format!("{id}_{i}.{ext}"));
                if p.exists() {
                    produced.push((i, p));
                }
            }
```

(`ext` is a `&'static str` computed in Step 3 before the `spawn_blocking` closure; `request` is not moved into the closure — only its clone `req` is — so `ext` and `request` remain usable here. No other `.png` literal exists in `generate`; the single-file fallback already uses `image_path`.)

- [ ] **Step 5: Build to verify it compiles**

Run: `cd /home/idaho/g/mst/muchai/src-tauri && cargo build 2>&1 | tail -5`
Expected: compiles with no errors and no new warnings.

- [ ] **Step 6: Run the full Rust test suite**

Run: `cd /home/idaho/g/mst/muchai/src-tauri && cargo test --lib 2>&1 | tail -2`
Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
cd /home/idaho/g/mst/muchai && git add src-tauri/src/command_builder.rs src-tauri/src/commands.rs && git commit -m "feat(jpeg-output): drive output extension from OutputFormat in generate

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Frontend — type mirror + dropdown + params row

**Files:**
- Modify: `src/lib/types.ts` (interface field, `defaultRequest`, `FORMATS`)
- Modify: `src/lib/components/SettingsPanel.svelte` (dropdown)
- Modify: `src/lib/components/ParamsPanel.svelte` (expanded-grid row)

- [ ] **Step 1: Add the wire type and interface field in `types.ts`**

In `src/lib/types.ts`, add the `OutputFormat` type just before the `GenerationRequest` interface (after the `Sampler` type block, around line 7):

```ts
// Wire values MUST match the Rust `OutputFormat` enum's serde snake_case form
// (src-tauri/src/types.rs). Extensions ("png"/"jpg") live only in Rust.
export type OutputFormat = "png" | "jpeg";
```

Then in the `GenerationRequest` interface, replace:

```ts
  seed: number;       // -1 = random
  batch_count: number;
}
```

with:

```ts
  seed: number;       // -1 = random
  batch_count: number;
  output_format: OutputFormat;
}
```

- [ ] **Step 2: Set the field in `defaultRequest()`**

In `src/lib/types.ts`, replace:

```ts
export const defaultRequest = (): GenerationRequest => ({
  model_path: "", prompt: "", negative_prompt: "",
  steps: 20, cfg_scale: 7.0, sampler: "euler_a",
  width: 512, height: 512, seed: -1, batch_count: 1,
});
```

with:

```ts
export const defaultRequest = (): GenerationRequest => ({
  model_path: "", prompt: "", negative_prompt: "",
  steps: 20, cfg_scale: 7.0, sampler: "euler_a",
  width: 512, height: 512, seed: -1, batch_count: 1,
  output_format: "png",
});
```

- [ ] **Step 3: Add the `FORMATS` const**

In `src/lib/types.ts`, directly after the closing `];` of the `SAMPLERS` array, add:

```ts
export const FORMATS: { value: OutputFormat; label: string }[] = [
  { value: "png", label: "PNG" },
  { value: "jpeg", label: "JPEG" },
];
```

- [ ] **Step 4: Add the dropdown in `SettingsPanel.svelte`**

In `src/lib/components/SettingsPanel.svelte`, update the import line:

```svelte
  import { SAMPLERS } from "../types";
```

to:

```svelte
  import { SAMPLERS, FORMATS } from "../types";
```

Then, inside the `<div class="grid">`, add a Format control immediately after the Batch `<label>` block (before the `seed` label):

```svelte
  <label class="label">Format
    <select bind:value={$request.output_format}>
      {#each FORMATS as f}<option value={f.value}>{f.label}</option>{/each}
    </select>
  </label>
```

- [ ] **Step 5: Add the Format row in `ParamsPanel.svelte`**

In `src/lib/components/ParamsPanel.svelte`, inside `<div class="grid">`, replace the Size line:

```svelte
        <span class="k">Size</span><span class="v">{r.width}×{r.height}</span>
```

with:

```svelte
        <span class="k">Size</span><span class="v">{r.width}×{r.height}</span>
        <span class="k">Format</span><span class="v">{r.output_format.toUpperCase()}</span>
```

- [ ] **Step 6: Type-check the frontend**

Run: `cd /home/idaho/g/mst/muchai && npm run check 2>&1 | tail -3`
Expected: 0 errors, 0 warnings. (`defaultRequest` now supplies `output_format`, so the new required interface field breaks no construction site; `$currentItem.request.output_format` is typed via the mirrored interface.)

- [ ] **Step 7: Commit**

```bash
cd /home/idaho/g/mst/muchai && git add src/lib/types.ts src/lib/components/SettingsPanel.svelte src/lib/components/ParamsPanel.svelte && git commit -m "feat(jpeg-output): format dropdown + params row, TS mirror

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Verification + finish branch

**Files:** none (verification only)

- [ ] **Step 1: Backend tests**

Run: `cd /home/idaho/g/mst/muchai/src-tauri && cargo test --lib 2>&1 | tail -2`
Expected: all tests pass (52 prior + 5 new = 57).

- [ ] **Step 2: Frontend check**

Run: `cd /home/idaho/g/mst/muchai && npm run check 2>&1 | tail -2`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Manual E2E (dev box, `npm run tauri dev`)**

Verify:
- The params grid shows a **Format** dropdown (PNG / JPEG), defaulting to PNG.
- Generate with **JPEG** → a `.jpg` file lands in the gallery, displays in the preview, and the ParamsPanel (expanded) shows `Format JPEG`. The sidecar `.json` exists next to it and embedded metadata is present.
- Generate with **batch count > 1** and JPEG → multiple `.jpg` files appear, all selectable.
- Switch back to **PNG** → produces `.png` again.
- Restart the app → the last-used format is restored (persisted via `last_request`).
- **Delete** a JPEG from the preview → both the `.jpg` and its `.json` sidecar move to trash.

- [ ] **Step 4: Update roadmap memory**

In `/home/idaho/.claude/projects/-home-idaho-g-mst-muchai/memory/muchai-roadmap.md`, mark roadmap item 2 (JPEG output format) DONE.

- [ ] **Step 5: Finish the branch**

Use superpowers:finishing-a-development-branch on `feat/jpeg-output`.
