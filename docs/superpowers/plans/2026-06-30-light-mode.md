# Light Mode Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a manual dark/light theme toggle in the header, persisted in `AppConfig`, applied via semantic CSS-variable tokens with no startup flash.

**Architecture:** All themeable colors become CSS custom properties in `src/app.css` (`:root` = dark defaults, `:root[data-theme="light"]` = light overrides). The Rust `AppConfig.theme` field is the source of truth; `localStorage.theme` is a write-through cache read by a pre-paint inline script so light-mode users see no flash. A header toggle flips the theme optimistically and persists it, reverting on error.

**Tech Stack:** Tauri v2 (Rust, serde) backend; SvelteKit + Svelte 5 (`$state`/`$derived`/runes) frontend; plain CSS custom properties.

**Spec:** `docs/superpowers/specs/2026-06-30-light-mode-design.md`

**Testing note:** The Rust side has `cargo test` (run from `src-tauri/`) and gets real failing-test-first TDD. The frontend has **no unit-test harness** — its gate is `npm run check` (svelte-check, run from repo root) plus the manual E2E checklist in Task 9. For frontend tasks, "verify" means svelte-check passes with 0 errors and the literal-sweep grep returns empty.

---

### Task 1: Rust `Theme` enum + `AppConfig.theme` field

**Files:**
- Modify: `src-tauri/src/types.rs` (add `Theme` enum after `OutputFormat`; add field to `AppConfig`; add a test)
- Modify: `src-tauri/src/config.rs` (set default in `default_config`; add tests)

- [ ] **Step 1: Write failing test for snake_case wire form**

In `src-tauri/src/types.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn theme_serializes_snake_case() {
        assert_eq!(serde_json::to_string(&Theme::Dark).unwrap(), "\"dark\"");
        assert_eq!(serde_json::to_string(&Theme::Light).unwrap(), "\"light\"");
    }

    #[test]
    fn theme_defaults_to_dark() {
        assert_eq!(Theme::default(), Theme::Dark);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib theme_`
Expected: FAIL — `cannot find type Theme` / `Theme` not in scope (does not compile yet).

- [ ] **Step 3: Add the `Theme` enum**

In `src-tauri/src/types.rs`, immediately after the `impl Default for OutputFormat { ... }` block (after line ~63), add:

```rust
/// UI color theme. Persisted in `AppConfig`. Defaults to Dark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Dark,
    Light,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::Dark
    }
}
```

- [ ] **Step 4: Add the `theme` field to `AppConfig`**

In `src-tauri/src/types.rs`, inside `pub struct AppConfig`, add this field directly above `pub last_request: GenerationRequest,`:

```rust
    /// UI color theme. Defaults to Dark; pre-feature config files lack this key
    /// and deserialize as Dark.
    #[serde(default)]
    pub theme: Theme,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib theme_`
Expected: still FAILS to compile — `default_config()` in `config.rs` does not yet build `AppConfig` with `theme`. That is fixed in Step 6.

- [ ] **Step 6: Set the default in `config.rs`**

In `src-tauri/src/config.rs`, change the import on line 1 to include `Theme`:

```rust
use crate::types::{AppConfig, GenerationRequest, Theme};
```

In `default_config()`, add `theme` directly above `last_request:` so the struct literal reads:

```rust
        params_expanded: false,
        theme: Theme::Dark,
        last_request: GenerationRequest::default(),
```

- [ ] **Step 7: Run the type tests to verify they pass**

Run: `cd src-tauri && cargo test --lib theme_`
Expected: PASS (2 tests).

- [ ] **Step 8: Write failing config tests (legacy default + round-trip)**

In `src-tauri/src/config.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn old_config_without_theme_defaults_to_dark() {
        use crate::types::Theme;
        let dir = std::env::temp_dir().join(format!("muchai-cfg-theme-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no theme key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert_eq!(cfg.theme, Theme::Dark, "missing theme must default to Dark");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn theme_round_trips() {
        use crate::types::Theme;
        let dir = std::env::temp_dir().join(format!("muchai-cfg-theme2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.theme = Theme::Light;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert_eq!(back.theme, Theme::Light);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 9: Run the config tests to verify they pass**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS — all config + types tests green (the new ones plus the existing suite).

- [ ] **Step 10: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/config.rs
git commit -m "feat(light-mode): add Theme enum + AppConfig.theme field

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Mirror `theme` in the TypeScript `AppConfig`

**Files:**
- Modify: `src/lib/types.ts:60-69` (the `AppConfig` interface)

- [ ] **Step 1: Add the field**

In `src/lib/types.ts`, add `theme` to the `AppConfig` interface, directly below `params_expanded: boolean;`:

```ts
export interface AppConfig {
  sd_binary_path: string | null;
  default_model_path: string | null;
  gallery_dir: string;
  models_dir: string;
  extra_model_dirs: string[];
  last_request: GenerationRequest;
  gpu_device: GpuSelection | null;
  params_expanded: boolean;
  // Wire values MUST match the Rust `Theme` enum's serde snake_case form
  // (src-tauri/src/types.rs).
  theme: "dark" | "light";
}
```

- [ ] **Step 2: Verify svelte-check still passes**

Run: `npm run check`
Expected: 0 errors. (No consumer references `theme` yet, so adding the field is safe.)

- [ ] **Step 3: Commit**

```bash
git add src/lib/types.ts
git commit -m "feat(light-mode): mirror theme field in TS AppConfig

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Define semantic color tokens in `app.css`

**Files:**
- Modify: `src/app.css` (replace entire file)

This adds the token set and de-hardcodes the global `body`/`button`/`.btn-primary` rules. Existing `var(--border)` / `var(--accent)` usages across components keep resolving because both tokens remain defined.

- [ ] **Step 1: Replace the file contents**

Overwrite `src/app.css` with:

```css
:root {
  /* Core surfaces / text / lines — dark defaults (current look). */
  --bg: #1b1b1f;
  --surface: #2a2a30;
  --dialog-bg: #1e1e1e;
  --text: #e8e8ea;
  --text-muted: #9a9aa2;
  --border: #333;
  --border-subtle: rgba(255,255,255,.1);
  --accent: #4f7cff;
  --accent-bright: #6ea8fe;
  --accent-tint: rgba(110,168,254,.12);
  --on-accent: #fff;
  /* Status. */
  --danger: #ff6b6b;
  --danger-soft: #ffb4b4;
  --danger-bg: rgba(180,40,40,.85);
  --danger-tint: rgba(255,80,80,.15);
  --warn: #ffd9a8;
  --warn-tint: rgba(255,180,80,.15);
  --success: #4caf83;
  /* Overlays (sit on dark image pixels — unchanged in light). */
  --overlay: rgba(0,0,0,.6);
  --overlay-soft: rgba(0,0,0,.2);
  --backdrop: rgba(0,0,0,.5);
  color-scheme: dark;
}

:root[data-theme="light"] {
  /* Cool neutral gray. Only tokens that change are overridden. */
  --bg: #f4f4f6;
  --surface: #ffffff;
  --dialog-bg: #ffffff;
  --text: #1d1d20;
  --text-muted: #6b6b75;
  --border: #dcdce3;
  --border-subtle: rgba(0,0,0,.08);
  --accent: #3a63d6;
  --accent-bright: #2a4fb0;
  --accent-tint: rgba(58,99,214,.10);
  --danger: #c8362f;
  --danger-soft: #c8362f;
  --danger-bg: #c8362f;
  --danger-tint: rgba(200,54,47,.10);
  --warn: #9a5b00;
  --warn-tint: rgba(154,91,0,.10);
  --success: #2e8b5f;
  --backdrop: rgba(0,0,0,.4);
  color-scheme: light;
}

body { margin:0; background:var(--bg); color:var(--text); font-family: system-ui, sans-serif; }
button { cursor:pointer; border-radius:6px; border:1px solid var(--border); background:var(--surface); color:inherit; }
.btn-primary { background:var(--accent); border-color:var(--accent); color:var(--on-accent); }
.label { font-size:.75rem; opacity:.85; }
```

- [ ] **Step 2: Verify svelte-check passes**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/app.css
git commit -m "feat(light-mode): semantic color tokens + light overrides in app.css

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `applyTheme` helper

**Files:**
- Create: `src/lib/theme.ts`

- [ ] **Step 1: Create the file**

Write `src/lib/theme.ts`:

```ts
import type { AppConfig } from "$lib/types";

export type Theme = AppConfig["theme"]; // "dark" | "light"

/**
 * Apply a theme to the document and cache it for pre-paint on the next launch.
 * Only "light" sets the data-theme attribute; anything else (including unknown
 * or missing values) removes it, so the default and every failure path is dark.
 */
export function applyTheme(theme: Theme | string | null | undefined): void {
  const root = document.documentElement;
  if (theme === "light") {
    root.dataset.theme = "light";
  } else {
    delete root.dataset.theme;
  }
  try {
    localStorage.setItem("theme", theme === "light" ? "light" : "dark");
  } catch {
    // localStorage blocked/unavailable — pre-paint cache just won't update;
    // config still drives the theme on the next mount.
  }
}
```

- [ ] **Step 2: Verify svelte-check passes**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/lib/theme.ts
git commit -m "feat(light-mode): applyTheme helper (data-theme + localStorage cache)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: Pre-paint inline script in `app.html`

**Files:**
- Modify: `src/app.html` (add a script in `<head>` before `%sveltekit.head%`)

- [ ] **Step 1: Add the script**

In `src/app.html`, insert the inline script on the line directly above `%sveltekit.head%`:

```html
    <script>
      try { if (localStorage.getItem('theme') === 'light') document.documentElement.dataset.theme = 'light'; } catch (e) {}
    </script>
    %sveltekit.head%
```

- [ ] **Step 2: Verify svelte-check passes**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/app.html
git commit -m "feat(light-mode): pre-paint theme script to avoid startup flash

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: `ThemeToggle` header component

**Files:**
- Create: `src/lib/components/ThemeToggle.svelte`

Mirrors the optimistic-persist-then-revert pattern from `ParamsPanel.svelte`.

- [ ] **Step 1: Create the component**

Write `src/lib/components/ThemeToggle.svelte`:

```svelte
<script lang="ts">
  import { settings } from "$lib/stores";
  import { setSettings } from "$lib/api";
  import { applyTheme } from "$lib/theme";

  let busy = $state(false);

  // A not-yet-loaded config is treated as dark.
  const theme = $derived($settings?.theme ?? "dark");

  async function toggle() {
    if (!$settings || busy) return;
    const prev = $settings;
    const nextTheme = prev.theme === "light" ? "dark" : "light";
    const next = { ...prev, theme: nextTheme };
    applyTheme(nextTheme); // instant visual swap + cache
    settings.set(next);    // optimistic
    busy = true;
    try {
      await setSettings(next);
    } catch {
      settings.set(prev);     // revert the store…
      applyTheme(prev.theme); // …and the visual theme
    } finally {
      busy = false;
    }
  }
</script>

<button
  class="theme-toggle"
  onclick={toggle}
  disabled={busy}
  aria-label={theme === "light" ? "Switch to dark theme" : "Switch to light theme"}
  title={theme === "light" ? "Switch to dark theme" : "Switch to light theme"}>
  {theme === "light" ? "☽" : "☀"}
</button>

<style>
  .theme-toggle {
    background:none; border:none; color:inherit; cursor:pointer;
    font-size:1rem; line-height:1; padding:.2rem .4rem; border-radius:6px;
  }
  .theme-toggle:hover { background:var(--surface); }
  .theme-toggle:disabled { cursor:default; opacity:.6; }
</style>
```

- [ ] **Step 2: Verify svelte-check passes**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/ThemeToggle.svelte
git commit -m "feat(light-mode): ThemeToggle header button

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: Wire the toggle into the header + apply theme on load

**Files:**
- Modify: `src/routes/+page.svelte` (import, render in header, call `applyTheme` after `getSettings`, adjust header CSS)

- [ ] **Step 1: Add imports**

In `src/routes/+page.svelte`, after the existing component imports (after the `ResourceMonitor` import on line 15), add:

```ts
  import ThemeToggle from "$lib/components/ThemeToggle.svelte";
  import { applyTheme } from "$lib/theme";
```

- [ ] **Step 2: Apply the theme right after settings load**

In the `onMount` async block, change:

```ts
      const cfg = await getSettings();
      settings.set(cfg);
```

to:

```ts
      const cfg = await getSettings();
      settings.set(cfg);
      applyTheme(cfg.theme); // reconcile against the pre-paint localStorage cache
```

- [ ] **Step 3: Render the toggle in the header**

Change the brand markup from:

```svelte
  <aside class="controls">
    <h1 class="brand">MuchAI</h1>
```

to:

```svelte
  <aside class="controls">
    <header class="brandbar">
      <h1 class="brand">MuchAI</h1>
      <ThemeToggle />
    </header>
```

- [ ] **Step 4: Update the header CSS**

In the `<style>` block, replace the `.brand` rule:

```css
  .brand { margin:0 0 .5rem; font-size:1.2rem; }
```

with:

```css
  .brandbar { display:flex; align-items:center; justify-content:space-between; margin:0 0 .5rem; }
  .brand { margin:0; font-size:1.2rem; }
```

- [ ] **Step 5: Verify svelte-check passes**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 6: Commit**

```bash
git add src/routes/+page.svelte
git commit -m "feat(light-mode): render ThemeToggle + apply theme on load

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: Sweep hardcoded colors in components → tokens

**Files (modify the `<style>` blocks of each):**
- `src/lib/components/DevicePicker.svelte`
- `src/lib/components/DownloadDialog.svelte`
- `src/lib/components/GalleryLocation.svelte`
- `src/lib/components/GenerateBar.svelte`
- `src/lib/components/HistoryStrip.svelte`
- `src/lib/components/ImagePreview.svelte`
- `src/lib/components/ModelFolders.svelte`
- `src/lib/components/ModelLibrary.svelte`
- `src/lib/components/ParamsPanel.svelte`

Each literal below is unique within its file. Read each file, replace every listed literal with its token (`var(--…)`), preserving surrounding CSS (e.g. `color:#ff6b6b;` → `color:var(--danger);`). Do **not** touch `app.css` (its token definitions legitimately contain the raw literals).

- [ ] **Step 1: Apply the replacements per file**

```
DevicePicker.svelte
  #ff6b6b                 -> var(--danger)

DownloadDialog.svelte
  rgba(0,0,0,.5)          -> var(--backdrop)
  #1e1e1e                 -> var(--dialog-bg)
  rgba(110,168,254,.12)   -> var(--accent-tint)

GalleryLocation.svelte
  #ff6b6b                 -> var(--danger)

GenerateBar.svelte
  rgba(255,255,255,.1)    -> var(--border-subtle)
  rgba(255,80,80,.15)     -> var(--danger-tint)
  #ffb4b4                 -> var(--danger-soft)
  rgba(255,180,80,.15)    -> var(--warn-tint)
  #ffd9a8                 -> var(--warn)

HistoryStrip.svelte
  #fff                    -> var(--on-accent)
  rgba(0,0,0,.6)          -> var(--overlay)

ImagePreview.svelte
  rgba(0,0,0,.2)          -> var(--overlay-soft)
  #fff                    -> var(--on-accent)        (both occurrences)
  rgba(180,40,40,.85)     -> var(--danger-bg)
  rgba(0,0,0,.6)          -> var(--overlay)          (both occurrences)
  #ffb4b4                 -> var(--danger-soft)

ModelFolders.svelte
  #ff6b6b                 -> var(--danger)

ModelLibrary.svelte
  #fff                    -> var(--on-accent)
  rgba(180,40,40,.85)     -> var(--danger-bg)
  #ff6b6b                 -> var(--danger)            (both occurrences)
  rgba(255,255,255,.1)    -> var(--border-subtle)
  #6ea8fe                 -> var(--accent-bright)
  #4caf83                 -> var(--success)

ParamsPanel.svelte
  #ff6b6b                 -> var(--danger)
```

- [ ] **Step 2: Verify no raw color literals remain in components or routes**

Run:
```bash
grep -rnoE '#[0-9a-fA-F]{3,6}\b|rgba?\([0-9., ]+\)' src/lib/components src/routes | grep -vE '\{#(each|if|await|key|snippet)'
```
Expected: **no output** (empty). Any line printed is a literal that still needs converting. (`{#each}`-style false positives are filtered; `app.css` is not searched.)

- [ ] **Step 3: Verify svelte-check passes**

Run: `npm run check`
Expected: 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/
git commit -m "feat(light-mode): swap hardcoded component colors for theme tokens

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full Rust test suite**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS (all tests, including Task 1's theme tests).

- [ ] **Step 2: Full frontend check**

Run: `npm run check`
Expected: 0 errors, 0 warnings introduced by this work.

- [ ] **Step 3: Literal-sweep guard (final)**

Run:
```bash
grep -rnoE '#[0-9a-fA-F]{3,6}\b|rgba?\([0-9., ]+\)' src/lib/components src/routes | grep -vE '\{#(each|if|await|key|snippet)'
```
Expected: empty.

- [ ] **Step 4: Manual E2E (requires the dev box / GPU) — hand to the user**

Checklist for the user to confirm:
- Header sun ☀ in dark mode; clicking flips the entire UI to light instantly and the icon becomes a moon ☽.
- Choice persists across an app restart (relaunch stays light).
- A light-mode launch shows **no** dark flash before paint.
- Every status state is legible in light mode: the "Running on CPU" notice, error text and error notice background, the delete-confirm button, the download "✓ ready" line, the selection ring/highlight, and links.
- Dialogs (download), the history strip, params panel, device picker, and the resource monitor all read correctly in light mode.

---

## Self-Review

**Spec coverage:**
- Manual toggle, header placement, sun/moon, aria-label → Task 6, Task 7. ✓
- `AppConfig.theme` Rust enum (snake_case, default Dark) + serde default → Task 1. ✓
- TS mirror → Task 2. ✓
- Semantic tokens, dark `:root` / light override, `color-scheme` switch → Task 3. ✓
- `applyTheme` (guarded unknown→dark, localStorage try/catch) → Task 4. ✓
- Pre-paint inline script → Task 5. ✓
- Data flow (pre-paint → onMount reconcile; optimistic toggle + revert store & theme) → Task 5, Task 7, Task 6. ✓
- All 16 component literals de-hardcoded → Task 8 (matches the spec's per-component sweep list). ✓
- Rust tests (default dark, legacy→dark, dark/light round-trip, snake_case) → Task 1. ✓
- svelte-check + manual E2E → Task 9. ✓

**Placeholder scan:** No TBD/TODO; every code step shows complete content. ✓

**Type consistency:** `Theme` enum (Rust) ↔ `"dark" | "light"` (TS) ↔ `applyTheme` param ↔ `data-theme="light"` selector all agree. `setSettings(config: AppConfig)` / `settings` store (`AppConfig | null`) match the toggle's usage. Token names defined in Task 3 are exactly those referenced in Tasks 6 and 8. ✓

**Scope:** Single feature, one config field, additive. No decomposition needed. ✓
