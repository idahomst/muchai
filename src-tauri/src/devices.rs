use crate::types::{DeviceKind, GpuDevice};

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
