# Preferences & Secrets — Design

**Date:** 2026-07-16
**Status:** Approved (design), pending implementation plan
**Sub-project A of** the multi-file model UX rework. Sub-project B (the multi-file
model download/detection rework) is designed separately and depends on the tokens
stored here.

## Problem

MuchAI has no place to store credentials or consolidate app settings:

- HuggingFace (and, later, Civitai) tokens are typed into the download UI **every
  time**. HF tokens cannot be re-viewed after creation, so re-prompting is
  especially painful — a user who didn't save their token elsewhere is stuck.
- App configuration (model folders, gallery location, GPU device, theme) is
  scattered as inline controls in the left sidebar, cluttering the main workspace
  with settings a user touches rarely.

## Goal

Add a single **Preferences** dialog that stores HuggingFace and Civitai tokens and
consolidates existing app settings, so tokens are entered once and reused
automatically for downloads.

## Decisions (locked)

- **Consolidated dialog** (not tokens-only): fold the currently-scattered
  folder / GPU / theme controls into the Preferences dialog. The user expects to
  keep adding settings over time, so start with a real home now.
- **Plaintext storage in `config.json`**: no OS keyring, no separate secrets file.
  This is a local single-user desktop tool; keyring adds a dependency and is
  unreliable in this machine's sandboxed environment. Tokens are stored as normal
  config fields.
- **Read-only token guidance**: the Secrets section advises creating tokens with
  read-only permissions (all MuchAI needs).

## Architecture

### Data model

`AppConfig` (Rust `src-tauri/src/types.rs`) gains two fields:

```rust
#[serde(default)]
pub hf_token: Option<String>,
#[serde(default)]
pub civitai_token: Option<String>,
```

- `#[serde(default)]` guarantees existing `config.json` files (which lack these
  keys) deserialize with `None` — no migration needed.
- Empty input in the UI is stored as `None`, never `Some("")`, so "is a token set?"
  is a simple `Option::is_some()` check.

The TypeScript `AppConfig` interface (`src/lib/types.ts`) mirrors them:

```ts
hf_token: string | null;
civitai_token: string | null;
```

No new Tauri commands: the existing `get_settings` / `set_settings` already
round-trip the whole `AppConfig`, so the new fields ride along for free.

### Preferences dialog

New component `src/lib/components/PreferencesDialog.svelte`, opened from a ⚙ gear
button added to the header in `src/routes/+page.svelte` (next to the `?` help and
theme toggle). Modal dialog following the existing `ModelAssembly.svelte` /
`WelcomeDialog.svelte` backdrop + dialog pattern.

Sections, top to bottom:

1. **Secrets**
   - HuggingFace token: password `<input>` + show/hide toggle. Helper text: read-only
     recommendation + a "where do I get one?" hint (points at HF token settings).
   - Civitai token: password `<input>` + show/hide toggle. Helper text noting it's
     used for Civitai downloads (Sub-project B).
2. **Folders** — the existing `ModelFolders` (models dir + extra dirs) and
   `GalleryLocation` (gallery dir) controls, relocated here.
3. **Hardware** — the existing `DevicePicker` (GPU device), relocated here.
4. **Appearance** — theme (Dark / Light).

A **Done** button closes the dialog.

### Sidebar / header changes

- `src/routes/+page.svelte`: remove `<ModelFolders />`, `<DevicePicker />`,
  `<GalleryLocation />` from their inline positions; render them inside
  `PreferencesDialog` instead. The left sidebar keeps Model, Prompt, generation
  params, and Generate.
- The header **theme toggle stays** as a one-click shortcut. Both it and the
  Preferences → Appearance control write the same `theme` setting, so they stay in
  sync via the shared `settings` store. (This is intentional duplication for
  convenience, not a bug.)
- The relocated components are reused as-is where possible; only their placement
  changes. If a component's styling assumes sidebar width, adjust within the
  dialog's width.

### Save behavior

Changes autosave using the existing optimistic pattern already used for settings
(set the `settings` store, call `setSettings`, roll back on failure). Tokens save
on change/blur. There is no separate "Save" button beyond **Done** (close).

### Token usage (stops the re-prompting)

The download flows read the stored HF token instead of prompting:

- Single-file download (`DownloadDialog.svelte` → `startDownload` /
  `downloadModel`): read `hf_token` from the `settings` store and pass it; remove
  the per-download token input.
- Multi-file catalog download (`ModelAssembly.svelte` → `startMultiFileDownload` /
  `downloadMultifile`): same — read `hf_token` from settings, remove the inline
  `token` field.

The Civitai token is only **stored** in this sub-project; it is consumed in
Sub-project B when the user points at a Civitai URL.

## Error handling

- Missing token: downloads proceed with an empty/absent token exactly as today
  (public models still work; gated ones fail the same way they do now). No new
  blocking behavior.
- `setSettings` write failure: roll back the `settings` store to its previous
  value (existing pattern in `+page.svelte`'s `dismissWelcome`), so the UI never
  shows a persisted state that didn't persist.

## Testing

**Rust (`src-tauri`, `cargo test --lib`):**
- `AppConfig` round-trip: serialize a config with both tokens set, deserialize,
  assert equality.
- Backward-compat: deserialize a JSON object with **no** `hf_token` /
  `civitai_token` keys; assert both are `None`.
- Empty-string normalization is a UI concern (stored as `None`), so it's covered by
  the frontend, not a Rust test.

**Frontend:**
- `npm run check` stays 0 errors / 0 warnings.

**Manual acceptance:**
- Open ⚙, enter an HF token, close. Reopen — token persists (masked).
- Trigger a model download — it is **not** re-prompted for a token.
- Confirm model folders / GPU / theme still work from their new location and the
  sidebar no longer shows them.
- Load an old `config.json` (no token keys) — app starts, tokens show empty.

## Out of scope (this sub-project)

- Consuming the Civitai token (Sub-project B).
- The multi-file "point at a model → auto-detect & download companions" flow,
  FLUX.2 recipe, catalog rework, moving download out of the model selector — all
  Sub-project B.
- Encryption / keyring storage.
