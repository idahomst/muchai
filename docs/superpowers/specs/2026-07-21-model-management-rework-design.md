# Model Management Rework — Design

**Date:** 2026-07-21
**Status:** Approved (brainstorm), pending implementation plan

## Goal

Replace the current three-shape, config-driven model storage with **uniform per-model
folders backed by an on-disk `model.json` manifest**, split the overloaded model UI into a
**selection-only surface** plus a **source-first "New…" dialog**, move the curated catalog to a
**bundled JSON file** seeded from the Draw Things community list, and **formalize GGUF** support.

## Background — current state (grounded in code)

Storage today has three inconsistent shapes:

- **Single-file** downloads (starters / paste-URL) land *flat* in `models_dir` root and get **no
  `ModelDefinition`** — they are only loose files discovered by a recursive scan
  (`models.rs` `scan_models_excluding`, extensions `safetensors|ckpt|gguf`).
- **Multi-file** models use `models_dir/<id>/` for the diffusion file plus a cross-model shared
  encoder pool `models_dir/shared/<family>/` (`catalog.rs:149-150,203`). A `ModelDefinition`
  (`id/name/family/components`) is written to `config.json`.
- **Point-at-folder** leaves files in place and writes a `ModelDefinition` referencing them.

All metadata lives only in central `config.json` (`AppConfig.model_definitions`). `ModelDefinition`
(`types.rs:141-147`) is minimal: no source URL, no per-model recommended-settings override, no
on-disk manifest. The catalog is hardcoded Rust consts (`catalog.rs`): 2 single-file starters +
1 multi-file FLUX entry, all full-precision safetensors, no GGUF/quantized variants.

The UI overloads `ModelLibrary.svelte`: one `<select>` does selection + a `__new_multifile__`
sentinel + a Download button + Edit + Delete + badges + inline progress. Adding is split across
two dialogs — `DownloadDialog.svelte` (single-file: starters + paste-URL) and
`ModelAssembly.svelte` (multi-file: folder auto-detect / manual / catalog / HF). These three
components (runes-based) duplicate the token hint, size formatter, VRAM sourcing, three parallel
rating types, and three near-identical download helpers in `stores.ts`.

A clean migration template exists at `config.rs:77-160` (the fridai→muchai rebrand) — **not used
here** (see Clean Slate), but noted as the established pattern.

## Key decisions

| Area | Decision |
|------|----------|
| Storage layout | Uniform: every model is `models_dir/<id>/` + `model.json` |
| Source of truth | On-disk manifests (scan `models_dir/*/model.json`); drop `model_definitions` from `config.json` |
| Loose-file scanning | Removed — **manifest-only**; a model appears only if it has a `model.json` |
| Local files | **Hybrid**: downloads live in the folder; "add from disk" models referenced in place (no copy) |
| Shared encoders | Stay pooled in `models_dir/shared/<family>/`, referenced by absolute path from manifests |
| Migration | **None** — clean slate; existing models wiped and re-downloaded |
| Select UI | Inline sidebar list; `New / Edit / Delete` equal-width buttons; no add logic |
| New UI | Source-first dialog: Catalog / URL / From disk; single-vs-multi auto-detected |
| Catalog | Bundled `catalog.json`; author-seeded from Draw Things; ~6–10 entries, 12GB-focused, GGUF-forward |
| GGUF | Formalized for diffusion *and* components in catalog + recipes/detection |
| Deferred | Font-size polish (#4), param-panel reorg (#5), manifest extras (thumbnails/tags/notes/trigger words) |

## 1. On-disk storage & the `model.json` manifest

### Layout

```
models/
├── <id>/                         # one folder per model; <id> is stable
│   ├── model.json                # the manifest (source of truth)
│   └── <weight files…>           # present for downloaded models; absent for referenced-local
├── <id2>/
│   └── …
└── shared/
    └── <family>/                 # pooled encoders/VAE shared across models of a family
        ├── t5xxl_fp16.safetensors
        ├── clip_l.safetensors
        └── ae.safetensors
```

- `<id>` is a stable, generated identifier used as the folder name (e.g. `flux1-schnell-<uuid8>`).
- The shared pool is unchanged from today (`catalog.rs`): family encoders live once under
  `shared/<family>/` and manifests reference them by **absolute** path.

### Manifest schema (`model.json`)

```jsonc
{
  "schema_version": 1,             // integer; bump on incompatible manifest changes
  "id": "flux1-schnell-def456",    // matches folder name
  "name": "FLUX.1 schnell (Q4)",   // user-editable display label
  "family": "flux1",               // recipe key: flux1 | flux2 | sd3 | qwen-image | sdxl | sd15 | custom
  "source": {
    "kind": "catalog",             // "catalog" | "url" | "local"
    "catalog_id": "flux1-schnell", // present when kind == "catalog"
    "url": "https://…",            // present when kind == "catalog" | "url"
    "original_path": "/home/…"     // present when kind == "local" (the referenced file)
  },
  "components": {                  // role → path; RELATIVE to the model folder when in-folder, ABSOLUTE when pooled/referenced
    "diffusion_model": "flux1-schnell-Q4.gguf",
    "t5xxl": "/…/models/shared/flux1/t5xxl_fp16.safetensors",
    "clip_l": "/…/models/shared/flux1/clip_l.safetensors",
    "clip_g": null,
    "vae":    "/…/models/shared/flux1/ae.safetensors",
    "llm":    null
  },
  "flags": {                       // engine flags, NOT files
    "vae_format": null,
    "prediction": null
  },
  "recommended_settings": null     // null = derive from family; else { steps, cfg_scale, sampler, width, height }
}
```

**Path convention:** a component path is stored **relative to the model folder** when the file
lives inside it (downloaded weights), and **absolute** when it points at the shared pool or a
referenced-in-place local file. Resolution: relative → resolve against `models_dir/<id>/`;
absolute → use as-is. This keeps downloaded folders portable while allowing pooled/external files.

`recommended_settings`, when non-null, mirrors the existing `GenDefaults` shape
(`types.rs:210-217`): `{ steps: u32, cfg_scale: f32, sampler: Sampler, width: u32, height: u32 }`.

### Source of truth & config changes

- The library is built by scanning `models_dir/*/model.json` at startup (and on refresh). Each
  valid manifest becomes a library entry. Invalid/missing-component manifests surface as **broken**
  (see §3).
- `AppConfig.model_definitions` is **removed**. The `Vec<ModelDefinition>` field and its
  serialization go away; any value present in an old config is ignored (clean slate, see §2).
- **Selection** still persists via `last_request.model: ModelRef` in `config.json`, resolved from
  the chosen manifest at selection time. No separate "selected id" field is added.
- **Manifest → `ModelRef` rule:** a manifest whose only set component is `diffusion_model` (no
  `vae`/`clip_l`/`clip_g`/`t5xxl`/`llm` and no `flags`) resolves to `SingleFile { path }` (engine
  `-m`); any manifest with one or more companions or flags resolves to `MultiFile(components)`
  (engine `--diffusion-model` + friends). This preserves today's single-checkpoint (SD1.5/SDXL)
  vs. component-model behavior while letting both live in the uniform folder layout.
- Editing a model **rewrites its `model.json`**. There is no config-side copy to keep in sync,
  eliminating the clobber/drift bug class previously seen with `set_settings`.

### File handling (hybrid)

- **Downloaded** models (catalog / URL): weight files are written into `models_dir/<id>/`; the
  manifest stores relative paths for in-folder files and absolute paths for shared-pool files.
- **Local "add from disk"** models: files are **referenced in place** — the manifest's
  `components` store the files' absolute paths and `source.original_path` records the primary file.
  No large files are copied. The `models_dir/<id>/` folder then holds only `model.json`.

## 2. Migration — none (clean slate)

There is **no migration code**. On the first run of the reworked build:

- The app does **not** auto-delete anything. Because scanning is manifest-only, old-layout content
  (loose single-file weights, and old `<id>/` folders that have no `model.json`) simply **doesn't
  appear** in the library — it becomes invisible clutter the user can trash manually. We accept
  re-downloading through the new path to exercise it end-to-end.
- Any `model_definitions` in an existing `config.json` are ignored (the field is removed from the
  struct; serde drops unknown keys on load).
- Because the library is now built from on-disk manifests, an emptied models folder simply yields
  an empty library and the user re-adds models through the New… dialog.

The `config.rs:77-160` rebrand migration remains the reference pattern for any *future* manifest
`schema_version` migration, but is not invoked by this rework.

## 3. UI split

Both surfaces are Svelte 5 **runes** components (consistent with the current
`ModelLibrary`/`DownloadDialog`/`ModelAssembly`).

### Select surface (inline sidebar list)

- Renders the library as an inline list in the left column: each row shows the model **name** and a
  **family** label, with a **broken** (`⚠`) badge when the manifest's components are missing on disk.
- Selecting a row sets `request.model` to the resolved `ModelRef` (single-file → `{type:"single_file",path}`;
  multi-file → `{type:"multi_file", ...components}`), exactly as today's `selectDefinition`.
- Below the list: **three equal-width buttons** — `New`, `Edit`, `Delete`. `New` is **not**
  stretched to fill the row; all three share the same width.
  - `New` opens the source-first New… dialog (§3, New surface).
  - `Edit` opens the model editor for the selected model (writes `model.json`).
  - `Delete` trashes the selected model (see Delete semantics).
- All *adding* logic is removed from this surface (no `__new_multifile__` sentinel option, no
  Download button here).

### New surface (source-first dialog)

Top level is a **menu of three sources**; single-vs-multi-file is auto-detected, never chosen:

1. **📚 Browse catalog** — lists curated `catalog.json` entries (name, family, quant, size, VRAM
   suitability, license). Selecting one downloads its files into `models_dir/<id>/` (shared
   components into the pool, skipped if already present) and writes the manifest.
2. **🔗 From a URL** — one field accepting a direct or HuggingFace URL. Unifies today's paste-URL
   (`download_model`) and HF variant picker (`list_hf_variants`):
   - A lone weight file (`.safetensors`/`.gguf`/`.ckpt`) → **single-file** model; download → manifest.
   - A repo/variant that needs companions (e.g. FLUX) → download the diffusion file, then resolve
     companions from the family recipe's shared list (download to the pool if absent) → manifest.
     If the family is ambiguous, fall through to manual role assignment.
3. **📂 From files on disk** — folder auto-detect (`detect_folder` recipe matching) or manual role
   assignment (`pick_model_file` per role). Files are **referenced in place**; a manifest is written
   with absolute component paths.

Each source ends by writing a `models_dir/<id>/model.json` and returning the new library entry,
which the Select surface upserts and selects.

### Editor

- Reachable from `Edit`. Fields: `name`, `family`, per-role component paths, `vae_format`/`prediction`
  flags, and an optional `recommended_settings` override (leave empty → derive from family).
- Save rewrites `model.json` in place (no config write). Validation mirrors today's
  `validate_model_definition`: non-blank name + all required roles for the family filled.

### Delete semantics

- Moves `models_dir/<id>/` to OS trash (manifest + any in-folder weights).
- **Leaves the shared encoder pool intact** (other models reference it) — no reference-counting /
  orphan pruning in v1.
- **Never touches referenced-in-place external files** (`kind: "local"`), since they are not ours.

### De-duplication

Fold the duplicated concerns surfaced during exploration into shared helpers: one size formatter,
one VRAM-source accessor, one token-hint component, one rating/badge model, and one download helper
(collapse `startDownload`/`startFileDownload`/`startMultiFileDownload` into a single parameterized
flow). The `definitions` upsert logic (duplicated in `stores.ts` and `ModelLibrary`) becomes one
function over the manifest-derived library.

## 4. Catalog & GGUF

### Bundled `catalog.json`

- Replace the hardcoded Rust consts (`starter_catalog()`, `multi_file_catalog()`) with a single
  **bundled JSON resource** loaded at startup. One unified entry shape covers single- and
  multi-file:

```jsonc
{
  "schema_version": 1,
  "entries": [
    {
      "id": "flux1-schnell-q4",
      "name": "FLUX.1 schnell (Q4_K_M GGUF)",
      "family": "flux1",
      "license": "Apache-2.0",            // underlying weight license, surfaced in UI
      "source_url": "https://huggingface.co/…/flux1-schnell-Q4_K_M.gguf",
      "diffusion": { "url": "…", "filename": "flux1-schnell-Q4_K_M.gguf", "size_bytes": 0 },
      "shared": [ /* optional per-entry component overrides; else family recipe shared list */ ],
      "min_vram_mb": 8192,
      "recommended_vram_mb": 12288
    }
    // …
  ]
}
```

- Entries carry `license` (surfaced per entry in the catalog UI) and `source_url` for provenance.
- Loading validates the JSON against the schema; a malformed catalog degrades to an empty catalog
  with a logged warning rather than crashing.
- Rating (`Suitability` / VRAM fit) is computed from `min_vram_mb`/`recommended_vram_mb` as today.

### Author-time seeding from Draw Things

- Populate `catalog.json` by mining `github.com/drawthingsai/community-models`: each entry there is
  CC0/public-domain metadata + LICENSE pointing at a **public HF/Civitai** source. Extract the
  source URL and license; **verify a few actually load in sd.cpp** before inclusion.
- This is an **authoring activity**, not a runtime feature — there is no live dependency on Draw
  Things servers or format. Underlying weights keep their own (non-CC0) licenses, recorded in each
  entry's `license`.

### v1 scope

- ~6–10 verified entries across supported families (`sd15`, `sdxl`, `flux1` schnell/dev, `sd3`,
  `qwen-image`, `flux2`), prioritizing **quantized GGUF variants that run on 12GB** (matching the
  RTX 3060 12GB target hardware).

### GGUF formalization

- Catalog entries and family recipes accept `.gguf` for **diffusion and components**, not just
  diffusion. `recipes` detection patterns match gguf filenames; `command_builder` already passes
  component paths verbatim regardless of extension, and `models.rs` already accepts `gguf`.
- `pick_model_file` / folder detection already include `gguf`; formalize by ensuring recipe pattern
  tables and catalog validation treat gguf as first-class.

## 5. Recommended settings resolution

The `recommended_settings` command changes to resolve in this order:

1. If the selected model's manifest has a non-null `recommended_settings`, return it.
2. Otherwise derive from the model's `family` via `recipes::family_defaults` (single-file family
   inferred as today: name contains "xl" → `sdxl`, else `sd15`).
3. If neither yields a result, return `null` (button hidden), as today.

## 6. Testing strategy

Follow the existing gates: `cargo test` from `src-tauri/`, `npm run check` (svelte-check). Cover:

- **Manifest round-trip:** serialize/deserialize `model.json`; relative vs absolute path resolution;
  `recommended_settings` null vs override; unknown extra keys ignored.
- **Library scan:** a folder of `*/model.json` builds the expected library; missing-component
  manifest → broken; folder without `model.json` → ignored (manifest-only).
- **Config:** `model_definitions` removed; old config with the field still loads; `last_request.model`
  persists selection.
- **Catalog:** `catalog.json` parses; malformed catalog → empty + warning; VRAM rating; gguf entry
  accepted; per-entry license present.
- **New flows:** catalog download writes folder + manifest + pools shared; URL single vs multi
  branch; from-disk references in place (no copy) and writes absolute paths.
- **Delete:** trashes `<id>/`; shared pool untouched; referenced local file untouched.
- **Recommended settings:** manifest override wins; family fallback; null when neither.

## Out of scope (deferred)

- Font-size normalization between Generate and other buttons (backlog #4).
- Parameter panel layout reorg — Width/Height/Steps/CFG/Sampler (backlog #5).
- Manifest extras: thumbnail, tags, notes/description, favorite, last-used, trigger words/default
  prompt. Schema is versioned (`schema_version`) so these can be added later without breaking.
- Shared-encoder orphan pruning / reference counting on delete.
- User-overridable / remote catalog (bundled JSON only for now).
