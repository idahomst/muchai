# Cross-Vendor GPU Resource Monitor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the resource monitor report the GPU the user actually selected — NVIDIA, AMD, or Intel — instead of always NVML device 0.

**Architecture:** Per-vendor *providers* (NVML for NVIDIA, Linux sysfs for AMD/Intel) each return a `Vec<GpuStats>`; `gather()` concatenates them and a pure `select_gpu` picks the entry matching a `Target` derived from the selected device. CPU/RAM via `sysinfo` is unchanged. `GpuStats` fields are unchanged, so the frontend contract is stable.

**Tech Stack:** Rust (Tauri v2 backend), `nvml-wrapper` (kept), Linux `sysfs` (no new dependency), `sysinfo` (kept); SvelteKit/Svelte 5 frontend.

**Spec:** `docs/superpowers/specs/2026-06-30-cross-vendor-monitor-design.md` (read for context; the spike that rejected `all-smi` is recorded there).

**Gates:** `cargo test --lib` (run from `src-tauri/`), `npm run check` (run from repo root).

---

## File Structure

- `src-tauri/src/sysmon.rs` → split into a module directory:
  - `src-tauri/src/sysmon/mod.rs` — `Target` enum, pure helpers (`resolve_target`, `name_matches`, `select_gpu`), `GpuProvider` trait, `gather`, `default_providers`.
  - `src-tauri/src/sysmon/providers.rs` — `NvmlProvider`, and (Linux) `AmdSysfsProvider` / `IntelSysfsProvider` plus the shared `read_drm_cards` helper.
- `src-tauri/src/lib.rs` — background stats thread reads the selected device from `AppState` each tick and calls the new `gather`.
- `src/lib/components/ResourceMonitor.svelte` — vendor-neutral empty-state text.

`GpuStats` / `SystemStats` in `src-tauri/src/types.rs` are **unchanged**.

---

## Task 1: Pure helpers — `Target`, `resolve_target`, `name_matches`, `select_gpu`

Adds the pure, hardware-free core to the existing single-file `sysmon.rs`. The old `gather`/`gather_gpu` stay untouched in this task (rewritten in Task 2). The new functions are unused until Task 2, so `cargo test` will print `dead_code` warnings for them — that is expected and cleared in Task 2; tests still pass.

**Files:**
- Modify: `src-tauri/src/sysmon.rs`

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod tests` block in `src-tauri/src/sysmon.rs` (which currently contains `gathers_cpu_and_ram_without_gpu`). Append these tests inside that module:

```rust
    use crate::types::{DeviceKind, GpuDevice, GpuSelection};

    fn gpu_dev(index: u32, name: &str, kind: DeviceKind) -> GpuDevice {
        GpuDevice { index, name: name.into(), kind }
    }

    fn stat(name: &str) -> GpuStats {
        GpuStats { name: name.into(), utilization_pct: 7, vram_used_mb: 100, vram_total_mb: 200 }
    }

    #[test]
    fn name_matches_is_case_and_whitespace_insensitive() {
        assert!(name_matches("NVIDIA GeForce RTX 3060", "nvidia   geforce rtx 3060"));
        assert!(name_matches("Intel", "Intel(R) UHD Graphics 770 (ADL-S GT1)")); // substring
        assert!(name_matches("AMD Radeon RX 7900", "amd")); // either direction
        assert!(!name_matches("Intel", "NVIDIA GeForce RTX 3060"));
        assert!(!name_matches("", "anything"));
    }

    #[test]
    fn resolve_target_cpu_selection_is_none() {
        let devices = vec![gpu_dev(0, "Intel", DeviceKind::Integrated), crate::devices::cpu_device()];
        let sel = Some(GpuSelection { index: crate::devices::CPU_DEVICE_INDEX, name: "CPU".into() });
        assert_eq!(resolve_target(sel, &devices), Target::None);
    }

    #[test]
    fn resolve_target_valid_gpu_selection_keys_to_its_name() {
        let devices = vec![gpu_dev(0, "Intel", DeviceKind::Integrated), gpu_dev(1, "NVIDIA GeForce RTX 3060", DeviceKind::Discrete)];
        let sel = Some(GpuSelection { index: 1, name: "NVIDIA GeForce RTX 3060".into() });
        assert_eq!(resolve_target(sel, &devices), Target::Name("NVIDIA GeForce RTX 3060".into()));
    }

    #[test]
    fn resolve_target_no_selection_keys_to_first_non_cpu_device() {
        let devices = vec![gpu_dev(0, "Intel", DeviceKind::Integrated), gpu_dev(1, "NVIDIA", DeviceKind::Discrete), crate::devices::cpu_device()];
        assert_eq!(resolve_target(None, &devices), Target::Name("Intel".into()));
    }

    #[test]
    fn resolve_target_cpu_only_is_none() {
        let devices = vec![crate::devices::cpu_device()];
        assert_eq!(resolve_target(None, &devices), Target::None);
        // empty device list (cache not populated yet) is also None
        assert_eq!(resolve_target(None, &[]), Target::None);
    }

    #[test]
    fn resolve_target_stale_selection_follows_gpu_presence() {
        let stale = Some(GpuSelection { index: 9, name: "Ghost".into() });
        let with_gpu = vec![gpu_dev(0, "Intel", DeviceKind::Integrated), crate::devices::cpu_device()];
        assert_eq!(resolve_target(stale.clone(), &with_gpu), Target::Name("Intel".into()));
        let cpu_only = vec![crate::devices::cpu_device()];
        assert_eq!(resolve_target(stale, &cpu_only), Target::None);
    }

    #[test]
    fn select_gpu_none_target_yields_none() {
        assert_eq!(select_gpu(&[stat("NVIDIA")], &Target::None), None);
    }

    #[test]
    fn select_gpu_name_matches_and_overwrites_display_name() {
        let gpus = vec![stat("Intel"), stat("NVIDIA GeForce RTX 3060")];
        let got = select_gpu(&gpus, &Target::Name("NVIDIA GeForce RTX 3060".into())).unwrap();
        assert_eq!(got.name, "NVIDIA GeForce RTX 3060");
        assert_eq!(got.vram_total_mb, 200);
        // vendor-keyword provider name still matches the full selected name
        let amd = vec![stat("AMD")];
        let got = select_gpu(&amd, &Target::Name("AMD Radeon RX 7900 XTX".into())).unwrap();
        assert_eq!(got.name, "AMD Radeon RX 7900 XTX"); // overwritten with the selected display name
    }

    #[test]
    fn select_gpu_name_no_match_or_empty_yields_none() {
        assert_eq!(select_gpu(&[stat("Intel")], &Target::Name("NVIDIA".into())), None);
        assert_eq!(select_gpu(&[], &Target::Name("NVIDIA".into())), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test --lib sysmon`
Expected: FAIL — `cannot find type Target`, `cannot find function resolve_target/name_matches/select_gpu`.

- [ ] **Step 3: Write the implementation**

At the top of `src-tauri/src/sysmon.rs`, add `GpuDevice`/`GpuSelection` to the types import and insert the new items above the existing `gather`. Replace the import line:

```rust
use crate::types::{GpuStats, SystemStats};
```

with:

```rust
use crate::types::{GpuDevice, GpuSelection, GpuStats, SystemStats};
```

Then add, just below the imports:

```rust
/// Which GPU the monitor should report, derived from the user's selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// CPU selected, or no GPU available — hide the GPU section.
    None,
    /// Report the GPU whose name matches this selected-device name.
    Name(String),
}

/// Map the saved selection + enumerated devices to a monitor `Target`, mirroring
/// the generation backend rule. A valid CPU selection (or no real GPU) hides the
/// GPU section; a valid GPU selection keys to that device's name; an absent/stale
/// selection keys to the first non-CPU device (the engine's default backend is the
/// first Vulkan device).
pub fn resolve_target(selection: Option<GpuSelection>, devices: &[GpuDevice]) -> Target {
    use crate::types::DeviceKind;
    if let Some(sel) = crate::devices::validate_gpu_selection(selection, devices) {
        let device = devices
            .iter()
            .find(|d| d.index == sel.index && d.name == sel.name)
            .expect("validate_gpu_selection guarantees the device exists");
        return match device.kind {
            DeviceKind::Cpu => Target::None,
            _ => Target::Name(device.name.clone()),
        };
    }
    match devices.iter().find(|d| d.kind != DeviceKind::Cpu) {
        Some(d) => Target::Name(d.name.clone()),
        None => Target::None,
    }
}

/// Tolerant name comparison: case-insensitive, whitespace-collapsed, and matching
/// when either name contains the other. Bridges Vulkan device names (e.g.
/// "Intel(R) UHD Graphics 770 (ADL-S GT1)") and vendor-keyword provider names
/// (e.g. "Intel").
pub fn name_matches(candidate: &str, target: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let c = norm(candidate);
    let t = norm(target);
    !c.is_empty() && !t.is_empty() && (c.contains(&t) || t.contains(&c))
}

/// Pick the GPU matching `target` from an already-gathered list. For a `Name`
/// match the returned `name` is overwritten with the selected display name, so the
/// monitor shows the familiar Vulkan name while the live numbers come from the
/// provider.
pub fn select_gpu(gpus: &[GpuStats], target: &Target) -> Option<GpuStats> {
    match target {
        Target::None => None,
        Target::Name(name) => gpus
            .iter()
            .find(|g| name_matches(&g.name, name))
            .map(|g| GpuStats { name: name.clone(), ..g.clone() }),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib sysmon`
Expected: PASS — all `sysmon` tests pass (including the pre-existing `gathers_cpu_and_ram_without_gpu`). `dead_code` warnings for the new functions are expected here and cleared in Task 2.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/sysmon.rs
git commit -m "feat(monitor): pure Target/resolve_target/select_gpu helpers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `GpuProvider` trait + `NvmlProvider`; rewrite `gather`; split into module

Converts `sysmon.rs` into a module directory, introduces the provider abstraction, moves the NVML read into `NvmlProvider` (enumerating *all* NVIDIA devices, not just index 0), and rewrites `gather` to use providers + `select_gpu`. This clears Task 1's `dead_code` warnings.

**Files:**
- Move: `src-tauri/src/sysmon.rs` → `src-tauri/src/sysmon/mod.rs`
- Create: `src-tauri/src/sysmon/providers.rs`

- [ ] **Step 1: Move the file into a module directory**

Run:

```bash
cd src-tauri/src && mkdir sysmon && git mv sysmon.rs sysmon/mod.rs && cd ../..
```

(No code change yet; `mod sysmon;` in `lib.rs` resolves to `sysmon/mod.rs` automatically.)

- [ ] **Step 2: Write the failing test for `NvmlProvider` construction + empty behavior**

In `src-tauri/src/sysmon/mod.rs`, add this test inside the `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn nvml_provider_without_handle_probes_empty() {
        let p = crate::sysmon::providers::NvmlProvider { nvml: None };
        assert!(p.probe().is_empty());
    }

    #[test]
    fn gather_with_no_providers_reports_cpu_ram_and_no_gpu() {
        let mut sys = System::new();
        let providers: Vec<Box<dyn GpuProvider>> = Vec::new();
        let stats = gather(&mut sys, &providers, &Target::None);
        assert!(stats.gpu.is_none());
        assert!(stats.ram_total_mb > 0);
        assert!(stats.ram_used_mb <= stats.ram_total_mb);
    }
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib sysmon`
Expected: FAIL — `module providers not found`, `trait GpuProvider not found`, and `gather` arity mismatch.

- [ ] **Step 4: Rewrite `sysmon/mod.rs` — declare the module, add the trait, rewrite `gather`, add `default_providers`, drop the old `gather_gpu`**

In `src-tauri/src/sysmon/mod.rs`:

Replace the import block and the old `gather`/`gather_gpu` (everything from `use nvml_wrapper::Nvml;` down through the end of `fn gather_gpu`) with:

```rust
pub mod providers;

use crate::types::{GpuDevice, GpuSelection, GpuStats, SystemStats};
use providers::NvmlProvider;
use sysinfo::System;

/// A source of live GPU stats for one vendor/backend. Providers are built and used
/// entirely inside the stats thread, so no `Send` bound is needed.
pub trait GpuProvider {
    /// Return live stats for every GPU this provider can currently see.
    fn probe(&self) -> Vec<GpuStats>;
}

/// Build the provider list for this platform. The NVML handle is built once by
/// the caller (with the `libnvidia-ml.so.1` init fix) and moved in.
pub fn default_providers(nvml: Option<nvml_wrapper::Nvml>) -> Vec<Box<dyn GpuProvider>> {
    let mut v: Vec<Box<dyn GpuProvider>> = vec![Box::new(NvmlProvider { nvml })];
    #[cfg(target_os = "linux")]
    {
        v.push(Box::new(providers::AmdSysfsProvider::new()));
        v.push(Box::new(providers::IntelSysfsProvider::new()));
    }
    v
}

/// Gather CPU/RAM via sysinfo, collect GPU stats from every provider, and pick the
/// one matching `target`.
pub fn gather(sys: &mut System, providers: &[Box<dyn GpuProvider>], target: &Target) -> SystemStats {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu_pct = sys.global_cpu_usage();
    let ram_used_mb = sys.used_memory() / 1024 / 1024; // sysinfo 0.30+ returns bytes
    let ram_total_mb = sys.total_memory() / 1024 / 1024;
    let mut all: Vec<GpuStats> = Vec::new();
    for p in providers {
        all.extend(p.probe());
    }
    let gpu = select_gpu(&all, target);
    SystemStats {
        gpu,
        cpu_pct,
        ram_used_mb,
        ram_total_mb,
    }
}
```

Keep the `Target` / `resolve_target` / `name_matches` / `select_gpu` definitions from Task 1 (they already import `GpuDevice`/`GpuSelection` via the new shared `use` line above — remove any now-duplicate `use crate::types::{...}` line that Task 1 added so only the single import block above remains).

> Note: the existing test `gathers_cpu_and_ram_without_gpu` calls `gather(&mut sys, None)` — delete that old test (it is replaced by `gather_with_no_providers_reports_cpu_ram_and_no_gpu` in Step 2).

- [ ] **Step 5: Create `src-tauri/src/sysmon/providers.rs` with `NvmlProvider`**

```rust
use super::GpuProvider;
use crate::types::GpuStats;
use nvml_wrapper::Nvml;

/// NVIDIA GPUs via NVML. Enumerates every NVML device (not just index 0). The
/// handle is `None` when NVML is unavailable (non-NVIDIA host / missing lib).
pub struct NvmlProvider {
    pub nvml: Option<Nvml>,
}

impl GpuProvider for NvmlProvider {
    fn probe(&self) -> Vec<GpuStats> {
        let Some(nvml) = &self.nvml else {
            return Vec::new();
        };
        let count = nvml.device_count().unwrap_or(0);
        (0..count)
            .filter_map(|i| {
                let device = nvml.device_by_index(i).ok()?;
                let name = device.name().ok()?;
                let util = device.utilization_rates().ok()?;
                let mem = device.memory_info().ok()?;
                Some(GpuStats {
                    name,
                    utilization_pct: util.gpu,
                    vram_used_mb: mem.used / 1024 / 1024,
                    vram_total_mb: mem.total / 1024 / 1024,
                })
            })
            .collect()
    }
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib sysmon`
Expected: PASS — all `sysmon` tests pass; no `dead_code` warnings remain for `select_gpu`/`resolve_target`/`name_matches` (now used by `gather`/`default_providers`).

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/sysmon
git commit -m "feat(monitor): GpuProvider trait + NvmlProvider; provider-based gather

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: AMD & Intel sysfs providers (Linux)

Adds dependency-free Linux sysfs providers. A shared `read_drm_cards` helper takes an injectable `root` so tests run against a fixture tree. AMD reads `gpu_busy_percent` + VRAM; Intel reads VRAM only (util omitted → 0). Cards exposing no VRAM (e.g. integrated GPUs) are skipped so they fall through to the vendor-neutral empty state.

**Files:**
- Modify: `src-tauri/src/sysmon/providers.rs`

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/src/sysmon/providers.rs`:

```rust
#[cfg(all(test, target_os = "linux"))]
mod sysfs_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Build a fake sysfs tree at <tmp>/class/drm/<card>/device with the given files.
    fn fixture(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fridai-sysfs-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dev = root.join("class/drm/card0/device");
        fs::create_dir_all(&dev).unwrap();
        for (name, body) in files {
            fs::write(dev.join(name), body).unwrap();
        }
        root
    }

    #[test]
    fn amd_card_is_parsed() {
        let root = fixture("amd", &[
            ("vendor", "0x1002\n"),
            ("gpu_busy_percent", "42\n"),
            ("mem_info_vram_used", "536870912\n"),   // 512 MiB
            ("mem_info_vram_total", "8589934592\n"), // 8192 MiB
        ]);
        let p = AmdSysfsProvider { root: root.clone() };
        let gpus = p.probe();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].name, "AMD");
        assert_eq!(gpus[0].utilization_pct, 42);
        assert_eq!(gpus[0].vram_used_mb, 512);
        assert_eq!(gpus[0].vram_total_mb, 8192);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn amd_missing_busy_defaults_util_zero() {
        let root = fixture("amd-nobusy", &[
            ("vendor", "0x1002\n"),
            ("mem_info_vram_used", "0\n"),
            ("mem_info_vram_total", "8589934592\n"),
        ]);
        let p = AmdSysfsProvider { root: root.clone() };
        let gpus = p.probe();
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].utilization_pct, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn amd_provider_skips_non_amd_vendor() {
        let root = fixture("intel-for-amd", &[
            ("vendor", "0x8086\n"),
            ("mem_info_vram_used", "0\n"),
            ("mem_info_vram_total", "8589934592\n"),
        ]);
        let p = AmdSysfsProvider { root: root.clone() };
        assert!(p.probe().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn card_without_vram_is_skipped() {
        // An integrated GPU exposes no mem_info_vram_* — nothing useful to show.
        let root = fixture("igpu", &[
            ("vendor", "0x1002\n"),
            ("gpu_busy_percent", "10\n"),
        ]);
        let p = AmdSysfsProvider { root: root.clone() };
        assert!(p.probe().is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn missing_sysfs_root_is_empty() {
        let p = AmdSysfsProvider { root: PathBuf::from("/no/such/sysfs") };
        assert!(p.probe().is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test --lib sysmon`
Expected: FAIL — `cannot find struct AmdSysfsProvider`.

- [ ] **Step 3: Implement the sysfs providers**

Append to `src-tauri/src/sysmon/providers.rs` (above the test module is fine; the `#[cfg(target_os = "linux")]` gate keeps it Linux-only):

```rust
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};

/// AMD GPUs via Linux sysfs (`/sys/class/drm/cardN/device`). Reads
/// `gpu_busy_percent` for utilization and `mem_info_vram_*` for VRAM. `root` is
/// injectable for tests (defaults to `/sys`).
#[cfg(target_os = "linux")]
pub struct AmdSysfsProvider {
    pub root: PathBuf,
}

#[cfg(target_os = "linux")]
impl AmdSysfsProvider {
    pub fn new() -> Self {
        Self { root: PathBuf::from("/sys") }
    }
}

#[cfg(target_os = "linux")]
impl GpuProvider for AmdSysfsProvider {
    fn probe(&self) -> Vec<GpuStats> {
        read_drm_cards(&self.root, "0x1002", true, "AMD")
    }
}

/// Intel GPUs via Linux sysfs. Discrete Intel (Arc) may expose `mem_info_vram_*`;
/// integrated GPUs expose neither VRAM nor a busy%, so they are skipped. Util is
/// not read (Intel utilization needs root/PMU) and reported as 0.
#[cfg(target_os = "linux")]
pub struct IntelSysfsProvider {
    pub root: PathBuf,
}

#[cfg(target_os = "linux")]
impl IntelSysfsProvider {
    pub fn new() -> Self {
        Self { root: PathBuf::from("/sys") }
    }
}

#[cfg(target_os = "linux")]
impl GpuProvider for IntelSysfsProvider {
    fn probe(&self) -> Vec<GpuStats> {
        read_drm_cards(&self.root, "0x8086", false, "Intel")
    }
}

/// Scan `<root>/class/drm/card*/device` for cards whose PCI `vendor` id equals
/// `want_vendor`, reading VRAM (required) and optionally `gpu_busy_percent`. Cards
/// without VRAM info are skipped. Never panics.
#[cfg(target_os = "linux")]
fn read_drm_cards(root: &Path, want_vendor: &str, read_busy: bool, label: &str) -> Vec<GpuStats> {
    let drm = root.join("class/drm");
    let Ok(entries) = std::fs::read_dir(&drm) else {
        return Vec::new();
    };
    let mut cards: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                // "card0" yes; "card0-DP-1" (connectors) no.
                .map(|n| n.starts_with("card") && !n.contains('-'))
                .unwrap_or(false)
        })
        .collect();
    cards.sort();

    let mut out = Vec::new();
    for card in cards {
        let dev = card.join("device");
        if read_trim(&dev.join("vendor")).as_deref() != Some(want_vendor) {
            continue;
        }
        let (Some(total), Some(used)) = (
            read_u64(&dev.join("mem_info_vram_total")),
            read_u64(&dev.join("mem_info_vram_used")),
        ) else {
            continue; // no VRAM info (e.g. integrated GPU) — nothing useful to show
        };
        let utilization_pct = if read_busy {
            read_u64(&dev.join("gpu_busy_percent")).unwrap_or(0) as u32
        } else {
            0
        };
        out.push(GpuStats {
            name: label.to_string(),
            utilization_pct,
            vram_used_mb: used / 1024 / 1024,
            vram_total_mb: total / 1024 / 1024,
        });
    }
    out
}

#[cfg(target_os = "linux")]
fn read_trim(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

#[cfg(target_os = "linux")]
fn read_u64(path: &Path) -> Option<u64> {
    read_trim(path).and_then(|s| s.parse::<u64>().ok())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test --lib sysmon`
Expected: PASS — the five `sysfs_tests` plus all earlier `sysmon` tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/sysmon/providers.rs
git commit -m "feat(monitor): AMD/Intel Linux sysfs GPU providers

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Wire the background stats thread to the selected device

Updates the `lib.rs` stats loop to build providers once and, each tick, read the selected device + cached device list from `AppState`, resolve a `Target`, and call the new `gather`. This is integration glue; there is no unit test (the loop needs the Tauri runtime), so verification is a clean build + the full lib test run, and the change is small and reviewed.

**Files:**
- Modify: `src-tauri/src/lib.rs:38-56`

- [ ] **Step 1: Replace the stats-thread body**

In `src-tauri/src/lib.rs`, replace the thread spawn block (the `let handle = app.handle().clone();` through the end of its `std::thread::spawn(move || { ... });`) with:

```rust
            // Background system-stats loop: emit "system:stats" ~every second,
            // keyed to the device the user has selected for generation.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                // Many driver installs ship only the versioned NVML
                // (libnvidia-ml.so.1) without the unversioned dev symlink that
                // Nvml::init() loads by default. Try the versioned name first,
                // then fall back to the default so both layouts work.
                let nvml = nvml_wrapper::Nvml::builder()
                    .lib_path(std::ffi::OsStr::new("libnvidia-ml.so.1"))
                    .init()
                    .or_else(|_| nvml_wrapper::Nvml::init())
                    .ok();
                let providers = sysmon::default_providers(nvml);
                let mut sys = sysinfo::System::new();
                loop {
                    // Re-read the selection each tick so changing the device in the
                    // UI re-keys the monitor without restarting the thread.
                    let target = {
                        let state = handle.state::<AppState>();
                        let selection = state.config.lock().unwrap().gpu_device.clone();
                        let devices = state.gpu_devices.lock().unwrap().clone().unwrap_or_default();
                        sysmon::resolve_target(selection, &devices)
                    };
                    let stats = sysmon::gather(&mut sys, &providers, &target);
                    let _ = handle.emit("system:stats", stats);
                    std::thread::sleep(std::time::Duration::from_millis(1000));
                }
            });
```

(`tauri::Manager` — needed for `handle.state::<AppState>()` — is already imported at `src-tauri/src/lib.rs:17` (`use tauri::{Emitter, Manager};`), and `AppState` at line 14.)

- [ ] **Step 2: Verify it builds and all lib tests pass**

Run: `cd src-tauri && cargo build && cargo test --lib`
Expected: build succeeds with no warnings; all lib tests pass (the full suite, ~70+ tests).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(monitor): key the stats thread to the selected device each tick

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Vendor-neutral empty state in the monitor UI

The only frontend change: the GPU section's empty-state text is NVIDIA-specific. Make it vendor-neutral. Fields and layout are unchanged.

**Files:**
- Modify: `src/lib/components/ResourceMonitor.svelte:18`

- [ ] **Step 1: Update the empty-state text**

In `src/lib/components/ResourceMonitor.svelte`, replace:

```svelte
      <span class="stat">No NVIDIA GPU detected</span>
```

with:

```svelte
      <span class="stat">No GPU stats</span>
```

- [ ] **Step 2: Run the frontend check**

Run: `npm run check`
Expected: PASS — svelte-check reports 0 errors, 0 warnings.

- [ ] **Step 3: Commit**

```bash
git add src/lib/components/ResourceMonitor.svelte
git commit -m "feat(monitor): vendor-neutral GPU empty-state text

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Final Verification (after all tasks)

- [ ] `cd src-tauri && cargo test --lib` — all tests pass, no warnings.
- [ ] `npm run check` — 0 errors.
- [ ] **E2E on the dev box** (manual — needs the GPU): launch the app and, via the Device picker,
  - select the **NVIDIA** GPU → monitor shows NVIDIA name + util% + VRAM;
  - select the **Intel** iGPU → monitor shows the vendor-neutral empty state (documented limitation — Intel iGPU util/VRAM are not exposed without root);
  - select **CPU (slow)** → GPU section shows the empty state;
  - select **Default (engine picks)** → monitor reports the first Vulkan device (the Intel iGPU at index 0 on this box → empty state; on an NVIDIA-first box → NVIDIA stats).
- [ ] Then use **superpowers:finishing-a-development-branch**.
