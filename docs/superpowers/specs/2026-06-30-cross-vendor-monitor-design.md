# Cross-Vendor GPU Resource Monitor — Design

**Date:** 2026-06-30
**Status:** Approved; backend revised after spike (see Spike Outcome)

## Problem

The resource monitor reads GPU stats through `nvml-wrapper`, which is NVIDIA-only and
hardcoded to device index 0. On AMD or Intel hosts the GPU section shows nothing
("No NVIDIA GPU detected"); on multi-GPU hosts it always reports device 0 regardless
of which device the user selected for generation. We want the monitor to report the
correct GPU — NVIDIA, AMD, or Intel — keyed to the device the user actually selected.

## Goal

Replace the NVIDIA-only path with a cross-vendor source, and key the reported GPU to
the selected device. CPU/RAM reporting (via `sysinfo`) is unchanged.

## Spike Outcome (gated the backend choice)

The originally-approved backend was the `all-smi` crate, gated on a footprint spike.
The spike inspected `all-smi` 0.15.0's source and **rejected it**:

- **Footprint:** the library is the whole CLI exposed as modules; its dependency tree
  pulls in `axum`/`hyper`/`tower-http` (HTTP server), `tonic`/`prost`/`tonic-prost-build`
  (gRPC + protobuf — makes `protoc` a build requirement), `reqwest`, `tokio`, `clap`,
  `crossterm`, plus exotic NPU backends. The only Cargo feature is `mock`; none of it
  can be trimmed.
- **NVIDIA correctness:** its NVIDIA reader calls plain `Nvml::init()` — the exact call
  that fails on the dev box (only `libnvidia-ml.so.1` present, no unversioned symlink),
  falling back to spawning `nvidia-smi` every tick.

**Revised decision (chosen): a per-vendor "sysfs + NVML" hybrid.** No heavy crate; reuse
the existing `nvml-wrapper` for NVIDIA (keeping the `libnvidia-ml.so.1` init fix), and
read AMD/Intel stats from Linux `sysfs` directly (no new dependency, no root).

## E2E Revision (post-implementation): explicit default device

The original design assumed `Default (engine picks)` = the engine runs on "the first
Vulkan device" (banner index 0). **E2E falsified this:** on a discrete+iGPU box
(index 0 = Intel iGPU, index 1 = NVIDIA), omitting `--backend` made the engine generate
on the *NVIDIA* (index 1). ggml-vulkan's default device is loader/ICD-dependent, not the
banner-index-0 device, and we can't reliably predict it from outside.

**Fix (chosen): stop relying on the engine's opaque default.** A shared
`pick_default_device(devices)` (prefer discrete > integrated > other non-CPU) is used by
*both* the generation path (`resolve_backend` now emits `--backend vulkan{index}` for the
default device instead of omitting the flag) and the monitor (`resolve_target`). Generation
and the monitor therefore always agree on a known device, and "Default" is deterministic.

## Decisions

- **Backend:** per-vendor providers feeding one unified `Vec<GpuStats>`:
  - **NVIDIA** — `nvml-wrapper` (kept), enumerating *all* NVIDIA devices (not just index 0),
    reusing the thread-level `libnvidia-ml.so.1` init fix. Gives name, util%, VRAM.
  - **AMD** (Linux) — `sysfs` `/sys/class/drm/card*/device/`: `gpu_busy_percent` (util%),
    `mem_info_vram_used` / `mem_info_vram_total` (bytes). No dependency, no root.
  - **Intel** (Linux) — best-effort `sysfs`: discrete Intel (Arc, `xe`/`i915`) may expose
    `mem_info_vram_*`; integrated GPUs share system RAM and expose neither `gpu_busy_percent`
    nor `mem_info_vram_*` in sysfs (those are `amdgpu`-specific attributes). Where a
    field is unavailable it is simply not reported (no fake zeros).
- **Which device:** the monitor follows the **selected** device, matched by name.
  For `Default`, fridAI picks the default device *explicitly* (see "Default device"
  below) and the monitor keys to that same device. CPU selected → GPU section hidden.
  A GPU with no obtainable live stats (e.g. an Intel iGPU) → GPU section shows the
  vendor-neutral empty state, same as no-match.
- **Display fields:** unchanged — GPU name, utilization%, VRAM used/total; CPU%; RAM.
  No power, no temperature, no multi-GPU listing.
- **Platform:** `sysfs` providers are `#[cfg(target_os = "linux")]` (fridAI ships as a
  Linux AppImage). On non-Linux, only the NVML provider compiles in.

## Architecture

GPU stats come from a small set of **providers**, each returning the GPUs it can see as a
`Vec<GpuStats>`. `gather()` concatenates them into one list, then a pure `select_gpu`
picks the entry matching the selected-device `Target`. `GpuStats`'s fields are unchanged,
so the Tauri event payload and the Svelte frontend contract stay stable.

### Components

- **`sysmon.rs`** (orchestration + pure helpers)
  - `gather(sys, providers, target) -> SystemStats` — refreshes CPU/RAM via `sysinfo` as
    today, collects `Vec<GpuStats>` from each provider, then `select_gpu(&all, target)`.
  - `select_gpu(gpus: &[GpuStats], target: &Target) -> Option<GpuStats>` — **pure**, no
    I/O. Unit-tested with hand-built `GpuStats` fixtures.
  - `resolve_target(selection: Option<GpuSelection>, devices: &[GpuDevice]) -> Target` —
    **pure**. Mirrors the generation rule exactly: a valid CPU selection, or no real GPU
    present → `Target::None`; a valid GPU selection → `Target::Name(device.name)`; no/stale
    selection with a real GPU present → `Target::Name(pick_default_device(devices).name)`,
    the *same* device the generation path (`resolve_backend`) targets, so the monitor never
    diverges from where generation actually runs.
  - `pick_default_device(devices) -> Option<&GpuDevice>` (in `devices.rs`, shared with
    `resolve_backend`) — **pure**. Picks the default device when there's no valid selection:
    prefer discrete, then integrated, then any other non-CPU device; `None` if only CPU.
  - `Target` enum: `None` | `Name(String)`.
  - `name_matches(candidate: &str, target: &str) -> bool` — **pure**, case-insensitive,
    normalized (lowercase, collapse whitespace), matches when either name contains the
    other. Tolerates Vulkan-vs-NVML/sysfs naming differences.
- **`sysmon/providers.rs`** (I/O, thin)
  - `trait GpuProvider { fn probe(&self) -> Vec<GpuStats>; }`
  - `NvmlProvider { nvml: Option<Nvml> }` — enumerates all NVIDIA devices via NVML.
  - `AmdSysfsProvider { root: PathBuf }` (Linux) — scans `<root>/class/drm/card*/device/`,
    reading each card's PCI `vendor` id; `0x1002` (AMD) cards report `gpu_busy_percent`
    + `mem_info_vram_*`. `root` is injectable (defaults to `/sys`) so tests run against a
    fixture tree.
  - `IntelSysfsProvider { root: PathBuf }` (Linux) — same pattern for `0x8086` (Intel);
    reports `mem_info_vram_*` where present, omits util%.
  - Each provider names its GPUs by vendor keyword ("AMD"/"Intel") so the tolerant
    `name_matches` links them to the selected device's Vulkan name.
- **`lib.rs` stats thread**
  - Build the provider list once (NVML init with the `libnvidia-ml.so.1` fix, as today).
  - Each tick: read `state.config.gpu_device` + the `state.gpu_devices` cache via the
    captured `AppHandle` (`handle.state::<AppState>()`), `resolve_target(...)`, `gather`,
    emit `system:stats`.
- **`ResourceMonitor.svelte`**
  - Same fields. Replace "No NVIDIA GPU detected" with vendor-neutral "No GPU stats".

### Data flow (per tick, ~1s)

```
AppState (config.gpu_device + gpu_devices cache)
  -> resolve_target(selection, devices) -> Target (None | Name)
  -> each provider.probe() -> Vec<GpuStats>  (concatenated)
  -> select_gpu(all, &target) -> Option<GpuStats>
  -> SystemStats { gpu, cpu_pct, ram_* }  -> emit "system:stats"
```

### Device matching

`select_gpu` uses `name_matches` (case-insensitive, normalized, either-contains-other) so
a Vulkan name ("Intel(R) UHD Graphics 770 (ADL-S GT1)") matches a vendor-keyword provider
name ("Intel"). For a `Target::Name` match the returned `GpuStats.name` is **overwritten
with the selected device's display name**, so the monitor shows the familiar Vulkan name
while the live numbers come from the provider. Empty list → `None`. On no match, report
no GPU rather than the wrong one. Multiple GPUs
of the same vendor are matched by name; if names are indistinguishable the first is used
(documented limitation).

## Error handling

- NVML init fails (no NVIDIA, missing lib) → `NvmlProvider` returns empty; CPU/RAM still
  report. The `libnvidia-ml.so.1` fix is preserved so the dev box keeps working.
- sysfs paths absent or unreadable (non-AMD/Intel host, permission) → that provider
  returns empty; never panics. Per-file reads use `Option` (a missing `gpu_busy_percent`
  just omits util%).
- Selected GPU has no live stats (Intel iGPU) → `select_gpu` yields `None` → vendor-neutral
  empty state. No fake zeros.
- No new bundling: providers read host NVML/sysfs at runtime. No AppImage packaging change.

## Testing

- **Rust unit tests** (pure functions, no hardware):
  - `resolve_target`: CPU selection → `None`; valid GPU selection → `Name(that name)`; no
    selection with a GPU present → `Name(pick_default_device)` (the discrete GPU when one
    exists); no selection, CPU-only → `None`; stale selection follows the same rule.
  - `pick_default_device` / `resolve_backend`: discrete preferred over integrated; no
    selection with a GPU → `vulkan{index}` of the default device; CPU-only → `cpu`.
  - `select_gpu`: `None` → `None`; `Name` matches the right entry in a multi-entry list and
    overwrites the returned name with the target; `Name` with no match → `None`; empty
    list → `None`.
  - `name_matches`: exact, case/whitespace-insensitive, substring either direction,
    non-match.
  - existing CPU/RAM test (`gather` with no providers / empty list) still passes.
- **sysfs provider tests** against a fixture `<tmp>/class/drm/card0/device/` tree with
  `vendor` (PCI id), `gpu_busy_percent`, `mem_info_vram_used`, `mem_info_vram_total`:
  AMD card (`0x1002`) parsed to `GpuStats`; missing `gpu_busy_percent` omits util;
  non-AMD vendor id skipped by the AMD provider.
- `npm run check` (svelte-check) for the frontend wording change.
- **E2E** on the dev box: select the NVIDIA GPU → NVIDIA stats; select the Intel iGPU →
  vendor-neutral empty state (documented limitation); select CPU → GPU row hidden;
  Default → the discrete GPU (NVIDIA on this box) is used for generation *and* shown in
  the monitor (re-verify after the explicit-default fix above).

## Scope / YAGNI

No power/temperature, no multi-GPU listing, no per-process breakdown, no config changes.
CPU/RAM via `sysinfo` unchanged. `GpuStats` fields unchanged. Intel iGPU utilization is
out of scope: it isn't in sysfs (`gpu_busy_percent` is `amdgpu`-specific), and the
i915/xe paths that *do* expose it — DRM fdinfo (per-PID engine-busy aggregation) and the
i915 PMU — would each need a new stateful provider. fdinfo is readable by a regular user
(it's how `nvtop` shows Intel), so this is a scope decision, not a permissions limit.
