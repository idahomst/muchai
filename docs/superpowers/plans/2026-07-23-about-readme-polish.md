# About Dialog + README Polish — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Address E2E feedback on the shipped About dialog + README: (1) make the version label visibly clickable, (2) show the actual stable-diffusion.cpp engine version, (3) add the Neural-Pixel hyperlink, and (4) add real screenshots to the README.

**Architecture:** Mostly frontend/docs, plus ONE small backend command. The engine version is obtained by a runtime probe (`sd-cli --version` → stdout banner `... commit <hash>`), cached in `AppState` exactly like `list_gpu_devices` caches GPU devices, and exposed via a new `engine_version` Tauri command. The About dialog fetches it on open and shows it beside the engine credit. The clickable cue is a link-style underline on the existing `.ver` button. README gains a hero screenshot + a Screenshots gallery and the Neural-Pixel link.

**Tech Stack:** Rust (Tauri v2 command + pure parser with a unit test), Svelte 5 (runes), TypeScript, Markdown.

**Design decisions (user-approved 2026-07-23):**
- Engine version = **runtime probe, cached** (reflects the ACTUAL bundled binary; never drifts). Verified: `sd-cli --version` prints `stable-diffusion.cpp version unknown, commit b290693` to **stdout** and exits 0.
- Clickable cue = **link-style underline** (matches the dialog's credit links).
- Screenshots = **hero (dark main window) + a Screenshots gallery** with the other four.

**Testing note:** Backend has real unit tests → the engine-version *parser* is a pure function and MUST get a TDD unit test in `devices.rs` (mirror `parse_vulkan_devices` tests). `cargo test` must go from 211 → 212+ (new test) with 0 failures. Frontend has NO unit-test runner; its only gate is `npm run check` (0/0). Do NOT add vitest/jest.

**Commit convention:** every commit body ends with:
`Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`

---

## File Structure

- `src-tauri/src/devices.rs` (modify) — add a pure `parse_engine_version` + a `engine_version` spawn-probe, mirroring the existing `parse_vulkan_devices` / `enumerate` pair. Unit tests live in the existing `#[cfg(test)] mod tests`.
- `src-tauri/src/commands.rs` (modify) — add an `engine_version: Arc<Mutex<Option<Option<String>>>>` cache field to `AppState` and an `engine_version` command mirroring `list_gpu_devices`.
- `src-tauri/src/lib.rs` (modify) — initialise the new cache field and register the command.
- `src/lib/api.ts` (modify) — add the `engineVersion` wrapper.
- `src/lib/about.ts` (modify) — add the Neural-Pixel URL.
- `src/lib/components/AboutDialog.svelte` (modify) — fetch + show the engine commit; underline the header version chip is NOT needed here (dialog links already underlined).
- `src/lib/components/ResourceMonitor.svelte` (modify) — persistent underline on the `.ver` trigger button.
- `README.md` (modify) — Neural-Pixel link, hero screenshot, Screenshots gallery.
- `docs/screenshots/*.png` (create) — the five provided screenshots, renamed.

> **Cache type note:** the engine version cache is `Option<Option<String>>`: outer `None` = "not probed yet", `Some(None)` = "probed, but no version found / no binary", `Some(Some(s))` = "probed, got commit `s`". This lets a negative result stay cached (never re-probe on every open) — unlike `list_gpu_devices`, whose empty `Vec` already doubles as its negative result.

---

### Task 1: Backend — engine-version probe, cache, and command

**Files:**
- Modify: `src-tauri/src/devices.rs` (add `parse_engine_version` + `engine_version`; tests in existing `mod tests`)
- Modify: `src-tauri/src/commands.rs` (`AppState` field + `engine_version` command)
- Modify: `src-tauri/src/lib.rs` (init field + register command)

- [ ] **Step 1: Write the failing parser tests**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/devices.rs` (after the existing tests, before the closing `}`):

```rust
    #[test]
    fn parses_commit_from_version_banner() {
        // The pinned engine prints this exact banner to stdout on `--version`.
        let out = "stable-diffusion.cpp version unknown, commit b290693\n";
        assert_eq!(parse_engine_version(out), Some("b290693".to_string()));
    }

    #[test]
    fn parses_commit_ignoring_surrounding_lines() {
        let out = "some preamble\nstable-diffusion.cpp version 1.2, commit deadbeef1\nmore\n";
        assert_eq!(parse_engine_version(out), Some("deadbeef1".to_string()));
    }

    #[test]
    fn engine_version_parse_none_when_absent() {
        assert_eq!(parse_engine_version(""), None);
        assert_eq!(parse_engine_version("no version banner here\n"), None);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test parses_commit`
Expected: FAIL to compile — `cannot find function parse_engine_version`.

- [ ] **Step 3: Implement the pure parser + the spawn probe**

Add to `src-tauri/src/devices.rs` (place `parse_engine_version` right after `parse_vulkan_devices`, and `engine_version` right after `enumerate`, to mirror the pure/spawner pairing):

```rust
/// Parse the engine's `--version` banner (`stable-diffusion.cpp version <v>,
/// commit <hash>`) into the commit hash. Returns the first `commit <hex>` token
/// found; `None` if the banner is absent or malformed. Pure so it is testable.
pub fn parse_engine_version(output: &str) -> Option<String> {
    let idx = output.find("commit ")?;
    let after = &output[idx + "commit ".len()..];
    let hash: String = after.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    (!hash.is_empty()).then_some(hash)
}

/// One-time probe: run `sd-cli --version` and parse the commit hash from its
/// stdout banner. The engine exits 0 immediately, but we still bound the wait
/// (mirrors `enumerate`) so a wedged binary can never hang the UI. Never panics;
/// returns `None` on any failure, timeout, or unparseable output.
pub fn engine_version(binary: &Path) -> Option<String> {
    if !binary.exists() {
        return None;
    }
    let mut child = Command::new(binary)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    let mut stdout = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });

    let captured = rx.recv_timeout(Duration::from_secs(10)).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    parse_engine_version(&captured)
}
```

- [ ] **Step 4: Run the parser tests to verify they pass**

Run: `cd src-tauri && cargo test parses_commit engine_version_parse`
Expected: PASS (3 new tests green).

- [ ] **Step 5: Add the cache field to `AppState`**

In `src-tauri/src/commands.rs`, extend the `AppState` struct (around line 27-32). The cache is `Option<Option<String>>` — see the Cache type note above:

```rust
pub struct AppState {
    pub config: Mutex<AppConfig>,
    pub child: ChildSlot,
    pub download_cancel: Arc<AtomicBool>,
    pub gpu_devices: Arc<Mutex<Option<Vec<GpuDevice>>>>,
    pub engine_version: Arc<Mutex<Option<Option<String>>>>,
}
```

- [ ] **Step 6: Add the `engine_version` command**

In `src-tauri/src/commands.rs`, add right after `list_gpu_devices` (mirrors its caching + `resolve_binary` pattern):

```rust
#[tauri::command]
pub fn engine_version(app: AppHandle, state: State<AppState>) -> Option<String> {
    if let Some(cached) = state.engine_version.lock().unwrap().as_ref() {
        return cached.clone();
    }
    let cfg = state.config.lock().unwrap().clone();
    let version = match resolve_binary(&app, &cfg) {
        Some(bin) => crate::devices::engine_version(&bin),
        None => None,
    };
    *state.engine_version.lock().unwrap() = Some(version.clone());
    version
}
```

- [ ] **Step 7: Initialise the field + register the command in `lib.rs`**

In `src-tauri/src/lib.rs`, add to the `AppState { ... }` initialiser (after `gpu_devices: Arc::new(Mutex::new(None)),`):

```rust
            engine_version: Arc::new(Mutex::new(None)),
```

And add to the `tauri::generate_handler![ ... ]` list (after `commands::list_gpu_devices,`):

```rust
            commands::engine_version,
```

- [ ] **Step 8: Verify the whole backend compiles + all tests pass**

Run: `cd src-tauri && cargo test`
Expected: PASS, test count 211 → 214 (3 new), 0 failures.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/devices.rs src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(about): probe + cache engine commit version

Add a bounded `sd-cli --version` probe (parse_engine_version pure fn +
engine_version spawner in devices.rs), cached in AppState like GPU
devices, exposed via the engine_version Tauri command.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Frontend — Neural-Pixel link, engine version display, clickable cue

**Files:**
- Modify: `src/lib/about.ts` (add Neural-Pixel URL)
- Modify: `src/lib/api.ts` (add `engineVersion` wrapper)
- Modify: `src/lib/components/AboutDialog.svelte` (fetch + show commit)
- Modify: `src/lib/components/ResourceMonitor.svelte` (persistent underline)

There is no frontend unit-test runner; the gate for this task is `npm run check` (0 errors / 0 warnings). Do NOT add vitest/jest.

- [ ] **Step 1: Add the Neural-Pixel URL in `about.ts`**

In `src/lib/about.ts`, the "Inspired by" section, give Neural-Pixel its URL:

```ts
  {
    heading: "Inspired by",
    items: [
      { label: "Draw Things", url: "https://drawthings.ai" },
      { label: "Neural-Pixel", url: "https://github.com/Luiz-Alcantara/Neural-Pixel" },
    ],
  },
```

- [ ] **Step 2: Add the `engineVersion` wrapper in `api.ts`**

In `src/lib/api.ts`, right after the `listGpuDevices` line (line 7), add:

```ts
/** The bundled engine's commit hash (e.g. "b290693"), or null if unknown. Cached server-side. */
export const engineVersion = () => invoke<string | null>("engine_version");
```

- [ ] **Step 3: Fetch + display the engine commit in `AboutDialog.svelte`**

The engine credit is the first item of the first CREDITS section (`heading: "Image engine"`). We show the probed commit as a small suffix on the About header's engine line WITHOUT hardcoding it. Fetch on mount via `$state` + `$effect`, and render it appended to the matching credit item's note.

Update the `<script>` block of `src/lib/components/AboutDialog.svelte`:

```svelte
<script lang="ts">
  import { version } from "../../../package.json";
  import { APP_TAGLINE, CREDITS } from "../about";
  import { openExternal, engineVersion } from "../api";

  let { onclose }: { onclose: () => void } = $props();
  let closeBtn = $state<HTMLButtonElement>();
  let engineCommit = $state<string | null>(null);

  $effect(() => { closeBtn?.focus(); });

  // Best-effort engine-version probe; a failure just leaves the commit hidden.
  $effect(() => {
    engineVersion().then((v) => { engineCommit = v; }).catch(() => {});
  });

  function onkeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
  }

  // Best-effort: a dead/unreachable link must never crash or alert (mirrors
  // openFolder's fire-and-forget spirit).
  function open(url: string | undefined) {
    if (url) openExternal(url).catch(() => {});
  }
</script>
```

Then, in the `{#each section.items as item}` loop, show the commit next to the engine credit. The engine item is identified by its heading being "Image engine". Add a commit chip after the item's note, only for that section. Replace the `<li>` body:

```svelte
            <li>
              {#if item.url}
                <button class="link" onclick={() => open(item.url)}>{item.label}</button>
              {:else}
                <span class="name">{item.label}</span>
              {/if}
              {#if item.note}<span class="note"> — {item.note}</span>{/if}
              {#if section.heading === "Image engine" && engineCommit}
                <span class="commit" title="stable-diffusion.cpp build in use">(commit {engineCommit})</span>
              {/if}
            </li>
```

Add a `.commit` style in the `<style>` block (after `.note`):

```css
  .commit { opacity:.6; font-variant-numeric:tabular-nums; margin-left:.3rem; }
```

- [ ] **Step 4: Persistent link-style underline on the `ResourceMonitor` version trigger**

In `src/lib/components/ResourceMonitor.svelte`, update the `.ver` styles (lines 42-44) so the button reads clickable at rest (underline) and highlights on hover (accent):

```css
  .ver { margin-left:auto; opacity:.7; padding-left:1rem; font:inherit;
    background:none; border:none; cursor:pointer; color:inherit;
    text-decoration:underline; text-underline-offset:2px; }
  .ver:hover { opacity:1; color:var(--accent-bright); }
```

- [ ] **Step 5: Run the frontend gate**

Run: `npm run check`
Expected: `svelte-check` reports 0 errors, 0 warnings across all files.

- [ ] **Step 6: Commit**

```bash
git add src/lib/about.ts src/lib/api.ts src/lib/components/AboutDialog.svelte src/lib/components/ResourceMonitor.svelte
git commit -m "$(cat <<'EOF'
feat(about): show engine commit, link Neural-Pixel, mark version clickable

About dialog now fetches the engine commit and shows it beside the
engine credit; Neural-Pixel gets its GitHub link; the resource-bar
version trigger carries a persistent underline so it reads as clickable.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: README — Neural-Pixel link + screenshots

**Files:**
- Create: `docs/screenshots/main-dark.png`, `main-light.png`, `edit-model.png`, `add-model.png`, `preferences.png`
- Modify: `README.md`

Source screenshots (all confirmed to exist) → destination names:
| Source | Destination |
|---|---|
| `/home/idaho/Pictures/Screenshot from 2026-07-23 11-17-56.png` | `docs/screenshots/main-dark.png` (hero) |
| `/home/idaho/Pictures/Screenshot from 2026-07-23 11-23-07.png` | `docs/screenshots/main-light.png` |
| `/home/idaho/Pictures/Screenshot from 2026-07-23 11-21-28.png` | `docs/screenshots/edit-model.png` |
| `/home/idaho/Pictures/Screenshot from 2026-07-23 11-21-59.png` | `docs/screenshots/add-model.png` |
| `/home/idaho/Pictures/Screenshot from 2026-07-23 11-22-39.png` | `docs/screenshots/preferences.png` |

- [ ] **Step 1: Copy the screenshots into the repo**

```bash
mkdir -p docs/screenshots
cp "/home/idaho/Pictures/Screenshot from 2026-07-23 11-17-56.png" docs/screenshots/main-dark.png
cp "/home/idaho/Pictures/Screenshot from 2026-07-23 11-23-07.png" docs/screenshots/main-light.png
cp "/home/idaho/Pictures/Screenshot from 2026-07-23 11-21-28.png" docs/screenshots/edit-model.png
cp "/home/idaho/Pictures/Screenshot from 2026-07-23 11-21-59.png" docs/screenshots/add-model.png
cp "/home/idaho/Pictures/Screenshot from 2026-07-23 11-22-39.png" docs/screenshots/preferences.png
ls -l docs/screenshots
```
Expected: five `.png` files listed.

- [ ] **Step 2: Replace the hero placeholder in `README.md`**

Replace line 10 (`<!-- screenshot: docs/screenshot.png -->`) with the hero image:

```markdown
![MuchAI main window](docs/screenshots/main-dark.png)
```

- [ ] **Step 3: Add the Neural-Pixel link in Acknowledgements**

In `README.md`, change the "Inspired by" line (line 71):

```markdown
- **Inspired by:** [Draw Things](https://drawthings.ai) and
  [Neural-Pixel](https://github.com/Luiz-Alcantara/Neural-Pixel).
```

- [ ] **Step 4: Add a Screenshots gallery section**

Insert a new `## Screenshots` section immediately after the Features section (before `## Requirements`, i.e. after current line 22). Use a 2-column table so the captions stay readable:

```markdown
## Screenshots

| | |
|---|---|
| ![Main window, light theme](docs/screenshots/main-light.png)<br>Main window (light theme) | ![Add a model](docs/screenshots/add-model.png)<br>Add a model from the curated catalog or a URL |
| ![Edit a model](docs/screenshots/edit-model.png)<br>Edit a model's components and defaults | ![Preferences](docs/screenshots/preferences.png)<br>Preferences |

```

- [ ] **Step 5: Sanity-check the Markdown**

Run: `grep -n "docs/screenshots" README.md`
Expected: 5 image references (1 hero + 4 gallery). Confirm no leftover `docs/screenshot.png` placeholder:
Run: `grep -n "docs/screenshot.png" README.md` → Expected: no output.

- [ ] **Step 6: Commit**

```bash
git add docs/screenshots README.md
git commit -m "$(cat <<'EOF'
docs(readme): add screenshots gallery + Neural-Pixel link

Hero shot + a four-image gallery (light main window, add/edit model,
preferences); link Neural-Pixel in Acknowledgements.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Full verification

**Files:** none (verification only).

- [ ] **Step 1: Backend tests**

Run: `cd src-tauri && cargo test`
Expected: 0 failures; count 211 → 214.

- [ ] **Step 2: Frontend check**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Confirm a clean tree + review the log**

Run: `git status` (expect clean) and `git log --oneline -4` (expect the three feature commits from Tasks 1-3).

---

## Out of scope (captured for later)

- **Grandma-friendly user how-to doc/web page** — a separate, non-technical walkthrough of the MuchAI flow (models, parameters, settings) written for a completely inexperienced user. Recorded in roadmap memory; NOT implemented here.
