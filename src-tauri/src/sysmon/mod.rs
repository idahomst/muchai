pub mod providers;
pub mod budget;

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
/// the caller (with the `libnvidia-ml.so.1` init fix) and moved in. On Linux,
/// AMD and Intel sysfs providers are also added to cover non-NVIDIA GPUs.
pub fn default_providers(nvml: Option<nvml_wrapper::Nvml>) -> Vec<Box<dyn GpuProvider>> {
    let mut v: Vec<Box<dyn GpuProvider>> = vec![Box::new(NvmlProvider { nvml })];
    #[cfg(target_os = "linux")]
    {
        v.push(Box::new(providers::AmdSysfsProvider::new()));
        v.push(Box::new(providers::IntelSysfsProvider::new()));
    }
    v
}

/// Which GPU the monitor should report, derived from the user's selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// CPU selected, or no GPU available — hide the GPU section.
    None,
    /// Report the GPU whose name matches this selected-device name.
    Name(String),
}

/// Map the saved selection + enumerated devices to a monitor `Target`, mirroring
/// the generation backend rule exactly so the monitor always reports the device
/// generation actually runs on. A valid CPU selection (or no real GPU) hides the
/// GPU section; a valid GPU selection keys to that device's name; an absent/stale
/// selection keys to the same default device the generation path picks
/// (`pick_default_device`: discrete > integrated > other).
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
    match crate::devices::pick_default_device(devices) {
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

/// Gather CPU/RAM via sysinfo, collect GPU stats from every provider, and pick the
/// one matching `target`.
pub fn gather(sys: &mut System, providers: &[Box<dyn GpuProvider>], target: &Target) -> SystemStats {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu_pct = sys.global_cpu_usage();
    let ram_used_mb = sys.used_memory() / 1024 / 1024; // sysinfo 0.30+ returns bytes
    let ram_total_mb = sys.total_memory() / 1024 / 1024;
    // Skip probing providers entirely when no GPU is targeted (CPU selected): the
    // NVML enumeration + sysfs read_dir would just be discarded by select_gpu.
    let gpu = match target {
        Target::None => None,
        _ => {
            let mut all: Vec<GpuStats> = Vec::new();
            for p in providers {
                all.extend(p.probe());
            }
            select_gpu(&all, target)
        }
    };
    SystemStats {
        gpu,
        cpu_pct,
        ram_used_mb,
        ram_total_mb,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn resolve_target_no_selection_keys_to_default_discrete_device() {
        // Mirrors resolve_backend: the default is the discrete GPU, not banner index 0.
        let devices = vec![gpu_dev(0, "Intel", DeviceKind::Integrated), gpu_dev(1, "NVIDIA", DeviceKind::Discrete), crate::devices::cpu_device()];
        assert_eq!(resolve_target(None, &devices), Target::Name("NVIDIA".into()));
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
}
