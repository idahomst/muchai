# Multi-file (split) model support — design

**Date:** 2026-07-13 (amended 2026-07-17)
**Status:** core implemented & merged to `main`. **Amended 2026-07-17** with two direction
decisions — platform/distribution and recipe source (Draw Things catalog) — that guide the
multi-file UX rework.
**Milestone:** this is the **v1.0 gate**.

## Problem

MuchAI assumes **one model = one file**: `GenerationRequest.model_path` is a single
`String` passed to stable-diffusion.cpp via `-m` (the all-in-one checkpoint flag).
That works for SD1.5 / SDXL but **cannot load split models** (FLUX, Krea, Qwen-Image),
whose transformer, text encoders, and VAE are separate `.safetensors` files loaded via
`--diffusion-model`, `--clip_l` / `--t5xxl` / `--llm`, and `--vae`. Any attempt to load a
Flux-layout file via `-m` fails with `get sd version from file failed`.

This feature adds multi-file model loading, plus a curated catalog and filename-based
auto-assembly so the common case is "point at it, done" rather than hand-wiring flags.

## Scope

**In scope:**
- Loading split models by wiring typed component files to the correct engine flags.
- A **recipe** system describing model families and how to recognize their component files.
- Three ways to define a multi-file model: **download from a curated catalog**,
  **point at a folder** (auto-detect), and **assign files manually** (fallback).
- Saved, named multi-file model **definitions** that appear in the Model dropdown like any
  other model.
- A **shared component pool** so family-common encoders/VAE download once and are reused.

**Out of scope (explicitly deferred):**
- Full HuggingFace / Civitai in-app browsing (post-1.0).
- Pre-download free-disk-space check + cleanup offer (separate, independent feature).
- Reference-counted deletion of shared components (delete removes per-model files only).
- GGUF containers (dropped project-wide — safetensors only).
- LoRA / ControlNet (post-1.0).
- A loose-encoder-file name filter for the single-file list (minor cosmetic; later).

**Compatibility:** we are in beta; no config/gallery backward-compat is required. We take
the cleaner data model.

## Platform & distribution (amendment — 2026-07-17)

**Target: Linux-only.** macOS and Windows are dropped as targets — Draw Things already serves
those platforms well; the gap MuchAI fills is a native, DrawThings-class image-generation app
for Linux, which is missing today. Narrowing to Linux removes macOS notarization, Windows code
signing, and cross-compilation from the picture.

**Stack unchanged: Tauri + Svelte + `stable-diffusion.cpp` (CLI).** The heavy lifting is the
sd.cpp CLI; the Tauri/Svelte layer is a thin shell over it, so no GUI-framework change is
warranted (a rewrite to GTK4/Qt/egui would be pure cost). The one Linux fragility to watch is
**WebKitGTK** (`webkit2gtk-4.1`), Tauri's Linux webview — behavior can vary across distros.

**Distribution:**
- **AppImage** — current artifact; remains the dev/test build and a portable "download and run"
  option throughout the rework.
- **Flatpak / Flathub** — intended **primary release channel**, but **deferred to a pre-1.0
  release milestone**. It is a distribution concern, not a dev-loop one: the work is the sandbox
  (GPU/CUDA `--device=dri`, filesystem holes for user-chosen model/gallery dirs, network for
  downloads, bundling the sd.cpp binary + libs). Packaging is done **once the file layout is
  stable** after the multi-file rework, so sandbox/permission work isn't redone each time storage
  changes. Flatpak is also the clean fix for the known `libcuda.so.1` runtime quirk (the lib
  lives in the GL runtime).

## Confirmed engine flags (from `src-tauri/fixtures/sd-help.txt`)

`--diffusion-model` (standalone transformer), `--clip_l`, `--clip_g`, `--t5xxl`,
`--llm` (LLM text encoder: qwenvl2.5 for qwen-image, mistral/qwen for flux2),
`--vae`, `--vae-format` (auto/flux/sd3/flux2), `--prediction`
(eps/v/edm_v/sd3_flow/flux_flow/flux2_flow). Everything else in the command
(`-p`, `--steps`, `-W/-H`, `-o`, `--backend`, `-v`, …) is shared with single-file.

## Section 1 — Data model

A model is **either** one all-in-one file **or** a set of typed component files. Represented
as a sum type so illegal states (both / neither) are unrepresentable.

```rust
/// A model reference: single all-in-one file, or split components.
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ModelRef {
    SingleFile { path: String },   // -> -m <path>
    MultiFile(ModelComponents),    // -> --diffusion-model + friends
}

/// Typed component files of a split model, each wired to a specific engine flag.
pub struct ModelComponents {
    pub diffusion_model: String,                       // --diffusion-model (required)
    #[serde(default)] pub vae: Option<String>,         // --vae
    #[serde(default)] pub clip_l: Option<String>,      // --clip_l
    #[serde(default)] pub clip_g: Option<String>,      // --clip_g
    #[serde(default)] pub t5xxl: Option<String>,       // --t5xxl
    #[serde(default)] pub llm: Option<String>,         // --llm
    #[serde(default)] pub vae_format: Option<String>,  // --vae-format
    #[serde(default)] pub prediction: Option<String>,  // --prediction
}

/// A saved multi-file model — the library entry shown in the Model dropdown.
pub struct ModelDefinition {
    pub id: String,        // stable, generated
    pub name: String,      // user-facing label
    pub family: String,    // recipe id: "flux1", "qwen-image", … (drives roles & defaults)
    pub components: ModelComponents,
}
```

**Changes to existing types:**
- `GenerationRequest.model_path: String` **becomes** `model: ModelRef`.
- `AppConfig` gains `model_definitions: Vec<ModelDefinition>` (the persisted library).

**Selection:** picking a single-file model sets `model = SingleFile { path }`; picking a
saved definition sets `model = MultiFile(def.components.clone())`. The request **snapshots**
the resolved component paths (not a reference id), so gallery images stay reproducible even
if the definition is later edited or deleted.

TS types in `src/lib/types.ts` mirror these as a discriminated union.

## Section 2 — Recipes & filename detection

A **recipe** describes one model family and how to recognize its parts.

```rust
pub enum ComponentRole { Diffusion, Vae, ClipL, ClipG, T5xxl, Llm }

pub struct RoleSpec {
    pub role: ComponentRole,
    pub required: bool,
    pub patterns: Vec<&'static str>, // case-insensitive filename matches, e.g. ["t5xxl","t5-xxl"]
}

pub struct ModelRecipe {
    pub family: String,                   // "flux1"
    pub name: String,                     // "FLUX.1 (dev / schnell / krea)"
    pub roles: Vec<RoleSpec>,
    pub vae_format: Option<&'static str>, // "flux"
    pub prediction: Option<&'static str>, // "flux_flow"
    pub shared: Vec<SharedComponent>,     // family-common downloadable parts (Section 4)
}
```

A built-in `recipes()` table lives in a new `src-tauri/src/recipes.rs`. Initial families:
**flux1**, **sd3**, **qwen-image** (add more later). Plus a `"custom"` pseudo-family used by
the manual flow only: diffusion required, all other roles optional, no filename patterns, no
shared components, `vae_format`/`prediction` unset (engine auto-detects).

**Detection** is a pure function `detect(recipe, filenames) -> DetectedComponents`, where
`DetectedComponents` maps each role to the matched filename (or none). For each role it finds
files whose name matches any pattern, picks the most-specific (longest) match, and leaves
unmatched required roles empty. No filesystem access — takes a list of filenames.

Powers the point-at-a-folder flow: run detection against each recipe, pick the family with the
most confident match, and present the role→file assignments pre-filled for confirmation.

### Recipe source: Draw Things community-models catalog, CC0 (amendment — 2026-07-17)

Recipe knowledge is sourced from the **Draw Things community-models** catalog
(`github.com/drawthingsai/community-models`), published under **CC0 1.0 (public domain)**. Each
model's `metadata.json` encodes exactly what a recipe needs: the family, which component roles it
uses, the source HuggingFace repo, and defaults (VAE format, prediction objective, scale).

**We harvest metadata, not files.** The catalog's own model files are Draw Things' *converted,
quantized `.ckpt`* format (their `q8p`/`q6p`/`i8x` schemes), which **`stable-diffusion.cpp`
cannot load**. So we use the catalog only to *learn the recipe*, then resolve each role to the
**original `.safetensors` on HuggingFace** (which sd.cpp loads) — we never point at Draw Things'
converted file URLs. Downloads still come from HF/Civitai as designed in Section 4.

This is the fast path to expanding the recipe table beyond hand-authored families. Concrete
example — the previously-blocked **Flux2-Klein** test case. Its catalog entry reveals:

| Role | FLUX.1 (`flux1`) | FLUX.2 klein (new `flux2` family) |
|---|---|---|
| text encoder | `t5xxl` + `clip_l` | **`llm` = Qwen3-8B** (no T5/CLIP) |
| VAE | flux VAE | FLUX.2 VAE |
| `vae_format` / `prediction` | `flux` / `flux_flow` | `flux2` / `flux2_flow` |

Our `command_builder` already emits `--llm` and supports `--vae-format flux2` /
`--prediction flux2_flow` (see "Confirmed engine flags"), so adding a **`flux2` recipe** is
metadata-only. This motivates adding `flux2` to the initial recipe families.

**Attribution:** CC0 *waives* the attribution requirement, but we credit Draw Things and its
community regardless — a line in an About/credits surface (e.g. *"Model recipes adapted from the
Draw Things community-models catalog — CC0"*). This is a thank-you, not an obligation. It is
**separate** from the model files' own licenses (FLUX is non-commercial, etc.), which are
governed by HuggingFace/BFL and unaffected.

## Section 3 — The three entry flows & dropdown integration

A saved definition appears in the **Model** dropdown badged `multi-file`. A
`＋ New multi-file model…` entry opens an assembly dialog with three ways in; all converge on
the same confirmation panel (role slots) and produce a `ModelDefinition`.

1. **Download from catalog** — curated multi-file entries (Section 4). Download components into
   a per-model subfolder + shared pool, auto-assemble via the recipe, save the definition,
   select it. The "point at it, done" path.
2. **From a folder I have** (auto-detect) — pick a folder → detection → best-matching family →
   pre-filled slots for confirmation. Adjust, name, Save. Unmatched required roles fall through
   to manual.
3. **Assign files manually** (fallback) — pick a family (or "custom"), assign each role via file
   picker, `vae_format`/`prediction` pre-filled from the recipe. Name, Save.

Editing/deleting definitions lives in the Model Library alongside single-file management.

**Single-file list cleanup:** component files are `.safetensors` too, so today's recursive scan
would list them as bogus single-file models. Fix: **exclude any path referenced by a saved
`ModelDefinition`** from the single-file list. (Loose, not-yet-assembled encoder/VAE files may
still show as noise — minor cosmetic issue, refine later; out of scope.)

**Components layout:** `models_dir/<model-slug>/` per downloaded model; shared parts in
`models_dir/shared/<family>/` (Section 4).

## Section 4 — Engine flag mapping & download mechanics

**Flag mapping** — `build_args` becomes a `match` on `req.model`:

```rust
match &req.model {
    ModelRef::SingleFile { path } => push("-m", path.clone()),
    ModelRef::MultiFile(c) => {
        push("--diffusion-model", c.diffusion_model.clone());   // required
        if let Some(v) = &c.vae        { push("--vae", v.clone()); }
        if let Some(v) = &c.clip_l     { push("--clip_l", v.clone()); }
        if let Some(v) = &c.clip_g     { push("--clip_g", v.clone()); }
        if let Some(v) = &c.t5xxl      { push("--t5xxl", v.clone()); }
        if let Some(v) = &c.llm        { push("--llm", v.clone()); }
        if let Some(v) = &c.vae_format { push("--vae-format", v.clone()); }
        if let Some(v) = &c.prediction { push("--prediction", v.clone()); }
    }
}
```

Everything after the model flags is unchanged and shared.

**Shared component pool.** The diffusion file is unique per model; the VAE and text encoders are
shared per family (a Flux `t5xxl` is ~9.8 GB and identical across Flux models — never download it
per-model).

```rust
pub struct SharedComponent {
    pub role: ComponentRole,
    pub url: String,
    pub size_bytes: u64,
    pub filename: String,  // stable name in the shared pool
}

/// A curated multi-file catalog entry — one downloadable split model.
pub struct MultiFileCatalogEntry {
    pub id: String,
    pub name: String,
    pub family: String,
    pub diffusion_url: String,
    pub diffusion_size_bytes: u64,
    pub overrides: Vec<SharedComponent>,  // model ships its OWN vae/encoder (usually empty)
    pub min_vram_mb: u64,
    pub recommended_vram_mb: u64,
}
```

**Storage layout:**
```
models_dir/
  shared/flux1/t5xxl_fp16.safetensors     <- downloaded once, reused
  shared/flux1/clip_l.safetensors
  shared/flux1/ae.safetensors
  flux1-schnell/flux1-schnell.safetensors <- per-model diffusion (+ overrides)
```

**Download resolution, per role:**
1. Diffusion → always the entry's own URL → per-model folder.
2. Each shared role → if the entry **overrides** it, download into the model folder; otherwise
   use the family shared component, downloaded **only if not already in the pool**. Second Flux
   model = diffusion file only.

A pure planning function resolves an entry to the list of `(url, dest_path)` to fetch
(skipping already-present shared files); the downloader executes it. Extends the existing
single-flight downloader to fetch several files **sequentially**, reusing byte-streaming +
cancel (`download_cancel: Arc<AtomicBool>`).

**Progress:** the emitted event gains file context — `{ file_index, file_count, file_name,
downloaded, total }` — so the UI shows "Downloading t5xxl (2/4) — 1.2/9.8 GB". The
`downloadStatus` store's `active` variant gains those optional fields; single-file downloads
leave them unset.

**On completion:** assemble via the recipe (filenames known), build the `ModelDefinition`,
persist to `AppConfig.model_definitions`, return it; UI drops it into the dropdown, selected.

**Cancel / failure:** abort the current file, remove the partial per-model folder, and do NOT
persist a definition. Already-present shared-pool files are left (valid and reusable).

**Deletion:** deleting a multi-file model removes its **per-model folder only**; the shared pool
is left intact so other models keep working. (Reference-counted shared cleanup: later.)

## Section 5 — Validation & error handling

- **A. Required-role completeness.** Each recipe marks required roles (Flux: diffusion + t5xxl
  + clip_l + vae; "custom": diffusion only). Save is disabled and missing slots highlighted while
  any required role is empty. Same check gates generation, so a broken definition never reaches
  the engine.
- **B. File-existence at generation time.** Pure helper
  `missing_components(&ModelComponents) -> Vec<(ComponentRole, String)>`. If any component path no
  longer exists, block with a clear message and flag the definition as **broken** (⚠) in the
  dropdown instead of letting sd.cpp fail cryptically.
- **C. `vae_format` / `prediction`.** Defaulted from the recipe; may be unset for "custom"
  (flags omitted → engine auto-detects). If set, must be a known enum value (enforced by the
  dropdown, not free text).
- **D. Download failures.** Reuse the `downloadStatus` `error` variant. On any failure/cancel
  mid-download, roll back the partial per-model folder and persist no definition.
- **E. No confident detection match.** Fall through to manual assignment with empty slots + a
  family picker — never a dead end.
- **F. Engine-level rejection.** sd.cpp may still reject a genuinely bad combination; surface its
  stderr as today. The pre-checks catch the common, predictable failures early.

## Section 6 — Testing strategy

Logic is concentrated in pure functions, covered by fast `cargo test --lib` unit tests
(currently 86).

**Rust unit tests (pure):**
1. `build_args` multi-file mapping — one test per role present → correct flag+value; optional
   roles absent → omitted; `SingleFile` emits `-m` and no `--diffusion-model`; `MultiFile` always
   emits `--diffusion-model` and never `-m`.
2. `detect(recipe, filenames)` — canonical Flux set → all matched; missing required → empty;
   competing candidates → most-specific wins; junk ignored.
3. Recipe table integrity — required roles have ≥1 pattern; `vae_format`/`prediction` within
   known sets; family ids unique.
4. `missing_components` — all present → empty; each missing required role → reported; missing
   optional → not reported.
5. Download resolution — override vs family-shared per role; already-pooled shared file not
   re-listed. Pure planning function, no network.
6. Serde round-trips — `ModelRef` (both variants), `ModelComponents`, `ModelDefinition`,
   `AppConfig` with `model_definitions`. Pins the exact wire form the TS types mirror.

**Rust integration test (light filesystem):**
7. Single-file exclusion — a diffusion file referenced by a saved definition is excluded from the
   single-file scan; unreferenced `.safetensors` still listed. Temp dirs like existing
   `scan_models` tests.

**Frontend:** `npm run check` stays at 0/0; TS types mirror the Rust wire forms. No frontend unit
framework exists today; none added (consistent with the app).

**Manual E2E (dev box):** download a curated Flux model (verify shared encoders download once,
second model reuses them), point-at-folder auto-detect, manual assembly, generate with each, and
delete a model (per-model folder gone, shared pool intact).

## Section 7 — Model variants & low-VRAM offload (amendment — 2026-07-17)

Two additions to the "point at an HF URL" flow and generation, so the user can (a) choose among a
model's quantization variants with real sizes + a fit estimate, and (b) run models that exceed
their VRAM by spreading weights into RAM.

### 7.1 HF variant discovery & picker (HuggingFace only)

**URL classification** — pure fn over the pasted string:
- **Repo URL** (`huggingface.co/<org>/<repo>`) → enumerate.
- **Direct file URL** (`.../resolve/<rev>/<file>.safetensors`) → skip enumeration; the file *is*
  the chosen diffusion component.

**Enumeration** — `GET https://huggingface.co/api/models/<org>/<repo>/tree/<rev>?recursive=true`.
Each entry yields `path` and a size (`lfs.size ?? size`). Public repos need no auth; gated repos
(401/403) reuse the stored **HF token** as an `Authorization: Bearer` header. Keep only
`.safetensors`.

**Grouping into variants** — for each file:
1. Tag its **role** via the existing `recipes::detect` (diffusion / t5xxl / clip_l / clip_g / vae /
   llm), so a VAE is never shown as a "variant" of the transformer.
2. Extract a quant label via a new pure `precision_label(filename) -> Option<String>`
   (`fp16`, `bf16`, `fp8_e4m3fn`, `fp8_e5m2`, `q8_0`, `q6_k`, `q4_k_m`, `q4_0`, …).

The **diffusion-role files are the variant list**; the user picks one. Companion files (T5/CLIP/
VAE) keep coming from the recipe's curated shared components; if the repo bundles them, auto-pick
the closest-precision match. **Per-companion variant override is deferred** (documented, not built
now).

Direct-file input skips enumeration but still runs `detect` on the filename to choose the family
and shows a single-row picker (size + fit badge).

**GGUF note:** the project is safetensors-only, so `.gguf` quant variants on a repo are not
offered. A user wanting more compression than the fp8/fp16 variants can use sd.cpp's runtime
`--type` down-quantization — out of scope here, noted for later.

### 7.2 Fit estimate (simple heuristic)

Pure fn:

```
estimate_vram_mb(file_size_bytes) ≈ (file_size_bytes / 1_048_576) * 1.15 + ACTIVATION_BUDGET_MB
```

`ACTIVATION_BUDGET_MB` starts at **1500** — one documented, tunable constant. Multi-file sums the
on-GPU components. Verdict against the **selected device's** detected VRAM (from the live monitor):

| Condition | Verdict |
|---|---|
| `est ≤ 0.9 × VRAM` | **Fits** |
| `est ≤ VRAM` | **Tight** |
| `est > VRAM` | **Won't fit** (Low-VRAM mode can help) |
| VRAM unknown / no GPU | **size only, no verdict** (mirrors `catalog::rate`'s `None` path) |

Always labeled an *estimate*. Reuses the existing suitability vocabulary for UI consistency.

### 7.3 Low-VRAM offload mode

- New persisted **`AppConfig.low_vram: bool`** (`#[serde(default)]`, default `false`). Toggle lives
  in **Preferences → Hardware**, beside the Device picker. Label: *"Low-VRAM mode (slower; fits
  bigger models)."*
- When on, generation appends **`--offload-to-cpu` + `--vae-tiling` + `--diffusion-fa`** (weights
  paged from RAM, tiled VAE decode, flash attention — the low-UI/high-headroom bundle).
- **Auto-suggest, never auto-enable:** when a selected / about-to-generate model estimates **Won't
  fit** and the toggle is off, show a one-click, non-blocking suggestion to enable it, stating the
  speed tradeoff. The user stays in control.
- **Threading:** `build_args` gains the `low_vram` flag (via a small engine-options struct rather
  than a bare bool sprawl), pushing the three flags after the model/backend flags. Stays a pure,
  unit-testable function.
- **Deferred (documented future):** granular expert controls — `--max-vram <GiB>` budget,
  `--stream-layers`, `--params-backend`, and per-component `--backend clip=cpu,vae=cuda0,…` split.
  These are acknowledged in the GPU device-selection spec; the single toggle ships first.

### 7.4 Error handling (never a dead end)

- HF API / network failure → fall back to direct-file handling if the URL looks like a file, else a
  clear error with a "paste a direct file URL" escape.
- Gated repo (401/403) → prompt that an HF token is needed (Preferences), reuse the stored token.
- No `.safetensors` in the repo → error + direct-URL suggestion.
- Offload on but still OOM → surface sd.cpp stderr as today; the suggestion text already set
  expectations.

### 7.5 Testing (pure-first, `cargo test --lib`)

- URL classification (repo vs direct file vs junk).
- Variant grouping from a **fixture HF tree JSON** → correct roles + sizes; non-safetensors
  dropped; diffusion files become the variant list.
- `precision_label` across the label set incl. no-match.
- `estimate_vram_mb` + verdict thresholds, including `None` VRAM → size-only.
- `build_args` with `low_vram` true → three offload flags present; false → all absent.
- Serde round-trip of `AppConfig.low_vram` + old-config default (missing key → `false`).
- Frontend: `npm run check` stays 0/0; no new test framework (consistent with the app).

## New / changed files (anticipated)

- `src-tauri/src/recipes.rs` — NEW: `ComponentRole`, `RoleSpec`, `ModelRecipe`, `SharedComponent`,
  `recipes()`, `detect()`.
- `src-tauri/src/catalog.rs` — add `MultiFileCatalogEntry` + curated multi-file entries + download
  resolution planning.
- `src-tauri/src/types.rs` — `ModelRef`, `ModelComponents`, `ModelDefinition`; `GenerationRequest`
  and `AppConfig` changes; `missing_components`.
- `src-tauri/src/command_builder.rs` — `match`-based flag mapping; **+ low-VRAM offload flags**
  (`--offload-to-cpu`, `--vae-tiling`, `--diffusion-fa`) via an engine-options struct (§7.3).
- `src-tauri/src/models.rs` — exclude definition-referenced paths from the single-file list.
- `src-tauri/src/commands.rs` + downloader — `download_multifile`, definition CRUD, multi-file
  progress; **+ `list_hf_variants` command** (§7.1).
- Frontend: `types.ts` (discriminated union), `stores.ts` (definitions + richer download status),
  `api.ts`, `ModelLibrary.svelte` (dropdown + badges + broken flag), a new assembly dialog
  component, and Model Library management.

**New / changed for §7 (variants & low-VRAM):**
- `src-tauri/src/hf.rs` — NEW: HF URL classification, tree-API parse, variant grouping,
  `precision_label` (pure); the async fetch behind `list_hf_variants`.
- `src-tauri/src/catalog.rs` (or `fit.rs`) — `estimate_vram_mb` + fit verdict (pure).
- `src-tauri/src/types.rs` — `AppConfig.low_vram: bool`; a `Variant` DTO; engine-options struct.
- Frontend: `Variant` type in `types.ts`; variant-picker UI in the assembly dialog; Low-VRAM
  toggle in `PreferencesDialog.svelte` (Hardware section); Won't-fit auto-suggest on model
  selection / generate.
