# Catalog Expansion — VRAM-tier spread + RAM-aware fit Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand the 4-entry catalog into a ~16-entry, 7-family VRAM/RAM-tier spread (ultra-light CPU floor → 24 GB+), with a RAM-fallback fit rating so CPU / integrated-GPU users get a verdict, and per-family encoder pooling so multi-file entries dedupe their big text encoders.

**Architecture:** Rust owns the catalog data (`resources/catalog.json`), the recipe/pool table (`recipes.rs`), and the fit rating (`catalog.rs`); a Tauri command rates entries against `(vram, ram)`. Svelte renders a basis-aware fit badge. Every DiT family (FLUX.1/2, SD 3.5, Qwen-Image, Z-Image) is diffusion-only and pulls encoders + VAE from its family `shared` pool (downloaded once under `models_dir/shared/<family>/`); a per-entry `shared` override drops a lighter encoder into the model folder for tiers that need it.

**Tech Stack:** Rust (Tauri v2, serde), Svelte 5 (runes), stable-diffusion.cpp (Vulkan, pinned `b290693`).

**Design spec:** `docs/superpowers/specs/2026-07-21-catalog-expansion-design.md`. All repos/filenames/byte sizes below are HF-API-verified 2026-07-22.

---

## File Structure

| File | Responsibility | Change |
|---|---|---|
| `src-tauri/src/catalog.rs` | Catalog model + fit rating + download planning | Add `RatingBasis`; `rate_entry`/`rated_catalog_entries` gain RAM fallback; `RatedCatalogEntry.basis`; guard test for tier/family spread |
| `src-tauri/src/commands.rs` | Tauri command surface | `catalog_entries` gains `ram_total_mb` arg |
| `src-tauri/src/recipes.rs` | Family recipes + shared pools + gen defaults | flux1 pool → fp8 T5 + camenduru ae; add sd3/qwen-image/flux2 pools; add `z-image` recipe + `family_defaults` arm |
| `src-tauri/resources/catalog.json` | The curated catalog data | Full rewrite: ~16 entries across 5 tiers / 7 families |
| `src/lib/types.ts` | Frontend types | Add `RatingBasis`; `RatedCatalogEntry` gains `basis` |
| `src/lib/api.ts` | Command bindings | `catalogEntries` gains `ramTotalMb` |
| `src/lib/modelFormat.ts` | Presentation helpers | `suitabilityBadge` becomes basis-aware |
| `src/lib/components/NewModelDialog.svelte` | Add-a-model dialog | Add `ramTotalMb` prop; basis-aware context header |
| `src/routes/+page.svelte` | Mounts the dialog | Provide `ramTotalMb` from `$sysStats.ram_total_mb` |

**Verified component sources (used verbatim below):**
- flux1 pool: `comfyanonymous/flux_text_encoders/t5xxl_fp8_e4m3fn.safetensors` 4,893,934,904 · `.../clip_l.safetensors` 246,144,152 · `camenduru/FLUX.1-dev-ungated/ae.safetensors` 335,304,388
- sd3 pool: `Comfy-Org/stable-diffusion-3.5-fp8/text_encoders/{clip_l 246,144,152, clip_g 1,389,382,176, t5xxl_fp8_e4m3fn 4,893,934,904}` · VAE `stabilityai/stable-diffusion-3.5-large/vae/diffusion_pytorch_model.safetensors` 167,666,902 (gated)
- qwen-image pool: `Comfy-Org/Qwen-Image_ComfyUI/split_files/text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors` 9,384,670,680 · `.../vae/qwen_image_vae.safetensors` 253,806,246
- flux2 pool: `unsloth/Qwen3-8B-GGUF/Qwen3-8B-Q4_K_M.gguf` 5,027,784,512 · `Comfy-Org/flux2-dev/split_files/vae/flux2-vae.safetensors` 336,213,556 · (4B override) `unsloth/Qwen3-4B-GGUF/Qwen3-4B-Q4_K_M.gguf` 2,497,281,312
- z-image pool: `unsloth/Qwen3-4B-Instruct-2507-GGUF/Qwen3-4B-Instruct-2507-Q4_K_M.gguf` 2,497,281,120 · `camenduru/FLUX.1-dev-ungated/ae.safetensors` 335,304,388

**Test commands:** Rust `cargo test --manifest-path src-tauri/Cargo.toml`; frontend `npm run check` (expect 0/0) + `npm run build`.

**License-string note:** The `license` strings in Task 6 are best-effort from each model card (`Apache-2.0`, `Stability AI Community License`, etc.). They are display-only (never gate anything) — the executor should copy them verbatim and NOT invent or "correct" them.

---

### Task 0: Create the feature branch

**Files:** none (git only).

- [ ] **Step 1: Branch off main**

Subagent-driven-development and finishing-a-development-branch both forbid working on `main` without consent. Create the branch (or worktree) first.

Run:
```bash
git checkout -b feat/catalog-expansion
git status
```
Expected: `On branch feat/catalog-expansion`, clean tree.

---

### Task 1: `RatingBasis` enum + RAM-fallback fit rating

Add a fit basis so CPU / integrated-GPU users (no VRAM) get a verdict computed against system RAM, reusing the same thresholds. `rate_entry` returns `(Suitability, RatingBasis)`.

**Files:**
- Modify: `src-tauri/src/catalog.rs` (add `RatingBasis`; rewrite `rate_entry`; add `rate_value` helper)
- Test: `src-tauri/src/catalog.rs` (tests module)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/src/catalog.rs` (before the closing `}` of `mod tests`):

```rust
    fn sample_entry() -> CatalogEntry {
        CatalogEntry {
            id: "e".into(), name: "E".into(), family: "flux1".into(),
            license: "Apache-2.0".into(), source_url: "https://h/e".into(),
            diffusion: CatalogFile { url: "https://h/e.gguf".into(), filename: "e.gguf".into(), size_bytes: 0 },
            shared: vec![], min_vram_mb: 8192, recommended_vram_mb: 12288,
        }
    }

    #[test]
    fn rate_entry_prefers_vram_basis() {
        let e = sample_entry();
        assert_eq!(rate_entry(&e, Some(16384), Some(4096)), (Suitability::Recommended, RatingBasis::Vram));
        assert_eq!(rate_entry(&e, Some(10240), Some(65536)), (Suitability::Tight, RatingBasis::Vram));
        assert_eq!(rate_entry(&e, Some(4096), Some(65536)), (Suitability::TooBig, RatingBasis::Vram));
    }

    #[test]
    fn rate_entry_falls_back_to_ram_when_no_vram() {
        let e = sample_entry();
        // No GPU (None or 0 VRAM) → rate against RAM with the same thresholds.
        assert_eq!(rate_entry(&e, None, Some(16384)), (Suitability::Recommended, RatingBasis::Ram));
        assert_eq!(rate_entry(&e, Some(0), Some(10240)), (Suitability::Tight, RatingBasis::Ram));
        assert_eq!(rate_entry(&e, None, Some(4096)), (Suitability::TooBig, RatingBasis::Ram));
    }

    #[test]
    fn rate_entry_unknown_when_neither_known() {
        let e = sample_entry();
        assert_eq!(rate_entry(&e, None, None), (Suitability::Unknown, RatingBasis::None));
        assert_eq!(rate_entry(&e, Some(0), Some(0)), (Suitability::Unknown, RatingBasis::None));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml rate_entry`
Expected: FAIL — `RatingBasis` not found, `rate_entry` takes 2 args.

- [ ] **Step 3: Implement**

In `src-tauri/src/catalog.rs`, add the enum right after the `Suitability` enum (after line 11):

```rust
/// What the fit verdict was computed against. `Ram` means no usable GPU was
/// found, so the entry was rated against system RAM (CPU/iGPU path).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RatingBasis {
    Vram,
    Ram,
    None,
}
```

Replace the whole `rate_entry` function (lines 113-121) with:

```rust
/// Rate a memory total (MB) against an entry's thresholds.
fn rate_value(entry: &CatalogEntry, total_mb: u64) -> Suitability {
    if total_mb >= entry.recommended_vram_mb {
        Suitability::Recommended
    } else if total_mb >= entry.min_vram_mb {
        Suitability::Tight
    } else {
        Suitability::TooBig
    }
}

/// Rate an entry against total VRAM, falling back to system RAM when no usable
/// GPU is present (0 or unknown VRAM). Returns the verdict and which memory it
/// was computed against.
pub fn rate_entry(
    entry: &CatalogEntry,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> (Suitability, RatingBasis) {
    if let Some(v) = vram_total_mb.filter(|v| *v > 0) {
        (rate_value(entry, v), RatingBasis::Vram)
    } else if let Some(r) = ram_total_mb.filter(|r| *r > 0) {
        (rate_value(entry, r), RatingBasis::Ram)
    } else {
        (Suitability::Unknown, RatingBasis::None)
    }
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml rate_entry`
Expected: PASS (3 tests). `rated_catalog_entries` (below) will not compile yet — that's Task 2.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/catalog.rs
git commit -m "feat(catalog): add RatingBasis + RAM-fallback fit rating"
```

---

### Task 2: Thread `basis` through `RatedCatalogEntry` + the command

`rate_entry` now returns a tuple; update the aggregator, the serialized DTO, and the Tauri command to carry `basis` and accept `ram_total_mb`.

**Files:**
- Modify: `src-tauri/src/catalog.rs` (`RatedCatalogEntry`, `rated_catalog_entries`)
- Modify: `src-tauri/src/commands.rs:81-84` (`catalog_entries`)
- Test: `src-tauri/src/catalog.rs` (tests module)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/catalog.rs`:

```rust
    #[test]
    fn rated_entries_carry_basis() {
        let rated = rated_catalog_entries(vec![sample_entry()], None, Some(16384));
        assert_eq!(rated.len(), 1);
        assert_eq!(rated[0].suitability, Suitability::Recommended);
        assert_eq!(rated[0].basis, RatingBasis::Ram);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml rated_entries_carry_basis`
Expected: FAIL — `rated_catalog_entries` takes 2 args / no field `basis`.

- [ ] **Step 3: Implement**

In `src-tauri/src/catalog.rs`, replace the `RatedCatalogEntry` struct (lines 106-111) with:

```rust
/// A catalog entry plus its fit verdict + what it was rated against, for the New… dialog.
#[derive(Debug, Clone, Serialize)]
pub struct RatedCatalogEntry {
    #[serde(flatten)]
    pub entry: CatalogEntry,
    pub suitability: Suitability,
    pub basis: RatingBasis,
}
```

Replace `rated_catalog_entries` (lines 123-132) with:

```rust
/// The full catalog rated against VRAM (RAM fallback). See `rate_entry`.
pub fn rated_catalog_entries(
    entries: Vec<CatalogEntry>,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> Vec<RatedCatalogEntry> {
    entries
        .into_iter()
        .map(|entry| {
            let (suitability, basis) = rate_entry(&entry, vram_total_mb, ram_total_mb);
            RatedCatalogEntry { entry, suitability, basis }
        })
        .collect()
}
```

In `src-tauri/src/commands.rs`, replace `catalog_entries` (lines 81-84) with:

```rust
#[tauri::command]
pub fn catalog_entries(
    app: AppHandle,
    vram_total_mb: Option<u64>,
    ram_total_mb: Option<u64>,
) -> Vec<catalog::RatedCatalogEntry> {
    catalog::rated_catalog_entries(load_bundled_catalog(&app), vram_total_mb, ram_total_mb)
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS (whole lib compiles + all tests green).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/catalog.rs src-tauri/src/commands.rs
git commit -m "feat(catalog): carry rating basis + accept ram_total_mb in command"
```

---

### Task 3: Fix the flux1 shared pool (fp8 T5 + ungated AE)

The flux1 pool currently points at the 9.8 GB `t5xxl_fp16` and the **gated** BFL `ae.safetensors`. Switch to the 4.9 GB `t5xxl_fp8_e4m3fn` (smaller, fits the low-VRAM tiers we're adding) and the ungated `camenduru/FLUX.1-dev-ungated/ae.safetensors` so no token is needed.

**Files:**
- Modify: `src-tauri/src/recipes.rs` (flux1 recipe `shared` vec, lines 152-171)
- Test: `src-tauri/src/recipes.rs` (tests module)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src-tauri/src/recipes.rs`:

```rust
    #[test]
    fn flux1_pool_uses_fp8_t5_and_ungated_ae() {
        let r = recipe_for("flux1").unwrap();
        let t5 = r.shared.iter().find(|s| s.role == ComponentRole::T5xxl).unwrap();
        assert_eq!(t5.filename, "t5xxl_fp8_e4m3fn.safetensors");
        assert_eq!(t5.size_bytes, 4_893_934_904);
        let ae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert!(ae.url.contains("camenduru/FLUX.1-dev-ungated"), "AE must be the ungated mirror");
        assert_eq!(ae.size_bytes, 335_304_388);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml flux1_pool_uses_fp8`
Expected: FAIL — filename is still `t5xxl_fp16.safetensors`.

- [ ] **Step 3: Implement**

In `src-tauri/src/recipes.rs`, replace the flux1 `shared` vec (lines 152-171) with:

```rust
            shared: vec![
                SharedComponent {
                    role: ComponentRole::T5xxl,
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/t5xxl_fp8_e4m3fn.safetensors",
                    size_bytes: 4_893_934_904,
                    filename: "t5xxl_fp8_e4m3fn.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::ClipL,
                    url: "https://huggingface.co/comfyanonymous/flux_text_encoders/resolve/main/clip_l.safetensors",
                    size_bytes: 246_144_152,
                    filename: "clip_l.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/camenduru/FLUX.1-dev-ungated/resolve/main/ae.safetensors",
                    size_bytes: 335_304_388,
                    filename: "ae.safetensors",
                },
            ],
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml flux1_pool_uses_fp8`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recipes.rs
git commit -m "fix(recipes): flux1 pool → fp8 T5 + ungated AE mirror"
```

---

### Task 4: Populate sd3 / qwen-image / flux2 shared pools

These three families have empty `shared` pools, so their catalog entries could never download encoders. Fill each pool with its ungated encoders + VAE. Also broaden the qwen-image `llm` patterns so the underscored `qwen_2.5_vl_*` encoder is detected in point-at-folder assembly (catalog assigns roles explicitly, but folder-scan relies on patterns).

**Files:**
- Modify: `src-tauri/src/recipes.rs` (sd3 `shared` line 185; qwen-image `llm` patterns line 192 + `shared` line 197; flux2 `shared` line 209)
- Test: `src-tauri/src/recipes.rs` (tests module)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/src/recipes.rs`:

```rust
    #[test]
    fn sd3_pool_has_encoders_and_gated_vae() {
        let r = recipe_for("sd3").unwrap();
        let roles: Vec<ComponentRole> = r.shared.iter().map(|s| s.role).collect();
        assert!(roles.contains(&ComponentRole::ClipL));
        assert!(roles.contains(&ComponentRole::ClipG));
        assert!(roles.contains(&ComponentRole::T5xxl));
        let vae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.filename, "sd3.5_vae.safetensors");
        assert!(vae.url.contains("stabilityai/stable-diffusion-3.5-large"));
        let cg = r.shared.iter().find(|s| s.role == ComponentRole::ClipG).unwrap();
        assert_eq!(cg.size_bytes, 1_389_382_176);
    }

    #[test]
    fn qwen_image_pool_has_llm_and_vae_and_detects_underscored_llm() {
        let r = recipe_for("qwen-image").unwrap();
        let llm = r.shared.iter().find(|s| s.role == ComponentRole::Llm).unwrap();
        assert_eq!(llm.filename, "qwen_2.5_vl_7b_fp8_scaled.safetensors");
        assert_eq!(llm.size_bytes, 9_384_670_680);
        assert!(r.shared.iter().any(|s| s.role == ComponentRole::Vae));
        // Folder-scan detection must now find the underscored encoder.
        let files = vec![
            "qwen-image-Q4_K_S.gguf".to_string(),
            "qwen_2.5_vl_7b_fp8_scaled.safetensors".to_string(),
            "qwen_image_vae.safetensors".to_string(),
        ];
        let d = detect(&r, &files);
        assert_eq!(d.get(ComponentRole::Llm), Some("qwen_2.5_vl_7b_fp8_scaled.safetensors"));
        assert_eq!(d.get(ComponentRole::Diffusion), Some("qwen-image-Q4_K_S.gguf"));
    }

    #[test]
    fn flux2_pool_has_qwen3_llm_and_vae() {
        let r = recipe_for("flux2").unwrap();
        let llm = r.shared.iter().find(|s| s.role == ComponentRole::Llm).unwrap();
        assert_eq!(llm.filename, "Qwen3-8B-Q4_K_M.gguf");
        assert_eq!(llm.size_bytes, 5_027_784_512);
        let vae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.filename, "flux2-vae.safetensors");
        assert_eq!(vae.size_bytes, 336_213_556);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml pool_`
Expected: FAIL — `unwrap()` on `None` (pools empty).

- [ ] **Step 3: Implement**

In `src-tauri/src/recipes.rs`, in the **sd3** recipe replace `shared: vec![],` (line 185) with:

```rust
            shared: vec![
                SharedComponent {
                    role: ComponentRole::ClipL,
                    url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5-fp8/resolve/main/text_encoders/clip_l.safetensors",
                    size_bytes: 246_144_152,
                    filename: "clip_l.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::ClipG,
                    url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5-fp8/resolve/main/text_encoders/clip_g.safetensors",
                    size_bytes: 1_389_382_176,
                    filename: "clip_g.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::T5xxl,
                    url: "https://huggingface.co/Comfy-Org/stable-diffusion-3.5-fp8/resolve/main/text_encoders/t5xxl_fp8_e4m3fn.safetensors",
                    size_bytes: 4_893_934_904,
                    filename: "t5xxl_fp8_e4m3fn.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/stabilityai/stable-diffusion-3.5-large/resolve/main/vae/diffusion_pytorch_model.safetensors",
                    size_bytes: 167_666_902,
                    filename: "sd3.5_vae.safetensors",
                },
            ],
```

In the **qwen-image** recipe, replace the `llm` role line (line 192):

```rust
                role(ComponentRole::Llm, true, &["qwenvl", "qwen2.5", "qwen2_5", "qwen_2.5", "llm"]),
```

and replace its `shared: vec![],` (line 197) with:

```rust
            shared: vec![
                SharedComponent {
                    role: ComponentRole::Llm,
                    url: "https://huggingface.co/Comfy-Org/Qwen-Image_ComfyUI/resolve/main/split_files/text_encoders/qwen_2.5_vl_7b_fp8_scaled.safetensors",
                    size_bytes: 9_384_670_680,
                    filename: "qwen_2.5_vl_7b_fp8_scaled.safetensors",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/Comfy-Org/Qwen-Image_ComfyUI/resolve/main/split_files/vae/qwen_image_vae.safetensors",
                    size_bytes: 253_806_246,
                    filename: "qwen_image_vae.safetensors",
                },
            ],
```

In the **flux2** recipe, replace `shared: vec![],` (line 209) with:

```rust
            shared: vec![
                SharedComponent {
                    role: ComponentRole::Llm,
                    url: "https://huggingface.co/unsloth/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf",
                    size_bytes: 5_027_784_512,
                    filename: "Qwen3-8B-Q4_K_M.gguf",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/Comfy-Org/flux2-dev/resolve/main/split_files/vae/flux2-vae.safetensors",
                    size_bytes: 336_213_556,
                    filename: "flux2-vae.safetensors",
                },
            ],
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS (all lib tests, including the 3 new pool tests + existing detect tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recipes.rs
git commit -m "feat(recipes): populate sd3/qwen-image/flux2 shared pools"
```

---

### Task 5: Add the `z-image` family (recipe + gen defaults)

Z-Image Turbo (leejet GGUF) is diffusion + a Qwen3-4B **Instruct** LLM + the FLUX AE VAE, invoked with no `--vae-format`/`--prediction` (so `vae_format`/`prediction` stay `None` and the command builder needs no change). Add its recipe and an 8-step gen-defaults arm.

**Files:**
- Modify: `src-tauri/src/recipes.rs` (add recipe to `recipes()` before the `custom` entry, line 211; add `family_defaults` arm line 260)
- Test: `src-tauri/src/recipes.rs` (tests module)

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `src-tauri/src/recipes.rs`:

```rust
    #[test]
    fn z_image_recipe_registered() {
        let r = recipe_for("z-image").expect("z-image recipe must exist");
        assert_eq!(r.vae_format, None);
        assert_eq!(r.prediction, None);
        let required: Vec<ComponentRole> =
            r.roles.iter().filter(|s| s.required).map(|s| s.role).collect();
        assert!(required.contains(&ComponentRole::Diffusion));
        assert!(required.contains(&ComponentRole::Llm));
        assert!(required.contains(&ComponentRole::Vae));
        let llm = r.shared.iter().find(|s| s.role == ComponentRole::Llm).unwrap();
        assert_eq!(llm.filename, "Qwen3-4B-Instruct-2507-Q4_K_M.gguf");
        assert_eq!(llm.size_bytes, 2_497_281_120);
        let vae = r.shared.iter().find(|s| s.role == ComponentRole::Vae).unwrap();
        assert_eq!(vae.size_bytes, 335_304_388);
    }

    #[test]
    fn detect_best_picks_z_image_for_z_image_files() {
        let files = vec![
            "z_image_turbo-Q4_K.gguf".to_string(),
            "Qwen3-4B-Instruct-2507-Q4_K_M.gguf".to_string(),
            "ae.safetensors".to_string(),
        ];
        let (recipe, _d) = detect_best(&files).unwrap();
        assert_eq!(recipe.family, "z-image");
    }

    #[test]
    fn family_defaults_z_image_uses_eight_steps() {
        let d = family_defaults("z-image", None).unwrap();
        assert_eq!(d.steps, 8);
        assert_eq!(d.cfg_scale, 1.0);
        assert_eq!(d.sampler, crate::types::Sampler::Euler);
        assert_eq!((d.width, d.height), (1024, 1024));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml z_image`
Expected: FAIL — no `z-image` recipe.

- [ ] **Step 3: Implement**

In `src-tauri/src/recipes.rs`, insert this recipe into the `recipes()` vec immediately before the `custom` recipe (before line 211's `ModelRecipe { family: "custom", ...`):

```rust
        ModelRecipe {
            family: "z-image",
            name: "Z-Image (Turbo)",
            roles: vec![
                role(ComponentRole::Diffusion, true, &["z_image", "z-image", "zimage"]),
                role(ComponentRole::Llm, true, &["qwen3", "qwen", "llm"]),
                role(ComponentRole::Vae, true, &["ae.", "vae"]),
            ],
            vae_format: None,
            prediction: None,
            shared: vec![
                SharedComponent {
                    role: ComponentRole::Llm,
                    url: "https://huggingface.co/unsloth/Qwen3-4B-Instruct-2507-GGUF/resolve/main/Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
                    size_bytes: 2_497_281_120,
                    filename: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf",
                },
                SharedComponent {
                    role: ComponentRole::Vae,
                    url: "https://huggingface.co/camenduru/FLUX.1-dev-ungated/resolve/main/ae.safetensors",
                    size_bytes: 335_304_388,
                    filename: "ae.safetensors",
                },
            ],
        },
```

Add the `family_defaults` arm — in the `match family { … }` block, add after the `"qwen-image"` arm (line 258):

```rust
        "z-image" => Some(d(8, 1.0, Sampler::Euler, 1024, 1024)),
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS (all lib tests, including 3 new z-image tests; `recipe_table_integrity` still green since z-image uses `None`/`None`).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/recipes.rs
git commit -m "feat(recipes): add z-image family recipe + gen defaults"
```

---

### Task 6: Rewrite `catalog.json` into a 16-entry tier spread + add a guard test

Replace the 4-entry catalog with 16 entries spanning 5 VRAM tiers and all 7 families. Only `flux2-klein-4b` carries a per-entry `shared` override (the lighter Qwen3-4B LLM); every other multi-file entry inherits its family pool. Add a guard test so the tier/family spread can't silently regress.

**Files:**
- Overwrite: `src-tauri/resources/catalog.json`
- Test: `src-tauri/src/catalog.rs` (tests module)

- [ ] **Step 1: Write the failing guard test**

Add to the `tests` module in `src-tauri/src/catalog.rs`:

```rust
    #[test]
    fn bundled_catalog_spans_tiers_and_families() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/resources/catalog.json");
        let s = std::fs::read_to_string(path).unwrap();
        let entries = parse_catalog(&s).unwrap();
        // Tier floor + ceiling present.
        assert!(entries.iter().any(|e| e.recommended_vram_mb <= 4096), "need an ultra-light entry");
        assert!(entries.iter().any(|e| e.recommended_vram_mb > 16000), "need a 24GB-tier entry");
        // Every engine family represented.
        for fam in ["sd15", "sdxl", "flux1", "flux2", "sd3", "qwen-image", "z-image"] {
            assert!(entries.iter().any(|e| e.family == fam), "family {fam} missing from catalog");
        }
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml bundled_catalog_spans`
Expected: FAIL — no `z-image` / no `qwen-image` entry, no rec>16000 entry.

- [ ] **Step 3: Implement — overwrite `src-tauri/resources/catalog.json` with exactly:**

```json
{
  "schema_version": 1,
  "entries": [
    {
      "id": "sd15",
      "name": "Stable Diffusion 1.5",
      "family": "sd15",
      "license": "CreativeML-OpenRAIL-M",
      "source_url": "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5",
      "diffusion": {
        "url": "https://huggingface.co/stable-diffusion-v1-5/stable-diffusion-v1-5/resolve/main/v1-5-pruned-emaonly.safetensors",
        "filename": "v1-5-pruned-emaonly.safetensors",
        "size_bytes": 4265146304
      },
      "shared": [],
      "min_vram_mb": 2048,
      "recommended_vram_mb": 4096
    },
    {
      "id": "sdxl-lightning-4step",
      "name": "SDXL Lightning 4-step (GGUF Q4_0)",
      "family": "sdxl",
      "license": "openrail++",
      "source_url": "https://huggingface.co/mzwing/SDXL-Lightning-GGUF",
      "diffusion": {
        "url": "https://huggingface.co/mzwing/SDXL-Lightning-GGUF/resolve/main/sdxl_lightning_4step.q4_0.gguf",
        "filename": "sdxl_lightning_4step.q4_0.gguf",
        "size_bytes": 2584858432
      },
      "shared": [],
      "min_vram_mb": 3072,
      "recommended_vram_mb": 4096
    },
    {
      "id": "z-image-turbo-q2k",
      "name": "Z-Image Turbo (GGUF Q2_K)",
      "family": "z-image",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/leejet/Z-Image-Turbo-GGUF",
      "diffusion": {
        "url": "https://huggingface.co/leejet/Z-Image-Turbo-GGUF/resolve/main/z_image_turbo-Q2_K.gguf",
        "filename": "z_image_turbo-Q2_K.gguf",
        "size_bytes": 2592442304
      },
      "shared": [],
      "min_vram_mb": 3072,
      "recommended_vram_mb": 4096
    },
    {
      "id": "flux2-klein-4b-q4",
      "name": "FLUX.2 klein 4B (GGUF Q4_0)",
      "family": "flux2",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF",
      "diffusion": {
        "url": "https://huggingface.co/unsloth/FLUX.2-klein-4B-GGUF/resolve/main/flux-2-klein-4b-Q4_0.gguf",
        "filename": "flux-2-klein-4b-Q4_0.gguf",
        "size_bytes": 2460394048
      },
      "shared": [
        {
          "role": "llm",
          "url": "https://huggingface.co/unsloth/Qwen3-4B-GGUF/resolve/main/Qwen3-4B-Q4_K_M.gguf",
          "filename": "Qwen3-4B-Q4_K_M.gguf",
          "size_bytes": 2497281312
        }
      ],
      "min_vram_mb": 4096,
      "recommended_vram_mb": 6144
    },
    {
      "id": "flux1-schnell-q2k",
      "name": "FLUX.1 schnell (GGUF Q2_K)",
      "family": "flux1",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/city96/FLUX.1-schnell-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/FLUX.1-schnell-gguf/resolve/main/flux1-schnell-Q2_K.gguf",
        "filename": "flux1-schnell-Q2_K.gguf",
        "size_bytes": 4010296352
      },
      "shared": [],
      "min_vram_mb": 4096,
      "recommended_vram_mb": 6144
    },
    {
      "id": "z-image-turbo-q4k",
      "name": "Z-Image Turbo (GGUF Q4_K)",
      "family": "z-image",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/leejet/Z-Image-Turbo-GGUF",
      "diffusion": {
        "url": "https://huggingface.co/leejet/Z-Image-Turbo-GGUF/resolve/main/z_image_turbo-Q4_K.gguf",
        "filename": "z_image_turbo-Q4_K.gguf",
        "size_bytes": 3864250304
      },
      "shared": [],
      "min_vram_mb": 4096,
      "recommended_vram_mb": 6144
    },
    {
      "id": "sdxl-base-1.0",
      "name": "Stable Diffusion XL 1.0 (base)",
      "family": "sdxl",
      "license": "CreativeML Open RAIL++-M",
      "source_url": "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0",
      "diffusion": {
        "url": "https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/resolve/main/sd_xl_base_1.0.safetensors",
        "filename": "sd_xl_base_1.0.safetensors",
        "size_bytes": 6938078334
      },
      "shared": [],
      "min_vram_mb": 6144,
      "recommended_vram_mb": 8192
    },
    {
      "id": "sd35-large-turbo-q4",
      "name": "SD 3.5 Large Turbo (GGUF Q4_0)",
      "family": "sd3",
      "license": "Stability AI Community License",
      "source_url": "https://huggingface.co/city96/stable-diffusion-3.5-large-turbo-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/stable-diffusion-3.5-large-turbo-gguf/resolve/main/sd3.5_large_turbo-Q4_0.gguf",
        "filename": "sd3.5_large_turbo-Q4_0.gguf",
        "size_bytes": 4772054752
      },
      "shared": [],
      "min_vram_mb": 6144,
      "recommended_vram_mb": 8192
    },
    {
      "id": "flux2-klein-9b-q4",
      "name": "FLUX.2 klein 9B (GGUF Q4_0)",
      "family": "flux2",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/leejet/FLUX.2-klein-9B-GGUF",
      "diffusion": {
        "url": "https://huggingface.co/leejet/FLUX.2-klein-9B-GGUF/resolve/main/flux-2-klein-9b-Q4_0.gguf",
        "filename": "flux-2-klein-9b-Q4_0.gguf",
        "size_bytes": 5616208032
      },
      "shared": [],
      "min_vram_mb": 6144,
      "recommended_vram_mb": 8192
    },
    {
      "id": "flux1-schnell-q4ks",
      "name": "FLUX.1 schnell (GGUF Q4_K_S)",
      "family": "flux1",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/city96/FLUX.1-schnell-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/FLUX.1-schnell-gguf/resolve/main/flux1-schnell-Q4_K_S.gguf",
        "filename": "flux1-schnell-Q4_K_S.gguf",
        "size_bytes": 6783943712
      },
      "shared": [],
      "min_vram_mb": 8192,
      "recommended_vram_mb": 12288
    },
    {
      "id": "flux1-dev-q4ks",
      "name": "FLUX.1 dev (GGUF Q4_K_S)",
      "family": "flux1",
      "license": "flux-1-dev-non-commercial-license",
      "source_url": "https://huggingface.co/city96/FLUX.1-dev-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/FLUX.1-dev-gguf/resolve/main/flux1-dev-Q4_K_S.gguf",
        "filename": "flux1-dev-Q4_K_S.gguf",
        "size_bytes": 6805988640
      },
      "shared": [],
      "min_vram_mb": 8192,
      "recommended_vram_mb": 12288
    },
    {
      "id": "sd35-large-q5",
      "name": "SD 3.5 Large (GGUF Q5_0)",
      "family": "sd3",
      "license": "Stability AI Community License",
      "source_url": "https://huggingface.co/city96/stable-diffusion-3.5-large-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/stable-diffusion-3.5-large-gguf/resolve/main/sd3.5_large-Q5_0.gguf",
        "filename": "sd3.5_large-Q5_0.gguf",
        "size_bytes": 5773844192
      },
      "shared": [],
      "min_vram_mb": 8192,
      "recommended_vram_mb": 12288
    },
    {
      "id": "flux2-klein-9b-q8",
      "name": "FLUX.2 klein 9B (GGUF Q8_0)",
      "family": "flux2",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/leejet/FLUX.2-klein-9B-GGUF",
      "diffusion": {
        "url": "https://huggingface.co/leejet/FLUX.2-klein-9B-GGUF/resolve/main/flux-2-klein-9b-Q8_0.gguf",
        "filename": "flux-2-klein-9b-Q8_0.gguf",
        "size_bytes": 9978284192
      },
      "shared": [],
      "min_vram_mb": 10240,
      "recommended_vram_mb": 12288
    },
    {
      "id": "sd35-large-q8",
      "name": "SD 3.5 Large (GGUF Q8_0)",
      "family": "sd3",
      "license": "Stability AI Community License",
      "source_url": "https://huggingface.co/city96/stable-diffusion-3.5-large-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/stable-diffusion-3.5-large-gguf/resolve/main/sd3.5_large-Q8_0.gguf",
        "filename": "sd3.5_large-Q8_0.gguf",
        "size_bytes": 8779212512
      },
      "shared": [],
      "min_vram_mb": 12288,
      "recommended_vram_mb": 16384
    },
    {
      "id": "qwen-image-q4ks",
      "name": "Qwen-Image (GGUF Q4_K_S)",
      "family": "qwen-image",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/city96/Qwen-Image-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/Qwen-Image-gguf/resolve/main/qwen-image-Q4_K_S.gguf",
        "filename": "qwen-image-Q4_K_S.gguf",
        "size_bytes": 12140608032
      },
      "shared": [],
      "min_vram_mb": 12288,
      "recommended_vram_mb": 16384
    },
    {
      "id": "qwen-image-q8",
      "name": "Qwen-Image (GGUF Q8_0)",
      "family": "qwen-image",
      "license": "Apache-2.0",
      "source_url": "https://huggingface.co/city96/Qwen-Image-gguf",
      "diffusion": {
        "url": "https://huggingface.co/city96/Qwen-Image-gguf/resolve/main/qwen-image-Q8_0.gguf",
        "filename": "qwen-image-Q8_0.gguf",
        "size_bytes": 21761817120
      },
      "shared": [],
      "min_vram_mb": 16384,
      "recommended_vram_mb": 24576
    }
  ]
}
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS — `bundled_catalog_spans_tiers_and_families`, `bundled_catalog_file_is_valid` (every entry validates), and all other lib tests green.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/resources/catalog.json src-tauri/src/catalog.rs
git commit -m "feat(catalog): expand to 16-entry, 7-family VRAM-tier spread"
```

---

### Task 7: Frontend plumbing — `basis`, RAM arg, basis-aware badge

Thread the new `basis` field and `ram_total_mb` argument through the frontend, and make the fit badge say "RAM" (with a CPU-slow note) when there's no GPU. These five files are coupled through the `catalogEntries` signature, so `npm run check` is only expected to pass once all edits land (Step 6).

**Files:**
- Modify: `src/lib/types.ts:76,198` (add `RatingBasis`, extend `RatedCatalogEntry`)
- Modify: `src/lib/api.ts:48-49` (`catalogEntries` gains `ramTotalMb`)
- Modify: `src/lib/modelFormat.ts:1,38-46` (basis-aware `suitabilityBadge`)
- Modify: `src/lib/components/NewModelDialog.svelte:5,7,9,15-21,57,60` (prop + header + badge call)
- Modify: `src/routes/+page.svelte:26-27,108` (provide `ramTotalMb`)

- [ ] **Step 1: `src/lib/types.ts`**

After the `Suitability` type (line 198), add:

```ts
// Wire values MUST match the Rust `RatingBasis` enum's serde snake_case form
// (src-tauri/src/catalog.rs). "ram" = no GPU found, rated against system RAM.
export type RatingBasis = "vram" | "ram" | "none";
```

Replace the `RatedCatalogEntry` type (line 76) with:

```ts
export type RatedCatalogEntry = CatalogEntry & { suitability: Suitability; basis: RatingBasis };
```

- [ ] **Step 2: `src/lib/api.ts`**

Replace `catalogEntries` (lines 48-49) with:

```ts
export const catalogEntries = (vramTotalMb: number | null, ramTotalMb: number | null) =>
  invoke<RatedCatalogEntry[]>("catalog_entries", { vramTotalMb, ramTotalMb });
```

- [ ] **Step 3: `src/lib/modelFormat.ts`**

Change the import on line 1 to add `RatingBasis`:

```ts
import type { LibraryEntry, Suitability, RatingBasis, ModelRef, CatalogEntry } from "./types";
```

Replace `suitabilityBadge` (lines 38-46) with:

```ts
/** Fit badge text + tone for a catalog row. When `basis` is "ram" (no GPU found)
 *  the copy names RAM and flags that CPU generation is slow. */
export function suitabilityBadge(
  s: Suitability,
  basis: RatingBasis = "vram",
): { text: string; tone: "good" | "warn" | "bad" | "muted" } {
  if (basis === "ram") {
    switch (s) {
      case "recommended": return { text: "Fits in RAM · CPU: slow", tone: "good" };
      case "tight": return { text: "Tight in RAM · CPU: slow", tone: "warn" };
      case "too_big": return { text: "Too big for RAM", tone: "bad" };
      default: return { text: "Unknown", tone: "muted" };
    }
  }
  switch (s) {
    case "recommended": return { text: "Recommended", tone: "good" };
    case "tight": return { text: "Tight fit", tone: "warn" };
    case "too_big": return { text: "Too big", tone: "bad" };
    default: return { text: "Unknown", tone: "muted" };
  }
}
```

- [ ] **Step 4: `src/lib/components/NewModelDialog.svelte`**

Replace the props line (line 9):

```svelte
  let { vramTotalMb, ramTotalMb, onClose }: { vramTotalMb: number | null; ramTotalMb: number | null; onClose: () => void } = $props();
```

Replace the catalog `$effect` (lines 16-18):

```svelte
  $effect(() => {
    catalogEntries(vramTotalMb, ramTotalMb).then((c) => (catalog = c)).catch((e) => (catalogError = String(e)));
  });
```

Replace the `vramLabel` derived (lines 20-21) with a basis-aware fit label:

```svelte
  // Show what fit is rated against: VRAM if a GPU is present, else RAM (CPU path).
  const fitLabel = $derived(
    vramTotalMb
      ? `Your VRAM: ${+(vramTotalMb / 1024).toFixed(1)} GB`
      : ramTotalMb
        ? `No GPU detected — fit vs. RAM: ${+(ramTotalMb / 1024).toFixed(1)} GB (CPU generation is slow)`
        : "Hardware unknown — fit not rated",
  );
```

Replace the vram-note paragraph (line 57):

```svelte
      <p class="vram-note">{fitLabel}</p>
```

Replace the badge call (line 60):

```svelte
          {@const b = suitabilityBadge(e.suitability, e.basis)}
```

- [ ] **Step 5: `src/routes/+page.svelte`**

Replace the VRAM state + subscription (lines 26-27) with:

```svelte
  let vramTotalMb = $state<number | null>(null);
  let ramTotalMb = $state<number | null>(null);
  sysStats.subscribe((s) => {
    vramTotalMb = s?.gpu?.vram_total_mb ?? null;
    ramTotalMb = s?.ram_total_mb ?? null;
  });
```

Replace the dialog mount (line 108):

```svelte
  <NewModelDialog {vramTotalMb} {ramTotalMb} onClose={() => (showNew = false)} />
```

- [ ] **Step 6: Verify + commit**

Run: `npm run check`
Expected: `svelte-check` reports 0 errors, 0 warnings.

Run: `npm run build`
Expected: build succeeds.

```bash
git add src/lib/types.ts src/lib/api.ts src/lib/modelFormat.ts src/lib/components/NewModelDialog.svelte src/routes/+page.svelte
git commit -m "feat(ui): basis-aware fit badge + RAM-fallback rating in New dialog"
```

---

### Task 8: Full-suite verification

**Files:** none (verification only).

- [ ] **Step 1: Rust suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all tests PASS (lib + any integration).

- [ ] **Step 2: Frontend gates**

Run: `npm run check`
Expected: 0 errors, 0 warnings.

Run: `npm run build`
Expected: success.

- [ ] **Step 3: Sanity-scan the catalog for role coverage**

For each multi-file family, confirm the recipe pool + any per-entry override together satisfy the recipe's REQUIRED roles (so no downloaded model is born broken):
- `flux1` pool → T5xxl + ClipL + Vae (diffusion from entry). ✓
- `sd3` pool → ClipL + ClipG + T5xxl + Vae. ✓
- `qwen-image` pool → Llm + Vae. ✓
- `flux2` pool → Llm + Vae; `flux2-klein-4b-q4` overrides Llm → Qwen3-4B (pool Vae still applies). ✓
- `z-image` pool → Llm + Vae. ✓

No commit (verification only).

---

## Manual Acceptance (user-run, post-merge)

Automated tests cover data integrity and plumbing, not real downloads/generation. After merge, download + generate one entry per family/tier and confirm each loads:

- **CPU/RAM path:** with no GPU (or iGPU), confirm the New dialog shows "No GPU detected — fit vs. RAM …" and RAM-based badges.
- **Per family:** sd15, sdxl (lightning + base), flux1 (schnell/dev), sd3 (turbo/large), qwen-image, z-image — download and generate one image each.
- **Gated component:** sd3 entries pull the gated `stabilityai` VAE — confirm it downloads with an HF token set (and errors helpfully without one).
- **FLUX.2 (TENTATIVE):** the three `flux2-klein-*` entries ship but are provisional — they stay only if they actually load in the pinned engine (`b290693`). If they fail to load, remove those three entries (and note it) before the feature is considered done.

## Self-Review Notes (author)

- **Spec coverage:** RatingBasis/RAM-fallback (Task 1-2, spec §1); encoder pooling all families (Task 3-4, spec §4.1); z-image family (Task 5, spec §4.3); tier/family spread (Task 6, spec §4.2); basis-aware UI (Task 7). All spec sections mapped.
- **Type consistency:** `rate_entry` → `(Suitability, RatingBasis)` used identically in Task 1/2; `RatedCatalogEntry.basis` (Rust) ↔ `RatingBasis` (TS) snake_case wire values match; `catalog_entries(vram_total_mb, ram_total_mb)` ↔ `catalogEntries(vramTotalMb, ramTotalMb)` (Tauri camelCase) align.
- **No new command-builder work:** z-image `vae_format`/`prediction` are `None`, so the existing family-agnostic builder emits `--diffusion-model --llm --vae` only, matching `docs/z_image.md`.

