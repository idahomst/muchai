# Collapsible Params Panel Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the parameters panel under the big image preview collapsible (collapsed by default, with a one-line summary when collapsed), persisting the choice in `config.json`, so the image area can grow.

**Architecture:** Add a `params_expanded: bool` field to `AppConfig` (`#[serde(default)]` → defaults to `false`/collapsed, backward-compatible with old config files). `ParamsPanel.svelte` renders a clickable header bar that toggles the flag, persisting via the existing `setSettings`/`settings.set` pattern (as in `DevicePicker.svelte`). Since `ImagePreview` is already `flex:1`, collapsing the panel grows the image automatically — no other layout change.

**Tech Stack:** Tauri v2 (Rust), SvelteKit + Svelte 5 runes, serde JSON config.

**Reference:** Design spec at `docs/superpowers/specs/2026-06-29-collapsible-params-design.md`.

---

## File structure

- `src-tauri/src/types.rs` — add `params_expanded` to `AppConfig`.
- `src-tauri/src/config.rs` — set `params_expanded: false` in `default_config()`; add backward-compat test.
- `src/lib/types.ts` — mirror `params_expanded` on the `AppConfig` interface.
- `src/lib/components/ParamsPanel.svelte` — collapsible header + summary + toggle/persist.

---

### Task 1: Add `params_expanded` to `AppConfig`

**Files:**
- Modify: `src-tauri/src/types.rs:154-169` (`AppConfig` struct)
- Modify: `src-tauri/src/config.rs:27-37` (`default_config`)
- Test: `src-tauri/src/config.rs` (tests module)
- Modify: `src/lib/types.ts:55-63` (`AppConfig` interface)

- [ ] **Step 1: Add the field to the Rust struct**

In `src-tauri/src/types.rs`, the `AppConfig` struct currently ends with `gpu_device` and `last_request`. Add `params_expanded` after `gpu_device` (keep `last_request` last). Replace:

```rust
    /// Chosen Vulkan device. `None` = engine default (auto-picks best device).
    #[serde(default)]
    pub gpu_device: Option<GpuSelection>,
    pub last_request: GenerationRequest,
}
```

with:

```rust
    /// Chosen Vulkan device. `None` = engine default (auto-picks best device).
    #[serde(default)]
    pub gpu_device: Option<GpuSelection>,
    /// Whether the params panel under the preview is expanded. Defaults to
    /// `false` (collapsed) for new and pre-feature config files.
    #[serde(default)]
    pub params_expanded: bool,
    pub last_request: GenerationRequest,
}
```

- [ ] **Step 2: Set the field in `default_config`**

In `src-tauri/src/config.rs`, the `default_config()` struct literal (lines 27-37) must set the new field or it won't compile. Replace:

```rust
        extra_model_dirs: Vec::new(),
        gpu_device: None,
        last_request: GenerationRequest::default(),
    }
```

with:

```rust
        extra_model_dirs: Vec::new(),
        gpu_device: None,
        params_expanded: false,
        last_request: GenerationRequest::default(),
    }
```

- [ ] **Step 3: Write the failing backward-compat test**

In `src-tauri/src/config.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn old_config_without_params_expanded_defaults_to_collapsed() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-pe-{}", std::process::id()));
        let path = dir.join("config.json");
        std::fs::create_dir_all(&dir).unwrap();
        // A pre-feature config file: no params_expanded key.
        std::fs::write(
            &path,
            r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","last_request":{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#,
        )
        .unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.params_expanded, "missing params_expanded must default to false (collapsed)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn params_expanded_round_trips() {
        let dir = std::env::temp_dir().join(format!("fridai-cfg-pe2-{}", std::process::id()));
        let path = dir.join("config.json");
        let mut cfg = default_config();
        cfg.params_expanded = true;
        save_config_to(&path, &cfg).unwrap();
        let back = load_config_from(&path);
        assert!(back.params_expanded);
        assert_eq!(back, cfg);
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 4: Run the tests**

Run: `cd /home/idaho/g/mst/fridai/src-tauri && cargo test --lib config`
Expected: all config tests PASS, including the two new ones. (The existing `save_then_load_round_trips` test still passes because both sides now carry `params_expanded: false` from `default_config`.)

- [ ] **Step 5: Mirror the field in TypeScript**

In `src/lib/types.ts`, the `AppConfig` interface (lines 55-63) ends with `gpu_device` and `last_request`. Replace:

```ts
  gpu_device: GpuSelection | null;
}
```

with:

```ts
  gpu_device: GpuSelection | null;
  params_expanded: boolean;
}
```

- [ ] **Step 6: Type-check the frontend**

Run: `cd /home/idaho/g/mst/fridai && npm run check`
Expected: 0 errors, 0 warnings. (No frontend code constructs an `AppConfig` literal — it's only read from / spread — so the new required field breaks no call sites.)

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/types.rs src-tauri/src/config.rs src/lib/types.ts
git commit -m "feat(collapsible-params): add params_expanded to AppConfig

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Make `ParamsPanel` collapsible

**Files:**
- Modify: `src/lib/components/ParamsPanel.svelte` (whole file)

This project has no frontend unit-test harness; the gate for Svelte components is `npm run check` plus manual E2E (consistent with prior features). Follow that here.

- [ ] **Step 1: Replace the component**

Replace the entire contents of `src/lib/components/ParamsPanel.svelte` with:

```svelte
<script lang="ts">
  import { currentItem, settings } from "../stores";
  import { setSettings } from "../api";
  import { SAMPLERS } from "../types";

  let busy = $state(false);
  let error = $state<string | null>(null);

  const basename = (p: string) => p.split(/[\\/]/).pop() || p;
  const samplerLabel = (v: string) =>
    SAMPLERS.find((s) => s.value === v)?.label ?? v;

  // Collapsed by default; a not-yet-loaded config is treated as collapsed.
  const expanded = $derived($settings?.params_expanded ?? false);

  async function toggle() {
    if (!$settings || busy) return;
    const prev = $settings;
    const next = { ...prev, params_expanded: !prev.params_expanded };
    settings.set(next); // optimistic — UI + image size respond immediately
    busy = true;
    error = null;
    try {
      await setSettings(next);
    } catch (e) {
      settings.set(prev); // revert to what's actually persisted
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if $currentItem}
  {@const r = $currentItem.request}
  <div class="params">
    <button class="hdr" onclick={toggle} disabled={busy}
            aria-expanded={expanded}
            title={expanded ? "Collapse parameters" : "Expand parameters"}>
      <span class="chev">{expanded ? "▾" : "▸"}</span>
      <span class="title">Parameters</span>
      {#if !expanded}
        <span class="summary">Seed {r.seed} · {r.steps} steps · CFG {r.cfg_scale} · {r.width}×{r.height}</span>
      {/if}
    </button>
    {#if expanded}
      <div class="grid">
        <span class="k">Model</span><span class="v" title={r.model_path}>{basename(r.model_path)}</span>
        <span class="k">Seed</span><span class="v">{r.seed}</span>
        <span class="k">Steps</span><span class="v">{r.steps}</span>
        <span class="k">CFG</span><span class="v">{r.cfg_scale}</span>
        <span class="k">Sampler</span><span class="v">{samplerLabel(r.sampler)}</span>
        <span class="k">Size</span><span class="v">{r.width}×{r.height}</span>
      </div>
      <div class="prompts">
        <div class="pl">Prompt</div>
        <div class="pt">{r.prompt}</div>
        {#if r.negative_prompt}
          <div class="pl">Negative</div>
          <div class="pt neg">{r.negative_prompt}</div>
        {/if}
      </div>
    {/if}
    {#if error}<span class="err">{error}</span>{/if}
  </div>
{/if}

<style>
  .params { font-size:.78rem; border:1px solid var(--border); border-radius:8px;
    padding:.5rem .7rem; display:flex; flex-direction:column; gap:.5rem; flex:0 0 auto; }
  .hdr { display:flex; align-items:center; gap:.45rem; width:100%; padding:0;
    background:none; border:none; color:inherit; font:inherit; font-size:.78rem;
    text-align:left; cursor:pointer; }
  .hdr:disabled { cursor:default; }
  .chev { opacity:.6; width:.8rem; flex:0 0 auto; }
  .title { opacity:.7; font-weight:600; flex:0 0 auto; }
  .summary { opacity:.55; font-variant-numeric:tabular-nums; overflow:hidden;
    text-overflow:ellipsis; white-space:nowrap; min-width:0; }
  .grid { display:grid; grid-template-columns:auto 1fr auto 1fr auto 1fr; gap:.2rem .5rem;
    align-items:baseline; }
  .k { opacity:.55; }
  .v { font-variant-numeric:tabular-nums; overflow:hidden; text-overflow:ellipsis; white-space:nowrap; }
  .prompts { display:flex; flex-direction:column; gap:.15rem; }
  .pl { opacity:.55; }
  .pt { white-space:pre-wrap; word-break:break-word; }
  .pt.neg { opacity:.8; }
  .err { color:#ff6b6b; }
</style>
```

- [ ] **Step 2: Type-check**

Run: `cd /home/idaho/g/mst/fridai && npm run check`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/ParamsPanel.svelte
git commit -m "feat(collapsible-params): collapsible ParamsPanel with summary + persisted toggle

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Verification + finish branch

**Files:** none (verification only)

- [ ] **Step 1: Backend tests**

Run: `cd /home/idaho/g/mst/fridai/src-tauri && cargo test --lib 2>&1 | tail -3`
Expected: all tests pass (52 prior + the 2 new config tests = 54).

- [ ] **Step 2: Frontend check**

Run: `cd /home/idaho/g/mst/fridai && npm run check 2>&1 | tail -3`
Expected: 0 errors, 0 warnings.

- [ ] **Step 3: Manual E2E (dev box, `npm run tauri dev`)**

Verify:
- With an image selected, the params panel starts **collapsed**, showing the header bar with the summary `Seed … · … steps · CFG … · …×…`.
- Clicking the header **expands** to the full grid + prompt/negative blocks; clicking again collapses it. The chevron flips (▸ ↔ ▾).
- Collapsing visibly **enlarges** the image preview.
- The collapsed summary values match the expanded grid (seed/steps/cfg/size).
- Restart the app: the last collapse/expand state is **restored** from `config.json`.
- A fresh config (or a pre-feature one without the field) starts collapsed.

- [ ] **Step 4: Update roadmap memory**

In `/home/idaho/.claude/projects/-home-idaho-g-mst-fridai/memory/fridai-roadmap.md`, mark roadmap item 1 (collapsible params panel) DONE.

- [ ] **Step 5: Finish the branch**

Use superpowers:finishing-a-development-branch on `feat/collapsible-params`.
