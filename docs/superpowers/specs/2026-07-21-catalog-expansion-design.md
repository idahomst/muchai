# Catalog Expansion — VRAM-tier spread + RAM-aware fit — Design

**Date:** 2026-07-21
**Status:** Approved (design)
**Feature branch:** `feat/catalog-expansion`

## Problem

The curated catalog (`src-tauri/resources/catalog.json`) is thin: 4 entries (SD 1.5,
SDXL base, FLUX.1-schnell Q4, FLUX.1-dev Q4). It covers only 3 of the 6 families the
engine can run (missing SD 3.x, FLUX.2, Qwen-Image), has no spread across VRAM tiers,
and gives **no fit guidance at all** to CPU-only or integrated-GPU users (the rating is
VRAM-only, so those users see "Unknown" on every entry).

## Goals

1. **Full VRAM-tier spread.** Entries from an ultra-light CPU-friendly floor up through
   16–24 GB+, several per family at different quant levels, so any user finds a fit.
2. **CPU / integrated-GPU users are first-class.** When there is no usable VRAM, rate
   entries against system RAM so those users still get a verdict (with a "CPU is slow"
   caveat).
3. **Hand-curated, Hugging-Face-only sources.** Every entry points at an original,
   engine-loadable HF file with a verified https URL and exact byte size. The
   `drawthingsai/community-models` repo is used only as a curation *reference* for what
   is worth including — never its (proprietary, non-loadable) files or format.

## Non-goals

- Programmatic mining / auto-generation of entries from DT metadata.
- Civitai sources (license/permanence risk; deferred).
- In-app HuggingFace/Civitai browsing (separate post-1.0 item).
- Automated in-engine load verification (manual acceptance step).

## Decisions (from brainstorming)

- **Catalog goal:** full VRAM-tier spread; CPU/iGPU users must be covered.
- **Sourcing:** hand-curated, HF-only.
- **CPU/iGPU fit:** RAM fallback rating with a basis flag.
- **Multi-file encoders in small tiers:** per-entry quantized-encoder override (existing
  `shared` field), preferring known-good fp8 safetensors over GGUF for text encoders.

---

## 1. Rating model (RAM fallback)

`rate_entry` gains a RAM fallback; the result gains a *basis* so the UI knows what the
verdict was computed against.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RatingBasis { Vram, Ram, None }

/// Rate an entry, preferring VRAM and falling back to system RAM when no usable
/// VRAM is known (CPU / integrated GPU). Returns the verdict and what it was
/// rated against. Reuses the entry's min/recommended_vram_mb thresholds for RAM.
pub fn rate_entry(
    entry: &CatalogEntry,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> (Suitability, RatingBasis)
```

Logic:
- `vram_total_mb` present & `> 0` → rate against VRAM, basis `Vram` (today's behavior).
- else `ram_total_mb` present & `> 0` → rate against RAM using the **same**
  `min_vram_mb` / `recommended_vram_mb` thresholds, basis `Ram`.
- else → `(Suitability::Unknown, RatingBasis::None)`.

`RatedCatalogEntry` gains `basis: RatingBasis`. `rated_catalog_entries` threads both
figures:

```rust
pub fn rated_catalog_entries(
    entries: Vec<CatalogEntry>,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> Vec<RatedCatalogEntry>
```

**Why reuse the VRAM thresholds for RAM** (not add `min_ram_mb`): CPU inference needs
roughly the weight size resident in RAM, and RAM is usually more plentiful than VRAM, so
the VRAM floor is a safe proxy — a box with enough RAM shows *Recommended*, a tight one
*Tight*. A second per-entry threshold set is maintenance we don't need yet (YAGNI).

## 2. Command / API / prop plumbing

`SystemStats` already carries `ram_total_mb`; `gpu?.vram_total_mb` is the VRAM source.

- `catalog_entries(app, vram_total_mb: Option<u64>, ram_total_mb: Option<u64>)`
  (commands.rs) → `rated_catalog_entries(load_bundled_catalog(&app), vram, ram)`.
- `api.ts`: `catalogEntries(vramTotalMb: number | null, ramTotalMb: number | null)`.
- `NewModelDialog.svelte`: add `ramTotalMb: number | null` prop, pass to
  `catalogEntries`.
- `+page.svelte` (where `NewModelDialog` is mounted): supply both from `$sysStats`
  (`gpu?.vram_total_mb ?? null` and `ram_total_mb ?? null`).
- `src/lib/types.ts`: `RatedCatalogEntry` gains `basis: "vram" | "ram" | "none"`.

## 3. Badge / header UI

`modelFormat.ts::suitabilityBadge` becomes basis-aware:

- basis `Vram` → current labels (Recommended / Tight / Too big / —) and colors.
- basis `Ram` → "Fits in RAM" / "Tight in RAM" / "Too big for RAM", each with a
  "· CPU: slow" hint and a tooltip noting CPU inference is much slower. Reuses the same
  color mapping as the VRAM verdicts.
- basis `None` → size only, no verdict (today's behavior).

The dialog's context header (currently "Your VRAM: X GB"):
- VRAM known → "Your VRAM: **X GB**".
- no VRAM, RAM known → "Your RAM: **Y GB** (no dedicated GPU — CPU is slow)".
- neither → "VRAM/RAM: unknown".

## 4. Catalog content — full tier spread, hand-curated HF-only

All entries below are **HF-API-verified** (exact repo, filename, byte size, gating) as of
2026-07-22. Single-file families (SD 1.5, SDXL, SDXL-Lightning-GGUF) stay self-contained.
Every DiT family (FLUX.1, FLUX.2, SD 3.5, Qwen-Image, Z-Image) is diffusion-only and pulls
its text-encoder(s) + VAE as components (§4.1).

### 4.1 Encoder pooling (all multi-file families)

Per the "never enough disk" decision, **every** multi-file family recipe carries a
`shared` pool so entries that use the same encoder precision download it **once** (pooled
under `models_dir/shared/<family>/`). An entry that needs a *lighter* encoder to fit a
small tier lists a per-entry `shared` override (existing mechanism → lands in the model
folder, pool copy skipped). Pool precision is chosen as the one the majority of that
family's entries use:

| Family | Pooled components (default precision) | Per-entry override cases |
|---|---|---|
| `flux1` | `t5xxl_fp8_e4m3fn` 4.89 GB · `clip_l` 246 MB · **`ae` 335 MB (camenduru ungated — replaces the current gated BFL url)** | ultra-light entry overrides T5 → `t5-v1_1-xxl-encoder-Q4_K_S.gguf` 2.74 GB |
| `sd3` | `clip_l` 246 MB · `clip_g` 1.39 GB · `t5xxl_fp8_e4m3fn` 4.89 GB *(all `Comfy-Org/stable-diffusion-3.5-fp8`, ungated)* · **`vae` 168 MB (`stabilityai/stable-diffusion-3.5-large`, gated `is_gated`)** | none (all sd3 entries share the pool) |
| `qwen-image` | `qwen_2.5_vl_7b_fp8` LLM 9.38 GB · `qwen_image_vae` 254 MB *(both `Comfy-Org/Qwen-Image_ComfyUI`, ungated)* | none |
| `flux2` *(tentative)* | `flux2-vae` 336 MB *(`Comfy-Org/flux2-dev`, ungated)* · Qwen3 LLM | 9B entries override LLM if it differs from the 4B pool default |
| `z-image` *(new family)* | Qwen3-4B-Instruct-2507 LLM · `ae` 335 MB *(shares FLUX's ungated camenduru ae)* | none |

The **only gated component** is the SD 3.5 VAE (auto-approve on login); it is marked
`is_gated` and covered by the app's existing HF-token support. If an ungated SD 3.5 VAE
mirror surfaces, it is a one-line swap.

### 4.2 Entry list by fit tier

| Tier (rec. VRAM / CPU-RAM) | Entry | Family | Diffusion (verified) |
|---|---|---|---|
| **Ultra-light** ≤4 GB (CPU-friendly) | SD 1.5 *(existing)* | sd15 | `v1-5-pruned-emaonly.safetensors` 3.97 GB |
| | SDXL-Lightning 4-step Q4_0 | sdxl | `mzwing/…/sdxl_lightning_4step.q4_0.gguf` 2.41 GB (single-file GGUF) |
| | Z-Image-Turbo Q2_K | z-image | `leejet/…/z_image_turbo-Q2_K.gguf` 2.59 GB |
| **Light** 4–8 GB | FLUX.1-schnell Q2_K (+ GGUF-T5 override) | flux1 | `city96/…/flux1-schnell-Q2_K.gguf` ~4.0 GB |
| | Z-Image-Turbo Q4_K | z-image | `leejet/…/z_image_turbo-Q4_K.gguf` 3.86 GB |
| | SD 3.5 Large-Turbo Q4_0 | sd3 | `city96/…/sd3.5_large_turbo-Q4_0.gguf` 4.44 GB |
| | FLUX.2-klein 4B Q4_0 *(tentative)* | flux2 | `unsloth/…/flux-2-klein-4b-Q4_0.gguf` 2.46 GB |
| **Mid** 8–12 GB | FLUX.1-schnell Q4_K_S *(existing)* | flux1 | `city96/…/flux1-schnell-Q4_K_S.gguf` 6.78 GB |
| | FLUX.1-dev Q4_K_S *(existing)* | flux1 | `city96/…/flux1-dev-Q4_K_S.gguf` 6.81 GB |
| | SD 3.5 Large Q5_0 | sd3 | `city96/…/sd3.5_large-Q5_0.gguf` 5.77 GB |
| | Qwen-Image Q4_K_S | qwen-image | `city96/…/qwen-image-Q4_K_S.gguf` 12.14 GB |
| | FLUX.2-klein 9B Q4_0 *(tentative)* | flux2 | `leejet/…/flux-2-klein-9b-Q4_0.gguf` 5.62 GB |
| **High** 12–16 GB | SDXL base 1.0 *(existing)* | sdxl | `sd_xl_base_1.0.safetensors` 6.94 GB |
| | SD 3.5 Large Q8_0 | sd3 | `city96/…/sd3.5_large-Q8_0.gguf` 8.78 GB |
| | FLUX.2-klein 9B Q8_0 *(tentative)* | flux2 | `leejet/…/flux-2-klein-9b-Q8_0.gguf` 9.98 GB |
| **Max** 16–24 GB+ | Qwen-Image Q8_0 | qwen-image | `city96/…/qwen-image-Q8_0.gguf` 21.76 GB |

`is_gated` is a **catalog-metadata** note in the design; whether it becomes a real
`CatalogFile`/`CatalogShared` field or is inferred from the license is decided in the
plan. FLUX.2 entries are shipped but **flagged tentative** — they only stay if they load
in manual acceptance (§5); their exact Qwen3 LLM file is verified at plan-write time.

### 4.3 New family: `z-image`

Z-Image is a new recipe family (engine support landed upstream ~2025-12-01; our pinned
`b290693`, dated 2026-07-16, includes it). Invocation (from `docs/z_image.md`):
`--diffusion-model z_image_turbo-*.gguf --llm Qwen3-4B-Instruct-2507-*.gguf --vae ae.sft`,
`--cfg-scale 1.0`. **No `--vae-format` / `--prediction`** → the recipe sets both to
`None`, so the family-agnostic command builder needs no change. `family_defaults` gains a
`z-image` arm (≈8 steps, cfg 1.0, Euler, 1024²).

License note: each entry records its upstream license (`FLUX.1-dev` non-commercial,
`Apache-2.0` for schnell, `stabilityai-ai-community` for SD 3.5, etc.) as it does today —
surfaced, not altered.

## 5. Testing & acceptance

**Unit (catalog.rs):**
- RAM fallback: VRAM `None` + RAM `Some(big)` → `(Recommended, Ram)`; VRAM `Some` wins
  when both present (basis `Vram`); both `None` → `(Unknown, None)`; RAM below `min` →
  `(TooBig, Ram)`.
- Per-entry encoder override lands in the model folder, not the fp16 shared pool
  (extends the existing `plan_entry_downloads` override coverage with an fp8 T5 case).

**Guard test (catalog.rs):** the bundled catalog spans tiers — at least one entry with
`recommended_vram_mb <= 4096` and at least one with `recommended_vram_mb > 16000` — and
includes each family the design claims to cover. Prevents silent regression to a thin
catalog.

**Existing:** `bundled_catalog_file_is_valid` continues to validate every expanded entry
(https urls, known family, VRAM ordering).

**Frontend gate:** `npm run check` (0/0) + `npm run build`.

**Manual acceptance (user, at the machine):** download + generate one entry per
family/tier to confirm real in-engine loading — the "validate a few load in-engine" step;
cannot be automated. FLUX.2-klein is included only if it passes here.

## Risks

- **Quantized/fp8 encoder loading** through `--t5xxl` / `--llm` is assumed, not proven.
  Mitigation: prefer fp8 safetensors (known-good); verify each multi-file entry in
  acceptance before it ships; drop or downgrade any entry that fails.
- **URL permanence / size drift.** HF resolve URLs can move. Mitigation: HEAD-verify at
  write time; `bundled_catalog_file_is_valid` catches structural breakage; a broken URL
  surfaces as a download error, not a crash.
- **RAM-basis over-optimism.** Reusing VRAM thresholds for RAM may call a model
  "Recommended" that runs but very slowly on CPU. Mitigation: the "CPU: slow" badge hint
  and header caveat set expectations.

## Out of scope / follow-ups

- Pre-download free-space check (separate queued item).
- FLUX.2 catalog entries beyond the single tentative klein test.
- Civitai sources, in-app HF/Civitai browsing.

