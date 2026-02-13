//! GPU mining support for ZION
//!
//! Supports CUDA and OpenCL backends for high-performance mining.

use anyhow::Result;

mod cuda;
mod opencl;
pub mod metal;
pub mod benchmark;

pub use cuda::CudaMiner;
pub use opencl::OpenCLMiner;
pub use metal::MetalGpuMiner;
pub use benchmark::{
    auto_tune, run_benchmark, print_benchmark_results,
    AutoTuneConfig,
};

/// GPU platform type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuPlatform {
    /// NVIDIA CUDA
    Cuda,
    /// OpenCL (AMD, Intel, NVIDIA)
    OpenCL,
    /// Apple Metal (M1/M2/M3/M4/M5)
    Metal,
}

/// GPU device information
#[derive(Debug, Clone)]
pub struct GpuDevice {
    pub id: usize,
    pub name: String,
    pub platform: GpuPlatform,
    pub compute_units: u32,
    pub memory_mb: u64,
}

/// GPU mining interface
pub trait GpuMiner: Send + Sync {
    /// Initialize GPU device
    fn init(&mut self) -> Result<()>;
    
    /// Mine with GPU (returns hash if found)
    fn mine_batch(
        &mut self,
        header: &[u8],
        target: &[u8; 32],
        nonce_start: u64,
        batch_size: u64,
    ) -> Result<Option<(u64, [u8; 32])>>;
    
    /// Get device information
    fn device_info(&self) -> &GpuDevice;
    
    /// Get current hashrate
    fn hashrate(&self) -> f64;
}

/// Detect available GPU devices
pub fn detect_gpus() -> Result<Vec<GpuDevice>> {
    let mut devices = Vec::new();
    
    // Try Metal first (Apple Silicon — fastest on macOS)
    if let Ok(metal_devices) = metal::detect_metal_devices() {
        if !metal_devices.is_empty() {
            log::debug!("Metal GPU detected: {} device(s)", metal_devices.len());
            devices.extend(metal_devices);
        }
    }
    
    // Try CUDA (NVIDIA)
    if let Ok(cuda_devices) = cuda::detect_cuda_devices() {
        devices.extend(cuda_devices);
    }
    
    // Try OpenCL (AMD, Intel, fallback NVIDIA)
    if let Ok(opencl_devices) = opencl::detect_opencl_devices() {
        // Filter out duplicates (NVIDIA cards already in CUDA, Apple already in Metal)
        let existing_names: Vec<String> = devices.iter()
            .map(|d| d.name.clone())
            .collect();
        
        for dev in opencl_devices {
            if !existing_names.iter().any(|n| dev.name.contains(n)) {
                devices.push(dev);
            }
        }
    }
    
    Ok(devices)
}

/// Create GPU miner for device
pub fn create_miner(device: &GpuDevice) -> Result<Box<dyn GpuMiner>> {
    match device.platform {
        GpuPlatform::Metal => {
            // Metal: use 500K batch for optimal throughput on Apple Silicon
            let miner = MetalGpuMiner::new(500_000)?;
            Ok(Box::new(miner))
        }
        GpuPlatform::Cuda => {
            let miner = CudaMiner::new(device.id)?;
            Ok(Box::new(miner))
        }
        GpuPlatform::OpenCL => {
            let miner = OpenCLMiner::new(device.id)?;
            Ok(Box::new(miner))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_gpus() {
        // Should not panic, even if no GPUs found
        let result = detect_gpus();
        assert!(result.is_ok());
        
        if let Ok(devices) = result {
            println!("Found {} GPU(s)", devices.len());
            for dev in devices {
                println!("  - {} ({:?}, {} CUs, {} MB)", 
                    dev.name, dev.platform, dev.compute_units, dev.memory_mb);
            }
        }
    }
}
