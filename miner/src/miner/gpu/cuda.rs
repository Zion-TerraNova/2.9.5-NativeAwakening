//! CUDA GPU mining implementation
//!
//! High-performance mining on NVIDIA GPUs using CUDA.

use super::{GpuDevice, GpuMiner};
#[cfg(feature = "cuda")]
use super::GpuPlatform;
use anyhow::{anyhow, Result};
use std::time::Instant;

#[cfg(feature = "cuda")]
use cudarc::driver::{CudaDevice, CudaSlice, LaunchAsync, LaunchConfig};
#[cfg(feature = "cuda")]
use cudarc::nvrtc::compile_ptx;

/// CUDA kernel source for cosmic_harmony mining
#[cfg(feature = "cuda")]
const CUDA_KERNEL: &str = include_str!("kernels/cosmic_harmony_v3.cu");

/// CUDA miner implementation
pub struct CudaMiner {
    device_id: usize,
    device_info: GpuDevice,
    hashes_computed: u64,
    start_time: Instant,
    #[cfg(feature = "cuda")]
    device: Option<std::sync::Arc<CudaDevice>>,
    #[cfg(feature = "cuda")]
    header_buf: Option<CudaSlice<u8>>,
    #[cfg(feature = "cuda")]
    results_buf: Option<CudaSlice<u64>>,
    #[cfg(feature = "cuda")]
    result_count_buf: Option<CudaSlice<u32>>,
}

impl CudaMiner {
    pub fn new(device_id: usize) -> Result<Self> {
        #[cfg(feature = "cuda")]
        {
            let devices = detect_cuda_devices()?;
            let device_info = devices
                .get(device_id)
                .cloned()
                .ok_or_else(|| anyhow!("CUDA device {} not found", device_id))?;

            Ok(Self {
                device_id,
                device_info,
                hashes_computed: 0,
                start_time: Instant::now(),
                device: None,
                header_buf: None,
                results_buf: None,
                result_count_buf: None,
            })
        }

        #[cfg(not(feature = "cuda"))]
        {
            let _ = device_id;
            Err(anyhow!("CUDA support not enabled. Build with --features cuda"))
        }
    }
}

impl GpuMiner for CudaMiner {
    fn init(&mut self) -> Result<()> {
        #[cfg(feature = "cuda")]
        {
            println!("[CUDA] Initializing device {}", self.device_id);

            let device = CudaDevice::new(self.device_id)?;
            
            // Compile PTX kernel
            println!("[CUDA] Compiling PTX for Cosmic Harmony v3...");
            let ptx = compile_ptx(CUDA_KERNEL).map_err(|e| anyhow!("Failed to compile PTX: {:?}", e))?;
            device.load_ptx(ptx, "cosmic_harmony", &["cosmic_harmony_v3_mine"])?;

            // Allocate buffers
            let header_buf = device.alloc_zeros::<u8>(80)?;
            let results_buf = device.alloc_zeros::<u64>(2)?;
            let result_count_buf = device.alloc_zeros::<u32>(1)?;

            self.device = Some(device);
            self.header_buf = Some(header_buf);
            self.results_buf = Some(results_buf);
            self.result_count_buf = Some(result_count_buf);

            println!("[CUDA] Device {} initialized: {}", self.device_id, self.device_info.name);
            Ok(())
        }

        #[cfg(not(feature = "cuda"))]
        {
            Err(anyhow!("CUDA support not enabled. Build with --features cuda"))
        }
    }

    fn mine_batch(
        &mut self,
        header: &[u8],
        target: &[u8; 32],
        nonce_start: u64,
        batch_size: u64,
    ) -> Result<Option<(u64, [u8; 32])>> {
        #[cfg(feature = "cuda")]
        {
            let device = self.device.as_ref()
                .ok_or_else(|| anyhow!("CUDA device not initialized"))?;
            let header_buf = self.header_buf.as_mut()
                .ok_or_else(|| anyhow!("CUDA header buffer not initialized"))?;
            let results_buf = self.results_buf.as_mut()
                .ok_or_else(|| anyhow!("CUDA results buffer not initialized"))?;
            let result_count_buf = self.result_count_buf.as_mut()
                .ok_or_else(|| anyhow!("CUDA result count buffer not initialized"))?;

            if batch_size == 0 {
                return Ok(None);
            }

            if header.len() > 80 {
                return Err(anyhow!("Header len {} > 80 not supported", header.len()));
            }

            device.htod_copy_into(header.to_vec(), header_buf)?;

            let mut target_u64_bytes = [0u8; 8];
            if target.len() == 32 {
                target_u64_bytes.copy_from_slice(&target[24..32]);
            }
            let target_difficulty = u64::from_le_bytes(target_u64_bytes);

            device.htod_copy_into(vec![0u64, 0u64], results_buf)?;
            device.htod_copy_into(vec![0u32], result_count_buf)?;

            let threads_per_block = 256u32;
            let num_blocks = ((batch_size as u32) + threads_per_block - 1) / threads_per_block;
            let num_blocks = num_blocks.min(65535);

            let func = device.get_func("cosmic_harmony", "cosmic_harmony_v3_mine")
                .ok_or_else(|| anyhow!("CUDA kernel not found"))?;

            let config = LaunchConfig {
                block_dim: (threads_per_block, 1, 1),
                grid_dim: (num_blocks, 1, 1),
                shared_mem_bytes: 0,
            };

            unsafe {
                func.launch(config, (
                    header_buf,
                    header.len() as u32,
                    nonce_start,
                    target_difficulty,
                    results_buf,
                    result_count_buf
                ))?;
            }

            device.synchronize()?;

            let count_vec = device.dtoh_sync_copy(result_count_buf)?;
            if count_vec[0] > 0 {
                let res_vec = device.dtoh_sync_copy(results_buf)?;
                let nonce = res_vec[1];
                return Ok(Some((nonce, [0u8; 32])));
            }

            self.hashes_computed += batch_size;
            Ok(None)
        }

        #[cfg(not(feature = "cuda"))]
        {
            let _ = (header, target, nonce_start, batch_size);
            Err(anyhow!("CUDA support not enabled. Build with --features cuda"))
        }
    }

    fn device_info(&self) -> &GpuDevice {
        &self.device_info
    }

    fn hashrate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            self.hashes_computed as f64 / elapsed
        } else {
            0.0
        }
    }
}

/// Detect CUDA devices
pub fn detect_cuda_devices() -> Result<Vec<GpuDevice>> {
    #[cfg(feature = "cuda")]
    {
        let count = cudarc::driver::result::device::get_count()
            .map_err(|e| anyhow!("Failed to get CUDA device count: {:?}", e))?;

        let mut devices = Vec::new();
        for i in 0..count {
            if let Ok(device) = CudaDevice::new(i) {
                let name = device.name()
                    .unwrap_or_else(|_| format!("CUDA Device {}", i));
                
                let memory_mb = device.total_memory()
                    .map(|m| (m / (1024 * 1024)) as u64)
                    .unwrap_or(0);

                devices.push(GpuDevice {
                    id: i,
                    name,
                    platform: GpuPlatform::Cuda,
                    compute_units: 0,
                    memory_mb,
                });
            }
        }

        Ok(devices)
    }

    #[cfg(not(feature = "cuda"))]
    {
        Ok(vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cuda_detection() {
        if let Ok(devices) = detect_cuda_devices() {
            println!("CUDA devices found: {}", devices.len());
            for dev in &devices {
                println!("  {} - {} ({} MB)", dev.id, dev.name, dev.memory_mb);
            }
        }
    }
}
