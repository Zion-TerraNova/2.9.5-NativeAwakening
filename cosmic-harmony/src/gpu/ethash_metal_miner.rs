//! Ethash Metal GPU backend for macOS/Apple Silicon
//!
//! Uses Apple Metal API for native Ethash mining (ETC, etc.)
//! Reuses the same Metal Device as CHv3 miner for dual-stream mining.
//!
//! Key differences from CHv3:
//! - Requires DAG buffer (~2.4 GB for current ETC epoch)
//! - DAG regenerated per epoch (every 30,000 blocks)
//! - Returns (nonce, mix_digest, result_hash) — pool needs mix_digest
//!
//! Buffer layout:
//!   buffer(0) = EthashMiningParams { start_nonce, dag_num_items, header_hash[32], target[32] }
//!   buffer(1) = DAG (full dataset, ~2.4 GB)
//!   buffer(2) = EthashMiningResult { found_nonce, mix_digest[32], result_hash[32], found }

use std::path::Path;

#[cfg(target_os = "macos")]
use metal::{
    Buffer, CommandQueue, ComputePipelineState,
    Device, Library, MTLResourceOptions, MTLSize,
};

use super::{GpuBackend, GpuDevice};

/// Ethash Mining Parameters — matches ethash_shader.metal struct
/// Layout:
///   offset 0:  uint64_t start_nonce  (8 bytes)
///   offset 8:  uint32_t dag_num_items (4 bytes) 
///   offset 12: uint8_t header_hash[32] (32 bytes)
///   offset 44: uint8_t target[32] (32 bytes)
///   Total: 76 → padded to 80
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone)]
pub struct EthashMiningParams {
    pub start_nonce: u64,       // offset 0, size 8
    pub dag_num_items: u32,     // offset 8, size 4
    pub header_hash: [u8; 32],  // offset 12, size 32
    pub target: [u8; 32],       // offset 44, size 32
}

/// Ethash Mining Result — matches ethash_shader.metal struct
/// Layout:
///   offset 0:  uint64_t found_nonce (8 bytes)
///   offset 8:  uint8_t mix_digest[32] (32 bytes)
///   offset 40: uint8_t result_hash[32] (32 bytes)
///   offset 72: uint32_t found (4 bytes)
///   Total: 76 → padded to 80
#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone)]
pub struct EthashMiningResult {
    pub found_nonce: u64,
    pub mix_digest: [u8; 32],
    pub result_hash: [u8; 32],
    pub found: u32,
}

/// Ethash epoch info
#[derive(Debug, Clone)]
pub struct EthashEpoch {
    pub number: u64,
    pub seed_hash: [u8; 32],
    pub dataset_size: u64,    // in bytes
    pub dataset_items: u32,   // number of 64-byte items
    pub cache_size: u64,
}

impl EthashEpoch {
    /// Calculate epoch from block height (ETC uses 30,000 blocks per epoch)
    pub fn from_height(height: u64) -> Self {
        let epoch_number = height / 30_000;
        let seed_hash = Self::compute_seed_hash(epoch_number);
        let dataset_size = Self::dataset_size(epoch_number);
        let cache_size = Self::cache_size(epoch_number);
        
        Self {
            number: epoch_number,
            seed_hash,
            dataset_size,
            dataset_items: (dataset_size / 64) as u32,
            cache_size,
        }
    }
    
    /// Calculate epoch from seed hash (received from pool)
    pub fn from_seed_hash(seed_hash_hex: &str) -> Self {
        // Count how many Keccak-256 rounds produce this seed
        let target_seed = hex_to_bytes32(seed_hash_hex);
        let mut seed = [0u8; 32];
        let mut epoch = 0u64;
        
        // Max 2048 epochs (plenty for ETC)
        for ep in 0..2048 {
            if seed == target_seed {
                epoch = ep;
                break;
            }
            seed = keccak256_cpu(&seed);
        }
        
        let dataset_size = Self::dataset_size(epoch);
        let cache_size = Self::cache_size(epoch);
        
        Self {
            number: epoch,
            seed_hash: target_seed,
            dataset_size,
            dataset_items: (dataset_size / 64) as u32,
            cache_size,
        }
    }
    
    fn compute_seed_hash(epoch: u64) -> [u8; 32] {
        let mut seed = [0u8; 32];
        for _ in 0..epoch {
            seed = keccak256_cpu(&seed);
        }
        seed
    }
    
    /// Dataset size for epoch (approximate, using prime search)
    fn dataset_size(epoch: u64) -> u64 {
        // Ethash dataset size growth: starts at ~1 GB, grows ~8 MB/epoch
        // Formula: dataset_size = DATASET_BYTES_INIT + DATASET_BYTES_GROWTH * epoch
        // Then round down to largest prime × MIX_BYTES (128)
        let init: u64 = 1_073_741_824; // 2^30 = 1 GB
        let growth: u64 = 8_388_608;   // 2^23 = 8 MB
        let mix_bytes: u64 = 128;
        
        let mut size = init + growth * epoch;
        // Round down to multiple of 128 that's also prime/128
        size = (size / mix_bytes) * mix_bytes;
        
        // Find largest prime * 128 <= size
        while !is_prime(size / mix_bytes) {
            size -= mix_bytes;
        }
        
        size
    }
    
    /// Cache size for epoch
    fn cache_size(epoch: u64) -> u64 {
        let init: u64 = 16_777_216; // 2^24 = 16 MB
        let growth: u64 = 131_072;  // 2^17 = 128 KB
        let hash_bytes: u64 = 64;
        
        let mut size = init + growth * epoch;
        size = (size / hash_bytes) * hash_bytes;
        
        while !is_prime(size / hash_bytes) {
            size -= hash_bytes;
        }
        
        size
    }
}

/// Simple primality test (sufficient for Ethash sizes)
fn is_prime(n: u64) -> bool {
    if n < 2 { return false; }
    if n < 4 { return true; }
    if n % 2 == 0 || n % 3 == 0 { return false; }
    let mut i = 5u64;
    while i * i <= n {
        if n % i == 0 || n % (i + 2) == 0 { return false; }
        i += 6;
    }
    true
}

/// CPU Keccak-256 for seed hash computation
fn keccak256_cpu(input: &[u8]) -> [u8; 32] {
    use tiny_keccak::{Hasher, Keccak};
    let mut hasher = Keccak::v256();
    hasher.update(input);
    let mut output = [0u8; 32];
    hasher.finalize(&mut output);
    output
}

/// CPU Keccak-512 for cache generation
fn keccak512_cpu(input: &[u8]) -> [u8; 64] {
    use tiny_keccak::{Hasher, Keccak};
    let mut hasher = Keccak::v512();
    hasher.update(input);
    let mut output = [0u8; 64];
    hasher.finalize(&mut output);
    output
}

fn hex_to_bytes32(hex: &str) -> [u8; 32] {
    let hex = hex.trim_start_matches("0x");
    let mut bytes = [0u8; 32];
    let decoded = hex::decode(hex).unwrap_or_default();
    let start = 32usize.saturating_sub(decoded.len());
    bytes[start..].copy_from_slice(&decoded[..decoded.len().min(32)]);
    bytes
}

/// Ethash DAG generator (CPU-based, uploaded to Metal buffer)
pub struct EthashDagGenerator;

impl EthashDagGenerator {
    /// Generate the full DAG for a given epoch
    /// This is CPU-intensive (~30-60 seconds for ~2.4 GB)
    pub fn generate_dag(epoch: &EthashEpoch) -> Vec<u8> {
        log::debug!("🔧 Generating Ethash DAG for epoch {} ({:.2} GB)...", 
            epoch.number, epoch.dataset_size as f64 / 1_073_741_824.0);
        
        let start = std::time::Instant::now();
        
        // Step 1: Generate cache
        let cache = Self::generate_cache(epoch);
        log::debug!("   Cache generated: {} MB in {:.1}s", 
            cache.len() / (1024 * 1024), start.elapsed().as_secs_f64());
        
        // Step 2: Generate dataset from cache
        let dataset = Self::generate_dataset(epoch, &cache);
        log::debug!("   DAG generated: {} MB in {:.1}s",
            dataset.len() / (1024 * 1024), start.elapsed().as_secs_f64());
        
        dataset
    }
    
    /// Generate cache from seed hash
    fn generate_cache(epoch: &EthashEpoch) -> Vec<[u8; 64]> {
        let cache_items = (epoch.cache_size / 64) as usize;
        let mut cache = Vec::with_capacity(cache_items);
        
        // First item = Keccak-512(seed_hash)
        cache.push(keccak512_cpu(&epoch.seed_hash));
        
        // Subsequent items = Keccak-512(previous_item)
        for i in 1..cache_items {
            let prev = cache[i - 1];
            cache.push(keccak512_cpu(&prev));
        }
        
        // RandMemoHash: 3 rounds of cache mixing
        let cache_len = cache.len();
        for _ in 0..3 {
            for i in 0..cache_len {
                let v = u32::from_le_bytes([
                    cache[i][0], cache[i][1], cache[i][2], cache[i][3]
                ]) as usize % cache_len;
                
                let prev_idx = if i == 0 { cache_len - 1 } else { i - 1 };
                
                let mut xored = [0u8; 64];
                for j in 0..64 {
                    xored[j] = cache[prev_idx][j] ^ cache[v][j];
                }
                cache[i] = keccak512_cpu(&xored);
            }
        }
        
        cache
    }
    
    /// Generate full dataset from cache
    fn generate_dataset(epoch: &EthashEpoch, cache: &[[u8; 64]]) -> Vec<u8> {
        let dataset_items = epoch.dataset_items as usize;
        let mut dataset = vec![0u8; dataset_items * 64];
        
        let cache_len = cache.len();
        
        // Use rayon for parallel generation if available
        // For now, single-threaded (still fast enough for ETC ~2.4 GB)
        for i in 0..dataset_items {
            let mut mix = cache[i % cache_len];
            
            // XOR with item index
            let idx_bytes = (i as u32).to_le_bytes();
            mix[0] ^= idx_bytes[0];
            mix[1] ^= idx_bytes[1];
            mix[2] ^= idx_bytes[2];
            mix[3] ^= idx_bytes[3];
            
            mix = keccak512_cpu(&mix);
            
            // 256 parent lookups with FNV
            for j in 0..256usize {
                let parent_idx = fnv_cpu(
                    i as u32 ^ j as u32,
                    u32::from_le_bytes([
                        mix[(j % 16) * 4],
                        mix[(j % 16) * 4 + 1],
                        mix[(j % 16) * 4 + 2],
                        mix[(j % 16) * 4 + 3],
                    ])
                ) as usize % cache_len;
                
                for k in 0..64 {
                    mix[k] = fnv_byte(mix[k], cache[parent_idx][k]);
                }
            }
            
            mix = keccak512_cpu(&mix);
            dataset[i * 64..(i + 1) * 64].copy_from_slice(&mix);
            
            // Progress reporting
            if i % 1_000_000 == 0 && i > 0 {
                log::debug!("   DAG progress: {:.1}%", (i as f64 / dataset_items as f64) * 100.0);
            }
        }
        
        dataset
    }
}

fn fnv_cpu(u: u32, v: u32) -> u32 {
    u.wrapping_mul(0x01000193) ^ v
}

fn fnv_byte(a: u8, b: u8) -> u8 {
    let r = (a as u32).wrapping_mul(0x01000193) ^ (b as u32);
    r as u8
}

// ============================================================================
// Metal Ethash Miner
// ============================================================================

#[cfg(target_os = "macos")]
pub struct EthashMetalMiner {
    device: Device,
    command_queue: CommandQueue,
    pipeline_mine: ComputePipelineState,
    pipeline_benchmark: ComputePipelineState,
    
    // Buffers
    params_buf: Buffer,
    dag_buf: Option<Buffer>,
    result_buf: Buffer,
    hashes_buf: Option<Buffer>,
    
    // Epoch tracking
    current_epoch: Option<u64>,
    dag_items: u32,
    
    // Config
    batch_size: usize,
    threads_per_threadgroup: usize,
    
    // Stats
    total_hashes: u64,
    solutions_found: u64,
}

#[cfg(target_os = "macos")]
impl EthashMetalMiner {
    /// Create new Ethash Metal miner (can share Device with CHv3 miner)
    pub fn new(batch_size: usize) -> Result<Self, EthashMetalError> {
        let device = Device::system_default()
            .ok_or(EthashMetalError::NoDevice)?;
        
        Self::new_with_device(device, batch_size)
    }
    
    /// Create with explicit Metal device (for dual-stream: share device with CHv3)
    pub fn new_with_device(device: Device, batch_size: usize) -> Result<Self, EthashMetalError> {
        log::debug!("🍎 Ethash Metal miner initializing on: {}", device.name());
        log::debug!("   Available memory: {} MB", 
            device.recommended_max_working_set_size() / (1024 * 1024));
        
        // Separate command queue for Ethash (dual-stream with CHv3)
        let command_queue = device.new_command_queue();
        
        // Load Ethash shader
        let library = Self::load_shader_library(&device)?;
        
        let mine_fn = library.get_function("ethash_mine", None)
            .map_err(|e| EthashMetalError::FunctionNotFound(format!("ethash_mine: {:?}", e)))?;
        let benchmark_fn = library.get_function("ethash_benchmark", None)
            .map_err(|e| EthashMetalError::FunctionNotFound(format!("ethash_benchmark: {:?}", e)))?;
        
        let pipeline_mine = device.new_compute_pipeline_state_with_function(&mine_fn)
            .map_err(|e| EthashMetalError::PipelineError(format!("{:?}", e)))?;
        let pipeline_benchmark = device.new_compute_pipeline_state_with_function(&benchmark_fn)
            .map_err(|e| EthashMetalError::PipelineError(format!("{:?}", e)))?;
        
        let max_threads = pipeline_mine.max_total_threads_per_threadgroup();
        let threads_per_threadgroup = (max_threads as usize).min(256); // Ethash is more register-heavy
        
        let options = MTLResourceOptions::StorageModeShared;
        
        let params_size = std::mem::size_of::<EthashMiningParams>() as u64;
        let result_size = std::mem::size_of::<EthashMiningResult>() as u64;
        
        let params_buf = device.new_buffer(params_size, options);
        let result_buf = device.new_buffer(result_size, options);
        
        log::debug!("   Params struct: {} bytes", params_size);
        log::debug!("   Result struct: {} bytes", result_size);
        log::debug!("   Threads per threadgroup: {}", threads_per_threadgroup);
        log::debug!("   Batch size: {}", batch_size);
        log::debug!("✅ Ethash Metal miner ready (DAG not loaded yet)");
        
        Ok(Self {
            device,
            command_queue,
            pipeline_mine,
            pipeline_benchmark,
            params_buf,
            dag_buf: None,
            result_buf,
            hashes_buf: None,
            current_epoch: None,
            dag_items: 0,
            batch_size,
            threads_per_threadgroup,
            total_hashes: 0,
            solutions_found: 0,
        })
    }
    
    /// Load Ethash Metal shader library
    fn load_shader_library(device: &Device) -> Result<Library, EthashMetalError> {
        // Try pre-compiled metallib first
        let metallib_paths = [
            "ethash.metallib",
            "src/gpu/ethash.metallib",
            "../ethash.metallib",
        ];
        
        for path in &metallib_paths {
            if Path::new(path).exists() {
                if let Ok(lib) = device.new_library_with_file(path) {
                    log::debug!("   Loaded pre-compiled Ethash shader: {}", path);
                    return Ok(lib);
                }
            }
        }
        
        // Compile from source
        let shader_source = include_str!("ethash_shader.metal");
        let options = metal::CompileOptions::new();
        
        let library = device.new_library_with_source(shader_source, &options)
            .map_err(|e| EthashMetalError::CompileError(format!("{:?}", e)))?;
        
        log::debug!("   Compiled Ethash shader from source");
        Ok(library)
    }
    
    /// Load/regenerate DAG for a given epoch
    /// Call this when epoch changes (every ~30K blocks / ~5 days)
    pub fn load_dag_for_epoch(&mut self, epoch: &EthashEpoch) -> Result<(), EthashMetalError> {
        if self.current_epoch == Some(epoch.number) {
            log::debug!("   DAG already loaded for epoch {}", epoch.number);
            return Ok(());
        }
        
        let dag_size = epoch.dataset_size;
        let available_mem = self.device.recommended_max_working_set_size();
        
        log::debug!("🔧 Loading DAG for epoch {} ({:.2} GB, available: {:.2} GB)",
            epoch.number,
            dag_size as f64 / 1_073_741_824.0,
            available_mem as f64 / 1_073_741_824.0,
        );
        
        if dag_size > available_mem {
            return Err(EthashMetalError::DagTooLarge(dag_size, available_mem));
        }
        
        // Generate DAG on CPU
        let dag_data = EthashDagGenerator::generate_dag(epoch);
        
        // Upload to Metal buffer
        let options = MTLResourceOptions::StorageModeShared;
        let dag_buf = self.device.new_buffer_with_data(
            dag_data.as_ptr() as *const _,
            dag_data.len() as u64,
            options,
        );
        
        self.dag_buf = Some(dag_buf);
        self.dag_items = epoch.dataset_items;
        self.current_epoch = Some(epoch.number);
        
        log::debug!("✅ DAG uploaded to Metal GPU: {} items ({:.2} GB)",
            epoch.dataset_items,
            dag_data.len() as f64 / 1_073_741_824.0,
        );
        
        Ok(())
    }
    
    /// Load pre-computed DAG data directly (skip CPU generation if cached)
    pub fn load_dag_data(&mut self, epoch_number: u64, dag_items: u32, data: &[u8]) -> Result<(), EthashMetalError> {
        let options = MTLResourceOptions::StorageModeShared;
        let dag_buf = self.device.new_buffer_with_data(
            data.as_ptr() as *const _,
            data.len() as u64,
            options,
        );
        
        self.dag_buf = Some(dag_buf);
        self.dag_items = dag_items;
        self.current_epoch = Some(epoch_number);
        
        log::debug!("✅ Pre-computed DAG loaded: epoch={}, items={}", epoch_number, dag_items);
        Ok(())
    }
    
    /// Mine for a valid nonce — returns (nonce, mix_digest, result_hash) if found
    pub fn mine(
        &mut self,
        header_hash: &[u8; 32],  // Pool-provided header hash (already hashed)
        target: &[u8; 32],       // 256-bit target boundary (LE)
        start_nonce: u64,
    ) -> Result<Option<(u64, [u8; 32], [u8; 32])>, EthashMetalError> {
        let dag_buf = self.dag_buf.as_ref()
            .ok_or(EthashMetalError::DagNotLoaded)?;
        
        // Write params
        unsafe {
            let ptr = self.params_buf.contents() as *mut EthashMiningParams;
            let params = &mut *ptr;
            params.start_nonce = start_nonce;
            params.dag_num_items = self.dag_items;
            params.header_hash.copy_from_slice(header_hash);
            params.target.copy_from_slice(target);
        }
        
        // Reset result
        unsafe {
            let ptr = self.result_buf.contents() as *mut EthashMiningResult;
            let result = &mut *ptr;
            result.found_nonce = 0;
            result.mix_digest = [0u8; 32];
            result.result_hash = [0u8; 32];
            result.found = 0;
        }
        
        // Encode and dispatch
        let command_buffer = self.command_queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();
        
        encoder.set_compute_pipeline_state(&self.pipeline_mine);
        encoder.set_buffer(0, Some(&self.params_buf), 0);
        encoder.set_buffer(1, Some(dag_buf), 0);
        encoder.set_buffer(2, Some(&self.result_buf), 0);
        
        let grid_size = MTLSize::new(self.batch_size as u64, 1, 1);
        let threadgroup_size = MTLSize::new(self.threads_per_threadgroup as u64, 1, 1);
        
        encoder.dispatch_threads(grid_size, threadgroup_size);
        encoder.end_encoding();
        
        command_buffer.commit();
        command_buffer.wait_until_completed();
        
        self.total_hashes += self.batch_size as u64;
        
        // Read result
        let result = unsafe { &*(self.result_buf.contents() as *const EthashMiningResult) };
        
        if result.found > 0 {
            self.solutions_found += 1;
            
            let mut mix_digest = [0u8; 32];
            let mut result_hash = [0u8; 32];
            mix_digest.copy_from_slice(&result.mix_digest);
            result_hash.copy_from_slice(&result.result_hash);
            
            Ok(Some((result.found_nonce, mix_digest, result_hash)))
        } else {
            Ok(None)
        }
    }
    
    /// Get device info
    pub fn device_info(&self) -> GpuDevice {
        GpuDevice {
            id: 0,
            name: format!("{} (Ethash)", self.device.name()),
            vendor: "Apple".to_string(),
            backend: GpuBackend::Metal,
            compute_units: 0,
            max_work_group_size: self.threads_per_threadgroup,
            global_memory: self.device.recommended_max_working_set_size(),
            local_memory: 32768,
        }
    }
    
    /// Get current epoch
    pub fn current_epoch(&self) -> Option<u64> {
        self.current_epoch
    }
    
    /// Is DAG loaded?
    pub fn dag_loaded(&self) -> bool {
        self.dag_buf.is_some()
    }
    
    /// Get batch size
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }
    
    /// Get stats
    pub fn stats(&self) -> EthashMetalStats {
        EthashMetalStats {
            total_hashes: self.total_hashes,
            solutions_found: self.solutions_found,
            batch_size: self.batch_size,
            current_epoch: self.current_epoch,
            dag_loaded: self.dag_buf.is_some(),
            dag_items: self.dag_items,
        }
    }
    
    /// Run benchmark with minimal DAG (for testing)
    pub fn benchmark_mini(&mut self, duration_secs: f64) -> Result<f64, EthashMetalError> {
        // Create a mini DAG (1 MB) for benchmarking dispatch overhead
        if !self.dag_loaded() {
            let mini_items = 16384u32; // 16K items × 64 bytes = 1 MB
            let mini_dag = vec![0xABu8; (mini_items as usize) * 64];
            self.load_dag_data(0, mini_items, &mini_dag)?;
        }
        
        let header = [0x42u8; 32];
        let target = [0xFFu8; 32]; // Easy target
        
        let start = std::time::Instant::now();
        let mut total = 0u64;
        let mut nonce = 0u64;
        
        while start.elapsed().as_secs_f64() < duration_secs {
            let _ = self.mine(&header, &target, nonce)?;
            nonce += self.batch_size as u64;
            total += self.batch_size as u64;
        }
        
        let elapsed = start.elapsed().as_secs_f64();
        let hashrate = total as f64 / elapsed;
        
        log::debug!("🍎 Ethash Metal Benchmark (mini DAG):");
        log::debug!("   Total hashes: {}", total);
        log::debug!("   Time: {:.2}s", elapsed);
        log::debug!("   Hashrate: {:.2} kH/s", hashrate / 1_000.0);
        
        Ok(hashrate)
    }
}

/// Ethash Metal statistics
#[derive(Debug, Clone)]
pub struct EthashMetalStats {
    pub total_hashes: u64,
    pub solutions_found: u64,
    pub batch_size: usize,
    pub current_epoch: Option<u64>,
    pub dag_loaded: bool,
    pub dag_items: u32,
}

/// Ethash Metal error types
#[derive(Debug)]
pub enum EthashMetalError {
    NoDevice,
    FunctionNotFound(String),
    CompileError(String),
    PipelineError(String),
    BufferError(String),
    DagNotLoaded,
    DagTooLarge(u64, u64),
    DagGenerationFailed(String),
}

impl std::fmt::Display for EthashMetalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => write!(f, "No Metal device found"),
            Self::FunctionNotFound(s) => write!(f, "Ethash function not found: {}", s),
            Self::CompileError(s) => write!(f, "Ethash shader compile error: {}", s),
            Self::PipelineError(s) => write!(f, "Ethash pipeline error: {}", s),
            Self::BufferError(s) => write!(f, "Ethash buffer error: {}", s),
            Self::DagNotLoaded => write!(f, "Ethash DAG not loaded — call load_dag_for_epoch() first"),
            Self::DagTooLarge(need, have) => write!(f, "DAG too large: need {} GB, have {} GB",
                *need as f64 / 1_073_741_824.0, *have as f64 / 1_073_741_824.0),
            Self::DagGenerationFailed(s) => write!(f, "DAG generation failed: {}", s),
        }
    }
}

impl std::error::Error for EthashMetalError {}

// Stub for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub struct EthashMetalMiner;

#[cfg(not(target_os = "macos"))]
impl EthashMetalMiner {
    pub fn new(_batch_size: usize) -> Result<Self, EthashMetalError> {
        Err(EthashMetalError::NoDevice)
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;
    
    #[test]
    fn test_ethash_epoch_from_height() {
        let epoch = EthashEpoch::from_height(0);
        assert_eq!(epoch.number, 0);
        assert_eq!(epoch.seed_hash, [0u8; 32]);
        
        let epoch = EthashEpoch::from_height(30_000);
        assert_eq!(epoch.number, 1);
        
        let epoch = EthashEpoch::from_height(21_000_000);
        assert_eq!(epoch.number, 700);
    }
    
    #[test]
    fn test_ethash_metal_init() {
        let miner = EthashMetalMiner::new(10_000);
        assert!(miner.is_ok(), "Ethash Metal should initialize on macOS");
        
        let miner = miner.unwrap();
        log::debug!("Device: {:?}", miner.device_info());
        assert!(!miner.dag_loaded());
    }
    
    #[test]
    fn test_ethash_mini_benchmark() {
        let mut miner = EthashMetalMiner::new(10_000).unwrap();
        let hashrate = miner.benchmark_mini(2.0);
        assert!(hashrate.is_ok());
        let hr = hashrate.unwrap();
        log::debug!("Ethash hashrate (mini DAG): {:.2} kH/s", hr / 1_000.0);
    }
}
