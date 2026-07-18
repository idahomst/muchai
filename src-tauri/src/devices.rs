use crate::types::{DeviceKind, GpuDevice, GpuSelection};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

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

/// Parse the `ggml_vulkan: N = ...` lines from the engine's stderr into devices.
/// Order-independent: each device's index comes from the line, not its position.
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
    let mut devices = parse_vulkan_devices(&captured);
    devices.push(cpu_device());
    devices
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

/// Return the selection only if it still matches an enumerated device by both
/// index and name; otherwise `None` (fall back to engine default).
pub fn validate_gpu_selection(sel: Option<GpuSelection>, devices: &[GpuDevice]) -> Option<GpuSelection> {
    let sel = sel?;
    devices
        .iter()
        .any(|d| d.index == sel.index && d.name == sel.name)
        .then_some(sel)
}

/// Pick the device MuchAI should default to when the user hasn't made a valid
/// selection. The ggml-vulkan backend's *own* default is loader-dependent and
/// not necessarily the first-enumerated device (on a discrete+iGPU box it was
/// observed to pick the discrete GPU, not banner index 0), so we choose
/// explicitly instead of trusting it: prefer a discrete GPU, then an integrated
/// one, then any other non-CPU device. Returns `None` when only the synthetic
/// CPU device (or nothing) is present.
pub fn pick_default_device(devices: &[GpuDevice]) -> Option<&GpuDevice> {
    [DeviceKind::Discrete, DeviceKind::Integrated, DeviceKind::Other]
        .iter()
        .find_map(|kind| devices.iter().find(|d| d.kind == *kind))
}

/// Map a (possibly stale) saved selection + the enumerated device list to the
/// engine `--backend` value. A valid GPU selection maps to `vulkan{index}`; a
/// valid CPU selection (or no real GPU anywhere) yields `Some("cpu")`. With no
/// valid selection but a real GPU present, MuchAI picks the default device
/// explicitly (`pick_default_device`) and targets it by index rather than
/// omitting `--backend` and letting the engine's opaque default decide.
pub fn resolve_backend(selection: Option<GpuSelection>, devices: &[GpuDevice]) -> Option<String> {
    if let Some(sel) = validate_gpu_selection(selection, devices) {
        // validate_gpu_selection guarantees a matching device exists.
        let device = devices
            .iter()
            .find(|d| d.index == sel.index && d.name == sel.name)
            .expect("validate_gpu_selection guarantees the device exists");
        return match device.kind {
            DeviceKind::Cpu => Some("cpu".into()),
            _ => Some(format!("vulkan{}", device.index)),
        };
    }
    // No valid selection: pick a default GPU explicitly, else fall back to CPU.
    match pick_default_device(devices) {
        Some(d) => Some(format!("vulkan{}", d.index)),
        None => Some("cpu".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn enumerate_missing_binary_yields_empty() {
        assert!(enumerate(std::path::Path::new("/no/such/sd-cli")).is_empty());
    }

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

    fn dev(index: u32, name: &str) -> GpuDevice {
        GpuDevice { index, name: name.into(), kind: DeviceKind::Discrete }
    }

    fn dev_kind(index: u32, name: &str, kind: DeviceKind) -> GpuDevice {
        GpuDevice { index, name: name.into(), kind }
    }

    #[test]
    fn pick_default_device_prefers_discrete_over_integrated() {
        let devs = vec![
            dev_kind(0, "Intel", DeviceKind::Integrated),
            dev_kind(1, "NVIDIA", DeviceKind::Discrete),
            cpu_device(),
        ];
        assert_eq!(pick_default_device(&devs).map(|d| d.index), Some(1));
    }

    #[test]
    fn pick_default_device_falls_back_to_integrated_then_none() {
        let igpu_only = vec![dev_kind(0, "Intel", DeviceKind::Integrated), cpu_device()];
        assert_eq!(pick_default_device(&igpu_only).map(|d| d.index), Some(0));
        assert_eq!(pick_default_device(&[cpu_device()]), None);
        assert_eq!(pick_default_device(&[]), None);
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
    fn resolve_backend_no_selection_picks_default_discrete_device() {
        let devs = vec![
            dev_kind(0, "Intel", DeviceKind::Integrated),
            dev_kind(1, "NVIDIA GeForce RTX 3060", DeviceKind::Discrete),
            cpu_device(),
        ];
        // Targets the discrete GPU explicitly instead of leaving it to the engine.
        assert_eq!(resolve_backend(None, &devs), Some("vulkan1".to_string()));
    }

    #[test]
    fn resolve_backend_no_selection_without_gpu_falls_back_to_cpu() {
        let devs = vec![cpu_device()];
        assert_eq!(resolve_backend(None, &devs), Some("cpu".to_string()));
    }

    #[test]
    fn resolve_backend_stale_selection_picks_default_or_cpu() {
        // Stale selection (index 5 absent) + a real GPU present → explicit default device.
        let devs = vec![dev_kind(0, "Intel", DeviceKind::Integrated), cpu_device()];
        let stale = Some(GpuSelection { index: 5, name: "Ghost".into() });
        assert_eq!(resolve_backend(stale.clone(), &devs), Some("vulkan0".to_string()));
        // Stale selection + no real GPU → CPU fallback.
        let devs_cpu_only = vec![cpu_device()];
        assert_eq!(resolve_backend(stale, &devs_cpu_only), Some("cpu".to_string()));
    }
}
