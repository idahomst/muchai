# GPU Device Selection (Linux, Vulkan) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user pick which GPU MuchAI generates on (NVIDIA / AMD / Intel) on Linux by switching the engine to a Vulkan build and exposing a device picker that maps to `--backend vulkanN`.

**Architecture:** Swap the single-file CUDA `sd-cli` for the prebuilt multi-file Vulkan bundle, shipped as a **colocated engine directory** (`sd-cli` + its `.so` siblings together; `RUNPATH=$ORIGIN` finds them). A one-time probe of `sd-cli` enumerates Vulkan devices from its stderr (`ggml_vulkan: Found N Vulkan devices:`). The chosen device index is persisted in `AppConfig.gpu_device` and threaded into the engine as `--backend vulkan{index}`.

**Tech Stack:** Tauri v2 (Rust), SvelteKit (Svelte 5 runes), stable-diffusion.cpp `sd-cli` (Vulkan build), serde snake_case wire format.

---

## Real-Binary Facts (validated on the dev box, 2026-06-29)

These are confirmed against the actual prebuilt Vulkan binary — not assumptions.

**Probe invocation** (exits in <1s; a nonexistent model path makes it error out *after* Vulkan init, so no temp files and no real generation):
```
sd-cli -M img_gen -m <nonexistent>.gguf -p x --steps 1 -W 64 -H 64 -o <tmp>.png
```
**Enumeration output** (always on **stderr**, no `-v` needed):
```
ggml_vulkan: Found 2 Vulkan devices:
ggml_vulkan: 0 = Intel(R) UHD Graphics 770 (ADL-S GT1) (Intel open-source Mesa driver) | uma: 1 | fp16: 1 | bf16: 0 | warp size: 32 | shared memory: 65536 | int dot: 0 | matrix cores: none
ggml_vulkan: 1 = NVIDIA GeForce RTX 3060 (NVIDIA) | uma: 0 | fp16: 1 | bf16: 0 | warp size: 32 | shared memory: 49152 | int dot: 0 | matrix cores: KHR_coopmat
```
- **Line shape:** `ggml_vulkan: <index> = <name> (<driver>) | uma: <0|1> | ...`. The device name itself can contain parentheses (`Intel(R) ... (ADL-S GT1)`); the **driver** is the *last* parenthesized group before the first ` | `.
- **Kind heuristic (real):** `uma: 1` → integrated; `uma: 0` → discrete. ggml-vulkan **filters out** the `llvmpipe` CPU device (host `vulkaninfo` lists 3; engine reports 2), so `cpu` realistically won't appear — keep it in the enum and classify any `llvmpipe`-named device as `cpu` defensively.
- **`Default` semantics (real):** with no `--backend`, the engine auto-picked **vulkan1** (the discrete NVIDIA), not device 0. So "Default" = `gpu_device: None` = "let the engine pick the best device."

**Packaging (multi-file, ~112 MB):** the bundle is `sd-cli` + `libstable-diffusion.so` (54 MB) + `libggml-vulkan.so` (40 MB) + `libggml.so*` + `libggml-base.so*` + all `libggml-cpu-*.so` microarch variants + `libwebp*`/`libwebm`/`libsharpyuv`. `sd-cli` and `libggml-vulkan.so` both have `RUNPATH=$ORIGIN`, so every `.so` must sit **next to `sd-cli`**. `libggml-vulkan.so` needs `libvulkan.so.1`, which is **not bundled** — it loads from the host (the host ICD loader is required, like libcuda was). The CPU-backend loader picks the best `libggml-cpu-*` variant for the host CPU at runtime, so keep all variants.

---

## File Structure

**Rust (`src-tauri/src/`):**
- `types.rs` — *modify*: add `DeviceKind`, `GpuDevice`, `GpuSelection`; add `gpu_device` to `AppConfig`.
- `devices.rs` — *create*: `parse_vulkan_devices()`, `enumerate()`, `validate_gpu_selection()`.
- `command_builder.rs` — *modify*: add `backend: Option<&str>` to `build_args`.
- `engine.rs` — *modify*: thread `backend` through `run_generation`.
- `commands.rs` — *modify*: engine-directory `resolve_binary`, new `list_gpu_devices` command, backend wiring in `generate`, `gpu_devices` cache on `AppState`.
- `lib.rs` — *modify*: `mod devices;`, register `list_gpu_devices`.

**Config / bundle:**
- `src-tauri/binaries/engine/` — *create*: colocated Vulkan engine (`sd-cli` + `.so` siblings).
- `src-tauri/tauri.conf.json` — *modify*: drop `externalBin`, add `resources` for the engine dir.
- `scripts/build-appimage.sh` — *modify*: Vulkan packaging (strip host `libvulkan`/`libcuda`/`libnvidia`, not just CUDA).

**Frontend (`src/lib/`):**
- `types.ts` — *modify*: `DeviceKind`, `GpuDevice`, `AppConfig.gpu_device`.
- `api.ts` — *modify*: `listGpuDevices()`.
- `stores.ts` — *modify*: `gpuDevices` store.
- `components/DevicePicker.svelte` — *create*.
- `../routes/+page.svelte` — *modify*: mount `DevicePicker`, load devices.

---

## Task 0: Engine asset prep (manual, no code)

Swap the CUDA single-file engine for the Vulkan multi-file bundle in a colocated directory. The extracted bundle already exists at `/tmp/sdvk/` on the dev box from the design phase.

**Files:**
- Create: `src-tauri/binaries/engine/` (sd-cli + all `.so`)
- Remove: `src-tauri/binaries/sd-cli`, `src-tauri/binaries/sd-cli-x86_64-unknown-linux-gnu`

- [ ] **Step 1: Create the colocated engine dir from the extracted Vulkan bundle**

```bash
cd /home/idaho/g/mst/muchai
mkdir -p src-tauri/binaries/engine
# Copy sd-cli and every shared lib (NOT sd-server) into the engine dir.
cp /tmp/sdvk/sd-cli src-tauri/binaries/engine/
cp /tmp/sdvk/*.so* src-tauri/binaries/engine/
chmod +x src-tauri/binaries/engine/sd-cli
```

- [ ] **Step 2: Verify the probe enumerates devices from the colocated dir**

Run:
```bash
cd /home/idaho/g/mst/muchai/src-tauri/binaries/engine
./sd-cli -M img_gen -m /tmp/does_not_exist.gguf -p x --steps 1 -W 64 -H 64 -o /tmp/_p.png 2>&1 \
  | grep -E "ggml_vulkan: (Found|[0-9] =)"
```
Expected: prints `ggml_vulkan: Found 2 Vulkan devices:` and two `ggml_vulkan: N = ...` lines (no `LD_LIBRARY_PATH` needed — `$ORIGIN` resolves siblings).

- [ ] **Step 3: Remove the obsolete CUDA single-file binaries**

```bash
cd /home/idaho/g/mst/muchai
rm -f src-tauri/binaries/sd-cli src-tauri/binaries/sd-cli-x86_64-unknown-linux-gnu
```

- [ ] **Step 4: Confirm whether the engine dir is git-tracked or ignored**

Run:
```bash
cd /home/idaho/g/mst/muchai
git check-ignore -v src-tauri/binaries/engine/sd-cli || echo "NOT ignored"
git status --short src-tauri/binaries/
```
Expected: report whether the bundle is ignored. If **not ignored**, the ~112 MB of binaries should be excluded — add `src-tauri/binaries/` to `.gitignore` (these are build inputs fetched per-machine, like the old CUDA binary). If already ignored, do nothing. Note: the engine bundle is a local build input; it is not committed.

- [ ] **Step 5: Commit (gitignore change only, if any)**

```bash
cd /home/idaho/g/mst/muchai
git add .gitignore 2>/dev/null || true
git commit -m "build: source Vulkan engine bundle into binaries/engine (local asset)" --allow-empty
```

---

## Task 1: GPU types (Rust)

**Files:**
- Modify: `src-tauri/src/types.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/types.rs`:

```rust
    #[test]
    fn gpu_device_serializes_snake_case_kind() {
        let d = GpuDevice { index: 1, name: "NVIDIA GeForce RTX 3060".into(), kind: DeviceKind::Discrete };
        let json = serde_json::to_string(&d).unwrap();
        assert!(json.contains("\"kind\":\"discrete\""), "got {json}");
        let back: GpuDevice = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn app_config_defaults_gpu_device_to_none_when_absent() {
        // Config JSON missing the gpu_device key must still deserialize.
        let json = r#"{"sd_binary_path":null,"default_model_path":null,"gallery_dir":"/tmp/g","models_dir":"/tmp/m","extra_model_dirs":[],"last_request":{"model_path":"","prompt":"","negative_prompt":"","steps":20,"cfg_scale":7.0,"sampler":"euler_a","width":512,"height":512,"seed":-1,"batch_count":1}}"#;
        let cfg: AppConfig = serde_json::from_str(json).unwrap();
        assert!(cfg.gpu_device.is_none());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib types:: 2>&1 | tail -20`
Expected: FAIL — `cannot find type GpuDevice`/`DeviceKind`, and `AppConfig` has no field `gpu_device`.

- [ ] **Step 3: Add the types**

In `src-tauri/src/types.rs`, after the `ProgressUpdate` struct (around line 77), add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceKind {
    Discrete,
    Integrated,
    Cpu,
    Other,
}

/// A Vulkan device as reported by the engine. `index` is the `vulkanN` index
/// used in `--backend vulkan{index}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuDevice {
    pub index: u32,
    pub name: String,
    pub kind: DeviceKind,
}

/// The user's persisted device choice. `name` is stored alongside `index` so a
/// stale selection (hardware/driver changed) can be detected and ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuSelection {
    pub index: u32,
    pub name: String,
}
```

In the `AppConfig` struct, after the `extra_model_dirs` field (line 128), add:

```rust
    /// Chosen Vulkan device. `None` = engine default (auto-picks best device).
    #[serde(default)]
    pub gpu_device: Option<GpuSelection>,
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib types:: 2>&1 | tail -20`
Expected: PASS (all `types::` tests green).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/types.rs
git commit -m "feat(gpu): add GpuDevice/GpuSelection/DeviceKind types and AppConfig.gpu_device"
```

---

## Task 2: Vulkan device-log parser (Rust)

**Files:**
- Create: `src-tauri/src/devices.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod devices;`)

- [ ] **Step 1: Create the module with a failing test**

Create `src-tauri/src/devices.rs`:

```rust
use crate::types::{DeviceKind, GpuDevice};

/// Parse the `ggml_vulkan: N = ...` lines from the engine's stderr into devices.
/// Order-independent: each device's index comes from the line, not its position.
pub fn parse_vulkan_devices(stderr: &str) -> Vec<GpuDevice> {
    unimplemented!()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "ggml_vulkan: Found 2 Vulkan devices:\n\
ggml_vulkan: 0 = Intel(R) UHD Graphics 770 (ADL-S GT1) (Intel open-source Mesa driver) | uma: 1 | fp16: 1 | bf16: 0 | warp size: 32 | shared memory: 65536 | int dot: 0 | matrix cores: none\n\
ggml_vulkan: 1 = NVIDIA GeForce RTX 3060 (NVIDIA) | uma: 0 | fp16: 1 | bf16: 0 | warp size: 32 | shared memory: 49152 | int dot: 0 | matrix cores: KHR_coopmat\n";

    #[test]
    fn parses_real_engine_output() {
        let d = parse_vulkan_devices(SAMPLE);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0], GpuDevice { index: 0, name: "Intel(R) UHD Graphics 770 (ADL-S GT1)".into(), kind: DeviceKind::Integrated });
        assert_eq!(d[1], GpuDevice { index: 1, name: "NVIDIA GeForce RTX 3060".into(), kind: DeviceKind::Discrete });
    }

    #[test]
    fn empty_or_garbled_input_yields_empty() {
        assert!(parse_vulkan_devices("").is_empty());
        assert!(parse_vulkan_devices("no vulkan here\nrandom line").is_empty());
    }

    #[test]
    fn llvmpipe_is_classified_cpu() {
        let line = "ggml_vulkan: 0 = llvmpipe (LLVM 17) (llvmpipe) | uma: 1 | fp16: 1\n";
        let d = parse_vulkan_devices(line);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, DeviceKind::Cpu);
    }
}
```

Add `mod devices;` to `src-tauri/src/lib.rs` in the module list (alphabetical, after `mod config;`):

```rust
mod config;
mod devices;
mod downloader;
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib devices:: 2>&1 | tail -20`
Expected: FAIL — `not implemented` panic (or compile error from `unimplemented!`).

- [ ] **Step 3: Implement the parser**

Replace the `parse_vulkan_devices` body in `src-tauri/src/devices.rs`:

```rust
pub fn parse_vulkan_devices(stderr: &str) -> Vec<GpuDevice> {
    let mut out = Vec::new();
    for line in stderr.lines() {
        let Some(rest) = line.strip_prefix("ggml_vulkan: ") else { continue };
        let Some(eq) = rest.find(" = ") else { continue };
        let Ok(index) = rest[..eq].trim().parse::<u32>() else { continue };
        let after = &rest[eq + 3..];
        let first_field = after.split(" | ").next().unwrap_or(after).trim();
        let name = strip_driver(first_field);
        let kind = if name.to_lowercase().contains("llvmpipe") || first_field.to_lowercase().contains("llvmpipe") {
            DeviceKind::Cpu
        } else if after.contains("uma: 1") {
            DeviceKind::Integrated
        } else if after.contains("uma: 0") {
            DeviceKind::Discrete
        } else {
            DeviceKind::Other
        };
        out.push(GpuDevice { index, name, kind });
    }
    out
}

/// Strip the trailing " (<driver>)" group. Device names may themselves contain
/// parentheses, so only the *last* parenthesized group is removed.
fn strip_driver(s: &str) -> String {
    if s.ends_with(')') {
        if let Some(pos) = s.rfind(" (") {
            return s[..pos].to_string();
        }
    }
    s.to_string()
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib devices:: 2>&1 | tail -20`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/devices.rs src-tauri/src/lib.rs
git commit -m "feat(gpu): parse ggml-vulkan device log into GpuDevice list"
```

---

## Task 3: Device enumeration probe (Rust)

**Files:**
- Modify: `src-tauri/src/devices.rs`

- [ ] **Step 1: Write the failing test (fake engine that prints the device log)**

Add to `src-tauri/src/devices.rs` `mod tests`:

```rust
    use std::path::PathBuf;

    fn write_fake_engine(body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("muchai-vkprobe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sd-cli");
        std::fs::write(&path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

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

    #[test]
    fn enumerate_missing_binary_yields_empty() {
        assert!(enumerate(std::path::Path::new("/no/such/sd-cli")).is_empty());
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib devices::tests::enumerate 2>&1 | tail -20`
Expected: FAIL — `cannot find function enumerate`.

- [ ] **Step 3: Implement `enumerate` with a timeout guard**

Add to the top of `src-tauri/src/devices.rs` (imports) and the function body:

```rust
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;
```

```rust
/// One-time probe: run `sd-cli` with a nonexistent model so it initializes the
/// Vulkan backend (printing the device list to stderr) then errors out fast.
/// Never panics; returns an empty vec on any failure or timeout.
pub fn enumerate(binary: &Path) -> Vec<GpuDevice> {
    if !binary.exists() {
        return Vec::new();
    }
    let tmp = std::env::temp_dir();
    let model = tmp.join("muchai-vk-probe-missing.gguf");
    let out = tmp.join("muchai-vk-probe.png");

    let mut child = match Command::new(binary)
        .args(["-M", "img_gen", "-m"])
        .arg(&model)
        .args(["-p", "x", "--steps", "1", "-W", "64", "-H", "64", "-o"])
        .arg(&out)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let Some(mut stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Vec::new();
    };

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        let _ = tx.send(buf);
    });

    let captured = rx.recv_timeout(Duration::from_secs(15)).unwrap_or_default();
    let _ = child.kill();
    let _ = child.wait();
    parse_vulkan_devices(&captured)
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib devices:: 2>&1 | tail -20`
Expected: PASS (all `devices::` tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/devices.rs
git commit -m "feat(gpu): probe sd-cli to enumerate Vulkan devices"
```

---

## Task 4: Validate a stored selection (Rust)

**Files:**
- Modify: `src-tauri/src/devices.rs`

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/devices.rs` `mod tests`:

```rust
    use crate::types::GpuSelection;

    fn dev(index: u32, name: &str) -> GpuDevice {
        GpuDevice { index, name: name.into(), kind: DeviceKind::Discrete }
    }

    #[test]
    fn valid_selection_passes_through() {
        let devs = vec![dev(0, "Intel"), dev(1, "NVIDIA GeForce RTX 3060")];
        let sel = Some(GpuSelection { index: 1, name: "NVIDIA GeForce RTX 3060".into() });
        assert_eq!(validate_gpu_selection(sel.clone(), &devs), sel);
    }

    #[test]
    fn stale_selection_falls_back_to_none() {
        let devs = vec![dev(0, "Intel")];
        // index 1 no longer exists
        let sel = Some(GpuSelection { index: 1, name: "NVIDIA GeForce RTX 3060".into() });
        assert_eq!(validate_gpu_selection(sel, &devs), None);
        // index exists but the name changed (driver/hardware swap)
        let sel2 = Some(GpuSelection { index: 0, name: "AMD".into() });
        assert_eq!(validate_gpu_selection(sel2, &devs), None);
    }

    #[test]
    fn none_stays_none() {
        assert_eq!(validate_gpu_selection(None, &[dev(0, "Intel")]), None);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib devices::tests::valid 2>&1 | tail -20`
Expected: FAIL — `cannot find function validate_gpu_selection`.

- [ ] **Step 3: Implement the validator**

Add to `src-tauri/src/devices.rs` (and add `GpuSelection` to the top `use crate::types::{...}`):

```rust
use crate::types::GpuSelection;

/// Return the selection only if it still matches an enumerated device by both
/// index and name; otherwise `None` (fall back to engine default).
pub fn validate_gpu_selection(sel: Option<GpuSelection>, devices: &[GpuDevice]) -> Option<GpuSelection> {
    let sel = sel?;
    devices
        .iter()
        .any(|d| d.index == sel.index && d.name == sel.name)
        .then_some(sel)
}
```

Update the module's top `use` line to:
```rust
use crate::types::{DeviceKind, GpuDevice, GpuSelection};
```
(and remove the duplicate `use crate::types::GpuSelection;` if you added one in the test block — keep the test-block `use` only if it isn't already imported at module scope; module-scope import is sufficient, so delete the test-block `use crate::types::GpuSelection;`.)

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib devices:: 2>&1 | tail -20`
Expected: PASS (all `devices::` tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/devices.rs
git commit -m "feat(gpu): validate stored device selection against enumerated devices"
```

---

## Task 5: Thread `--backend` into the arg builder (Rust)

**Files:**
- Modify: `src-tauri/src/command_builder.rs`

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/command_builder.rs` `mod tests`:

```rust
    #[test]
    fn appends_backend_when_some() {
        let args = build_args(&sample(), "/out/x.png", Some("vulkan1"));
        assert_eq!(val_after(&args, "--backend"), Some("vulkan1"));
    }

    #[test]
    fn omits_backend_when_none() {
        let args = build_args(&sample(), "/out/x.png", None);
        assert!(!args.iter().any(|x| x == "--backend"));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd src-tauri && cargo test --lib command_builder:: 2>&1 | tail -20`
Expected: FAIL — `build_args` takes 2 arguments but 3 were supplied (existing tests also won't compile).

- [ ] **Step 3: Add the `backend` parameter**

In `src-tauri/src/command_builder.rs`, change the signature and append the flag before the trailing `-v`:

```rust
pub fn build_args(req: &GenerationRequest, output_path: &str, backend: Option<&str>) -> Vec<String> {
```

Just before `a.push("-v".into());` (line 27), insert:

```rust
    if let Some(b) = backend {
        a.push("--backend".into());
        a.push(b.to_string());
    }
```

Update the two existing tests that call `build_args(&sample(), "/out/x.png")` and `build_args(&req, "/out/x.png")` to pass `None` as the third argument:
- `includes_core_flags_and_values`: `build_args(&sample(), "/out/x.png", None)`
- `uses_img_gen_mode`: `build_args(&sample(), "/out/x.png", None)`
- `omits_negative_prompt_when_empty`: `build_args(&req, "/out/x.png", None)`

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd src-tauri && cargo test --lib command_builder:: 2>&1 | tail -20`
Expected: PASS (all `command_builder::` tests).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/command_builder.rs
git commit -m "feat(gpu): add optional --backend vulkanN to build_args"
```

---

## Task 6: Thread `backend` through `run_generation` (Rust)

**Files:**
- Modify: `src-tauri/src/engine.rs`

- [ ] **Step 1: Update the signature and call site**

In `src-tauri/src/engine.rs`, change `run_generation` (line 36) to accept `backend`:

```rust
pub fn run_generation<F: FnMut(ProgressUpdate)>(
    binary: &Path,
    req: &GenerationRequest,
    output_path: &Path,
    backend: Option<&str>,
    slot: &ChildSlot,
    mut on_progress: F,
) -> Result<Vec<i64>, GenError> {
```

Change the `build_args` call (line 46) to:

```rust
    let args = build_args(req, &output_path.to_string_lossy(), backend);
```

- [ ] **Step 2: Update the in-file tests to pass `None`**

In `src-tauri/src/engine.rs` `mod tests`, each `run_generation(...)` call (4 call sites: ~lines 167, 189, 207, 227) takes a new `backend` argument positioned **after `output_path` and before `slot`**. Add `None` to each. For example the `missing_binary_errors` call becomes:

```rust
        let res = run_generation(
            Path::new("/no/such/sd-cli"),
            &req,
            tmp.as_path(),
            None,
            &slot,
            |_| {},
        );
```

Apply the same `None` insertion to the other three `run_generation(...)` calls (match each existing call's arguments, inserting `None` right after the output-path argument).

- [ ] **Step 3: Run the engine tests**

Run: `cd src-tauri && cargo test --lib engine:: 2>&1 | tail -25`
Expected: PASS (all 35 engine tests). If a call site was missed, the compiler names the exact line — fix and re-run.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/engine.rs
git commit -m "feat(gpu): thread backend selection through run_generation"
```

---

## Task 7: Engine-dir resolution, `list_gpu_devices`, and generate wiring (Rust)

**Files:**
- Modify: `src-tauri/src/commands.rs`

- [ ] **Step 1: Add the `gpu_devices` cache field to `AppState`**

In `src-tauri/src/commands.rs`, find the `AppState` struct definition (top of file). Add a session cache field:

```rust
    pub gpu_devices: Arc<Mutex<Option<Vec<GpuDevice>>>>,
```

Ensure the imports at the top of `commands.rs` include (add any missing):
```rust
use crate::types::{AppConfig, GalleryItem, GenerationRequest, GpuDevice};
use std::sync::{Arc, Mutex};
use tauri::Manager;
```
(`GpuDevice` for the cache/return type, `Manager` for `app.path()`, `Arc`/`Mutex` for the cache.)

> Note: `AppState` is constructed in `lib.rs`'s `.manage(...)`. Task 8 adds the new field there.

- [ ] **Step 2: Replace `resolve_binary` with engine-directory resolution**

Replace the existing `resolve_binary` (lines 20-34) with:

```rust
fn engine_binary_name() -> &'static str {
    if cfg!(windows) { "sd-cli.exe" } else { "sd-cli" }
}

/// Directory holding `sd-cli` and its sibling `.so` files. Bundled apps resolve
/// it from the Tauri resource dir (`<resources>/engine`); dev falls back to the
/// source tree. `RUNPATH=$ORIGIN` then loads the siblings next to `sd-cli`.
fn engine_dir(app: &AppHandle) -> Option<PathBuf> {
    if let Ok(res) = app.path().resource_dir() {
        let d = res.join("engine");
        if d.join(engine_binary_name()).exists() {
            return Some(d);
        }
    }
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/engine");
    if dev.join(engine_binary_name()).exists() {
        return Some(dev);
    }
    None
}

/// Resolve the engine binary: explicit config override, else the bundled engine.
fn resolve_binary(app: &AppHandle, cfg: &AppConfig) -> Option<PathBuf> {
    if let Some(p) = &cfg.sd_binary_path {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let bin = engine_dir(app)?.join(engine_binary_name());
    bin.exists().then_some(bin)
}
```

- [ ] **Step 3: Add the `list_gpu_devices` command (lazy, cached)**

Add a new command to `src-tauri/src/commands.rs` (near `get_settings`):

```rust
#[tauri::command]
pub fn list_gpu_devices(app: AppHandle, state: State<AppState>) -> Vec<GpuDevice> {
    if let Some(cached) = state.gpu_devices.lock().unwrap().as_ref() {
        return cached.clone();
    }
    let cfg = state.config.lock().unwrap().clone();
    let devices = match resolve_binary(&app, &cfg) {
        Some(bin) => crate::devices::enumerate(&bin),
        None => Vec::new(),
    };
    *state.gpu_devices.lock().unwrap() = Some(devices.clone());
    devices
}
```

- [ ] **Step 4: Wire the backend into `generate`**

In the `generate` command, change the `resolve_binary` call (line 77) to pass `&app`:

```rust
    let binary = resolve_binary(&app, &cfg)
        .ok_or_else(|| "stable-diffusion engine not found. Set its path in Settings.".to_string())?;
```

After `let cfg = state.config.lock().unwrap().clone();` (line 76), compute the validated backend token:

```rust
    // Validate the saved device against the enumerated list (cached); a stale
    // selection silently falls back to the engine default.
    let backend = {
        let cached = state.gpu_devices.lock().unwrap().clone();
        let sel = match cached {
            Some(devices) => crate::devices::validate_gpu_selection(cfg.gpu_device.clone(), &devices),
            None => cfg.gpu_device.clone(),
        };
        sel.map(|s| format!("vulkan{}", s.index))
    };
```

Then pass it into `run_generation`. Find the closure (line 91-95) and add `backend` (moved into the blocking closure). Change:

```rust
    let req = request.clone();
    let img = image_path.clone();
    let backend_owned = backend.clone();

    // Run the (blocking) engine on a worker thread so the async command yields.
    let result = tauri::async_runtime::spawn_blocking(move || {
        engine::run_generation(&binary, &req, &img, backend_owned.as_deref(), &slot, |p| {
            let _ = app2.emit("generation:progress", p);
        })
    })
    .await
    .map_err(|e| e.to_string())?;
```

- [ ] **Step 5: Build to verify it compiles**

Run: `cd src-tauri && cargo build 2>&1 | tail -25`
Expected: compiles. The only remaining error should be in `lib.rs` (missing `gpu_devices` field in `.manage(...)` and unregistered command) — fixed in Task 8. If `cargo build` flags the `AppState` field as missing in `lib.rs`, that is expected; proceed to Task 8.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands.rs
git commit -m "feat(gpu): engine-dir resolution + list_gpu_devices command + backend wiring"
```

---

## Task 8: Register command and AppState field (Rust)

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Initialize the `gpu_devices` cache in `.manage(...)`**

In `src-tauri/src/lib.rs`, the `.manage(AppState { ... })` block (lines 26-30) adds the new field:

```rust
        .manage(AppState {
            config: Mutex::new(initial),
            child: Arc::new(Mutex::new(None)),
            download_cancel: Arc::new(AtomicBool::new(false)),
            gpu_devices: Arc::new(Mutex::new(None)),
        })
```

- [ ] **Step 2: Register the command**

In the `tauri::generate_handler![...]` list (lines 57-72), add:

```rust
            commands::list_gpu_devices,
```

(`mod devices;` was already added in Task 2.)

- [ ] **Step 3: Build and run the full Rust test suite**

Run: `cd src-tauri && cargo build 2>&1 | tail -15 && cargo test --lib 2>&1 | tail -20`
Expected: build OK; all tests pass (types, devices, command_builder, engine, config, gallery, etc.).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(gpu): register list_gpu_devices and gpu_devices cache"
```

---

## Task 9: Bundle the engine as a colocated resource (Tauri config)

**Files:**
- Modify: `src-tauri/tauri.conf.json`

- [ ] **Step 1: Replace `externalBin` with a `resources` engine dir**

In `src-tauri/tauri.conf.json`, in the `"bundle"` object, **remove** the line:

```json
    "externalBin": ["binaries/sd-cli"],
```

and **add** (same `"bundle"` object):

```json
    "resources": {
      "binaries/engine": "engine"
    },
```

This copies the contents of `src-tauri/binaries/engine/` into `<resource_dir>/engine/`, matching `engine_dir()`'s `resource_dir().join("engine")` resolution (Task 7).

- [ ] **Step 2: Verify dev resolution works end to end**

Run (starts the app; needs a display):
```bash
cd /home/idaho/g/mst/muchai && npm run tauri dev
```
Expected: app launches; the device picker (added in Task 11) lists the Intel + NVIDIA devices. The dev path resolves via the `CARGO_MANIFEST_DIR/binaries/engine` fallback in `engine_dir()`. Close the app when verified. (If running headless, defer this to the Task 13 manual E2E.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json
git commit -m "build(gpu): ship Vulkan engine as a colocated resource dir"
```

---

## Task 10: Frontend types and API (TypeScript)

**Files:**
- Modify: `src/lib/types.ts`
- Modify: `src/lib/api.ts`

- [ ] **Step 1: Add the device types and config field**

In `src/lib/types.ts`, after the `SystemStats` interface (line 31), add:

```ts
export type DeviceKind = "discrete" | "integrated" | "cpu" | "other";
export interface GpuDevice {
  index: number;
  name: string;
  kind: DeviceKind;
}
```

In the `AppConfig` interface, after `extra_model_dirs: string[];` (line 43), add:

```ts
  gpu_device: { index: number; name: string } | null;
```

- [ ] **Step 2: Add the API binding**

In `src/lib/api.ts`, add `GpuDevice` to the type import (line 3) and add the function near `listModels`:

```ts
export const listGpuDevices = () => invoke<GpuDevice[]>("list_gpu_devices");
```

The import line becomes:
```ts
import type { AppConfig, GalleryItem, GenerationRequest, ProgressUpdate, SystemStats, ModelInfo, RatedModel, DownloadProgress, GpuDevice } from "./types";
```

- [ ] **Step 3: Type-check**

Run: `cd /home/idaho/g/mst/muchai && npm run check 2>&1 | tail -20`
Expected: no new errors from these files. (`AppConfig` consumers spread existing config objects, so the new non-optional `gpu_device` field is only constructed from backend data — no TS literal needs updating. If `npm run check` flags a place that builds an `AppConfig` literal, set `gpu_device: null` there.)

- [ ] **Step 4: Commit**

```bash
git add src/lib/types.ts src/lib/api.ts
git commit -m "feat(gpu): frontend GpuDevice types and listGpuDevices binding"
```

---

## Task 11: Device picker component and mount wiring (Svelte)

**Files:**
- Modify: `src/lib/stores.ts`
- Create: `src/lib/components/DevicePicker.svelte`
- Modify: `src/routes/+page.svelte`

- [ ] **Step 1: Add the `gpuDevices` store**

In `src/lib/stores.ts`, add `GpuDevice` to the type import (line 2) and a store after `sysStats` (line 11):

```ts
export const gpuDevices = writable<GpuDevice[]>([]);
```

Updated import:
```ts
import type { GenerationRequest, GalleryItem, SystemStats, AppConfig, ProgressUpdate, ModelInfo, GpuDevice } from "./types";
```

- [ ] **Step 2: Create `DevicePicker.svelte`**

Create `src/lib/components/DevicePicker.svelte` (mirrors the `ModelFolders` persistence pattern: read `settings`, write via `setSettings`):

```svelte
<script lang="ts">
  import { settings, gpuDevices } from "$lib/stores";
  import { setSettings } from "$lib/api";

  let busy = $state(false);
  let error = $state<string | null>(null);

  // The select's value: "" = engine default, otherwise the device index as string.
  const current = $derived(
    $settings?.gpu_device ? String($settings.gpu_device.index) : "",
  );

  // True when a device is saved but no longer present in the enumerated list.
  const stale = $derived(
    !!$settings?.gpu_device &&
      !$gpuDevices.some(
        (d) => d.index === $settings!.gpu_device!.index && d.name === $settings!.gpu_device!.name,
      ),
  );

  const label = (d: { index: number; name: string; kind: string }) =>
    `GPU ${d.index} — ${d.name} (${d.kind})`;

  async function onChange(e: Event) {
    if (!$settings || busy) return;
    const val = (e.currentTarget as HTMLSelectElement).value;
    busy = true;
    error = null;
    try {
      const gpu_device =
        val === ""
          ? null
          : (() => {
              const d = $gpuDevices.find((x) => String(x.index) === val);
              return d ? { index: d.index, name: d.name } : null;
            })();
      const next = { ...$settings, gpu_device };
      await setSettings(next);
      settings.set(next);
    } catch (err) {
      error = String(err);
    } finally {
      busy = false;
    }
  }
</script>

<div class="picker">
  <span class="lbl">GPU device</span>
  {#if $gpuDevices.length === 0}
    <span class="none">No Vulkan devices detected</span>
  {:else}
    <select value={current} onchange={onChange} disabled={busy}>
      <option value="">Default (let engine choose)</option>
      {#each $gpuDevices as d (d.index)}
        <option value={String(d.index)}>{label(d)}</option>
      {/each}
    </select>
    {#if stale}
      <span class="warn">Saved device unavailable — using engine default.</span>
    {/if}
  {/if}
  {#if error}<span class="err">{error}</span>{/if}
</div>

<style>
  .picker { font-size:.75rem; border-top:1px solid var(--border); padding:.45rem .2rem 0; display:flex; flex-direction:column; gap:.25rem; }
  .lbl { opacity:.6; }
  select { font:inherit; font-size:.72rem; padding:.25rem; }
  .none, .warn { opacity:.6; }
  .warn { color:#e0a800; opacity:1; }
  .err { color:#ff6b6b; }
</style>
```

- [ ] **Step 3: Mount the picker and load devices**

In `src/routes/+page.svelte`:

Add to the store import (line 3) and api import (line 4):
```ts
  import { settings, request, history, sysStats, models, gpuDevices } from "$lib/stores";
  import { getSettings, listHistory, onSystemStats, listModels, listGpuDevices } from "$lib/api";
```

Add the component import after `ModelFolders` (line 13):
```ts
  import DevicePicker from "$lib/components/DevicePicker.svelte";
```

In `onMount`'s async block, after `models.set(await listModels());` (line 23), add:
```ts
      gpuDevices.set(await listGpuDevices());
```

In the `.controls` aside, add `<DevicePicker />` right after `<ModelFolders />` (line 34):
```svelte
    <ModelFolders />
    <DevicePicker />
```

- [ ] **Step 4: Type-check and lint**

Run: `cd /home/idaho/g/mst/muchai && npm run check 2>&1 | tail -20`
Expected: clean (no errors).

- [ ] **Step 5: Commit**

```bash
git add src/lib/stores.ts src/lib/components/DevicePicker.svelte src/routes/+page.svelte
git commit -m "feat(gpu): device picker UI with persistence and stale-selection notice"
```

---

## Task 12: AppImage packaging for Vulkan

**Files:**
- Modify: `scripts/build-appimage.sh`

- [ ] **Step 1: Rewrite the script for the Vulkan engine**

Replace the contents of `scripts/build-appimage.sh` with:

```bash
#!/usr/bin/env bash
# Build a self-contained MuchAI AppImage (Vulkan engine).
#
# The engine (sd-cli + its .so siblings) ships as a Tauri *resource* directory
# (binaries/engine -> <resources>/engine) and is loaded via RUNPATH=$ORIGIN, so
# linuxdeploy does not walk it and will not bundle libvulkan. libvulkan.so.1 is
# the host ICD loader — it is driver/host-locked (like libcuda was), so it MUST
# come from the host. We defensively strip any libvulkan/libcuda/libnvidia that
# linuxdeploy may have pulled into usr/lib so the host's loader+driver are used.
#
# Prereq: src-tauri/binaries/engine/ must contain the Vulkan sd-cli + .so files.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APPIMAGE_DIR="src-tauri/target/release/bundle/appimage"
APPDIR="$APPIMAGE_DIR/muchai.AppDir"
PLUGIN="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"

echo ">> tauri build (appimage)…"
npm run tauri build -- --bundles appimage

echo ">> stripping host-provided loader/driver libs from AppDir…"
# These must come from the host: the Vulkan ICD loader and any vendor driver libs.
find "$APPDIR/usr/lib" \
  \( -iname 'libvulkan.so*' -o -iname 'libcuda.so*' -o -iname 'libnvidia-*' -o -iname 'libnvcuvid*' \) \
  -print -delete || true

echo ">> repacking AppImage without loader/driver libs…"
( cd "$APPIMAGE_DIR" \
  && ARCH=x86_64 OUTPUT="muchai_0.1.0_amd64.AppImage" \
     APPIMAGE_EXTRACT_AND_RUN=1 \
     "$PLUGIN" --appdir muchai.AppDir )

echo ">> done:"
ls -lh "$APPIMAGE_DIR"/muchai_0.1.0_amd64.AppImage
```

- [ ] **Step 2: Build the AppImage**

Run: `cd /home/idaho/g/mst/muchai && bash scripts/build-appimage.sh 2>&1 | tail -30`
Expected: completes and prints the resulting `muchai_0.1.0_amd64.AppImage` path and size. The `binaries/engine` resources land under the AppDir; the strip step removes only host-locked loader/driver libs from `usr/lib` (not the engine resource dir).

- [ ] **Step 3: Smoke-test the AppImage**

Run:
```bash
cd /home/idaho/g/mst/muchai
APPIMAGE_EXTRACT_AND_RUN=1 src-tauri/target/release/bundle/appimage/muchai_0.1.0_amd64.AppImage &
sleep 8 && kill %1 2>/dev/null || true
```
Expected: the app window opens without a missing-library error (engine resolves from `<resources>/engine`, Vulkan loader from host). Verify the device picker lists devices, then close.

- [ ] **Step 4: Commit**

```bash
git add scripts/build-appimage.sh
git commit -m "build(gpu): AppImage packaging for the Vulkan engine"
```

---

## Task 13: Full verification (E2E)

**Files:** none (verification only)

- [ ] **Step 1: Full Rust test suite**

Run: `cd /home/idaho/g/mst/muchai/src-tauri && cargo test 2>&1 | tail -25`
Expected: all tests pass.

- [ ] **Step 2: Frontend check**

Run: `cd /home/idaho/g/mst/muchai && npm run check 2>&1 | tail -15`
Expected: clean.

- [ ] **Step 3: Manual E2E on the dev box (`npm run tauri dev`)**

Verify each:
- [ ] Device picker lists **GPU 0 — Intel(R) UHD Graphics 770 (ADL-S GT1) (integrated)** and **GPU 1 — NVIDIA GeForce RTX 3060 (discrete)**, plus **Default (let engine choose)**.
- [ ] Selecting **NVIDIA** then generating a small image succeeds; the engine log shows `--backend vulkan1` (run with the engine's verbose output / check the process args).
- [ ] Selecting **Intel** then generating succeeds with `--backend vulkan0`.
- [ ] Selecting **Default** generates with no `--backend` flag (engine auto-picks).
- [ ] The selection **persists across an app restart** (re-open and confirm the dropdown shows the last choice).

- [ ] **Step 4: Update the roadmap memory**

Update `/home/idaho/.claude/projects/-home-idaho-g-mst-muchai/memory/muchai-roadmap.md`: record sub-project 1 (Linux Vulkan GPU selection) as done, and that sub-projects 2 (cross-vendor monitor) and 3 (macOS Metal) remain. Note the interim limitation: the resource monitor still uses NVML and reports NVIDIA stats regardless of the selected device.

- [ ] **Step 5: Finish the branch**

Use the **superpowers:finishing-a-development-branch** skill to merge/PR `feat/gpu-selector`.

---

## Notes / Known Interim Limitations

- The **resource monitor still uses NVML** during this sub-project — it shows NVIDIA stats regardless of the selected device (or nothing on a non-NVIDIA-primary machine). Resolved by sub-project 2 (cross-vendor monitor via Vulkan `VK_EXT_memory_budget`).
- Vulkan is typically **slower than CUDA on NVIDIA** — an accepted, user-approved tradeoff for one universal engine.
- The engine bundle is a **local build input** (~112 MB), not committed; Task 0 sources it into `src-tauri/binaries/engine/`.
