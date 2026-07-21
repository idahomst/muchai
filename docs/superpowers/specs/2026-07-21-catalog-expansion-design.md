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

Entries grouped by fit tier. Single-file SD-family models stay self-contained. Multi-file
FLUX / SD 3.x / Qwen entries in the small tiers carry **per-entry quantized-encoder
overrides** via the existing `shared` field (dest = model folder, not the fp16 pool);
high-tier entries keep the pooled fp16 encoder.

`size_bytes` and every resolve URL are **HEAD-verified at implementation time** (see §5).
Preferred quantized text-encoder = **fp8 safetensors** (`t5xxl_fp8_e4m3fn.safetensors`
~4.9 GB, comfyanonymous/flux_text_encoders — known-good) over GGUF T5, unless a GGUF
encoder is verified to load through `--t5xxl`.

| Tier (rec. VRAM / CPU-RAM) | Entries |
|---|---|
| **Ultra-light** ≤4 GB (CPU-friendly) | SD 1.5 *(existing)* |
| **Light** 4–8 GB | SDXL base 1.0 *(existing)*; SD 3.5 **Medium** (mmdit + fp8 clips/t5 override) |
| **Mid** 8–12 GB | FLUX.1-schnell **Q4** + fp8 T5 override; FLUX.1-dev **Q4** + fp8 T5 override; SD 3.5 **Large Q4/GGUF**; Qwen-Image **Q4 GGUF** + quantized LLM override |
| **High** 12–16 GB | FLUX.1-dev **Q8**; SD 3.5 Large (fp16) |
| **Max** 16–24 GB+ | FLUX.1-dev **fp16/fp8**; Qwen-Image (fp16); FLUX.2-klein *(tentative — only if it loads cleanly in acceptance)* |

License note: each entry records its upstream license (`FLUX.1-dev` non-commercial,
`Apache-2.0` for schnell, OpenRAIL for SD, etc.) as it does today — surfaced, not
altered.

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

