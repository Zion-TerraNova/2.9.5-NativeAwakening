//! GPU Miner implementation for Cosmic Harmony v3
//!
//! Supports OpenCL backends (AMD, NVIDIA, Intel)

use super::{GpuBackend, GpuDevice};
use super::opencl_kernel::get_kernel_source;
use anyhow::{Result, Context, anyhow};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

#[cfg(feature = "gpu")]
use opencl3::command_queue::{CommandQueue, CL_QUEUE_PROFILING_ENABLE, CL_BLOCKING};
#[cfg(feature = "gpu")]
use opencl3::context::Context as ClContext;
#[cfg(feature = "gpu")]
use opencl3::device::{get_all_devices, Device, CL_DEVICE_TYPE_GPU};
#[cfg(feature = "gpu")]
use opencl3::kernel::{ExecuteKernel, Kernel};
#[cfg(feature = "gpu")]
use opencl3::memory::{Buffer, CL_MEM_READ_ONLY, CL_MEM_READ_WRITE, CL_MEM_WRITE_ONLY};
#[cfg(feature = "gpu")]
use opencl3::program::Program;
#[cfg(feature = "gpu")]
use opencl3::types::{cl_uchar, cl_uint, cl_ulong};

/// GPU Mining configuration
#[derive(Debug, Clone)]
pub struct GpuConfig {
    /// Device ID to use
    pub device_id: usize,
    /// Batch size (work items per kernel launch)
    pub batch_size: usize,
    /// Work group size
    pub work_group_size: usize,
    /// Enable kernel profiling
    pub profiling: bool,
}

impl Default for GpuConfig {
    fn default() -> Self {
        Self {
            device_id: 0,
            batch_size: 1_000_000,  // 1M hashes per batch
            work_group_size: 256,
            profiling: false,
        }
    }
}

/// GPU Miner for Cosmic Harmony v3
#[cfg(feature = "gpu")]
pub struct GpuMiner {
    config: GpuConfig,
    device: Device,
    context: ClContext,
    queue: CommandQueue,
    program: Program,
    kernel_mine: Kernel,
    kernel_batch: Kernel,
    
    // Buffers
    header_buffer: Buffer<cl_uchar>,
    target_buffer: Buffer<cl_uchar>,
    found_nonce_buffer: Buffer<cl_ulong>,
    found_hash_buffer: Buffer<cl_uchar>,
    solution_count_buffer: Buffer<cl_uint>,
    
    // Stats
    total_hashes: AtomicU64,
    solutions_found: AtomicU64,
    running: AtomicBool,
}

#[cfg(feature = "gpu")]
impl GpuMiner {
    /// Create new GPU miner
    pub fn new(config: GpuConfig) -> Result<Self> {
        // Get GPU devices
        let device_ids = get_all_devices(CL_DEVICE_TYPE_GPU)
            .context("Failed to get GPU devices")?;
        
        if device_ids.is_empty() {
            return Err(anyhow!("No GPU devices found"));
        }
        
        if config.device_id >= device_ids.len() {
            return Err(anyhow!(
                "Invalid device ID {}. Available: 0-{}",
                config.device_id,
                device_ids.len() - 1
            ));
        }
        
        let device = Device::new(device_ids[config.device_id]);
        let device_name = device.name().unwrap_or_default();
        
        log::info!("Using GPU: {} (ID: {})", device_name, config.device_id);
        
        // Create context
        let context = ClContext::from_device(&device)
            .context("Failed to create OpenCL context")?;
        
        // Create command queue
        let queue_props = if config.profiling {
            CL_QUEUE_PROFILING_ENABLE
        } else {
            0
        };
        let queue = CommandQueue::create_default_with_properties(&context, queue_props, 0)
            .context("Failed to create command queue")?;
        
        // Build program
        let kernel_source = get_kernel_source(true);
        let program = Program::create_and_build_from_source(&context, &kernel_source, "")
            .map_err(|e| anyhow!("Failed to build OpenCL program: {}", e))?;
        
        // Create kernels
        let kernel_mine = Kernel::create(&program, "cosmic_harmony_v3_mine")
            .context("Failed to create mining kernel")?;
        let kernel_batch = Kernel::create(&program, "cosmic_harmony_v3_batch")
            .context("Failed to create batch kernel")?;
        
        // Create buffers
        let header_buffer = unsafe {
            Buffer::<cl_uchar>::create(&context, CL_MEM_READ_ONLY, 144, std::ptr::null_mut())
                .context("Failed to create header buffer")?
        };
        
        let target_buffer = unsafe {
            Buffer::<cl_uchar>::create(&context, CL_MEM_READ_ONLY, 32, std::ptr::null_mut())
                .context("Failed to create target buffer")?
        };
        
        let found_nonce_buffer = unsafe {
            Buffer::<cl_ulong>::create(&context, CL_MEM_WRITE_ONLY, 1, std::ptr::null_mut())
                .context("Failed to create nonce buffer")?
        };
        
        let found_hash_buffer = unsafe {
            Buffer::<cl_uchar>::create(&context, CL_MEM_WRITE_ONLY, 32, std::ptr::null_mut())
                .context("Failed to create hash buffer")?
        };
        
        let solution_count_buffer = unsafe {
            Buffer::<cl_uint>::create(&context, CL_MEM_READ_WRITE, 1, std::ptr::null_mut())
                .context("Failed to create solution count buffer")?
        };
        
        Ok(Self {
            config,
            device,
            context,
            queue,
            program,
            kernel_mine,
            kernel_batch,
            header_buffer,
            target_buffer,
            found_nonce_buffer,
            found_hash_buffer,
            solution_count_buffer,
            total_hashes: AtomicU64::new(0),
            solutions_found: AtomicU64::new(0),
            running: AtomicBool::new(false),
        })
    }
    
    /// List available GPU devices
    pub fn list_devices() -> Result<Vec<GpuDevice>> {
        let device_ids = get_all_devices(CL_DEVICE_TYPE_GPU)
            .context("Failed to get GPU devices")?;
        
        let mut devices = Vec::new();
        
        for (id, device_id) in device_ids.iter().enumerate() {
            let device = Device::new(*device_id);
            
            devices.push(GpuDevice {
                id,
                name: device.name().unwrap_or_default(),
                vendor: device.vendor().unwrap_or_default(),
                backend: GpuBackend::OpenCL,
                compute_units: device.max_compute_units().unwrap_or(0),
                max_work_group_size: device.max_work_group_size().unwrap_or(0),
                global_memory: device.global_mem_size().unwrap_or(0),
                local_memory: device.local_mem_size().unwrap_or(0),
            });
        }
        
        Ok(devices)
    }
    
    /// Mine for a valid nonce
    pub fn mine(
        &mut self,
        block_header: &[u8],
        start_nonce: u64,
        target: &[u8; 32],
    ) -> Result<Option<(u64, [u8; 32])>> {
        if block_header.len() > 136 {
            return Err(anyhow!("Block header too large (max 136 bytes)"));
        }
        
        // Prepare header (pad to 144 bytes)
        let mut header = [0u8; 144];
        header[..block_header.len()].copy_from_slice(block_header);
        
        // Upload buffers
        unsafe {
            self.queue.enqueue_write_buffer(
                &mut self.header_buffer,
                CL_BLOCKING,
                0,
                &header,
                &[],
            )?;
            
            self.queue.enqueue_write_buffer(
                &mut self.target_buffer,
                CL_BLOCKING,
                0,
                target,
                &[],
            )?;
            
            // Reset solution count
            let zero: [cl_uint; 1] = [0];
            self.queue.enqueue_write_buffer(
                &mut self.solution_count_buffer,
                CL_BLOCKING,
                0,
                &zero,
                &[],
            )?;
        }
        
        // Execute kernel
        let header_len = block_header.len() as cl_uint;
        let global_work_size = self.config.batch_size;
        let local_work_size = self.config.work_group_size;
        
        unsafe {
            ExecuteKernel::new(&self.kernel_mine)
                .set_arg(&self.header_buffer)
                .set_arg(&header_len)
                .set_arg(&start_nonce)
                .set_arg(&self.target_buffer)
                .set_arg(&self.found_nonce_buffer)
                .set_arg(&self.found_hash_buffer)
                .set_arg(&self.solution_count_buffer)
                .set_global_work_size(global_work_size)
                .set_local_work_size(local_work_size)
                .enqueue_nd_range(&self.queue)?;
        }
        
        self.queue.finish()?;
        
        // Update stats
        self.total_hashes.fetch_add(self.config.batch_size as u64, Ordering::Relaxed);
        
        // Read results
        let mut solution_count = [0u32; 1];
        unsafe {
            self.queue.enqueue_read_buffer(
                &self.solution_count_buffer,
                CL_BLOCKING,
                0,
                &mut solution_count,
                &[],
            )?;
        }
        
        if solution_count[0] > 0 {
            let mut found_nonce = [0u64; 1];
            let mut found_hash = [0u8; 32];
            
            unsafe {
                self.queue.enqueue_read_buffer(
                    &self.found_nonce_buffer,
                    CL_BLOCKING,
                    0,
                    &mut found_nonce,
                    &[],
                )?;
                
                self.queue.enqueue_read_buffer(
                    &self.found_hash_buffer,
                    CL_BLOCKING,
                    0,
                    &mut found_hash,
                    &[],
                )?;
            }
            
            self.solutions_found.fetch_add(1, Ordering::Relaxed);
            
            return Ok(Some((found_nonce[0], found_hash)));
        }
        
        Ok(None)
    }
    
    /// Get hashrate (hashes per second)
    pub fn get_hashrate(&self) -> f64 {
        // This would need timing integration
        0.0
    }
    
    /// Get total hashes computed
    pub fn total_hashes(&self) -> u64 {
        self.total_hashes.load(Ordering::Relaxed)
    }
    
    /// Get solutions found
    pub fn solutions_found(&self) -> u64 {
        self.solutions_found.load(Ordering::Relaxed)
    }
    
    /// Get device info
    pub fn device_info(&self) -> GpuDevice {
        GpuDevice {
            id: self.config.device_id,
            name: self.device.name().unwrap_or_default(),
            vendor: self.device.vendor().unwrap_or_default(),
            backend: GpuBackend::OpenCL,
            compute_units: self.device.max_compute_units().unwrap_or(0),
            max_work_group_size: self.device.max_work_group_size().unwrap_or(0),
            global_memory: self.device.global_mem_size().unwrap_or(0),
            local_memory: self.device.local_mem_size().unwrap_or(0),
        }
    }
}

// Stub for non-GPU builds
#[cfg(not(feature = "gpu"))]
pub struct GpuMiner;

#[cfg(not(feature = "gpu"))]
impl GpuMiner {
    pub fn new(_config: GpuConfig) -> Result<Self> {
        Err(anyhow!("GPU support not compiled. Enable 'gpu' feature."))
    }
    
    pub fn list_devices() -> Result<Vec<GpuDevice>> {
        Err(anyhow!("GPU support not compiled. Enable 'gpu' feature."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_gpu_config_default() {
        let config = GpuConfig::default();
        assert_eq!(config.device_id, 0);
        assert_eq!(config.batch_size, 1_000_000);
        assert_eq!(config.work_group_size, 256);
    }
}
