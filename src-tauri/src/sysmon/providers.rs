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
