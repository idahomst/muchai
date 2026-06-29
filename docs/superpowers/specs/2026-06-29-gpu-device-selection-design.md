# GPU Device Selection (Linux, Vulkan) — Design Spec

**Date:** 2026-06-29
**Status:** Approved (brainstorming complete)
**Branch:** `feat/gpu-selector`

## Goal

Let the user choose which GPU fridAI generates on, on Linux, across vendors
(NVIDIA, AMD, Intel) by switching the engine to a Vulkan build and exposing a
device picker. One cohesive, independently shippable sub-project.

## Background / why

Today the bundled `sd-cli` is a **CUDA-only build** (links `libcudart`,
`libcublas`, `libcuda`). It can only see/use NVIDIA hardware and would not run
on a machine without CUDA. Device enumeration and the resource monitor both go
through **NVML**, which is NVIDIA-only. The dev machine has two GPUs — an Intel
UHD 770 iGPU and an NVIDIA RTX 3060 — and the CUDA build cannot see the Intel
one at all.

stable-diffusion.cpp publishes a **prebuilt Vulkan Linux binary**. Vulkan is a
single backend that enumerates NVIDIA + AMD + Intel uniformly. On the dev box,
`vulkaninfo --summary` already lists three devices (Intel iGPU, NVIDIA dGPU, and
an `llvmpipe` CPU software device), so a Vulkan engine makes the picker fully
testable here.

**Accepted tradeoff:** Vulkan is typically slower than CUDA on NVIDIA, so RTX
3060 generations will be somewhat slower than the current CUDA build. The user
explicitly chose Vulkan-only (one binary, one enumeration path, one `--backend`
mapping) over a dual CUDA+Vulkan bundle.

## Decomposition (this spec covers sub-project 1 only)

1. **Sub-project 1 (this spec):** Vulkan engine swap + device enumeration +
   picker + persistence + `--backend` wiring + AppImage update.
2. **Sub-project 2 (next):** Cross-vendor resource monitor. Replace NVML with
   per-device VRAM via Vulkan `VK_EXT_memory_budget` (`ash` crate), keyed to the
   same device indices as the picker, plus best-effort per-vendor utilization%
   (NVML for NVIDIA, AMD `gpu_busy_percent` sysfs, Intel later). Degrade
   gracefully: always show name + VRAM, omit util where unavailable.
   - `gfxinfo` was evaluated and rejected: it implements all vendors but only
     exposes `active_gpu()` (singular) — no multi-GPU enumeration, so it cannot
     report a *selected* secondary device. `silicon-monitor` is too immature.
3. **Sub-project 3 (later):** macOS Metal port (prebuilt
   `…Darwin-macOS-…-arm64` engine + Metal enumeration + mac packaging).

### Interim monitor behavior (during sub-project 1)

The resource monitor keeps using NVML. It therefore shows **NVIDIA stats only,
regardless of the selected device** — if the user selects the Intel GPU, the
monitor still reports the NVIDIA card (or nothing on a non-NVIDIA-primary
machine). This is a known, documented limitation resolved by sub-project 2.

## Architecture

### Engine binary swap

- Source the prebuilt `sd-master-<rev>-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip`
  from stable-diffusion.cpp releases. Extract the `sd` CLI and place it at
  `src-tauri/binaries/sd-cli` and `src-tauri/binaries/sd-cli-x86_64-unknown-linux-gnu`
  (Tauri sidecar name unchanged → no wiring changes).
- The Vulkan build links `libvulkan` (host-provided ICD loader) and does **not**
  link `libcuda`. `scripts/build-appimage.sh` drops the libcuda-stripping step;
  it must rely on the host Vulkan loader and **not** bundle a fixed `libvulkan`
  (bundling would shadow the host ICDs). Net effect: the AppImage stops being
  NVIDIA-locked.
- The CLI flags relied on (`-M img_gen`, `-m`, `-p/-n`, `--steps`, `--cfg-scale`,
  `--sampling-method`, `-W/-H`, `-s`, `-b`, `-o`, `-v`, `--backend`) are
  identical across the CUDA and Vulkan builds of this master line; verify once
  against the real Vulkan binary before wiring.

### Device enumeration — "ask the engine"

The picker's device index MUST match exactly what the engine means by
`vulkan0/1/2`. The **engine is the source of truth**.

- New Rust module `devices.rs` exposes
  `enumerate(binary: &Path) -> Vec<GpuDevice>`.
- It runs a **one-time probe** of the bundled `sd-cli` that triggers ggml-vulkan
  backend initialization and captures the device list it prints to stderr
  (`ggml_vulkan: Found N Vulkan devices:` followed by per-device lines), then
  parses each into `GpuDevice { index: u32, name: String, kind: DeviceKind }`.
- `DeviceKind` ∈ `discrete | integrated | cpu | other`. The `llvmpipe`
  CPU-software device is kept but tagged `cpu` so the UI can label/deprioritize
  it.
- **The exact probe invocation is pinned during the implementation plan** after
  testing the real Vulkan binary on the dev box (which has NVIDIA + Intel to
  validate ordering/names against the engine's own log).
- **Fallback** (only if no clean probe exists): enumerate via the `ash` Vulkan
  crate in `vkEnumeratePhysicalDevices` order. The probe is strongly preferred
  because it guarantees index parity with `--backend vulkanN`.
- Enumeration result is cached in memory for the session; a manual rescan is out
  of scope (devices don't change at runtime).

### Config & arg wiring

- `AppConfig` gains `#[serde(default)] gpu_device: Option<GpuSelection>` where
  `GpuSelection { index: u32, name: String }`. `None` = engine default
  (whatever ggml-vulkan picks, typically device 0). Backward compatible with the
  same `#[serde(default)]` pattern used by the model-library fields.
- `name` is stored alongside `index` so that on load we can validate the saved
  index still maps to the same device. If the enumerated device at that index is
  missing or has a different name (hardware/driver changed), we silently fall
  back to `None` (engine default) and surface a one-line notice in the picker.
- `command_builder::build_args` gains a parameter:
  `build_args(req, output_path, backend: Option<&str>)`. When `Some("vulkan{i}")`,
  append `--backend vulkan{i}`; when `None`, append nothing.
- `commands::generate` reads `config.gpu_device`, computes the backend token
  (`format!("vulkan{}", sel.index)`), and threads it through
  `engine::run_generation` → `build_args`.

### UI

- New `DevicePicker.svelte` in the controls sidebar near `ModelLibrary`. A
  dropdown listing each device as `GPU {index} — {name} ({kind})`, e.g.
  `GPU 0 — NVIDIA GeForce RTX 3060 (discrete)`, plus a `Default` option mapping
  to `gpu_device = None`.
- Selecting an option persists immediately via the existing `set_settings`
  command (same pattern as `ModelFolders`).
- New Tauri command `list_gpu_devices() -> Vec<GpuDevice>`, called on mount and
  used to populate the dropdown. The current selection comes from settings.
- Single-GPU machines still show the one device (no special hiding).

### Data flow

```
app mount
  └─ list_gpu_devices() ── devices.rs probe ──> Vec<GpuDevice>  (populate dropdown)
  └─ get_settings() ── gpu_device ──> current selection (validated vs device list)

user picks device
  └─ set_settings({ ...cfg, gpu_device: { index, name } })

generate
  └─ commands::generate reads cfg.gpu_device
       └─ backend = Some("vulkan{index}") | None
            └─ run_generation(..., backend)
                 └─ build_args(req, out, backend)  ──> [..., "--backend", "vulkan{index}", ...]
```

## Error handling

- **No Vulkan / no devices:** probe returns an empty list. Picker shows
  "No Vulkan devices detected"; generation proceeds with the engine default
  (no `--backend`).
- **Probe failure (non-zero/timeout/garbled output):** treated as empty list;
  logged; generation falls back to engine default. Never blocks startup.
- **Selected device gone on load:** validation fails → fall back to `None` +
  one-line notice in the picker.
- **Engine rejects `--backend vulkanN`:** surfaces through the existing
  `GenError::NonZero` stderr-tail path (no special-casing).

## Wire contract (TS mirrors Rust serde)

- `GpuDevice { index: number; name: string; kind: "discrete"|"integrated"|"cpu"|"other" }`
  (snake_case `kind` values).
- `AppConfig.gpu_device: { index: number; name: string } | null` added to
  `src/lib/types.ts`.
- `listGpuDevices(): Promise<GpuDevice[]>` added to `src/lib/api.ts`.

## Testing

- `command_builder`: `--backend vulkanN` present when `backend = Some`, absent
  when `None`; value matches the index.
- `devices.rs`: parse a captured sample of the engine's
  "Found N Vulkan devices" stderr into the expected `Vec<GpuDevice>` (including
  the `llvmpipe`→`cpu` classification); empty/garbled input → empty vec.
- `config`: round-trip with `gpu_device = Some/None`; old config without the
  field loads (backfills `None`); validation/fallback when the stored
  index/name no longer matches an enumerated device.
- Frontend `svelte-check` clean; manual E2E on the dev box: dropdown lists
  Intel + NVIDIA (+ llvmpipe), selecting NVIDIA generates, selecting Intel
  generates, selection persists across restart, `--backend` visible in engine
  args (verbose log).

## Out of scope (explicit)

- Cross-vendor resource monitoring (sub-project 2).
- macOS / Metal (sub-project 3).
- Per-component device split (`diffusion=…,vae=…`) and `--offload-to-cpu`.
- Dual CUDA+Vulkan bundling.
- Runtime device hot-plug / rescan.
</content>
</invoke>
