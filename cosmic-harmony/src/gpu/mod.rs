//! GPU mining support for Cosmic Harmony v3
//! 
//! Supports:
//! - OpenCL (AMD, NVIDIA, Intel)
//! - Metal (Apple Silicon) - Native macOS performance
//! - CUDA (optional)

pub mod opencl_kernel;

#[cfg(feature = "gpu")]
pub mod gpu_miner;

#[cfg(feature = "gpu")]
pub use gpu_miner::{GpuMiner, GpuConfig};

// Metal backend for macOS — CHv3
#[cfg(all(feature = "metal", target_os = "macos"))]
pub mod metal_miner;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub use metal_miner::MetalMiner;

// Metal backend for macOS — Ethash (ETC mining)
#[cfg(all(feature = "metal", target_os = "macos"))]
pub mod ethash_metal_miner;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub use ethash_metal_miner::{EthashMetalMiner, EthashEpoch, EthashDagGenerator};

// Metal backend for macOS — Autolykos2 (ERG mining)
#[cfg(all(feature = "metal", target_os = "macos"))]
pub mod autolykos2_metal_miner;

#[cfg(all(feature = "metal", target_os = "macos"))]
pub use autolykos2_metal_miner::{autolykos2_hash_cpu, AutolykosMetalMiner, AutolykosTableInfo};

// Metal FFI exports
#[cfg(feature = "metal")]
pub mod metal_ffi;

/// GPU backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuBackend {
    OpenCL,
    Metal,
    Cuda,
}

/// GPU device info
#[derive(Debug, Clone)]
pub struct GpuDevice {
    pub id: usize,
    pub name: String,
    pub vendor: String,
    pub backend: GpuBackend,
    pub compute_units: u32,
    pub max_work_group_size: usize,
    pub global_memory: u64,
    pub local_memory: u64,
}

impl std::fmt::Display for GpuDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] {} ({}) - {} CUs, {} MB",
            self.id,
            self.name,
            self.vendor,
            self.compute_units,
            self.global_memory / (1024 * 1024)
        )
    }
}
