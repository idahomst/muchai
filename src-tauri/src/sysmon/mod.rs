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
    /// Report the GPU matching this selected-device name. `uma` marks a
    /// unified-memory device, whose memory total is derived from system RAM
    /// rather than read from the device.
    Device { name: String, uma: bool },
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
            // Only `Integrated` denotes a device whose memory comes from system
            // RAM; every other non-CPU kind (Discrete, Other) has its own pool.
            kind => Target::Device { name: device.name.clone(), uma: kind == DeviceKind::Integrated },
        };
    }
    match crate::devices::pick_default_device(devices) {
        Some(d) => Target::Device { name: d.name.clone(), uma: d.kind == DeviceKind::Integrated },
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

/// Pick the GPU matching `target` from an already-gathered list. For a `Device`
/// match the returned `name` is overwritten with the selected display name, so the
/// monitor shows the familiar Vulkan name while the live numbers come from the
/// provider.
///
/// Deliberately blind to `target`'s `uma` flag: matching is by name only, and
/// substituting the memory budget for a UMA device is `gather`'s job, not this one.
pub fn select_gpu(gpus: &[GpuStats], target: &Target) -> Option<GpuStats> {
    match target {
        Target::None => None,
        Target::Device { name, .. } => gpus
            .iter()
            .find(|g| name_matches(&g.name, name))
            .map(|g| GpuStats { name: name.clone(), ..g.clone() }),
    }
}

/// Gather CPU/RAM via sysinfo, collect GPU stats from every provider, and pick the
/// one matching `target`.
///
/// For a unified-memory target the row is *synthesised* rather than selected: the
/// device has no VRAM pool to read, so its total comes from `budget::uma_budget_mb`
/// (see `uma_override_mb`, the user's Preferences value). Whatever the provider did
/// manage to read — busy%, GTT used — is carried across. This is the only place
/// that knows whether a total came from the device or from the budget, which is
/// what `GpuStats::shared` records.
pub fn gather(
    sys: &mut System,
    providers: &[Box<dyn GpuProvider>],
    target: &Target,
    uma_override_mb: Option<u64>,
) -> SystemStats {
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
            match target {
                // A unified-memory device may match no provider at all (an Intel
                // iGPU publishes nothing in sysfs; a DGX Spark has no provider
                // here), so the row is built from the target rather than found.
                Target::Device { name, uma: true } => {
                    let base = select_gpu(&all, target);
                    Some(GpuStats {
                        name: name.clone(),
                        utilization_pct: base.as_ref().and_then(|b| b.utilization_pct),
                        // GTT used is what a shared device actually consumes; the
                        // amdgpu VRAM figures describe the BIOS carve-out only.
                        vram_used_mb: base.as_ref().and_then(|b| b.shared_used_mb),
                        vram_total_mb: Some(budget::uma_budget_mb(ram_total_mb, uma_override_mb)),
                        shared_used_mb: None,
                        shared: true,
                    })
                }
                _ => select_gpu(&all, target),
            }
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
        let stats = gather(&mut sys, &providers, &Target::None, None);
        assert!(stats.gpu.is_none());
        assert!(stats.ram_total_mb > 0);
        assert!(stats.ram_used_mb <= stats.ram_total_mb);
    }

    use crate::types::{DeviceKind, GpuDevice, GpuSelection};

    fn gpu_dev(index: u32, name: &str, kind: DeviceKind) -> GpuDevice {
        GpuDevice { index, name: name.into(), kind }
    }

    fn stat(name: &str) -> GpuStats {
        GpuStats {
            name: name.into(),
            utilization_pct: Some(7),
            vram_used_mb: Some(100),
            vram_total_mb: Some(200),
            shared_used_mb: None,
            shared: false,
        }
    }

    /// A provider that reports a fixed list and counts how often it was asked.
    struct FakeProvider {
        gpus: Vec<GpuStats>,
        probes: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl GpuProvider for FakeProvider {
        fn probe(&self) -> Vec<GpuStats> {
            self.probes.set(self.probes.get() + 1);
            self.gpus.clone()
        }
    }

    fn fake(gpus: Vec<GpuStats>) -> (Vec<Box<dyn GpuProvider>>, std::rc::Rc<std::cell::Cell<u32>>) {
        let probes = std::rc::Rc::new(std::cell::Cell::new(0));
        let providers: Vec<Box<dyn GpuProvider>> = vec![Box::new(FakeProvider { gpus, probes: probes.clone() })];
        (providers, probes)
    }

    #[test]
    fn gather_uma_target_without_a_provider_match_synthesises_a_row() {
        // The Intel iGPU case: no provider reports it, and today the whole GPU row
        // vanishes. The row must be synthesised from the target name alone.
        let mut sys = System::new();
        let (providers, _) = fake(vec![stat("NVIDIA GeForce RTX 3060")]);
        let target = Target::Device { name: "Intel(R) UHD Graphics 770 (ADL-S GT1)".into(), uma: true };
        let stats = gather(&mut sys, &providers, &target, None);
        let gpu = stats.gpu.expect("a UMA target always yields a row");
        assert_eq!(gpu.name, "Intel(R) UHD Graphics 770 (ADL-S GT1)");
        assert_eq!(gpu.vram_total_mb, Some(budget::uma_budget_mb(stats.ram_total_mb, None)));
        assert_eq!(gpu.vram_used_mb, None);
        assert_eq!(gpu.utilization_pct, None);
        assert_eq!(gpu.shared_used_mb, None);
        assert!(gpu.shared);
    }

    #[test]
    fn gather_uma_target_with_a_match_promotes_gtt_used_over_the_carve_out() {
        // The AMD APU case: mem_info_vram_total is the BIOS carve-out and must be
        // replaced by the budget, while busy% and GTT-used pass through.
        let mut sys = System::new();
        let apu = GpuStats {
            name: "AMD".into(),
            utilization_pct: Some(71),
            vram_used_mb: Some(128),
            vram_total_mb: Some(512), // carve-out
            shared_used_mb: Some(6400),
            shared: false,
        };
        let (providers, _) = fake(vec![apu]);
        let target = Target::Device { name: "AMD Radeon 890M".into(), uma: true };
        let stats = gather(&mut sys, &providers, &target, None);
        let gpu = stats.gpu.expect("a UMA target always yields a row");
        assert_eq!(gpu.name, "AMD Radeon 890M");
        assert_eq!(gpu.vram_total_mb, Some(budget::uma_budget_mb(stats.ram_total_mb, None)));
        assert_eq!(gpu.vram_used_mb, Some(6400), "GTT used, not the 128 MB carve-out use");
        assert_eq!(gpu.utilization_pct, Some(71));
        assert_eq!(gpu.shared_used_mb, None, "promoted into vram_used_mb, not duplicated");
        assert!(gpu.shared);
    }

    #[test]
    fn gather_uma_override_changes_only_the_total() {
        let mut sys = System::new();
        let (providers, _) = fake(vec![]);
        let target = Target::Device { name: "Intel".into(), uma: true };
        let stats = gather(&mut sys, &providers, &target, Some(6000));
        let gpu = stats.gpu.expect("a UMA target always yields a row");
        assert_eq!(gpu.vram_total_mb, Some(budget::uma_budget_mb(stats.ram_total_mb, Some(6000))));
        assert_eq!(gpu.name, "Intel");
        assert!(gpu.shared);
    }

    #[test]
    fn gather_discrete_target_is_untouched_by_the_budget() {
        // Regression guard: a discrete card must report exactly what the provider
        // said, with shared: false, no matter what the budget would have been.
        let mut sys = System::new();
        let (providers, _) = fake(vec![stat("NVIDIA GeForce RTX 3060")]);
        let target = Target::Device { name: "NVIDIA GeForce RTX 3060".into(), uma: false };
        let stats = gather(&mut sys, &providers, &target, Some(6000));
        let gpu = stats.gpu.expect("the provider reports this card");
        assert_eq!(gpu.vram_total_mb, Some(200));
        assert_eq!(gpu.vram_used_mb, Some(100));
        assert_eq!(gpu.utilization_pct, Some(7));
        assert!(!gpu.shared);
    }

    #[test]
    fn gather_none_target_does_not_probe_providers() {
        // The stats loop runs every second for the life of the app; enumerating
        // NVML and read_dir-ing sysfs only to discard the result is waste.
        let mut sys = System::new();
        let (providers, probes) = fake(vec![stat("NVIDIA")]);
        let stats = gather(&mut sys, &providers, &Target::None, None);
        assert!(stats.gpu.is_none());
        assert_eq!(probes.get(), 0);
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
        assert_eq!(resolve_target(sel, &devices), Target::Device { name: "NVIDIA GeForce RTX 3060".into(), uma: false });
    }

    #[test]
    fn resolve_target_marks_an_integrated_selection_as_uma() {
        // The uma flag is what makes gather substitute a budget; it comes from the
        // engine banner's own "uma: 1" marker via DeviceKind::Integrated.
        let devices = vec![gpu_dev(0, "Intel(R) UHD Graphics 770", DeviceKind::Integrated), gpu_dev(1, "NVIDIA", DeviceKind::Discrete)];
        let sel = Some(GpuSelection { index: 0, name: "Intel(R) UHD Graphics 770".into() });
        assert_eq!(resolve_target(sel, &devices), Target::Device { name: "Intel(R) UHD Graphics 770".into(), uma: true });
    }

    #[test]
    fn resolve_target_no_selection_keys_to_default_discrete_device() {
        // Mirrors resolve_backend: the default is the discrete GPU, not banner index 0.
        let devices = vec![gpu_dev(0, "Intel", DeviceKind::Integrated), gpu_dev(1, "NVIDIA", DeviceKind::Discrete), crate::devices::cpu_device()];
        assert_eq!(resolve_target(None, &devices), Target::Device { name: "NVIDIA".into(), uma: false });
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
        // Falling back to the default device must carry its uma flag too, or an
        // integrated-only machine would lose its budget on a stale selection.
        assert_eq!(resolve_target(stale.clone(), &with_gpu), Target::Device { name: "Intel".into(), uma: true });
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
        let target = Target::Device { name: "NVIDIA GeForce RTX 3060".into(), uma: false };
        let got = select_gpu(&gpus, &target).unwrap();
        assert_eq!(got.name, "NVIDIA GeForce RTX 3060");
        assert_eq!(got.vram_total_mb, Some(200));
        // vendor-keyword provider name still matches the full selected name
        let amd = vec![stat("AMD")];
        let target = Target::Device { name: "AMD Radeon RX 7900 XTX".into(), uma: false };
        let got = select_gpu(&amd, &target).unwrap();
        assert_eq!(got.name, "AMD Radeon RX 7900 XTX"); // overwritten with the selected display name
    }

    #[test]
    fn select_gpu_name_no_match_or_empty_yields_none() {
        let target = Target::Device { name: "NVIDIA".into(), uma: false };
        assert_eq!(select_gpu(&[stat("Intel")], &target), None);
        assert_eq!(select_gpu(&[], &target), None);
    }
}
