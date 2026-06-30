# Background Model Downloads Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a model download run in the background — with progress shown inline in the Model panel — so the user can dismiss the download dialog and keep generating with a different model.

**Architecture:** Frontend-only. The backend already runs `download_model` (in `spawn_blocking`) concurrently with `generate`; the only blocker is the modal `DownloadDialog`. Lift download state into a global `downloadStatus` store (mirroring the existing `genStatus` discriminated-union pattern) with `startDownload`/`cancelActiveDownload` helpers that own the download lifecycle. `DownloadDialog` becomes fire-and-close; `ModelLibrary` renders inline progress/notice and refreshes its list on completion without touching the active model selection.

**Tech Stack:** SvelteKit + Svelte 5 runes, TypeScript, Tauri v2 IPC (`@tauri-apps/api`).

**Reference:** Design spec at `docs/superpowers/specs/2026-06-30-background-downloads-design.md`.

**Testing note:** This project has no JS/Svelte component test runner — frontend changes are verified by `npm run check` (svelte-check) plus manual E2E, exactly as the JPEG-output and gallery-UX frontend tasks were. There are no new unit tests; `npm run check` is the per-task gate. No Rust files change, so the Rust suite is only re-confirmed at the end.

---

## File structure

- `src/lib/stores.ts` — add `DownloadStatus` type, `downloadStatus` store, `startDownload`, `cancelActiveDownload` (owns the download lifecycle + single-flight + cancel handling).
- `src/lib/components/DownloadDialog.svelte` — drop local progress/busy state and the mount-time listener; start-and-close; disable starts while a download is active.
- `src/lib/components/ModelLibrary.svelte` — inline status line (active/done/error), Cancel + dismiss, refresh-on-done `$effect`; stop passing `ondownloaded`.

No Rust files change.

---

### Task 1: `downloadStatus` store + lifecycle helpers

**Files:**
- Modify: `src/lib/stores.ts`

- [ ] **Step 1: Add imports**

In `src/lib/stores.ts`, replace the first line:

```ts
import { writable } from "svelte/store";
```

with:

```ts
import { writable, get } from "svelte/store";
import { downloadModel, cancelDownload, onDownloadProgress } from "./api";
```

(`api.ts` does not import `stores.ts`, so this introduces no import cycle.)

- [ ] **Step 2: Append the store + helpers**

At the end of `src/lib/stores.ts` (after the `genStatus` export), add:

```ts
export type DownloadStatus =
  | { kind: "idle" }
  | { kind: "active"; name: string; downloaded: number; total: number | null }
  | { kind: "done"; name: string }                    // dismissible notice
  | { kind: "error"; name: string; message: string }; // dismissible notice

export const downloadStatus = writable<DownloadStatus>({ kind: "idle" });

// Single-flight: at most one download runs at a time, so one module-level flag
// is enough. It lets a user-initiated cancel resolve to `idle` instead of
// surfacing the backend's cancellation as an error.
let cancelRequested = false;

/** Start a background download. No-op if one is already active (single-flight). */
export async function startDownload(url: string, token: string, name: string): Promise<void> {
  if (get(downloadStatus).kind === "active") return;
  cancelRequested = false;
  downloadStatus.set({ kind: "active", name, downloaded: 0, total: null });
  const unlisten = await onDownloadProgress((p) => {
    downloadStatus.update((s) =>
      s.kind === "active" ? { ...s, downloaded: p.downloaded, total: p.total } : s,
    );
  });
  try {
    await downloadModel(url, token);
    downloadStatus.set({ kind: "done", name });
  } catch (e) {
    downloadStatus.set(
      cancelRequested ? { kind: "idle" } : { kind: "error", name, message: String(e) },
    );
  } finally {
    unlisten();
  }
}

/** Cancel the active download; the backend removes the partial `.part` file. */
export function cancelActiveDownload(): void {
  cancelRequested = true;
  void cancelDownload();
}
```

- [ ] **Step 3: Type-check**

Run: `cd /home/idaho/g/mst/fridai && npm run check 2>&1 | tail -3`
Expected: 0 errors, 0 warnings. (`DownloadProgress.total` is `number | null`, matching the store field; the store compiles even though no component consumes it yet.)

- [ ] **Step 4: Commit**

```bash
cd /home/idaho/g/mst/fridai && git add src/lib/stores.ts && git commit -m "feat(background-downloads): downloadStatus store + start/cancel helpers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `DownloadDialog` — fire-and-close, no local progress

**Files:**
- Modify: `src/lib/components/DownloadDialog.svelte`

- [ ] **Step 1: Replace the entire file**

Replace the full contents of `src/lib/components/DownloadDialog.svelte` with:

```svelte
<script lang="ts">
  import { sysStats, downloadStatus, startDownload } from "../stores";
  import { starterModels } from "../api";
  import type { RatedModel, Suitability } from "../types";
  import { onMount } from "svelte";

  let { onclose }: { onclose: () => void } = $props();

  let starters = $state<RatedModel[]>([]);
  let url = $state("");
  let token = $state("");

  const fmt = (b: number) => (b >= 1e9 ? `${(b / 1e9).toFixed(1)} GB` : `${(b / 1e6).toFixed(0)} MB`);
  const active = $derived($downloadStatus.kind === "active");

  const badge: Record<Suitability, string> = {
    recommended: "✅ Recommended",
    tight: "⚠️ Tight for your GPU",
    too_big: "❌ Likely too big",
    unknown: "— GPU unknown",
  };

  onMount(() => {
    (async () => {
      starters = await starterModels($sysStats?.gpu?.vram_total_mb ?? null);
    })();
  });

  // Derive a display name from a URL (filename without extension) for paste-URL
  // downloads, where we have no catalog name.
  function nameFromUrl(u: string): string {
    const base = u.split("/").pop()?.split("?")[0] ?? u;
    return base.replace(/\.[^.]+$/, "") || u;
  }

  // Start the download in the background and close the dialog; progress shows
  // inline in the Model panel. No-op while a download is already active.
  function start(downloadUrl: string, name: string) {
    if (active || !downloadUrl) return;
    void startDownload(downloadUrl, token.trim(), name);
    onclose();
  }
</script>

<div class="backdrop" onclick={(e) => { if (e.target === e.currentTarget) onclose(); }} role="presentation">
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Download model">
    <h2>Download a model</h2>

    {#if active}
      <p class="note">A download is already running — see the Model panel for progress.</p>
    {/if}

    <section>
      <h3>Starter models</h3>
      {#each starters as s (s.id)}
        <div class="starter">
          <div class="meta">
            <span class="name">{s.name}</span>
            <span class="sub">{fmt(s.size_bytes)} · {badge[s.suitability]}</span>
          </div>
          <button class="btn-secondary" disabled={active} onclick={() => start(s.url, s.name)}>Get</button>
        </div>
      {/each}
    </section>

    <section>
      <h3>Or paste a URL</h3>
      <input class="in" type="text" placeholder="https://…/model.safetensors" bind:value={url} />
      <input class="in" type="password" placeholder="Access token (optional, for gated/civitai)" bind:value={token} />
      <div class="row">
        <button class="btn-primary" disabled={active || !url.trim()} onclick={() => start(url.trim(), nameFromUrl(url.trim()))}>Download</button>
        <button class="btn-secondary" onclick={onclose}>Close</button>
      </div>
    </section>
  </div>
</div>

<style>
  .backdrop { position:fixed; inset:0; background:rgba(0,0,0,.5); display:flex;
    align-items:center; justify-content:center; z-index:50; }
  .dialog { background:var(--bg, #1e1e1e); border:1px solid var(--border); border-radius:10px;
    padding:1.2rem; width:min(460px, 92vw); max-height:88vh; overflow-y:auto; display:flex; flex-direction:column; gap:.8rem; }
  h2 { margin:0; font-size:1.05rem; }
  h3 { margin:.2rem 0; font-size:.8rem; opacity:.7; }
  .note { font-size:.78rem; opacity:.85; margin:0; padding:.4rem .5rem;
    background:rgba(110,168,254,.12); border:1px solid var(--border); border-radius:6px; }
  .starter { display:flex; align-items:center; justify-content:space-between; gap:.6rem; padding:.35rem 0; }
  .meta { display:flex; flex-direction:column; }
  .name { font-size:.9rem; }
  .sub { font-size:.72rem; opacity:.7; }
  .in { width:100%; font:inherit; padding:.4rem; box-sizing:border-box; margin-bottom:.4rem; }
  .row { display:flex; gap:.5rem; }
  button { font:inherit; font-size:.8rem; padding:.35rem .7rem; cursor:pointer; }
  button:disabled { opacity:.5; cursor:default; }
</style>
```

Changes vs. the old file: removed the `ondownloaded` prop, the `busy`/`cancelling`/`downloaded`/`total`/`unlisten` state, the `pct` derived, the `cancel()` function, the `onDownloadProgress` mount listener, and the in-dialog progress view. The two `start(...)` call sites now pass a name and the dialog closes immediately; the start buttons disable while a download is `active`.

- [ ] **Step 2: Type-check**

Run: `cd /home/idaho/g/mst/fridai && npm run check 2>&1 | tail -3`
Expected: errors in `ModelLibrary.svelte` only — it still passes the now-removed `ondownloaded` prop (`Object literal may only specify known properties` / missing prop). `DownloadDialog.svelte` itself has 0 errors. Task 3 resolves the `ModelLibrary` errors. (If you prefer a clean check between tasks, this is acceptable mid-refactor; the build is green again after Task 3.)

- [ ] **Step 3: Commit**

```bash
cd /home/idaho/g/mst/fridai && git add src/lib/components/DownloadDialog.svelte && git commit -m "feat(background-downloads): DownloadDialog starts in background and closes

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: `ModelLibrary` — inline status, cancel/dismiss, refresh-on-done

**Files:**
- Modify: `src/lib/components/ModelLibrary.svelte`

- [ ] **Step 1: Update imports**

In `src/lib/components/ModelLibrary.svelte`, replace:

```svelte
  import { request, models } from "../stores";
  import { listModels, deleteModel } from "../api";
  import DownloadDialog from "./DownloadDialog.svelte";
```

with:

```svelte
  import { request, models, downloadStatus, cancelActiveDownload } from "../stores";
  import { listModels, deleteModel } from "../api";
  import DownloadDialog from "./DownloadDialog.svelte";
```

- [ ] **Step 2: Remove the `onDownloaded` handler, add progress derived + refresh effect**

In the same file, delete the entire `onDownloaded` function:

```svelte
  async function onDownloaded(path: string) {
    await refresh();
    request.update((r) => ({ ...r, model_path: path }));
    showDownload = false;
  }
```

Then, immediately after the `refresh` function, add:

```svelte
  const dlPct = $derived(
    $downloadStatus.kind === "active" && $downloadStatus.total
      ? Math.round(($downloadStatus.downloaded / $downloadStatus.total) * 100)
      : 0,
  );

  // Refresh the model list once when a background download completes, so the new
  // model appears in the dropdown. The active selection (`model_path`) is left
  // untouched. `handledDone` guards against re-refreshing for the same notice.
  let handledDone = $state<string | null>(null);
  $effect(() => {
    const s = $downloadStatus;
    if (s.kind === "done" && handledDone !== s.name) {
      handledDone = s.name;
      void refresh();
    } else if (s.kind === "idle" || s.kind === "active") {
      handledDone = null;
    }
  });
```

(`request` is still imported — it remains used by `onSelect`, `removeSelected`, and `orphanPath`.)

- [ ] **Step 3: Fix the `DownloadDialog` usage**

In the same file, replace:

```svelte
{#if showDownload}
  <DownloadDialog onclose={() => (showDownload = false)} ondownloaded={onDownloaded} />
{/if}
```

with:

```svelte
{#if showDownload}
  <DownloadDialog onclose={() => (showDownload = false)} />
{/if}
```

- [ ] **Step 4: Add the inline status line**

In the same file, find the `.field` block and the delete-error line near its end:

```svelte
  {#if $models.length === 0}
    <span class="hint">No models found. Click Download… to get one.</span>
  {/if}
  {#if error}<span class="err">{error}</span>{/if}
</div>
```

Replace it with:

```svelte
  {#if $models.length === 0}
    <span class="hint">No models found. Click Download… to get one.</span>
  {/if}
  {#if error}<span class="err">{error}</span>{/if}

  {#if $downloadStatus.kind === "active"}
    <div class="dl">
      <div class="bar"><div class="fill" style="width:{dlPct}%"></div></div>
      <span class="dl-text">⬇ {$downloadStatus.name}… {fmtSize($downloadStatus.downloaded)}{$downloadStatus.total ? ` / ${fmtSize($downloadStatus.total)} (${dlPct}%)` : "…"}</span>
      <button class="btn-secondary" onclick={cancelActiveDownload}>Cancel</button>
    </div>
  {:else if $downloadStatus.kind === "done"}
    <div class="dl">
      <span class="dl-text ok">✓ {$downloadStatus.name} ready</span>
      <button class="x" aria-label="Dismiss" onclick={() => downloadStatus.set({ kind: "idle" })}>✕</button>
    </div>
  {:else if $downloadStatus.kind === "error"}
    <div class="dl">
      <span class="dl-text err">⚠ {$downloadStatus.name}: {$downloadStatus.message}</span>
      <button class="x" aria-label="Dismiss" onclick={() => downloadStatus.set({ kind: "idle" })}>✕</button>
    </div>
  {/if}
</div>
```

- [ ] **Step 5: Add styles**

In the same file's `<style>` block, after the existing `.err` rule, add:

```svelte
  .dl { display:flex; align-items:center; gap:.5rem; margin-top:.4rem; flex-wrap:wrap; }
  .bar { flex:1 1 100%; height:8px; background:rgba(255,255,255,.1); border-radius:4px; overflow:hidden; }
  .fill { height:100%; background:var(--accent, #6ea8fe); transition:width .15s linear; }
  .dl-text { font-size:.72rem; opacity:.85; }
  .dl-text.ok { color:#4caf83; opacity:1; }
  .dl-text.err { color:#ff6b6b; opacity:1; }
  .x { background:none; border:none; color:inherit; cursor:pointer; font-size:.75rem; opacity:.7; padding:0 .2rem; }
```

- [ ] **Step 6: Type-check (now clean)**

Run: `cd /home/idaho/g/mst/fridai && npm run check 2>&1 | tail -3`
Expected: 0 errors, 0 warnings. (`$downloadStatus.kind === "..."` narrows the union in templates — the same idiom `GenerateBar.svelte` uses with `$genStatus`.)

- [ ] **Step 7: Commit**

```bash
cd /home/idaho/g/mst/fridai && git add src/lib/components/ModelLibrary.svelte && git commit -m "feat(background-downloads): inline download status + refresh-on-done in ModelLibrary

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: Verification + finish branch

**Files:** none (verification only)

- [ ] **Step 1: Frontend check**

Run: `cd /home/idaho/g/mst/fridai && npm run check 2>&1 | tail -3`
Expected: 0 errors, 0 warnings.

- [ ] **Step 2: Backend tests unchanged**

Run: `cd /home/idaho/g/mst/fridai/src-tauri && cargo test --lib 2>&1 | tail -2`
Expected: all 57 tests pass (no Rust files changed).

- [ ] **Step 3: Manual E2E (dev box, `npm run tauri dev`)**

Verify:
- Open **Download…**, click **Get** on a starter (or paste a URL + **Download**). The dialog closes immediately and an inline progress bar appears under the Model controls, advancing as bytes arrive.
- While it downloads, select a **different** model and **Generate** — generation runs normally and is unaffected by the download.
- On completion: the inline line shows **✓ {name} ready**, the new model appears in the **dropdown**, and the **active model selection is unchanged**. Dismiss the notice with ✕.
- **Cancel** mid-download → the inline status clears (returns to idle) and no `.part` file remains in the models folder.
- Reopen **Download…** while a download is active → the **Get**/**Download** buttons are disabled and a "download is already running" note shows.
- Paste-URL download → the inline status shows a sensible name derived from the URL filename.
- A failing download (e.g. bad URL) → an inline **⚠ {name}: {message}** error appears and is dismissible.

- [ ] **Step 4: Update roadmap memory**

In `/home/idaho/.claude/projects/-home-idaho-g-mst-fridai/memory/fridai-roadmap.md`, mark roadmap item 3 (Background model downloads) DONE, noting it was a frontend-only change (new `downloadStatus` store + helpers; `DownloadDialog` fire-and-close; inline status in `ModelLibrary`; no Rust change) and that it also clears the "Background model downloads" deferred item from the Gallery-UX cluster note.

- [ ] **Step 5: Finish the branch**

Use superpowers:finishing-a-development-branch on `feat/background-downloads`.
