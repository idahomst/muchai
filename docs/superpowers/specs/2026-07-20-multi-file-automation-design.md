# Multi-File Model Automation & Data-Loss Fix — Design

**Status:** Approved (design) — 2026-07-20
**Scope:** Spec 1 of the multi-file "rethink". Follow-ups: Spec 2 (catalog: quantized, fit-rated entries) and Spec 3 (unify download UX). Those are explicitly OUT of scope here.

## Problem

Real-hardware testing (RTX 3060 12 GB + Intel UHD 770) showed the multi-file
model feature is rough in practice. After separately fixing a stale bundled
engine (the "yellow square"; commit `b290693` now pinned), four issues remain:

1. **gguf not auto-detected.** The "point at a folder → auto-detect components"
   flow (`detect_folder`) is hardcoded safetensors-only, so a `.gguf` diffusion
   model must be assigned by hand even though the library scan accepts `.gguf`.
2. **Config-clobber data loss.** `model_definitions` lives inside the monolithic
   `AppConfig` that `set_settings` replaces wholesale, but the frontend edits
   definitions through a *separate* store that is never re-synced. Any unrelated
   settings toggle after a definition change ships a stale config and **erases
   every model added/downloaded since app launch**.
3. **Low-VRAM is manual-only.** A fit-estimator exists but only feeds the HF
   variant picker; nothing auto-enables offload, so large models silently OOM.
4. **Generation defaults are global.** `steps 20 / cfg 7 / EulerA / 512²` — wrong
   for FLUX (schnell wants ~4 steps, cfg 1). No per-family recommendation exists.

## Goals

- Adding a model never silently disappears (correctness).
- Pointing at a folder detects gguf-based split models, not just safetensors.
- A model that won't fit VRAM runs anyway (auto offload) with a clear notice.
- One click applies sensible per-family generation settings.

## Non-Goals

- Expanding the curated catalog / quantized variants (Spec 2).
- Changing or unifying the download surfaces (Spec 3).
- Auto-applying generation defaults on model switch (explicitly rejected — the
  user chose an opt-in button to avoid clobbering tuned params).
- Any new third-party dependency.

## Architecture Principles

- Decision logic (`resolve_low_vram`, family presets) as **pure functions** that
  are unit-tested in isolation; Tauri commands only wire them in.
- Graceful degradation: unknown VRAM or a failed file `stat` disables the
  automatic behavior rather than erroring.

---

## Item 2 — Fix the config-clobber (data loss)

**Root cause.** `set_settings` (`commands.rs:90-113`) does `*cfg = config` and
`save_config_to` writes the whole `AppConfig`. The incoming `AppConfig` carries
`model_definitions` copied from the frontend `settings` store, which is frozen at
startup and never updated when definitions change (those go through
`save_model_definition` / `download_multifile` / `delete_model_definition`, which
update only the separate `definitions` store). So a later settings toggle ships a
stale `model_definitions` and overwrites the authoritative list.

**Chosen approach: server preserves definitions (Approach A).**
`set_settings` treats `model_definitions` (and `last_request`) as **owned by the
backend**, not by the settings payload. Before saving, it overwrites the
incoming copies with the authoritative in-memory values:

```rust
#[tauri::command]
pub fn set_settings(app: AppHandle, state: State<AppState>, mut config: AppConfig) -> Result<(), String> {
    {
        let cur = state.config.lock().unwrap();
        // model_definitions and last_request are owned by their own command
        // surfaces (save/download/delete definition; generate). The settings
        // payload may carry a stale copy from a frontend store frozen at
        // startup, so never let it overwrite the authoritative list.
        config.model_definitions = cur.model_definitions.clone();
        config.last_request = cur.last_request.clone();
    }
    config::save_config_to(&config::config_file_path(), &config).map_err(|e| e.to_string())?;
    let _ = app.asset_protocol_scope().allow_directory(&config.gallery_dir, true);
    {
        let mut cfg = state.config.lock().unwrap();
        if cfg.sd_binary_path != config.sd_binary_path {
            *state.gpu_devices.lock().unwrap() = None;
        }
        *cfg = config;
    }
    Ok(())
}
```

Definitions therefore change **only** through their dedicated commands — which is
already the real invariant. This is minimal and closes the hole for both
`model_definitions` and `last_request`.

**Belt-and-suspenders (frontend).** Keep `$settings.model_definitions` in sync
when the `definitions` store changes, so the store the payload is built from is
never stale. This is defence-in-depth; the backend preserve is the real guard.

**Rejected alternatives.**
- *B — split `model_definitions` into their own persisted file.* Cleanest
  long-term but adds a migration + more churn than warranted now.
- *C — only sync the frontend stores.* Fragile: one missed mutation path and the
  bug silently returns; no backend guarantee.

**Tests.**
- Rust: `set_settings` called with an `AppConfig` whose `model_definitions` is
  empty must NOT erase pre-existing definitions in state/on disk; same for a
  mutated `last_request` (existing one preserved).
- Rust: after `save_model_definition` then `set_settings` (empty defs payload),
  the definition still resolves.

---

## Item 1 — gguf in folder auto-detect

**Change.** `detect_folder` (`commands.rs:588-601`) currently filters to
`safetensors` only. Reuse the shared extension set from `models.rs`
(`MODEL_EXTS = ["safetensors", "ckpt", "gguf"]`) so folder auto-detect sees the
same files the library scan does. Expose `is_model_file` / `MODEL_EXTS` from
`models.rs` (make `pub(crate)`) and call it in `detect_folder` instead of the
inline safetensors check.

Recipe matching (`recipes.rs::detect`) is case-insensitive **substring** on the
filename and is already extension-agnostic (`"flux1-"` matches
`flux1-schnell-Q4_K_S.gguf`), so no recipe change is needed once the file is no
longer filtered out.

**Tests.**
- Rust: `detect_folder` (or its pure detection helper) over a folder containing a
  `.gguf` diffusion file + `.safetensors` companions detects the full `flux1`
  recipe with the gguf assigned to the diffusion slot.

---

## Item 4 — Auto Low-VRAM per run

**Behavior (chosen).** Decide at generate time; enable offload for that run only;
show a notice. The manual Preferences toggle remains an explicit "force always
on". Nothing is persisted silently.

**Decision — pure function** (new, in `fit.rs` or `command_builder.rs`):

```rust
/// Decide whether to run the engine in Low-VRAM (offload) mode.
/// Returns (offload_enabled, auto_engaged).
pub fn resolve_low_vram(
    manual_toggle: bool,     // cfg.low_vram
    weights_mb: Option<u64>, // summed on-disk size of the model's files
    device_vram_mb: Option<u64>,
    is_cpu_device: bool,
) -> (bool, bool) {
    if manual_toggle { return (true, false); }         // force-on, not "auto"
    if is_cpu_device { return (false, false); }        // CPU: all in RAM anyway
    match (weights_mb, device_vram_mb) {
        (Some(w), Some(v)) if estimate_vram_mb(w) > v => (true, true), // won't fit
        _ => (false, false),                            // fits, or unknown -> leave off
    }
}
```

`estimate_vram_mb` is the existing `fit.rs` heuristic (`weights·1.15 + 1500`).

**Weights.** Sum the on-disk byte size of every component file for the selected
model (`diffusion_model` + any of `vae`/`clip_l`/`clip_g`/`t5xxl`/`llm`). For a
single-file model, the one file. Works for gguf and safetensors alike, with no
catalog dependency. A `stat` failure on any file → `weights_mb = None` → auto
disabled (degrade gracefully).

**Device VRAM.** Obtain the selected device's total VRAM from the same provider
the resource monitor uses (`sysmon` providers), keyed to `cfg.gpu_device`. If the
selection is "Default (engine picks)" resolve via `devices::pick_default_device`;
if CPU, `is_cpu_device = true`; if VRAM can't be read (e.g. Intel iGPU / unknown)
→ `device_vram_mb = None` → auto disabled.

**Wiring.** `generate` (`commands.rs`) computes `(offload, auto_engaged)` before
building `EngineOptions { low_vram: offload }`. When `auto_engaged`, emit a
one-shot Tauri event (e.g. `gen-notice` with a message payload) before/at spawn.

**Frontend notice.** Listen for the event and show *"Low-VRAM auto-enabled —
model is larger than this GPU's VRAM (slower)."* near the Generate bar, mirroring
the existing CPU-fallback `role="status"` notice pattern. The notice is transient
per run; it does not change the persisted toggle.

**Tests.**
- Rust: `resolve_low_vram` truth table — manual-on → (true,false); CPU → (false,false);
  fits → (false,false); won't-fit GPU → (true,true); unknown VRAM → (false,false);
  unknown weights → (false,false).
- Rust: weight-summing helper adds all present component files and returns `None`
  if any `stat` fails.

---

## Item 5 — "Use recommended settings" button

**Behavior (chosen).** Opt-in only. A button applies the model's family
recommended generation settings to the current request. No auto-apply on model
switch (rejected to avoid clobbering tuned params).

**Per-family presets.** A pure function keyed by family (and, for `flux1`, the
diffusion filename):

```rust
pub struct GenDefaults { pub steps: u32, pub cfg_scale: f32, pub sampler: Sampler,
                         pub width: u32, pub height: u32 }

/// Recommended generation settings for a model. `diffusion_filename` lets flux1
/// distinguish schnell (few steps) from dev/krea.
pub fn family_defaults(family: &str, diffusion_filename: Option<&str>) -> Option<GenDefaults>;
```

Proposed starting values (tunable at review / in code — these are starting
points, not engine-enforced):

| family | steps | cfg | sampler | size |
|---|---|---|---|---|
| `flux1` (schnell*) | 4 | 1.0 | euler | 1024×1024 |
| `flux1` (dev/krea) | 20 | 1.0 | euler | 1024×1024 |
| `flux2` | 4 | 1.0 | euler | 1024×1024 |
| `sd3` | 28 | 4.5 | euler | 1024×1024 |
| `qwen-image` | 20 | 2.5 | euler | 1024×1024 |
| single-file SD1.5 | 20 | 7.0 | euler_a | 512×512 |
| single-file SDXL | 28 | 7.0 | euler_a | 1024×1024 |
| `custom` / none | — | — | — | — (button hidden) |

\* schnell detected by case-insensitive `"schnell"` substring in the diffusion
filename.

**Surface.** The table lives in Rust (single source of truth, unit-tested) and is
exposed by a new command `recommended_settings(model_ref) -> Option<GenDefaults>`
(it needs the diffusion filename, which the backend already has via the resolved
`ModelRef`). A **"Use recommended settings for this model"** button in
`ParamsPanel` calls it and applies the result to the request store. The button is
hidden when the call returns `None` (e.g. `custom` family, or no model selected).

**Tests.**
- Rust: `family_defaults("flux1", Some("flux1-schnell-Q4_K_S.gguf"))` → 4 steps;
  `family_defaults("flux1", Some("flux1-dev.safetensors"))` → 20 steps; `sd3`,
  `qwen-image`, `flux2` return their rows; `custom`/unknown → `None`.

---

## Data Flow Summary

```
detect_folder ── uses MODEL_EXTS ──▶ recipe.detect() ──▶ slots (gguf allowed)   [Item 1]

save/download/delete definition ──▶ state.config.model_definitions (authoritative)
set_settings(payload) ──▶ overwrite payload.{model_definitions,last_request}
                          with authoritative ──▶ save                            [Item 2]

Generate:
  weights_mb = Σ stat(component files)
  device_vram_mb = sysmon provider for cfg.gpu_device
  (offload, auto) = resolve_low_vram(cfg.low_vram, weights_mb, vram, is_cpu)
  EngineOptions{ low_vram: offload };  if auto -> emit gen-notice                 [Item 4]

Params panel "Use recommended" ──▶ family_defaults(family, diffusion_name)
                                   ──▶ apply to request store                     [Item 5]
```

## Testing Strategy

- Pure-function unit tests: `resolve_low_vram` truth table, weight-sum helper,
  `family_defaults` per family, gguf detection.
- Regression test: `set_settings` cannot erase `model_definitions` / clobber
  `last_request`.
- `cargo test` green; `npm run check` (svelte-check) 0/0.
- Manual E2E on the RTX 3060 box: point at the gguf FLUX folder (auto-detects),
  generate (auto Low-VRAM notice appears, image renders), click "Use recommended"
  (params become 4/1.0/euler), toggle a setting and confirm the model list
  survives.

## Risks / Open Questions

- **flux1 schnell-vs-dev heuristic** relies on the filename containing
  `"schnell"`. Acceptable per design discussion; revisit if it misfires.
- **Intel iGPU VRAM** is not readable via the current providers, so auto
  Low-VRAM won't engage there — the manual toggle remains the escape hatch. In
  scope only as graceful degradation, not a new provider (that's monitor work).


