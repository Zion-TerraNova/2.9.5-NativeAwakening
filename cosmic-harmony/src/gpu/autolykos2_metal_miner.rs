//! Autolykos2 Metal GPU Miner — TABLELESS Mode
//!
//! Mines ERG (Ergo) using Apple Silicon Metal compute shaders.
//! 
//! TABLELESS: Instead of pre-computing the full table R (which at current ERG
//! block heights is ~63 GB), the GPU shader computes R values on-the-fly using
//! Blake2b256(j || h || M). This uses only 8 KB of GPU memory for the M constant.
//!
//! This approach trades compute for memory:
//! - 36 Blake2b256 hashes per nonce (vs table lookup)
//! - 0 bytes GPU memory for table (vs 63+ GB)
//! - Works on ANY GPU regardless of memory size

use std::path::Path;

use blake2::Blake2bVar;
use blake2::digest::{Update, VariableOutput};

#[cfg(target_os = "macos")]
use metal::*;

use crate::gpu::{GpuDevice, GpuBackend};

// Autolykos2 constants
const AUTOLYKOS_K: u32 = 32;           // Number of indexes (k-sum)
const AUTOLYKOS_N_BASE: u32 = 67_108_864; // N = 2^26 initially
const AUTOLYKOS_M_SIZE: usize = 8192;  // Padding constant M size (8 KB)
const AUTOLYKOS_INCREASE_START: u64 = 600 * 1024;   // = 614,400
const AUTOLYKOS_INCREASE_PERIOD: u64 = 50 * 1024;   // = 51,200 (N grows 5% each period)
const AUTOLYKOS_N_INCREASE_HEIGHT_MAX: u64 = 4_198_400; // N stops growing after this

// ============================================================================
// Metal struct layouts — must match autolykos2_shader.metal EXACTLY
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy)]
struct AutolykosMiningParams {
    start_nonce: u64,           // offset 0, size 8
    height: u32,                // offset 8, size 4
    n: u32,                     // offset 12, size 4
    header_hash: [u8; 32],      // offset 16, size 32
    target: [u8; 32],           // offset 48, size 32
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AutolykosMiningResult {
    found_nonce: u64,           // offset 0, size 8
    result_hash: [u8; 32],      // offset 8, size 32
    found: u32,                 // offset 40, size 4
}

// ============================================================================
// AutolykosTableInfo — N calculation from height
// ============================================================================

/// Calculates N (the modular table size) from block height
/// N starts at 2^26 = 67,108,864 and increases by 1 bit every 102,400 blocks
/// after block 614,400. Capped at AUTOLYKOS_N_MAX.
#[derive(Debug, Clone)]
pub struct AutolykosTableInfo {
    pub height: u64,
    pub n: u32,         // The value N (table size)
}

impl AutolykosTableInfo {
    /// Calculate N from block height
    pub fn from_height(height: u64) -> Self {
        let n = Self::calc_n(height);
        Self { height, n }
    }
    
    /// Ergo reference calcN — iterative 5% increase every 51,200 blocks
    /// From: ergoplatform/ergo AutolykosPowScheme.scala
    ///
    /// N starts at 2^26 = 67,108,864
    /// After height 614,400: grows by 5% every 51,200 blocks
    /// Formula: step = step / 100 * 105 (integer division!)
    /// Capped at height 4,198,400 → N = 2,143,944,600
    fn calc_n(height: u64) -> u32 {
        let height = std::cmp::min(AUTOLYKOS_N_INCREASE_HEIGHT_MAX, height);
        
        if height < AUTOLYKOS_INCREASE_START {
            return AUTOLYKOS_N_BASE;
        }
        
        let iters_number = ((height - AUTOLYKOS_INCREASE_START) / AUTOLYKOS_INCREASE_PERIOD + 1) as usize;
        let mut step = AUTOLYKOS_N_BASE as u64;
        for _ in 0..iters_number {
            step = step / 100 * 105;  // 5% increase, integer arithmetic
        }
        step as u32
    }
}

/// Generate the 8 KB padding constant M
/// Ergo reference: M = (0 until 1024).flatMap(i => Longs.toByteArray(i.toLong))
/// Each number 0..1023 encoded as 8 bytes big-endian = 1024 × 8 = 8192 bytes
fn generate_padding_m() -> Vec<u8> {
    let mut m = Vec::with_capacity(AUTOLYKOS_M_SIZE);
    for i in 0u64..1024 {
        m.extend_from_slice(&i.to_be_bytes()); // 8 bytes big-endian per number
    }
    assert_eq!(m.len(), AUTOLYKOS_M_SIZE, "M must be exactly 8192 bytes");
    m
}

fn blake2b256(input: &[u8]) -> [u8; 32] {
    let mut hasher = Blake2bVar::new(32).expect("blake2b var output");
    hasher.update(input);
    let mut out = [0u8; 32];
    hasher
        .finalize_variable(&mut out)
        .expect("blake2b finalize");
    out
}

fn add_be32_in_place(acc: &mut [u8; 32], addend: &[u8; 32]) {
    let mut carry: u16 = 0;
    for i in (0..32).rev() {
        let sum = acc[i] as u16 + addend[i] as u16 + carry;
        acc[i] = (sum & 0xFF) as u8;
        carry = sum >> 8;
    }
    // overflow ignored; max sum fits < 2^256
}

/// CPU reference for Autolykos v2 (Ergo) hash used for share checking.
///
/// Matches `hitForVersion2ForMessage` in ergoplatform/ergo:
/// - i = takeRight(8, Blake2b256(m||nonce)) mod N
/// - f = Blake2b256(i||h||M).drop(1)
/// - indexes = genIndexes(f||m||nonce)
/// - elems = map(idx => Blake2b256(idx||h||M).drop(1)) as BigInt
/// - sum = elems.sum, array = asUnsignedByteArray(32, sum), ha = Blake2b256(array)
pub fn autolykos2_hash_cpu(
    header_hash: &[u8; 32],
    nonce: u64,
    height: u32,
    n: u32,
) -> [u8; 32] {
    let m_padding = generate_padding_m();
    let nonce_bytes = nonce.to_be_bytes();
    let height_bytes = height.to_be_bytes();

    // Step 1: i = takeRight(8, Blake2b256(m || nonce)) mod N
    let mut mn = [0u8; 40];
    mn[..32].copy_from_slice(header_hash);
    mn[32..].copy_from_slice(&nonce_bytes);
    let hash_i = blake2b256(&mn);
    let prei8 = u64::from_be_bytes(hash_i[24..32].try_into().expect("8 bytes"));
    let i_idx = (prei8 % n as u64) as u32;

    // Step 2: f = Blake2b256(i || h || M).drop(1)  (31 bytes)
    let mut ihm = Vec::with_capacity(4 + 4 + AUTOLYKOS_M_SIZE);
    ihm.extend_from_slice(&i_idx.to_be_bytes());
    ihm.extend_from_slice(&height_bytes);
    ihm.extend_from_slice(&m_padding);
    let f_hash = blake2b256(&ihm);
    let f31 = &f_hash[1..];

    // Step 3: seed = f || m || nonce
    let mut seed = [0u8; 71];
    seed[..31].copy_from_slice(f31);
    seed[31..63].copy_from_slice(header_hash);
    seed[63..].copy_from_slice(&nonce_bytes);

    // genIndexes
    let seed_hash = blake2b256(&seed);
    let mut extended = [0u8; 35];
    extended[..32].copy_from_slice(&seed_hash);
    extended[32..35].copy_from_slice(&seed_hash[..3]);

    let mut sum32 = [0u8; 32];
    for i in 0..AUTOLYKOS_K as usize {
        let window = u32::from_be_bytes(extended[i..i + 4].try_into().expect("4 bytes"));
        let idx = window % n;

        // elem = Blake2b256(idx || h || M).drop(1) as unsigned BigInt
        let mut jhm = Vec::with_capacity(4 + 4 + AUTOLYKOS_M_SIZE);
        jhm.extend_from_slice(&idx.to_be_bytes());
        jhm.extend_from_slice(&height_bytes);
        jhm.extend_from_slice(&m_padding);
        let elem_hash = blake2b256(&jhm);
        let elem31 = &elem_hash[1..];

        let mut elem32 = [0u8; 32];
        elem32[0] = 0;
        elem32[1..].copy_from_slice(elem31);
        add_be32_in_place(&mut sum32, &elem32);
    }

    blake2b256(&sum32)
}

// ============================================================================
// Metal Autolykos2 Miner — TABLELESS
// ============================================================================

#[cfg(target_os = "macos")]
pub struct AutolykosMetalMiner {
    device: Device,
    command_queue: CommandQueue,
    pipeline_mine: ComputePipelineState,
    pipeline_benchmark: ComputePipelineState,
    
    // Buffers
    params_buf: Buffer,
    m_padding_buf: Buffer,     // 8 KB M constant (replaces 63 GB table!)
    result_buf: Buffer,
    
    // Height tracking
    current_height: Option<u64>,
    current_n: u32,
    
    // Config
    batch_size: usize,
    threads_per_threadgroup: usize,
    
    // Stats
    total_hashes: u64,
    solutions_found: u64,
}

#[cfg(target_os = "macos")]
impl AutolykosMetalMiner {
    /// Create new Autolykos2 Metal miner (tableless)
    pub fn new(batch_size: usize) -> Result<Self, AutolykosMetalError> {
        let device = Device::system_default()
            .ok_or(AutolykosMetalError::NoDevice)?;
        
        Self::new_with_device(device, batch_size)
    }
    
    /// Create with explicit Metal device
    pub fn new_with_device(device: Device, batch_size: usize) -> Result<Self, AutolykosMetalError> {
        log::debug!("🍎 Autolykos2 Metal miner (TABLELESS) initializing on: {}", device.name());
        log::debug!("   Available memory: {} MB",
            device.recommended_max_working_set_size() / (1024 * 1024));
        
        let command_queue = device.new_command_queue();
        
        // Load shader
        let library = Self::load_shader_library(&device)?;
        
        let mine_fn = library.get_function("autolykos2_mine", None)
            .map_err(|e| AutolykosMetalError::FunctionNotFound(format!("autolykos2_mine: {:?}", e)))?;
        let benchmark_fn = library.get_function("autolykos2_benchmark", None)
            .map_err(|e| AutolykosMetalError::FunctionNotFound(format!("autolykos2_benchmark: {:?}", e)))?;
        
        let pipeline_mine = device.new_compute_pipeline_state_with_function(&mine_fn)
            .map_err(|e| AutolykosMetalError::PipelineError(format!("{:?}", e)))?;
        let pipeline_benchmark = device.new_compute_pipeline_state_with_function(&benchmark_fn)
            .map_err(|e| AutolykosMetalError::PipelineError(format!("{:?}", e)))?;
        
        let max_threads = pipeline_mine.max_total_threads_per_threadgroup();
        // Use maximum threads the pipeline supports (up to 1024 on M1+)
        // Higher occupancy = better GPU utilization
        let threads_per_threadgroup = max_threads as usize;
        
        let options = MTLResourceOptions::StorageModeShared;
        
        // Params buffer
        let params_size = std::mem::size_of::<AutolykosMiningParams>() as u64;
        let result_size = std::mem::size_of::<AutolykosMiningResult>() as u64;
        let params_buf = device.new_buffer(params_size, options);
        let result_buf = device.new_buffer(result_size, options);
        
        // M padding buffer — only 8 KB! (replaces the 63+ GB table)
        let m_data = generate_padding_m();
        let m_padding_buf = device.new_buffer_with_data(
            m_data.as_ptr() as *const _,
            m_data.len() as u64,
            options,
        );
        
        log::debug!("   Params struct: {} bytes", params_size);
        log::debug!("   Result struct: {} bytes", result_size);
        log::debug!("   M padding buffer: {} bytes (8 KB — TABLELESS mode!)", m_data.len());
        log::debug!("   Threads per threadgroup: {}", threads_per_threadgroup);
        log::debug!("   Batch size: {}", batch_size);
        log::debug!("✅ Autolykos2 Metal miner ready (TABLELESS — 0 bytes table, R computed on-the-fly)");
        
        Ok(Self {
            device,
            command_queue,
            pipeline_mine,
            pipeline_benchmark,
            params_buf,
            m_padding_buf,
            result_buf,
            current_height: None,
            current_n: 0,
            batch_size,
            threads_per_threadgroup,
            total_hashes: 0,
            solutions_found: 0,
        })
    }
    
    /// Load Autolykos2 Metal shader library
    fn load_shader_library(device: &Device) -> Result<Library, AutolykosMetalError> {
        let metallib_paths = [
            "autolykos2.metallib",
            "src/gpu/autolykos2.metallib",
            "../autolykos2.metallib",
        ];
        
        for path in &metallib_paths {
            if Path::new(path).exists() {
                if let Ok(lib) = device.new_library_with_file(path) {
                    log::debug!("   Loaded pre-compiled Autolykos2 shader: {}", path);
                    return Ok(lib);
                }
            }
        }
        
        // Compile from source
        let shader_source = include_str!("autolykos2_shader.metal");
        let options = metal::CompileOptions::new();
        
        let library = device.new_library_with_source(shader_source, &options)
            .map_err(|e| AutolykosMetalError::CompileError(format!("{:?}", e)))?;
        
        log::debug!("   Compiled Autolykos2 shader from source (tableless mode)");
        Ok(library)
    }
    
    /// Prepare miner for a given block height (just sets N — NO table generation!)
    /// This replaces the old load_table_for_height() which tried to generate 63 GB.
    pub fn prepare_for_height(&mut self, height: u64) -> Result<(), AutolykosMetalError> {
        let info = AutolykosTableInfo::from_height(height);
        
        // Check if N changed
        if let Some(loaded_h) = self.current_height {
            let loaded_info = AutolykosTableInfo::from_height(loaded_h);
            if loaded_info.n == info.n {
                self.current_height = Some(height);
                return Ok(());
            }
        }
        
        log::debug!("🔧 Autolykos2 height {} → N={} — TABLELESS mode, no table needed!",
            height, info.n);
        
        self.current_n = info.n;
        self.current_height = Some(height);
        
        Ok(())
    }
    
    /// Legacy compatibility: load_table_for_height now just calls prepare_for_height
    pub fn load_table_for_height(&mut self, height: u64) -> Result<(), AutolykosMetalError> {
        self.prepare_for_height(height)
    }
    
    /// Mine for a valid nonce — returns (nonce, result_hash) if found
    pub fn mine(
        &mut self,
        header_hash: &[u8; 32],
        target: &[u8; 32],
        height: u32,
        start_nonce: u64,
    ) -> Result<Option<(u64, [u8; 32])>, AutolykosMetalError> {
        if self.current_height.is_none() {
            return Err(AutolykosMetalError::HeightNotSet);
        }
        
        // Write params
        unsafe {
            let ptr = self.params_buf.contents() as *mut AutolykosMiningParams;
            let params = &mut *ptr;
            params.start_nonce = start_nonce;
            params.height = height;
            params.n = self.current_n;
            params.header_hash.copy_from_slice(header_hash);
            params.target.copy_from_slice(target);
        }
        
        // Reset result
        unsafe {
            let ptr = self.result_buf.contents() as *mut AutolykosMiningResult;
            let result = &mut *ptr;
            result.found_nonce = 0;
            result.result_hash = [0u8; 32];
            result.found = 0;
        }
        
        // Encode and dispatch
        let command_buffer = self.command_queue.new_command_buffer();
        let encoder = command_buffer.new_compute_command_encoder();
        
        encoder.set_compute_pipeline_state(&self.pipeline_mine);
        encoder.set_buffer(0, Some(&self.params_buf), 0);
        encoder.set_buffer(1, Some(&self.m_padding_buf), 0);  // 8 KB M constant!
        encoder.set_buffer(2, Some(&self.result_buf), 0);
        
        let grid_size = MTLSize::new(self.batch_size as u64, 1, 1);
        let threadgroup_size = MTLSize::new(self.threads_per_threadgroup as u64, 1, 1);
        
        encoder.dispatch_threads(grid_size, threadgroup_size);
        encoder.end_encoding();
        
        command_buffer.commit();
        command_buffer.wait_until_completed();
        
        self.total_hashes += self.batch_size as u64;
        
        // Read result
        let result = unsafe { &*(self.result_buf.contents() as *const AutolykosMiningResult) };
        
        if result.found > 0 {
            self.solutions_found += 1;
            
            let mut result_hash = [0u8; 32];
            result_hash.copy_from_slice(&result.result_hash);
            
            Ok(Some((result.found_nonce, result_hash)))
        } else {
            Ok(None)
        }
    }
    
    /// Get device info
    pub fn device_info(&self) -> GpuDevice {
        GpuDevice {
            id: 0,
            name: format!("{} (Autolykos2 Tableless)", self.device.name()),
            vendor: "Apple".to_string(),
            backend: GpuBackend::Metal,
            compute_units: 0,
            max_work_group_size: self.threads_per_threadgroup,
            global_memory: self.device.recommended_max_working_set_size(),
            local_memory: 32768,
        }
    }
    
    /// Is height set? (replaces old table_loaded)
    pub fn table_loaded(&self) -> bool {
        self.current_height.is_some()
    }
    
    /// Get current height
    pub fn current_height(&self) -> Option<u64> {
        self.current_height
    }
    
    /// Get batch size
    pub fn batch_size(&self) -> usize {
        self.batch_size
    }
    
    /// Get stats
    pub fn stats(&self) -> AutolykosMetalStats {
        AutolykosMetalStats {
            total_hashes: self.total_hashes,
            solutions_found: self.solutions_found,
            batch_size: self.batch_size,
            current_height: self.current_height,
            table_loaded: self.current_height.is_some(),
            table_n: self.current_n,
        }
    }
    
    /// Run benchmark (tableless — no mini table needed!)
    pub fn benchmark_mini(&mut self, duration_secs: f64) -> Result<f64, AutolykosMetalError> {
        // Set a test height
        if self.current_height.is_none() {
            self.prepare_for_height(600_000)?; // Use a reasonable height
        }
        
        let header = [0x42u8; 32];
        let target = [0xFFu8; 32]; // Easy target
        
        let start = std::time::Instant::now();
        let mut total = 0u64;
        let mut nonce = 0u64;
        
        while start.elapsed().as_secs_f64() < duration_secs {
            let _ = self.mine(&header, &target, 600_000, nonce)?;
            nonce += self.batch_size as u64;
            total += self.batch_size as u64;
        }
        
        let elapsed = start.elapsed().as_secs_f64();
        let hashrate = total as f64 / elapsed;
        
        log::debug!("🍎 Autolykos2 Metal Benchmark (TABLELESS):");
        log::debug!("   Total hashes: {}", total);
        log::debug!("   Time: {:.2}s", elapsed);
        log::debug!("   Hashrate: {:.2} H/s ({:.2} kH/s)", hashrate, hashrate / 1_000.0);
        
        Ok(hashrate)
    }
}

/// Autolykos2 Metal statistics
#[derive(Debug, Clone)]
pub struct AutolykosMetalStats {
    pub total_hashes: u64,
    pub solutions_found: u64,
    pub batch_size: usize,
    pub current_height: Option<u64>,
    pub table_loaded: bool,
    pub table_n: u32,
}

/// Autolykos2 Metal error types
#[derive(Debug)]
pub enum AutolykosMetalError {
    NoDevice,
    FunctionNotFound(String),
    CompileError(String),
    PipelineError(String),
    BufferError(String),
    HeightNotSet,
    TableNotLoaded,
    TableTooLarge(u64, u64),
    TableGenerationFailed(String),
}

impl std::fmt::Display for AutolykosMetalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDevice => write!(f, "No Metal device found"),
            Self::FunctionNotFound(s) => write!(f, "Autolykos2 function not found: {}", s),
            Self::CompileError(s) => write!(f, "Autolykos2 shader compile error: {}", s),
            Self::PipelineError(s) => write!(f, "Autolykos2 pipeline error: {}", s),
            Self::BufferError(s) => write!(f, "Autolykos2 buffer error: {}", s),
            Self::HeightNotSet => write!(f, "Autolykos2 height not set — call prepare_for_height() first"),
            Self::TableNotLoaded => write!(f, "Autolykos2 height not set — call prepare_for_height() first"),
            Self::TableTooLarge(need, have) => write!(f, "Table too large: need {:.2} GB, have {:.2} GB (use tableless mode)",
                *need as f64 / 1_073_741_824.0, *have as f64 / 1_073_741_824.0),
            Self::TableGenerationFailed(s) => write!(f, "Table generation failed: {}", s),
        }
    }
}

impl std::error::Error for AutolykosMetalError {}

// Stub for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub struct AutolykosMetalMiner;

#[cfg(not(target_os = "macos"))]
impl AutolykosMetalMiner {
    pub fn new(_batch_size: usize) -> Result<Self, AutolykosMetalError> {
        Err(AutolykosMetalError::NoDevice)
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;
    
    #[test]
    fn test_autolykos_table_info_initial() {
        let info = AutolykosTableInfo::from_height(0);
        assert_eq!(info.n, 67_108_864); // 2^26
    }
    
    #[test]
    fn test_autolykos_calcn_ergo_test_vectors() {
        // Official test vectors from ergoplatform/ergo AutolykosPowSchemeSpec.scala
        assert_eq!(AutolykosTableInfo::from_height(500_000).n, 67_108_864);
        assert_eq!(AutolykosTableInfo::from_height(600_000).n, 67_108_864);
        assert_eq!(AutolykosTableInfo::from_height(614_400).n, 70_464_240);
        assert_eq!(AutolykosTableInfo::from_height(665_600).n, 73_987_410);
        assert_eq!(AutolykosTableInfo::from_height(700_000).n, 73_987_410);
        assert_eq!(AutolykosTableInfo::from_height(788_400).n, 81_571_035);
        assert_eq!(AutolykosTableInfo::from_height(1_051_200).n, 104_107_290);
        assert_eq!(AutolykosTableInfo::from_height(4_198_400).n, 2_143_944_600);
        assert_eq!(AutolykosTableInfo::from_height(41_984_000).n, 2_143_944_600); // capped
    }
    
    #[test]
    fn test_autolykos_table_info_current_ergo() {
        let info = AutolykosTableInfo::from_height(1_200_000);
        assert_eq!(info.n, 120_517_005);
        log::debug!("ERG height 1,200,000: N={}", info.n);
    }
    
    #[test]
    fn test_autolykos_metal_init() {
        let miner = AutolykosMetalMiner::new(1_000);
        assert!(miner.is_ok(), "Autolykos2 Metal should initialize on macOS");
        
        let miner = miner.unwrap();
        log::debug!("Device: {:?}", miner.device_info());
        assert!(!miner.table_loaded()); // No height set yet
    }
    
    #[test]
    fn test_autolykos_prepare_height() {
        let mut miner = AutolykosMetalMiner::new(1_000).unwrap();
        // This should work instantly — no table generation!
        let result = miner.prepare_for_height(1_200_000);
        assert!(result.is_ok());
        assert!(miner.table_loaded());
        assert_eq!(miner.current_height(), Some(1_200_000));
    }
    
    #[test]
    fn test_autolykos_benchmark() {
        let mut miner = AutolykosMetalMiner::new(1_000).unwrap();
        let hashrate = miner.benchmark_mini(2.0);
        assert!(hashrate.is_ok());
        let hr = hashrate.unwrap();
        log::debug!("Autolykos2 hashrate (tableless): {:.2} H/s", hr);
    }
}
