# Cross-Vendor GPU Resource Monitor — Design

**Date:** 2026-06-30
**Status:** Approved (design); pending spec review

## Problem

The resource monitor reads GPU stats through `nvml-wrapper`, which is NVIDIA-only and
hardcoded to device index 0. On AMD or Intel hosts the GPU section shows nothing
("No NVIDIA GPU detected"); on multi-GPU hosts it always reports device 0 regardless
of which device the user selected for generation. We want the monitor to report the
correct GPU — NVIDIA, AMD, or Intel — keyed to the device the user actually selected.

## Goal

Replace the NVIDIA-only NVML backend with a cross-vendor source, and key the reported
GPU to the selected device. CPU/RAM reporting (via `sysinfo`) is unchanged.

## Decisions

- **Backend:** the [`all-smi`](https://crates.io/crates/all-smi) crate (0.15), replacing
  `nvml-wrapper`. It provides name + utilization% + memory cross-vendor (NVIDIA via NVML,
  AMD via ROCm/amdgpu, Intel via i915/xe sysfs) and Apple Silicon. `sysinfo` stays for
  CPU/RAM.
- **Dependency footprint is a risk** — `all-smi` is also a full monitoring CLI. The first
  implementation task is a **spike**: measure the `cargo tree` and release-binary-size
  delta, and check whether feature flags can trim the CLI/network/exporter parts. If the
  footprint is unacceptable and cannot be trimmed, fall back to the **Vulkan-`ash` +
  NVML hybrid** (VRAM via `VK_EXT_memory_budget` for all vendors, util% via NVML for
  NVIDIA only). The spike's result is recorded and gates the rest of the work.
- **Which device:** the monitor follows the **selected** device. Matching is by name.
  `Default (engine picks)` → first real GPU. CPU selected → GPU section hidden.
- **Display fields:** unchanged — GPU name, utilization%, VRAM used/total; CPU%; RAM.
  No power, no temperature, no multi-GPU listing.

## Architecture

GPU stats source swaps from `nvml-wrapper` to `all-smi`; the rest of the pipeline keeps
its shape. `GpuStats`'s fields are unchanged, so the Tauri event payload and the Svelte
frontend contract stay stable — this is an internal source swap plus device-keying.

### Components

- **`sysmon.rs`**
  - `gather(sys, gpu_handle, target)` — refreshes CPU/RAM via `sysinfo` as today, asks
    `all-smi` for the GPU list, **maps each entry into a `GpuStats`** (name, util%, VRAM),
    then calls `select_gpu` to pick the one matching `target`.
  - `select_gpu(gpus: &[GpuStats], target) -> Option<GpuStats>` — a **pure** function
    (no I/O) that picks the matching GPU from the already-mapped list. Operating on
    `GpuStats` (not `all-smi`'s types) keeps it unit-testable with hand-built fixtures,
    fully isolated from `all-smi`'s hardware access.
  - `target` is a small descriptor computed from the selected device:
    - `Target::None` (CPU selected, or no GPU available) → returns `None`
    - `Target::First` (Default / engine picks) → first GPU in the list
    - `Target::Name(String)` (a specific GPU selected) → first GPU whose name matches
- **`lib.rs` stats thread**
  - Initialize the `all-smi` handle once before the loop.
  - Each tick: read `state.config.gpu_device` and the `state.gpu_devices` cache via the
    captured `AppHandle` (`handle.state::<AppState>()`), compute the `Target`, call
    `gather`, and emit `system:stats` as today.
- **`ResourceMonitor.svelte`**
  - Same fields. Replace the NVIDIA-specific empty-state text with vendor-neutral
    wording (e.g. "No GPU detected" / GPU section hidden when CPU is selected).

### Data flow (per tick, ~1s)

```
AppState (config.gpu_device + gpu_devices cache)
  -> compute Target (None | First | Name)
  -> all-smi get_gpu_info()  ->  [GpuInfo...]
  -> select_gpu(list, target) -> Option<GpuStats>
  -> SystemStats { gpu, cpu_pct, ram_* }  -> emit "system:stats"
```

### Device matching

Match the selected device to an `all-smi` GPU by name, case-insensitive and tolerant
(Vulkan and NVML/sysfs names can differ slightly — substring/normalized comparison
rather than strict equality). On no match, report no GPU rather than the wrong one.
`Default` → first real GPU. CPU → `None`.

## Error handling

- `all-smi` init fails (no GPU, missing driver lib) → GPU `None`; CPU/RAM still report.
- AMD without sufficient permissions / Intel sysfs quirks → degraded or missing GPU data;
  documented. The developer's NVIDIA box is unaffected (NVML needs no sudo).
- No new bundling: `all-smi` reads host NVML/sysfs at runtime, the same model as the
  current NVML dependency. No change to the AppImage packaging.

## Testing

- **Rust unit tests** on the pure `select_gpu`:
  - `Target::None` → `None`
  - `Target::Name` matches the right GPU in a multi-entry list
  - `Target::Name` with no match → `None`
  - `Target::First` → first entry; empty list → `None`
  - existing CPU/RAM test (`gather` with no GPU) still passes
- **Spike task** records `cargo tree` depth and release-binary-size delta, and the
  go/no-go decision on `all-smi` vs. the Vulkan+NVML fallback.
- `npm run check` (svelte-check) for the frontend wording change.
- **E2E** on the developer's box: select the NVIDIA GPU → NVIDIA stats; select the Intel
  iGPU → Intel stats (or documented degraded); select CPU → GPU row hidden.

## Scope / YAGNI

No power/temperature, no multi-GPU listing, no per-process breakdown, no config changes.
CPU/RAM via `sysinfo` unchanged. `GpuStats` fields unchanged.
