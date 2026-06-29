# Collapsible params panel — Design Spec

**Date:** 2026-06-29
**Status:** Approved (brainstorming complete)
**Branch:** `feat/collapsible-params`

## Goal

Let the user collapse the parameters panel that sits under the big image preview so
the image area can grow. The panel is **collapsed by default**; when collapsed it
shows a one-line summary; the collapsed/expanded choice **persists** across app
restarts.

## Background / current state

- The stage column (`src/routes/+page.svelte`) is a flexbox column:
  `ImagePreview` (`flex:1`, grows), then `ParamsPanel`, `HistoryStrip`,
  `GalleryLocation` at their natural height. Because `ImagePreview` already has
  `flex:1`, shrinking `ParamsPanel` automatically enlarges the image — **no
  `+page.svelte` layout change is required.**
- `ParamsPanel.svelte` renders nothing unless `$currentItem` is set; when it is, it
  shows a grid (Model/Seed/Steps/CFG/Sampler/Size) plus a prompt/negative block,
  all read from `$currentItem.request`.
- Config is persisted in `~/.config/fridai/config.json` as `AppConfig`
  (`src-tauri/src/types.rs`, mirrored in `src/lib/types.ts`). Newer fields use
  `#[serde(default)]` for backward compatibility (e.g. `gpu_device`,
  `extra_model_dirs`).
- The established pattern for persisting a settings change from the frontend
  (`DevicePicker.svelte`): build `next = { ...$settings, <field> }`,
  `await setSettings(next)`, then `settings.set(next)`. `set_settings`
  (`commands.rs`) saves the whole config to disk and updates the in-memory
  `state.config`.

## Architecture

### Data model

Add one field to `AppConfig`:

- Rust (`src-tauri/src/types.rs`): `#[serde(default)] pub params_expanded: bool`.
  `#[serde(default)]` makes it default to `false` (collapsed) when an existing
  `config.json` lacks the key — which is exactly the desired default.
- The default-config constructor (`src-tauri/src/config.rs`, the struct literal
  around line 35) must set `params_expanded: false` so a brand-new config is also
  collapsed.
- TypeScript (`src/lib/types.ts`): add `params_expanded: boolean;` to the
  `AppConfig` interface (snake_case, matching the serde wire form).

`false` = collapsed, `true` = expanded.

### Component (`ParamsPanel.svelte`)

- Keep the existing guard: render nothing when `$currentItem` is null.
- When `$currentItem` is set, always render a thin, clickable **header bar**:
  - A chevron indicator: `▸` when collapsed, `▾` when expanded.
  - The label "Parameters".
  - When **collapsed**, the header also shows a compact summary built from the
    same `$currentItem.request` the expanded view uses:
    `Seed {seed} · {steps} steps · CFG {cfg_scale} · {width}×{height}`.
- **Collapsed** (default): only the header bar (with summary) is shown.
- **Expanded**: header bar, then the existing full grid and prompt/negative blocks,
  visually unchanged from today.
- The displayed state derives from `$settings?.params_expanded`. If `$settings` is
  not yet loaded (null), treat as collapsed (the default).

### Behavior / data flow

On header click, toggle and persist (the `DevicePicker` pattern):

1. Guard: do nothing if `$settings` is null or a save is already in flight.
2. Compute `next = { ...$settings, params_expanded: !$settings.params_expanded }`.
3. Optimistically `settings.set(next)` so the UI responds immediately.
4. `await setSettings(next)`.
5. On error: revert by setting `settings` back to the previous value and surface a
   small inline error message (same shape as `DevicePicker`'s `.err`).

Collapsing shrinks the panel to the header line; `ImagePreview`'s `flex:1`
reclaims the freed vertical space, so the image grows with no other layout change.

### Data flow diagram

```
load (onMount)
  └─ getSettings() → settings store (includes params_expanded, default false)
       └─ ParamsPanel derives collapsed/expanded from $settings.params_expanded

click header
  └─ next = { ...$settings, params_expanded: !current }
       └─ settings.set(next)            (optimistic, UI updates, image grows/shrinks)
       └─ await setSettings(next)       (persists to config.json + state.config)
            └─ on error: settings.set(previous) + inline error
```

## Error handling

- **`setSettings` fails:** revert the optimistic `settings` value to the previous
  state and show a one-line inline error; on-screen state then matches what is
  actually persisted.
- **`$settings` not loaded:** panel treats `params_expanded` as `false` (collapsed);
  the header click guard prevents toggling until settings exist.
- **Legacy `config.json` without `params_expanded`:** `#[serde(default)]` yields
  `false` (collapsed). No migration needed.
- **Summary vs. grid divergence:** impossible — both read the same
  `$currentItem.request`.

## Wire contract (TS mirrors Rust serde)

- `AppConfig` gains `params_expanded: boolean;` (snake_case key `params_expanded`).
- No new commands; persistence reuses the existing `set_settings` /
  `setSettings`.

## Testing

- **Rust (`types.rs` or `config.rs` tests):**
  - `AppConfig` round-trips through JSON with `params_expanded` preserved.
  - A `config.json` string that omits `params_expanded` deserializes with
    `params_expanded == false` (the `#[serde(default)]` legacy path).
  - Existing config load/save tests still pass.
- **Frontend:** `svelte-check` clean.
- **Manual E2E (dev box):**
  - With an image selected, the panel starts collapsed showing the summary line;
    clicking the header expands to the full grid + prompts and back.
  - Collapsing visibly enlarges the image preview.
  - The summary line matches the values in the expanded grid.
  - Toggle state survives an app restart (persisted in `config.json`).
  - A fresh config (or an old one without the field) starts collapsed.

## Out of scope (deferred)

- Expand/collapse animations or transitions.
- Collapsing any other panel, or a global "collapse all" control.
- Fixing the pre-existing `last_request` clobber where saving the whole config from
  the startup-stale frontend store can overwrite a `last_request` that `generate`
  wrote server-side (affects all settings equally; tracked separately, not part of
  this feature).
