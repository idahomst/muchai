# CPU Fallback — Design Spec

**Date:** 2026-06-30
**Status:** Approved (design)
**Roadmap item:** Remaining-work order #1 — "CPU fallback for image generation when no usable GPU / Vulkan device is available."

## Goal

Let image generation run on the CPU, both:

1. **Automatically** — when no usable GPU/Vulkan device is detected, a generation
   falls back to the CPU instead of failing.
2. **Manually** — the user can explicitly select "CPU" in the device picker even
   when a GPU is present.

CPU generation is dramatically slower (an SDXL image can take minutes), so the
choice is labelled up front in the picker and a runtime notice appears while a
CPU generation is in progress.

## Core Insight

The bundled Vulkan stable-diffusion.cpp engine **already** has a CPU backend.
`fixtures/sd-help.txt:58` documents `--backend <string>` accepting `cpu` (e.g.
`cpu` or `clip=cpu,vae=cuda0,diffusion=vulkan0`), and `-t/--threads` defaults to
`-1` → the physical-core count. So "run on CPU" is simply passing
`--backend cpu` instead of `--backend vulkanN`. **No engine change, no threads
UI** (thread tuning is a non-goal).

This slots into the existing device-selection machinery. Today
`gpu_device: Option<GpuSelection { index, name }>` is validated against the
enumerated device list and mapped to `--backend vulkanN` (or omitted for the
engine default) at `commands.rs:127`. We add CPU as a synthetic device in that
same list and extend the mapping — no new persisted config field, no migration.

## Architecture

### Data model — CPU as a synthetic device (`src-tauri/src/devices.rs`)

`DeviceKind::Cpu` already exists (`types.rs`). Add:

```rust
/// Reserved index for the synthetic CPU device. u32::MAX never collides with a
/// real Vulkan device index (those are 0..N), so a persisted CPU selection
/// validates by (index, name) exactly like any GPU.
pub const CPU_DEVICE_INDEX: u32 = u32::MAX;

pub fn cpu_device() -> GpuDevice {
    GpuDevice {
        index: CPU_DEVICE_INDEX,
        name: "CPU".into(),
        kind: DeviceKind::Cpu,
    }
}
```

`enumerate()` appends `cpu_device()` to the parsed device list on its normal
probe-completion path (the final `parse_vulkan_devices(&captured)` return), so:

- Engine binary missing, or a pathological spawn/pipe failure → returns `[]`
  (unchanged; no engine ⇒ no devices, and generation errors earlier with
  "engine not found").
- Engine present and the probe completes, with any number of Vulkan devices
  (including zero) → the returned list always ends with the synthetic CPU device.

Because the cached `gpu_devices` list is the single source of truth shared by
`list_gpu_devices` (the picker) and `generate`, both see CPU consistently.

The synthetic device's `name` is exactly `"CPU"` (stable for validation and
persistence); the "(slow)" suffix is a cosmetic UI label only (see Frontend).

### Backend resolution — new pure function (`src-tauri/src/devices.rs`)

```rust
/// Map a (possibly stale) saved selection + the enumerated device list to the
/// engine `--backend` value. `None` means omit `--backend` (engine default).
pub fn resolve_backend(selection: Option<GpuSelection>, devices: &[GpuDevice]) -> Option<String> {
    if let Some(sel) = validate_gpu_selection(selection, devices) {
        let kind = devices
            .iter()
            .find(|d| d.index == sel.index && d.name == sel.name)
            .map(|d| d.kind);
        return match kind {
            Some(DeviceKind::Cpu) => Some("cpu".into()),
            _ => Some(format!("vulkan{}", sel.index)),
        };
    }
    // No valid selection: auto-fall back to CPU only when there is no real GPU.
    let has_real_gpu = devices.iter().any(|d| d.kind != DeviceKind::Cpu);
    if has_real_gpu {
        None // engine default (vulkan0) — unchanged behaviour
    } else {
        Some("cpu".into()) // auto-fallback
    }
}
```

`validate_gpu_selection` is unchanged and reused. This function replaces the
inline `validate_gpu_selection(...).map(|s| format!("vulkan{}", s.index))` at
`commands.rs:127` — the call site becomes
`crate::devices::resolve_backend(cfg.gpu_device.clone(), &devices)`.

### Frontend — picker label (`src/lib/components/DevicePicker.svelte`)

- `label(d)`: when `d.kind === "cpu"`, return `"CPU (slow)"`; otherwise unchanged
  (`GPU ${d.index} — ${d.name} (${d.kind})`).
- Rename the header text `GPU device` → `Device` (CPU is not a GPU).
- The `{#each}` key stays `d.index` (the sentinel is unique). Selecting the CPU
  option persists `gpu_device = { index: 4294967295, name: "CPU" }` through the
  existing `onchange`/`setSettings` flow — no new persistence code.
- The existing `default`/stale handling is unchanged: "Default (engine picks)"
  remains the first option, and a stale saved selection still shows the
  "Saved device unavailable" note and resolves via `resolve_backend`'s no-valid
  -selection branch.

### Frontend — runtime notice (`src/lib/components/GenerateBar.svelte`)

Add a derived predicate mirroring `resolve_backend` (kept trivially small so it
cannot drift from the Rust rule):

```ts
// True when the next/active generation will run on the CPU backend.
const willRunOnCpu = $derived.by(() => {
  const sel = $settings?.gpu_device ?? null;
  const devices = $gpuDevices;
  const match = sel && devices.find((d) => d.index === sel.index && d.name === sel.name);
  if (match) return match.kind === "cpu";          // valid selection
  return !devices.some((d) => d.kind !== "cpu");    // no/stale selection → CPU iff no real GPU
});
```

While a generation is running (the existing active `genStatus` state) **and**
`willRunOnCpu` is true, render an inline notice:
**"Running on CPU — this will be much slower."**

The picker's `(slow)` label is the up-front half of the chosen "label + runtime
notice" treatment; this is the runtime half.

## Data Flow

1. App calls `list_gpu_devices` → cached enumerated list, now ending with the
   synthetic `CPU` device (whenever the engine exists).
2. `DevicePicker` lists `Default`, each GPU, and `CPU (slow)`. The user may pick
   CPU; the choice persists like any device.
3. On `generate`, `resolve_backend(cfg.gpu_device, &devices)` yields:
   - `"cpu"` if CPU is selected, **or** if there is no valid selection and no real
     GPU (auto-fallback);
   - `"vulkanN"` if a GPU is selected;
   - `None` (engine default) if no selection but a real GPU exists.
4. `build_args` appends `--backend <value>` when `Some`, omits it when `None`
   (existing behaviour, already tested).
5. If `willRunOnCpu`, `GenerateBar` shows the runtime notice during the run.

## Error Handling

- **Engine missing:** `enumerate` returns `[]`; CPU is not offered; `generate`
  errors with the existing "engine not found" message before backend logic runs.
- **CPU run fails** (e.g. RAM exhaustion): surfaces through the existing engine
  error channel (`engine::run_generation`); no special-casing.
- **Stale CPU selection:** CPU is always present when the engine exists, so a
  persisted CPU selection validates. If the engine is missing the list is empty
  and the missing-engine error path takes precedence.

## Testing

- **Rust unit tests** (`devices.rs`): `resolve_backend` —
  1. valid GPU selection → `Some("vulkan1")`
  2. valid CPU selection → `Some("cpu")`
  3. no selection, ≥1 real GPU → `None`
  4. no selection, only CPU (no real GPU) → `Some("cpu")`
  5. stale selection → treated as no selection (falls through to 3/4 by GPU
     presence)
  Plus update `enumerate_captures_stderr_from_engine` to expect the trailing
  synthetic CPU device (len 1 → 2; last device `kind == Cpu`).
  Expected total ≈ 57 → ≈ 62 tests.
- **`npm run check`** (svelte-check): 0 errors, 0 warnings. `willRunOnCpu` is a
  small derived; its rule is covered by the Rust `resolve_backend` tests.
- **Manual E2E (dev box, `npm run tauri dev`):**
  - Picker shows `CPU (slow)`; selecting it runs a (slow) generation and the
    "Running on CPU" runtime notice appears; the produced image is correct.
  - Switching back to the GPU device generates fast again, with no notice.
  - Auto-fallback: if a no-GPU environment is reachable, generation runs on CPU
    automatically with the notice; otherwise trust the unit-tested rule.

## Non-Goals (YAGNI)

- Thread-count UI (engine default `-t -1` = physical cores is fine).
- Per-component backend splits (`clip=cpu,vae=vulkan0,...`).
- Other backends (CUDA/Metal) and the tagged-enum refactor of `gpu_device`
  (Approach C) — revisit when a second non-Vulkan backend actually lands.
- Warning/guarding against large dimensions or step counts on CPU.

## Files Touched

- `src-tauri/src/devices.rs` — `CPU_DEVICE_INDEX`, `cpu_device()`, append CPU in
  `enumerate()`, new `resolve_backend()`, tests.
- `src-tauri/src/commands.rs` — `generate` uses `resolve_backend` (replaces the
  inline validate+map at ~line 127).
- `src/lib/components/DevicePicker.svelte` — CPU label, header rename.
- `src/lib/components/GenerateBar.svelte` — `willRunOnCpu` derived + runtime
  notice.

No engine binary, config schema, or `GenerationRequest` changes.
