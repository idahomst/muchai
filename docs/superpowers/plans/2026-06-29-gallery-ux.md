# Gallery UX (delete + selection + batch grouping) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user delete a generated image (to OS trash, with confirmation), highlight which thumbnail the preview/metadata describe, and visually group images that were generated as one batch.

**Architecture:** Add self-describing batch fields to `GalleryItem` (backward-compatible via `#[serde(default)]`); add a trash-backed delete command; update `HistoryStrip` to highlight the selected thumbnail and group batch members under a colored bar; add an inline-confirm delete button to `ImagePreview`.

**Tech Stack:** Tauri v2 (Rust), SvelteKit + Svelte 5 runes, the `trash` crate (cross-platform OS trash).

**Reference:** Design spec at `docs/superpowers/specs/2026-06-29-gallery-ux-design.md`.

---

## File structure

- `src-tauri/src/types.rs` — add `batch_id`/`batch_index`/`batch_size` to `GalleryItem`.
- `src-tauri/src/commands.rs` — populate batch fields in `generate`; add `delete_image` command.
- `src-tauri/src/gallery.rs` — `deletion_targets` + `delete_to_trash`; update test helper.
- `src-tauri/src/lib.rs` — register `delete_image`.
- `src-tauri/Cargo.toml` — add `trash` dependency.
- `src/lib/types.ts` — mirror batch fields on `GalleryItem`.
- `src/lib/api.ts` — add `deleteImage`.
- `src/lib/components/HistoryStrip.svelte` — selection ring + batch grouping.
- `src/lib/components/ImagePreview.svelte` — inline-confirm delete + selection move.

---

### Task 1: Add batch fields to `GalleryItem` (Rust)

**Files:**
- Modify: `src-tauri/src/types.rs:121-127` (struct), and `src-tauri/src/commands.rs:186-191` (literal in `generate`)
- Test: `src-tauri/src/gallery.rs` (tests module) + update helper at `src-tauri/src/gallery.rs:37-44`

- [ ] **Step 1: Add the fields to the struct**

In `src-tauri/src/types.rs`, replace the `GalleryItem` struct (lines 121-127) with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GalleryItem {
    pub id: String,
    pub image_path: String,
    pub request: GenerationRequest,
    pub created_at_unix: u64,
    /// Shared key for all images produced by one generation run. Empty on
    /// pre-batch-field sidecars; consumers fall back to `id`.
    #[serde(default)]
    pub batch_id: String,
    /// 0-based position within the batch.
    #[serde(default)]
    pub batch_index: u32,
    /// Total images in the batch. 0 on legacy sidecars; normalize with `.max(1)`.
    #[serde(default)]
    pub batch_size: u32,
}
```

- [ ] **Step 2: Fix the `generate` struct literal**

In `src-tauri/src/commands.rs`, the loop building each item (around lines 186-191) currently is:

```rust
                let item = GalleryItem {
                    id: if multi { format!("{id}_{i}") } else { id.clone() },
                    image_path: path.to_string_lossy().into_owned(),
                    request: req_i,
                    created_at_unix: now_unix(),
                };
```

Replace it with (adds the three fields; `id` is the run's base uuid, `i` the index, `produced_len` the batch size):

```rust
                let item = GalleryItem {
                    id: if multi { format!("{id}_{i}") } else { id.clone() },
                    image_path: path.to_string_lossy().into_owned(),
                    request: req_i,
                    created_at_unix: now_unix(),
                    batch_id: id.clone(),
                    batch_index: i as u32,
                    batch_size: produced_len as u32,
                };
```

Then, immediately before the `let multi = produced.len() > 1;` line (currently `src-tauri/src/commands.rs:173`), capture the length so it isn't consumed by the loop. Replace:

```rust
            let multi = produced.len() > 1;
```

with:

```rust
            let produced_len = produced.len();
            let multi = produced_len > 1;
```

- [ ] **Step 3: Update the gallery test helper to set the new fields**

In `src-tauri/src/gallery.rs`, the helper (lines 37-44) builds a `GalleryItem`. Replace it with:

```rust
    fn item(id: &str, ts: u64) -> GalleryItem {
        GalleryItem {
            id: id.into(),
            image_path: format!("/g/{id}.png"),
            request: GenerationRequest::default(),
            created_at_unix: ts,
            batch_id: id.into(),
            batch_index: 0,
            batch_size: 1,
        }
    }
```

- [ ] **Step 4: Write the failing backward-compat test**

In `src-tauri/src/gallery.rs`, inside the `#[cfg(test)] mod tests` block, add:

```rust
    #[test]
    fn legacy_sidecar_without_batch_fields_loads_as_singleton() {
        // A sidecar written before batch fields existed has only the original
        // four keys. It must still deserialize, with the new fields defaulted.
        let it = item("legacy", 100);
        let mut v = serde_json::to_value(&it).unwrap();
        let obj = v.as_object_mut().unwrap();
        obj.remove("batch_id");
        obj.remove("batch_index");
        obj.remove("batch_size");

        let back: GalleryItem = serde_json::from_value(v).unwrap();
        assert_eq!(back.batch_id, "");
        assert_eq!(back.batch_index, 0);
        assert_eq!(back.batch_size, 0); // consumers normalize 0 -> 1
    }
```

- [ ] **Step 5: Run tests**

Run: `cd src-tauri && cargo test --lib gallery`
Expected: all gallery tests PASS (including the new one). The build compiles because both `GalleryItem` literals now set the fields.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/commands.rs src-tauri/src/gallery.rs
git commit -m "feat(gallery): add batch_id/batch_index/batch_size to GalleryItem

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Trash-backed delete in `gallery.rs`

**Files:**
- Modify: `src-tauri/Cargo.toml:20-` (dependencies)
- Modify: `src-tauri/src/gallery.rs` (add functions + tests)

- [ ] **Step 1: Add the `trash` dependency**

In `src-tauri/Cargo.toml`, under `[dependencies]`, add:

```toml
trash = "5"
```

- [ ] **Step 2: Write the failing test for `deletion_targets`**

In `src-tauri/src/gallery.rs` tests module, add:

```rust
    #[test]
    fn deletion_targets_includes_sidecar_only_when_present() {
        let dir = std::env::temp_dir().join(format!("fridai-del-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("pic.png");
        std::fs::write(&img, b"png").unwrap();

        // No sidecar yet -> just the image.
        assert_eq!(deletion_targets(&img), vec![img.clone()]);

        // With a sidecar -> both, image first.
        let side = dir.join("pic.json");
        std::fs::write(&side, b"{}").unwrap();
        assert_eq!(deletion_targets(&img), vec![img.clone(), side.clone()]);

        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib gallery::tests::deletion_targets`
Expected: FAIL — `deletion_targets` not found.

- [ ] **Step 4: Implement `deletion_targets` and `delete_to_trash`**

In `src-tauri/src/gallery.rs`, add (after `list_items`, before the tests module):

```rust
/// The files to remove for a gallery image: the image itself, plus its sibling
/// `.json` sidecar when one exists. Image is always first.
fn deletion_targets(image_path: &Path) -> Vec<PathBuf> {
    let mut targets = vec![image_path.to_path_buf()];
    let sidecar = image_path.with_extension("json");
    if sidecar.exists() {
        targets.push(sidecar);
    }
    targets
}

/// Move an image (and its sidecar, if any) to the OS trash. Recoverable from the
/// system file manager. Errors if the image itself cannot be trashed.
pub fn delete_to_trash(image_path: &Path) -> Result<(), String> {
    for target in deletion_targets(image_path) {
        trash::delete(&target).map_err(|e| e.to_string())?;
    }
    Ok(())
}
```

- [ ] **Step 5: Add a test that `delete_to_trash` removes both files**

In `src-tauri/src/gallery.rs` tests module, add:

```rust
    #[test]
    fn delete_to_trash_removes_image_and_sidecar() {
        let dir = std::env::temp_dir().join(format!("fridai-trash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("gone.png");
        let side = dir.join("gone.json");
        std::fs::write(&img, b"png").unwrap();
        std::fs::write(&side, b"{}").unwrap();

        delete_to_trash(&img).unwrap();

        assert!(!img.exists(), "image should be gone from the gallery dir");
        assert!(!side.exists(), "sidecar should be gone from the gallery dir");

        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 6: Run tests**

Run: `cd src-tauri && cargo test --lib gallery`
Expected: PASS. (Requires a working OS trash — true on the GUI dev box. `cargo build` will download the `trash` crate on first run.)

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/gallery.rs
git commit -m "feat(gallery): delete_to_trash moves image + sidecar to OS trash

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `delete_image` command + registration

**Files:**
- Modify: `src-tauri/src/commands.rs` (add command near `list_history`)
- Modify: `src-tauri/src/lib.rs:58-73` (invoke_handler)

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, after the `list_history` command (currently ends at line ~93), add:

```rust
#[tauri::command]
pub fn delete_image(image_path: String) -> Result<(), String> {
    gallery::delete_to_trash(std::path::Path::new(&image_path))
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![...]` list (after `commands::list_gpu_devices,`), add:

```rust
            commands::delete_image,
```

- [ ] **Step 3: Build to verify**

Run: `cd src-tauri && cargo build 2>&1 | tail -5`
Expected: compiles cleanly (Finished).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(gallery): delete_image command

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Frontend types + API

**Files:**
- Modify: `src/lib/types.ts:33-36` (`GalleryItem`)
- Modify: `src/lib/api.ts` (add `deleteImage`)

- [ ] **Step 1: Mirror the batch fields**

In `src/lib/types.ts`, replace the `GalleryItem` interface (lines 33-36):

```ts
export interface GalleryItem {
  id: string; image_path: string;
  request: GenerationRequest; created_at_unix: number;
}
```

with:

```ts
export interface GalleryItem {
  id: string; image_path: string;
  request: GenerationRequest; created_at_unix: number;
  batch_id: string; batch_index: number; batch_size: number;
}
```

- [ ] **Step 2: Add the delete API**

In `src/lib/api.ts`, after the `generate` export (line 9), add:

```ts
/** Move a generated image (and its sidecar) to the OS trash. */
export const deleteImage = (path: string) => invoke<void>("delete_image", { imagePath: path });
```

- [ ] **Step 3: Type-check**

Run: `npm run check`
Expected: 0 errors. (The new non-optional fields are only constructed on the Rust side; the TS interface is read-only here, so no call sites break.)

- [ ] **Step 4: Commit**

```bash
git add src/lib/types.ts src/lib/api.ts
git commit -m "feat(gallery): frontend batch fields + deleteImage API

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: HistoryStrip — selection ring + batch grouping

**Files:**
- Modify: `src/lib/components/HistoryStrip.svelte` (whole file)

- [ ] **Step 1: Replace the component**

Replace the entire contents of `src/lib/components/HistoryStrip.svelte` with:

```svelte
<script lang="ts">
  import { history, currentImage, currentItem, request } from "../stores";
  import { imageSrc } from "../api";
  import type { GalleryItem } from "../types";

  type Group = { batchId: string; items: GalleryItem[] };

  // Group the flat (newest-first) history by batch. First-encounter order is
  // preserved, so groups stay in recency order; members within a batch are
  // ordered by batch_index (disk read order isn't guaranteed).
  function groupByBatch(items: GalleryItem[]): Group[] {
    const groups: Group[] = [];
    const byKey = new Map<string, Group>();
    for (const it of items) {
      const key = it.batch_id || it.id;
      let g = byKey.get(key);
      if (!g) { g = { batchId: key, items: [] }; byKey.set(key, g); groups.push(g); }
      g.items.push(it);
    }
    for (const g of groups) g.items.sort((a, b) => a.batch_index - b.batch_index);
    return groups;
  }

  $: groups = groupByBatch($history);

  function open(item: GalleryItem) {
    currentImage.set(imageSrc(item.image_path));
    currentItem.set(item);
    request.set({ ...item.request });
  }
</script>

<div class="strip">
  {#each groups as g (g.batchId)}
    {#if g.items.length > 1}
      <div class="batch">
        {#each g.items as item (item.id)}
          <button class="thumb" class:selected={item.id === $currentItem?.id}
                  on:click={() => open(item)} title={item.request.prompt}>
            <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
            <span class="idx">{item.batch_index + 1}</span>
          </button>
        {/each}
      </div>
    {:else}
      {@const item = g.items[0]}
      <button class="thumb" class:selected={item.id === $currentItem?.id}
              on:click={() => open(item)} title={item.request.prompt}>
        <img src={imageSrc(item.image_path)} alt={item.request.prompt} />
      </button>
    {/if}
  {:else}
    <span class="empty">No images yet this session.</span>
  {/each}
</div>

<style>
  .strip { display:flex; gap:.4rem; overflow-x:auto; padding:.4rem; min-height:64px; align-items:center; }
  .batch { display:flex; gap:.4rem; position:relative; padding-bottom:5px; }
  .batch::after { content:''; position:absolute; left:0; right:0; bottom:0; height:3px;
    background:var(--accent); border-radius:2px; }
  .thumb { padding:0; border:1px solid var(--border); border-radius:6px; overflow:hidden;
    width:56px; height:56px; flex:0 0 auto; cursor:pointer; background:none; position:relative; }
  .thumb img { width:100%; height:100%; object-fit:cover; display:block; }
  .thumb.selected { outline:2px solid var(--accent); outline-offset:1px; border-color:var(--accent); }
  .idx { position:absolute; bottom:1px; left:1px; background:rgba(0,0,0,.6); color:#fff;
    font-size:.55rem; line-height:1.3; border-radius:3px; padding:0 .25rem; }
  .empty { opacity:.5; font-size:.8rem; }
</style>
```

- [ ] **Step 2: Type-check**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/HistoryStrip.svelte
git commit -m "feat(gallery): highlight selected thumbnail + group batches in strip

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: ImagePreview — inline-confirm delete + selection move

**Files:**
- Modify: `src/lib/components/ImagePreview.svelte` (whole file)

- [ ] **Step 1: Replace the component**

Replace the entire contents of `src/lib/components/ImagePreview.svelte` with:

```svelte
<script lang="ts">
  import { get } from "svelte/store";
  import { currentImage, currentItem, history, request } from "../stores";
  import { deleteImage, listHistory, imageSrc } from "../api";

  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  async function doDelete() {
    const item = get(currentItem);
    if (!item || busy) return;
    busy = true;
    error = null;
    try {
      const idx = get(history).findIndex((x) => x.id === item.id);
      await deleteImage(item.image_path);
      const next = await listHistory();
      history.set(next);
      // Select the item that slid into the deleted slot, else the new last, else clear.
      const pick = next[Math.min(idx, next.length - 1)] ?? null;
      if (pick) {
        currentImage.set(imageSrc(pick.image_path));
        currentItem.set(pick);
        request.set({ ...pick.request });
      } else {
        currentImage.set(null);
        currentItem.set(null);
      }
      confirming = false;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<div class="preview">
  {#if $currentImage}
    <img src={$currentImage} alt="generated result" />
    <div class="actions">
      {#if confirming}
        <span class="ask">Move to trash?</span>
        <button class="del" onclick={doDelete} disabled={busy}>Delete</button>
        <button onclick={() => (confirming = false)} disabled={busy}>Cancel</button>
      {:else}
        <button class="del" onclick={() => (confirming = true)} disabled={!$currentItem}>Delete</button>
      {/if}
    </div>
    {#if error}<span class="err">{error}</span>{/if}
  {:else}
    <p class="empty">Your generated image will appear here.</p>
  {/if}
</div>

<style>
  .preview { flex:1; display:flex; align-items:center; justify-content:center; position:relative;
    background:rgba(0,0,0,.2); border-radius:8px; overflow:hidden; }
  img { max-width:100%; max-height:100%; object-fit:contain; }
  .actions { position:absolute; top:.5rem; right:.5rem; display:flex; gap:.4rem; align-items:center; }
  .actions button { font:inherit; font-size:.75rem; padding:.25rem .6rem; cursor:pointer; }
  .actions button:disabled { opacity:.5; cursor:default; }
  .del { background:rgba(180,40,40,.85); color:#fff; border:none; border-radius:5px; }
  .ask { font-size:.75rem; background:rgba(0,0,0,.6); color:#fff; padding:.25rem .5rem; border-radius:5px; }
  .err { position:absolute; bottom:.5rem; left:.5rem; right:.5rem; color:#ffb4b4; font-size:.75rem;
    background:rgba(0,0,0,.6); padding:.3rem .5rem; border-radius:5px; }
  .empty { opacity:.5; }
</style>
```

- [ ] **Step 2: Type-check**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/ImagePreview.svelte
git commit -m "feat(gallery): inline-confirm delete on preview with selection move

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Full verification

**Files:** none (verification only)

- [ ] **Step 1: Backend tests**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5`
Expected: all tests pass (47 prior + the new gallery tests).

- [ ] **Step 2: Frontend check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Manual E2E (dev box, `npm run tauri dev`)**

Verify each:
- Generate a batch (batch_count ≥ 3): the images render grouped under the colored bar, tagged 1..N in order.
- Click different thumbnails: the amber ring follows the click; the big preview and the metadata panel match the ringed thumbnail.
- Click Delete on the preview → "Move to trash?" inline confirm appears → Delete: the image leaves the strip, the file is in the OS trash (check the file manager), and the selection moves to a neighbor.
- Delete one member of a batch: the other members remain, still grouped.
- Restart the app: grouping and selection-on-click still work (rebuilt from sidecars); legacy images from before this change still appear (as singletons).

- [ ] **Step 4: Update roadmap memory**

Append the completed feature to `/home/idaho/.claude/projects/-home-idaho-g-mst-fridai/memory/fridai-roadmap.md` (mark delete / selection-highlight / batch-grouping DONE; note the deferred app-settings menu and background downloads remain).

- [ ] **Step 5: Finish the branch**

Use superpowers:finishing-a-development-branch on `feat/gallery-ux`.
```
