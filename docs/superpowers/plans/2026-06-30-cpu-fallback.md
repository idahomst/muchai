# CPU Fallback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let image generation run on the CPU — automatically when no usable GPU/Vulkan device exists, and manually via a "CPU" entry in the device picker — with a clear "(slow)" label and a runtime notice.

**Architecture:** Add a synthetic CPU device (`index = u32::MAX`, name `"CPU"`, `kind = Cpu`) to the enumerated device list, and a pure `resolve_backend()` that maps the saved selection + device list to the engine `--backend` value (`cpu` / `vulkanN` / engine-default). The frontend labels CPU and shows a runtime notice while a CPU generation runs. No engine, config-schema, or `GenerationRequest` changes.

**Tech Stack:** Rust (Tauri v2 backend, `cargo test --lib`), SvelteKit + Svelte 5 frontend (`npm run check` / svelte-check). Engine: stable-diffusion.cpp Vulkan build (already supports `--backend cpu`).

**Spec:** `docs/superpowers/specs/2026-06-30-cpu-fallback-design.md`

---

## File Structure

- `src-tauri/src/devices.rs` — add `CPU_DEVICE_INDEX`, `cpu_device()`, append CPU in `enumerate()`, add pure `resolve_backend()`, and their unit tests. (Owns device enumeration + backend mapping.)
- `src-tauri/src/commands.rs` — `generate` calls `resolve_backend` instead of the inline validate+`format!("vulkan{}")`. (Wiring only.)
- `src/lib/components/DevicePicker.svelte` — CPU label + header rename. (Device selection UI.)
- `src/lib/components/GenerateBar.svelte` — `willRunOnCpu` reactive + runtime notice. (Generation status UI.)

**Note on frontend testing:** this repo has no JS test runner; the frontend gate is `npm run check` (svelte-check, must report 0 errors / 0 warnings). The `willRunOnCpu` rule is identical to `resolve_backend`, which IS unit-tested in Rust. Frontend tasks therefore verify via svelte-check, not a failing test first.

**Note on Svelte syntax:** `DevicePicker.svelte` uses Svelte 5 runes (`$state`/`$derived`); `GenerateBar.svelte` uses legacy reactive syntax (`$:`, `on:click`). Match each file's existing idiom — do NOT convert GenerateBar to runes.

---

## Task 1: Synthetic CPU device in `enumerate()`

**Files:**
- Modify: `src-tauri/src/devices.rs` (add const + `cpu_device()`; append in `enumerate()` at the `parse_vulkan_devices(&captured)` return, currently `devices.rs:72`; update the existing `enumerate_captures_stderr_from_engine` test).

- [ ] **Step 1: Update the existing enumerate test to expect the appended CPU device**

In `src-tauri/src/devices.rs`, replace the existing test:

```rust
    #[test]
    fn enumerate_captures_stderr_from_engine() {
        // Fake engine ignores args and prints the device banner to stderr.
        let script = "#!/bin/sh\n>&2 echo 'ggml_vulkan: Found 1 Vulkan devices:'\n>&2 echo 'ggml_vulkan: 0 = NVIDIA GeForce RTX 3060 (NVIDIA) | uma: 0 | fp16: 1'\nexit 1\n";
        let bin = write_fake_engine(script);
        let devices = enumerate(&bin);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].kind, DeviceKind::Discrete);
        let _ = std::fs::remove_dir_all(bin.parent().unwrap());
    }
```

with:

```rust
    #[test]
    fn enumerate_appends_synthetic_cpu_after_probed_devices() {
        // Fake engine ignores args and prints the device banner to stderr.
        let script = "#!/bin/sh\n>&2 echo 'ggml_vulkan: Found 1 Vulkan devices:'\n>&2 echo 'ggml_vulkan: 0 = NVIDIA GeForce RTX 3060 (NVIDIA) | uma: 0 | fp16: 1'\nexit 1\n";
        let bin = write_fake_engine(script);
        let devices = enumerate(&bin);
        // Probed GPU first, synthetic CPU appended last.
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].kind, DeviceKind::Discrete);
        assert_eq!(devices[1], cpu_device());
        assert_eq!(devices[1].kind, DeviceKind::Cpu);
        let _ = std::fs::remove_dir_all(bin.parent().unwrap());
    }
```

- [ ] **Step 2: Run the test to verify it fails (does not compile)**

Run: `cd src-tauri && cargo test --lib enumerate_appends_synthetic_cpu_after_probed_devices`
Expected: FAIL — compile error `cannot find function cpu_device in this scope`.

- [ ] **Step 3: Add the constant, constructor, and the append in `enumerate()`**

In `src-tauri/src/devices.rs`, add near the top of the file (after the `use` lines, before `parse_vulkan_devices`):

```rust
/// Reserved index for the synthetic CPU device. `u32::MAX` never collides with a
/// real Vulkan device index (those are `0..N`), so a persisted CPU selection
/// validates by `(index, name)` exactly like any GPU device.
pub const CPU_DEVICE_INDEX: u32 = u32::MAX;

/// The synthetic "CPU" device. The engine always has a CPU backend, so this is
/// offered whenever the engine binary exists (appended by `enumerate`). The
/// `name` is exactly "CPU" for stable validation/persistence; the UI adds the
/// "(slow)" suffix cosmetically.
pub fn cpu_device() -> GpuDevice {
    GpuDevice {
        index: CPU_DEVICE_INDEX,
        name: "CPU".into(),
        kind: DeviceKind::Cpu,
    }
}
```

Then change the final line of `enumerate()` (currently `parse_vulkan_devices(&captured)`):

```rust
    let captured = rx.recv_timeout(Duration::from_secs(15)).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    let mut devices = parse_vulkan_devices(&captured);
    devices.push(cpu_device());
    devices
```

(The early `return Vec::new()` paths for a missing binary / spawn failure are left
unchanged: no engine ⇒ no devices.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib enumerate_appends_synthetic_cpu_after_probed_devices`
Expected: PASS.

Also confirm the missing-binary test still passes:
Run: `cd src-tauri && cargo test --lib enumerate_missing_binary_yields_empty`
Expected: PASS (empty list — CPU not appended without an engine).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/devices.rs
git commit -m "feat(cpu-fallback): synthetic CPU device in enumerate

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `resolve_backend()` pure mapping

**Files:**
- Modify: `src-tauri/src/devices.rs` (add `resolve_backend()` after `validate_gpu_selection`; add unit tests in the `tests` module — the `dev()` helper and `GpuSelection`/`cpu_device`/`CPU_DEVICE_INDEX` are already in scope there).

- [ ] **Step 1: Write the failing tests**

In `src-tauri/src/devices.rs`, add these tests to the `#[cfg(test)] mod tests` block (after the existing `none_stays_none` test):

```rust
    #[test]
    fn resolve_backend_valid_gpu_selection_maps_to_vulkan_index() {
        let devs = vec![dev(0, "Intel"), dev(1, "NVIDIA GeForce RTX 3060"), cpu_device()];
        let sel = Some(GpuSelection { index: 1, name: "NVIDIA GeForce RTX 3060".into() });
        assert_eq!(resolve_backend(sel, &devs), Some("vulkan1".to_string()));
    }

    #[test]
    fn resolve_backend_cpu_selection_maps_to_cpu() {
        let devs = vec![dev(0, "Intel"), cpu_device()];
        let sel = Some(GpuSelection { index: CPU_DEVICE_INDEX, name: "CPU".into() });
        assert_eq!(resolve_backend(sel, &devs), Some("cpu".to_string()));
    }

    #[test]
    fn resolve_backend_no_selection_with_gpu_is_engine_default() {
        let devs = vec![dev(0, "Intel"), cpu_device()];
        assert_eq!(resolve_backend(None, &devs), None);
    }

    #[test]
    fn resolve_backend_no_selection_without_gpu_falls_back_to_cpu() {
        let devs = vec![cpu_device()];
        assert_eq!(resolve_backend(None, &devs), Some("cpu".to_string()));
    }

    #[test]
    fn resolve_backend_stale_selection_uses_gpu_presence_rule() {
        // Stale selection (index 5 absent) + a real GPU present → engine default.
        let devs = vec![dev(0, "Intel"), cpu_device()];
        let stale = Some(GpuSelection { index: 5, name: "Ghost".into() });
        assert_eq!(resolve_backend(stale.clone(), &devs), None);
        // Stale selection + no real GPU → CPU fallback.
        let devs_cpu_only = vec![cpu_device()];
        assert_eq!(resolve_backend(stale, &devs_cpu_only), Some("cpu".to_string()));
    }
```

- [ ] **Step 2: Run the tests to verify they fail (do not compile)**

Run: `cd src-tauri && cargo test --lib resolve_backend`
Expected: FAIL — compile error `cannot find function resolve_backend in this scope`.

- [ ] **Step 3: Implement `resolve_backend()`**

In `src-tauri/src/devices.rs`, add this function immediately after `validate_gpu_selection`:

```rust
/// Map a (possibly stale) saved selection + the enumerated device list to the
/// engine `--backend` value. `None` means omit `--backend` (engine default,
/// i.e. the first Vulkan device). A valid CPU selection, or no valid selection
/// when there is no real GPU, yields `Some("cpu")` (the auto-fallback).
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
    if devices.iter().any(|d| d.kind != DeviceKind::Cpu) {
        None
    } else {
        Some("cpu".into())
    }
}
```

`.map(|d| d.kind)` relies on `DeviceKind: Copy`, which is already derived
(`src-tauri/src/types.rs:107` — `#[derive(Debug, Clone, Copy, PartialEq, Eq, ...)]`).
No new derive needed.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test --lib resolve_backend`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/devices.rs
git commit -m "feat(cpu-fallback): resolve_backend maps selection to cpu/vulkan/default

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Wire `resolve_backend` into `generate`

**Files:**
- Modify: `src-tauri/src/commands.rs` (the `backend` block inside `generate`, currently around `commands.rs:127-136`).

- [ ] **Step 1: Replace the inline backend mapping**

In `src-tauri/src/commands.rs`, find this block inside `generate`:

```rust
    let backend = {
        // Enumerate on demand (and cache) if the picker never warmed the list,
        // so a stored selection is always validated before mapping to a backend.
        let mut guard = state.gpu_devices.lock().unwrap();
        let devices = guard
            .get_or_insert_with(|| crate::devices::enumerate(&binary))
            .clone();
        crate::devices::validate_gpu_selection(cfg.gpu_device.clone(), &devices)
            .map(|s| format!("vulkan{}", s.index))
    };
```

and replace its final statement so the block reads:

```rust
    let backend = {
        // Enumerate on demand (and cache) if the picker never warmed the list,
        // so a stored selection is always validated before mapping to a backend.
        // resolve_backend also handles the no-GPU auto-fallback to CPU and an
        // explicit CPU selection.
        let mut guard = state.gpu_devices.lock().unwrap();
        let devices = guard
            .get_or_insert_with(|| crate::devices::enumerate(&binary))
            .clone();
        crate::devices::resolve_backend(cfg.gpu_device.clone(), &devices)
    };
```

(The downstream `let backend_owned = backend;` and `backend_owned.as_deref()`
passed to `engine::run_generation` are unchanged — `backend` is still
`Option<String>`.)

- [ ] **Step 2: Build and run the full backend suite**

Run: `cd src-tauri && cargo test --lib`
Expected: PASS — all tests (≈62), including the Task 1/2 additions. No warnings about an unused `validate_gpu_selection` (it is still used inside `resolve_backend`).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(cpu-fallback): generate uses resolve_backend for device mapping

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Picker label for CPU

**Files:**
- Modify: `src/lib/components/DevicePicker.svelte` (the `label()` function and the header `<span class="lbl">`).

- [ ] **Step 1: Special-case the CPU label**

In `src/lib/components/DevicePicker.svelte`, replace:

```ts
  function label(d: GpuDevice): string {
    return `GPU ${d.index} — ${d.name} (${d.kind})`;
  }
```

with:

```ts
  function label(d: GpuDevice): string {
    if (d.kind === "cpu") return "CPU (slow)";
    return `GPU ${d.index} — ${d.name} (${d.kind})`;
  }
```

- [ ] **Step 2: Rename the header (CPU is not a GPU)**

In the same file, change:

```svelte
    <span class="lbl">GPU device</span>
```

to:

```svelte
    <span class="lbl">Device</span>
```

- [ ] **Step 3: Verify svelte-check is clean**

Run: `npm run check`
Expected: `0 ERRORS 0 WARNINGS`.

- [ ] **Step 4: Commit**

```bash
git add src/lib/components/DevicePicker.svelte
git commit -m "feat(cpu-fallback): label CPU device as 'CPU (slow)', rename picker header

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Runtime "Running on CPU" notice

**Files:**
- Modify: `src/lib/components/GenerateBar.svelte` (add store imports, `willRunOnCpu` reactive, notice markup, CSS). This file uses **legacy** Svelte syntax (`$:`, `on:click`) — match it.

- [ ] **Step 1: Import the `settings` and `gpuDevices` stores**

In `src/lib/components/GenerateBar.svelte`, replace the import line:

```ts
  import { request, genStatus, history, currentImage, currentItem } from "../stores";
```

with:

```ts
  import { request, genStatus, history, currentImage, currentItem, settings, gpuDevices } from "../stores";
```

- [ ] **Step 2: Add the `willRunOnCpu` reactive predicate**

In the same `<script>`, add after the existing `$: pct = ...` reactive statement:

```ts
  // Mirrors src-tauri resolve_backend: the next/active run uses the CPU backend
  // when CPU is the (valid) selection, or when there is no valid selection and
  // no real GPU is present (auto-fallback).
  $: willRunOnCpu = (() => {
    const sel = $settings?.gpu_device ?? null;
    const devices = $gpuDevices;
    const match = sel ? devices.find((d) => d.index === sel.index && d.name === sel.name) : undefined;
    if (match) return match.kind === "cpu";
    return !devices.some((d) => d.kind !== "cpu");
  })();
```

- [ ] **Step 3: Render the notice while a CPU generation runs**

In the same file, after the closing `</div>` of `<div class="bar">` (and before or
after the existing error block), add:

```svelte
{#if $genStatus.kind === "running" && willRunOnCpu}
  <div class="cpu-note">Running on CPU — this will be much slower.</div>
{/if}
```

- [ ] **Step 4: Add the notice CSS**

In the `<style>` block of the same file, add:

```css
  .cpu-note { margin-top:.5rem; padding:.4rem .5rem; border-radius:6px;
    background:rgba(255,180,80,.15); color:#ffd9a8; font-size:.75rem; }
```

- [ ] **Step 5: Verify svelte-check is clean**

Run: `npm run check`
Expected: `0 ERRORS 0 WARNINGS`.

- [ ] **Step 6: Commit**

```bash
git add src/lib/components/GenerateBar.svelte
git commit -m "feat(cpu-fallback): runtime 'Running on CPU' notice in GenerateBar

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final verification (after all tasks)

- [ ] `cd src-tauri && cargo test --lib` → all ≈62 tests pass.
- [ ] `npm run check` → 0 errors, 0 warnings.
- [ ] Manual E2E (dev box, `npm run tauri dev`):
  - Picker shows `CPU (slow)`; selecting it runs a (slow) generation and the
    "Running on CPU — this will be much slower." notice appears during the run.
  - Switching back to the GPU device generates fast again, with no notice.
  - (If a no-GPU environment is reachable) generation auto-runs on CPU with the
    notice; otherwise trust the unit-tested `resolve_backend` rule.
