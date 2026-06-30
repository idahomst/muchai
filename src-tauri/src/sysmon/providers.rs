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
            // Skip any device whose name/util/memory can't be read this tick; a
            // partially-readable GPU can't populate a meaningful GpuStats. Self-
            // corrects on the next ~1s probe.
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
