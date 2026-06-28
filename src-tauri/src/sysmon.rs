use crate::types::{GpuStats, SystemStats};
use nvml_wrapper::Nvml;
use sysinfo::System;

/// Gather CPU/RAM via sysinfo and (optionally) GPU via NVML.
/// `nvml` = None hides the GPU section (e.g. on non-NVIDIA machines).
pub fn gather(sys: &mut System, nvml: Option<&Nvml>) -> SystemStats {
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu_pct = sys.global_cpu_usage();
    let ram_used_mb = sys.used_memory() / 1024 / 1024; // sysinfo 0.30+ returns bytes
    let ram_total_mb = sys.total_memory() / 1024 / 1024;
    let gpu = nvml.and_then(gather_gpu);
    SystemStats {
        gpu,
        cpu_pct,
        ram_used_mb,
        ram_total_mb,
    }
}

fn gather_gpu(nvml: &Nvml) -> Option<GpuStats> {
    let device = nvml.device_by_index(0).ok()?;
    let name = device.name().ok()?;
    let util = device.utilization_rates().ok()?;
    let mem = device.memory_info().ok()?;
    Some(GpuStats {
        name,
        utilization_pct: util.gpu,
        vram_used_mb: mem.used / 1024 / 1024,
        vram_total_mb: mem.total / 1024 / 1024,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gathers_cpu_and_ram_without_gpu() {
        let mut sys = System::new();
        let stats = gather(&mut sys, None);
        assert!(stats.gpu.is_none());
        assert!(stats.ram_total_mb > 0);
        assert!(stats.ram_used_mb <= stats.ram_total_mb);
    }
}
