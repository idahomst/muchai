use crate::types::{DeviceKind, GpuDevice, GpuSelection};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

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
    let model = tmp.join("fridai-vk-probe-missing.gguf");
    let out = tmp.join("fridai-vk-probe.png");

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn write_fake_engine(body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fridai-vkprobe-{}", std::process::id()));
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
}
