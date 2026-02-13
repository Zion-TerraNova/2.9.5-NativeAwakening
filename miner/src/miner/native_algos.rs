//! Native Algorithm FFI for Universal Miner
//!
//! Direct bindings to C libraries in ../native-libs/
//! Supports all 12 mining algorithms for multi-chain mining.
//!
//! ## P2-06: GPU Algorithm Stubs
//!
//! Some algorithms (e.g. YescryptR32, Equihash, ProgPow, Octopus, ZelHash,
//! Autolykos2, FishHash) only have CPU FFI bindings via native-libs `.dylib`
//! (macOS) / `.so` (Linux). When the native library is not present for the
//! current platform, the compute function returns a Keccak-256 fallback hash.
//!
//! **This is by design for v2.9.5** — the primary ZION mining algorithm is
//! Cosmic Harmony v3, which is fully implemented in pure Rust. The native-libs
//! stubs exist only for multi-chain/external-pool mining scenarios and are NOT
//! used for ZION mainnet block validation.
//!
//! For GPU mining of Cosmic Harmony v3, see the `gpu.rs` module (Metal/CUDA/OpenCL).

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

static RANDOMX_INITIALIZED: AtomicBool = AtomicBool::new(false);
static RANDOMX_KEY: RwLock<Vec<u8>> = RwLock::new(Vec::new());

thread_local! {
    // Cache RandomX VM per thread (RandomXVM is not Send+Sync).
    // Stores (key, hasher) so we can recreate if key changes.
    static RANDOMX_HASHER: RefCell<Option<(Vec<u8>, zion_core::algorithms::randomx::RandomXHasher)>> =
        RefCell::new(None);

    // Cache Cosmic Harmony v2 hasher per thread (scratchpad is large and not thread-safe to share).
    // Stores (prev_hash, height, hasher) so we can recreate on job change.
    static CHV2_HASHER: RefCell<Option<([u8; 32], u64, zion_core::algorithms::cosmic_harmony_v2::CosmicHarmonyV2)>> =
        RefCell::new(None);
}

/// Initialize RandomX with key (typically seed_hash from pool job)
/// Can be called multiple times with different keys (e.g. when MoneroOcean
/// sends a new seed_hash after epoch change).
pub fn init_randomx_with_key(key: &[u8]) -> Result<()> {
    // Check if key is the same as current — skip reinit
    {
        let current = RANDOMX_KEY.read().map_err(|e| anyhow!("lock error: {}", e))?;
        if !current.is_empty() && current.as_slice() == key {
            return Ok(()); // Same key, no need to reinitialize
        }
    }
    
    // Store the new key
    {
        let mut k = RANDOMX_KEY.write().map_err(|e| anyhow!("lock error: {}", e))?;
        *k = key.to_vec();
    }
    
    RANDOMX_INITIALIZED.store(true, Ordering::SeqCst);
    
    // Validate by creating a test hasher (using zion-core's RandomXHasher)
    let _test_hasher = zion_core::algorithms::randomx::RandomXHasher::new(key)?;
    
    log::info!("✅ RandomX initialized with key (len={}, hash={}...)", 
        key.len(), hex::encode(&key[..key.len().min(8)]));
    Ok(())
}

/// Initialize RandomX (placeholder - actual init via randomx-rs crate)
pub fn init_randomx() -> Result<()> {
    // Use ZION default key if no specific key provided
    let default_key = b"ZION_RANDOMX_TESTNET_2026";
    init_randomx_with_key(default_key)
}

// ============================================================================
// ALGORITHM ENUM (Extended)
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeAlgorithm {
    // CPU Algorithms
    RandomX,        // XMR
    Yescrypt,       // LTC/YTN
    CosmicHarmony,  // ZION
    CosmicHarmonyV2, // ZION (v2 memory-hard)
    Argon2d,        // DYN
    
    // GPU Algorithms
    Ethash,         // ETC
    KawPow,         // RVN/CLORE
    KawPowGpu,      // RVN/CLORE (GPU accelerated)
    Autolykos,      // ERG
    KHeavyHash,     // KAS
    Equihash,       // ZEC
    ProgPow,        // VEIL
    Blake3,         // ALPH
}

impl NativeAlgorithm {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "randomx" | "rx/0" => Some(Self::RandomX),
            "yescrypt" => Some(Self::Yescrypt),
            "cosmic_harmony" | "cosmic_harmony_v3" | "cosmicharmony" => Some(Self::CosmicHarmony),
            "cosmic_harmony_v2" | "cosmicharmonyv2" | "cosmic-harmony-v2" => Some(Self::CosmicHarmonyV2),
            "ethash" | "etchash" => Some(Self::Ethash),
            "kawpow" => Some(Self::KawPow),
            "kawpow_gpu" | "kawpow-gpu" => Some(Self::KawPowGpu),
            "autolykos" | "autolykos2" | "autolykos_v2" => Some(Self::Autolykos),
            "kheavyhash" | "heavyhash" | "kHeavyHash" => Some(Self::KHeavyHash),
            "equihash" | "equihash_200_9" => Some(Self::Equihash),
            "progpow" => Some(Self::ProgPow),
            "argon2d" => Some(Self::Argon2d),
            "blake3" => Some(Self::Blake3),
            _ => None,
        }
    }
    
    pub fn coin(&self) -> &'static str {
        match self {
            Self::RandomX => "XMR",
            Self::Yescrypt => "LTC",
            Self::CosmicHarmony => "ZION",
            Self::CosmicHarmonyV2 => "ZION",
            Self::Ethash => "ETC",
            Self::KawPow | Self::KawPowGpu => "RVN",
            Self::Autolykos => "ERG",
            Self::KHeavyHash => "KAS",
            Self::Equihash => "ZEC",
            Self::ProgPow => "VEIL",
            Self::Argon2d => "DYN",
            Self::Blake3 => "ALPH",
        }
    }
    
    pub fn is_gpu(&self) -> bool {
        matches!(self, 
            Self::Ethash | Self::KawPow | Self::KawPowGpu | 
            Self::Autolykos | Self::KHeavyHash | Self::ProgPow
        )
    }
}

// ============================================================================
// ETHASH FFI
// ============================================================================

#[cfg(feature = "native-ethash")]
mod ethash_ffi {
    use super::*;
    
    #[link(name = "ethash_zion")]
    extern "C" {
        fn ethash_init();
        fn ethash_hash(header: *const u8, header_len: usize, nonce: u64, height: u32, output: *mut u8);
        fn ethash_verify(header: *const u8, header_len: usize, nonce: u64, height: u32, target: *const u8) -> i32;
        fn ethash_get_epoch(block_number: u32) -> u32;
        fn ethash_benchmark(iterations: i32) -> f64;
    }
    
    pub fn init() {
        unsafe { ethash_init() }
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> [u8; 32] {
        let mut output = [0u8; 32];
        unsafe {
            ethash_hash(header.as_ptr(), header.len(), nonce, height, output.as_mut_ptr());
        }
        output
    }
    
    pub fn verify(header: &[u8], nonce: u64, height: u32, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { ethash_verify(header.as_ptr(), header.len(), nonce, height, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { ethash_benchmark(iterations) }
    }
}

// ============================================================================
// KAWPOW FFI
// ============================================================================

#[cfg(feature = "native-kawpow")]
mod kawpow_ffi {
    use super::*;
    
    #[link(name = "kawpow_zion")]
    extern "C" {
        fn kawpow_hash(header: *const u8, nonce: u64, height: u32, epoch: u32, mix_out: *mut u8, hash_out: *mut u8);
        fn kawpow_verify(header: *const u8, nonce: u64, height: u32, epoch: u32, expected_mix: *const u8, target: *const u8) -> i32;
        fn kawpow_get_epoch(height: u32) -> u32;
        fn kawpow_benchmark_cpu(iterations: i32) -> f64;
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> ([u8; 32], [u8; 32]) {
        let epoch = unsafe { kawpow_get_epoch(height) };
        let mut mix = [0u8; 32];
        let mut hash = [0u8; 32];
        unsafe {
            kawpow_hash(header.as_ptr(), nonce, height, epoch, mix.as_mut_ptr(), hash.as_mut_ptr());
        }
        (hash, mix)
    }
    
    pub fn verify(header: &[u8], nonce: u64, height: u32, mix: &[u8], target: &[u8]) -> bool {
        if mix.len() != 32 || target.len() != 32 { return false; }
        let epoch = unsafe { kawpow_get_epoch(height) };
        unsafe { kawpow_verify(header.as_ptr(), nonce, height, epoch, mix.as_ptr(), target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { kawpow_benchmark_cpu(iterations) }
    }
}

// ============================================================================
// KAWPOW GPU FFI
// ============================================================================

#[cfg(feature = "native-kawpow-gpu")]
mod kawpow_gpu_ffi {
    use super::*;
    
    #[link(name = "kawpow_gpu_zion")]
    extern "C" {
        fn kawpow_gpu_init(device_id: i32, platform_id: i32) -> i32;
        fn kawpow_gpu_shutdown();
        fn kawpow_gpu_set_epoch(epoch: u32) -> i32;
        fn kawpow_gpu_hash(header: *const u8, nonce: u64, height: u32, mix_out: *mut u8, hash_out: *mut u8);
        fn kawpow_gpu_benchmark(iterations: i32) -> f64;
        fn kawpow_gpu_get_hashrate() -> f64;
    }
    
    pub fn init(device_id: i32) -> Result<()> {
        let result = unsafe { kawpow_gpu_init(device_id, 0) };
        if result == 0 { Ok(()) }
        else { Err(anyhow!("KawPow GPU init failed")) }
    }
    
    pub fn shutdown() {
        unsafe { kawpow_gpu_shutdown() }
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> ([u8; 32], [u8; 32]) {
        let mut mix = [0u8; 32];
        let mut hash = [0u8; 32];
        unsafe {
            kawpow_gpu_hash(header.as_ptr(), nonce, height, mix.as_mut_ptr(), hash.as_mut_ptr());
        }
        (hash, mix)
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { kawpow_gpu_benchmark(iterations) }
    }
    
    pub fn get_hashrate() -> f64 {
        unsafe { kawpow_gpu_get_hashrate() }
    }
}

// ============================================================================
// AUTOLYKOS FFI
// ============================================================================

#[cfg(feature = "native-autolykos")]
mod autolykos_ffi {
    use super::*;
    
    #[link(name = "autolykos_zion")]
    extern "C" {
        fn autolykos_hash(header: *const u8, header_len: usize, nonce: u64, height: u32, output: *mut u8) -> u64;
        fn autolykos_verify(header: *const u8, header_len: usize, nonce: u64, height: u32, target: u64) -> i32;
        fn autolykos_benchmark_cpu(iterations: i32) -> f64;
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> [u8; 32] {
        let mut output = [0u8; 32];
        unsafe {
            autolykos_hash(header.as_ptr(), header.len(), nonce, height, output.as_mut_ptr());
        }
        output
    }
    
    pub fn verify(header: &[u8], nonce: u64, height: u32, target: u64) -> bool {
        unsafe { autolykos_verify(header.as_ptr(), header.len(), nonce, height, target) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { autolykos_benchmark_cpu(iterations) }
    }
}

// ============================================================================
// KHEAVYHASH FFI
// ============================================================================

#[cfg(feature = "native-kheavyhash")]
mod kheavyhash_ffi {
    use super::*;
    
    #[link(name = "kheavyhash_zion")]
    extern "C" {
        fn kheavyhash_mine(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        fn kheavyhash_verify(header: *const u8, header_len: usize, nonce: u64, target: *const u8) -> i32;
        fn kheavyhash_benchmark(iterations: i32) -> f64;
    }
    
    pub fn hash(header: &[u8], nonce: u64) -> [u8; 32] {
        let mut output = [0u8; 32];
        unsafe {
            kheavyhash_mine(header.as_ptr(), header.len(), nonce, output.as_mut_ptr());
        }
        output
    }
    
    pub fn verify(header: &[u8], nonce: u64, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { kheavyhash_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { kheavyhash_benchmark(iterations) }
    }
}

// ============================================================================
// EQUIHASH FFI
// ============================================================================

#[cfg(feature = "native-equihash")]
mod equihash_ffi {
    use super::*;
    
    #[link(name = "equihash_zion")]
    extern "C" {
        fn equihash_solve(header: *const u8, header_len: usize, nonce: u64, solution: *mut u8) -> i32;
        fn equihash_verify(header: *const u8, header_len: usize, solution: *const u8) -> i32;
        fn equihash_benchmark(iterations: i32) -> f64;
    }
    
    pub fn solve(header: &[u8], nonce: u64) -> Option<Vec<u8>> {
        let mut solution = vec![0u8; 1344]; // Equihash(200,9)
        let result = unsafe { equihash_solve(header.as_ptr(), header.len(), nonce, solution.as_mut_ptr()) };
        if result == 0 { Some(solution) } else { None }
    }
    
    pub fn verify(header: &[u8], solution: &[u8]) -> bool {
        unsafe { equihash_verify(header.as_ptr(), header.len(), solution.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { equihash_benchmark(iterations) }
    }
}

// ============================================================================
// PROGPOW FFI
// ============================================================================

#[cfg(feature = "native-progpow")]
mod progpow_ffi {
    use super::*;
    
    #[link(name = "progpow_zion")]
    extern "C" {
        fn progpow_hash(header: *const u8, header_len: usize, nonce: u64, height: u32, output: *mut u8);
        fn progpow_verify(header: *const u8, header_len: usize, nonce: u64, height: u32, target: *const u8) -> i32;
        fn progpow_benchmark(iterations: i32) -> f64;
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> [u8; 32] {
        let mut output = [0u8; 32];
        unsafe {
            progpow_hash(header.as_ptr(), header.len(), nonce, height, output.as_mut_ptr());
        }
        output
    }
    
    pub fn verify(header: &[u8], nonce: u64, height: u32, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { progpow_verify(header.as_ptr(), header.len(), nonce, height, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { progpow_benchmark(iterations) }
    }
}

// ============================================================================
// ARGON2D FFI
// ============================================================================

#[cfg(feature = "native-argon2d")]
mod argon2d_ffi {
    use super::*;
    
    #[link(name = "argon2d_zion")]
    extern "C" {
        fn argon2d_mine(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        fn argon2d_verify(header: *const u8, header_len: usize, nonce: u64, target: *const u8) -> i32;
        fn argon2d_benchmark(iterations: i32) -> f64;
    }
    
    pub fn hash(header: &[u8], nonce: u64) -> [u8; 32] {
        let mut output = [0u8; 32];
        unsafe {
            argon2d_mine(header.as_ptr(), header.len(), nonce, output.as_mut_ptr());
        }
        output
    }
    
    pub fn verify(header: &[u8], nonce: u64, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { argon2d_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { argon2d_benchmark(iterations) }
    }
}

// ============================================================================
// BLAKE3 FFI
// ============================================================================

#[cfg(feature = "native-blake3")]
mod blake3_ffi {
    use super::*;
    
    #[link(name = "blake3_zion")]
    extern "C" {
        fn blake3_mine(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        fn blake3_alph(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        fn blake3_verify(header: *const u8, header_len: usize, nonce: u64, target: *const u8) -> i32;
        fn blake3_benchmark(iterations: i32) -> f64;
    }
    
    pub fn hash(header: &[u8], nonce: u64) -> [u8; 32] {
        let mut output = [0u8; 32];
        unsafe {
            blake3_mine(header.as_ptr(), header.len(), nonce, output.as_mut_ptr());
        }
        output
    }
    
    /// Alephium-style double Blake3
    pub fn alph_hash(header: &[u8], nonce: u64) -> [u8; 32] {
        let mut output = [0u8; 32];
        unsafe {
            blake3_alph(header.as_ptr(), header.len(), nonce, output.as_mut_ptr());
        }
        output
    }
    
    pub fn verify(header: &[u8], nonce: u64, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { blake3_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { blake3_benchmark(iterations) }
    }
}

// ============================================================================
// UNIFIED INTERFACE
// ============================================================================

/// Batch-compute RandomX hashes using pipeline mode (calculate_hash_set).
///
/// Returns Vec of (nonce, hash) for each nonce in the range.
/// Uses RandomX's internal first/next pipeline for ~1.5-2× throughput
/// vs sequential calculate_hash calls.
pub fn compute_hash_batch_randomx(
    header: &[u8],
    nonce_start: u64,
    count: u32,
) -> Result<Vec<(u64, [u8; 32])>> {
    let key_vec = {
        let key = RANDOMX_KEY.read()
            .map_err(|e| anyhow!("RandomX key lock: {}", e))?;
        if key.is_empty() {
            return Err(anyhow!("RandomX not initialized"));
        }
        key.clone()
    };

    RANDOMX_HASHER.with(|cell| {
        let mut guard = cell.borrow_mut();
        let needs_reinit = match guard.as_ref() {
            None => true,
            Some((existing_key, _)) => existing_key.as_slice() != key_vec.as_slice(),
        };

        if needs_reinit {
            let hasher = zion_core::algorithms::randomx::RandomXHasher::new(&key_vec)?;
            *guard = Some((key_vec.clone(), hasher));
        }

        let (_, hasher) = guard
            .as_mut()
            .ok_or_else(|| anyhow!("RandomX hasher init failed"))?;

        // Build batch inputs: header + nonce_le for each nonce
        let mut inputs = Vec::with_capacity(count as usize);
        for i in 0..count as u64 {
            let nonce = nonce_start + i;
            let mut input = header.to_vec();
            input.extend_from_slice(&nonce.to_le_bytes());
            inputs.push(input);
        }

        let hashes = hasher.hash_batch(&inputs)?;

        let results: Vec<(u64, [u8; 32])> = hashes
            .into_iter()
            .enumerate()
            .map(|(i, h)| (nonce_start + i as u64, h))
            .collect();

        Ok(results)
    })
}

/// Compute hash for any supported algorithm
pub fn compute_hash(algo: NativeAlgorithm, header: &[u8], nonce: u64, _height: u32) -> Result<Vec<u8>> {
    match algo {
        // Cosmic Harmony v3 - use the canonical implementation (same as pool native lib)
        NativeAlgorithm::CosmicHarmony => {
            let h = zion_cosmic_harmony_v3::algorithms_opt::cosmic_harmony_v3(header, nonce);
            Ok(h.data.to_vec())
        }

        // Cosmic Harmony v2 - zion-core implementation (memory hard, scratchpad cached per thread)
        NativeAlgorithm::CosmicHarmonyV2 => {
            let height = _height as u64;
            let prev_hash: [u8; 32] = if header.len() >= 32 {
                let mut tmp = [0u8; 32];
                tmp.copy_from_slice(&header[..32]);
                tmp
            } else {
                // Fallback: derive a deterministic 32-byte seed from header
                zion_core::algorithms::blake3::hash(header)
            };

            CHV2_HASHER.with(|cell| {
                let mut guard = cell.borrow_mut();
                let needs_reinit = match guard.as_ref() {
                    None => true,
                    Some((existing_prev, existing_height, _)) => {
                        existing_prev != &prev_hash || *existing_height != height
                    }
                };

                if needs_reinit {
                    let hasher = zion_core::algorithms::cosmic_harmony_v2::CosmicHarmonyV2::new(
                        &prev_hash,
                        height,
                    );
                    *guard = Some((prev_hash, height, hasher));
                }

                let (_, _, hasher) = guard
                    .as_mut()
                    .ok_or_else(|| anyhow!("Cosmic Harmony v2 hasher initialization failed"))?;

                let hash = hasher.hash(header, nonce);
                Ok(hash.to_vec())
            })
        }
        
        // RandomX - use zion-core's thread-local hasher
        NativeAlgorithm::RandomX => {
            let key_vec = {
                let key = RANDOMX_KEY.read()
                    .map_err(|e| anyhow!("RandomX key lock: {}", e))?;
                if key.is_empty() {
                    return Err(anyhow!("RandomX not initialized - call init_randomx() first"));
                }
                key.clone()
            };

            // Append nonce to header
            let mut input = header.to_vec();
            input.extend_from_slice(&nonce.to_le_bytes());

            RANDOMX_HASHER.with(|cell| {
                let mut guard = cell.borrow_mut();
                let needs_reinit = match guard.as_ref() {
                    None => true,
                    Some((existing_key, _)) => existing_key.as_slice() != key_vec.as_slice(),
                };

                if needs_reinit {
                    let hasher = zion_core::algorithms::randomx::RandomXHasher::new(&key_vec)?;
                    *guard = Some((key_vec.clone(), hasher));
                }

                let (_, hasher) = guard
                    .as_mut()
                    .ok_or_else(|| anyhow!("RandomX hasher initialization failed"))?;

                let hash = hasher.hash(&input)?;
                Ok(hash.to_vec())
            })
        }
        
        // Yescrypt - use zion-core implementation (scrypt-based)
        NativeAlgorithm::Yescrypt => {
            // Must match native pool share validator input construction:
            // take first 156 bytes of the template header and append nonce as u64 LE.
            const HEADER_LEN: usize = 156;
            let mut data = if header.len() >= HEADER_LEN {
                header[..HEADER_LEN].to_vec()
            } else {
                header.to_vec()
            };
            data.extend_from_slice(&nonce.to_le_bytes());

            let hash = zion_core::algorithms::yescrypt::yescrypt_hash_mining(&data, nonce)?;
            Ok(hash.to_vec())
        }
        
        #[cfg(feature = "native-ethash")]
        NativeAlgorithm::Ethash => Ok(ethash_ffi::hash(header, nonce, height).to_vec()),
        
        #[cfg(feature = "native-kawpow")]
        NativeAlgorithm::KawPow => {
            let (hash, _mix) = kawpow_ffi::hash(header, nonce, height);
            Ok(hash.to_vec())
        }
        
        #[cfg(feature = "native-kawpow-gpu")]
        NativeAlgorithm::KawPowGpu => {
            let (hash, _mix) = kawpow_gpu_ffi::hash(header, nonce, height);
            Ok(hash.to_vec())
        }
        
        #[cfg(feature = "native-autolykos")]
        NativeAlgorithm::Autolykos => Ok(autolykos_ffi::hash(header, nonce, height).to_vec()),
        
        #[cfg(feature = "native-kheavyhash")]
        NativeAlgorithm::KHeavyHash => Ok(kheavyhash_ffi::hash(header, nonce).to_vec()),
        
        #[cfg(feature = "native-progpow")]
        NativeAlgorithm::ProgPow => Ok(progpow_ffi::hash(header, nonce, height).to_vec()),
        
        #[cfg(feature = "native-argon2d")]
        NativeAlgorithm::Argon2d => Ok(argon2d_ffi::hash(header, nonce).to_vec()),
        
        // Blake3 - use zion-core fallback implementation (fast, always available)
        NativeAlgorithm::Blake3 => {
            let hash = zion_core::algorithms::blake3::hash_with_nonce(header, nonce as u32);
            Ok(hash.to_vec())
        }
        
        _ => Err(anyhow!("Algorithm {:?} not compiled or not supported", algo)),
    }
}

// ============================================================================
// COSMIC HARMONY INLINE IMPLEMENTATION
// ============================================================================

/// Golden ratio constant φ
const PHI: f64 = 1.618033988749895;

/// Cosmic Harmony hash - ZION native algorithm
fn cosmic_harmony_hash(data: &[u8], nonce: u32) -> [u8; 32] {
    // Create input with nonce
    let mut input = data.to_vec();
    input.extend_from_slice(&nonce.to_le_bytes());
    
    // Initialize state with golden ratio
    let mut state = [0u64; 4];
    for (i, chunk) in input.chunks(8).enumerate() {
        let mut bytes = [0u8; 8];
        bytes[..chunk.len()].copy_from_slice(chunk);
        state[i % 4] ^= u64::from_le_bytes(bytes);
    }
    
    // Apply golden ratio mixing
    for round in 0..8 {
        let phi_scaled = ((PHI * (round as f64 + 1.0)) * 1e15) as u64;
        state[0] = state[0].wrapping_add(state[1]).rotate_left(17) ^ phi_scaled;
        state[1] = state[1].wrapping_add(state[2]).rotate_left(23) ^ state[0];
        state[2] = state[2].wrapping_add(state[3]).rotate_left(31) ^ state[1];
        state[3] = state[3].wrapping_add(state[0]).rotate_left(37) ^ state[2];
    }
    
    // Finalize
    let mut output = [0u8; 32];
    for (i, &s) in state.iter().enumerate() {
        output[i * 8..(i + 1) * 8].copy_from_slice(&s.to_le_bytes());
    }
    output
}

/// Verify hash meets target
pub fn verify_hash(algo: NativeAlgorithm, _header: &[u8], _nonce: u64, _height: u32, _target: &[u8]) -> bool {
    match algo {
        #[cfg(feature = "native-ethash")]
        NativeAlgorithm::Ethash => ethash_ffi::verify(header, nonce, height, target),
        
        #[cfg(feature = "native-kawpow")]
        NativeAlgorithm::KawPow => {
            let (hash, mix) = kawpow_ffi::hash(header, nonce, height);
            kawpow_ffi::verify(header, nonce, height, &mix, target)
        }
        
        #[cfg(feature = "native-kheavyhash")]
        NativeAlgorithm::KHeavyHash => kheavyhash_ffi::verify(header, nonce, target),
        
        #[cfg(feature = "native-progpow")]
        NativeAlgorithm::ProgPow => progpow_ffi::verify(header, nonce, height, target),
        
        #[cfg(feature = "native-argon2d")]
        NativeAlgorithm::Argon2d => argon2d_ffi::verify(header, nonce, target),
        
        #[cfg(feature = "native-blake3")]
        NativeAlgorithm::Blake3 => blake3_ffi::verify(header, nonce, target),
        
        _ => false,
    }
}

/// Run benchmark for algorithm
pub fn benchmark(algo: NativeAlgorithm, _iterations: i32) -> Result<f64> {
    match algo {
        #[cfg(feature = "native-ethash")]
        NativeAlgorithm::Ethash => Ok(ethash_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-kawpow")]
        NativeAlgorithm::KawPow => Ok(kawpow_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-kawpow-gpu")]
        NativeAlgorithm::KawPowGpu => Ok(kawpow_gpu_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-autolykos")]
        NativeAlgorithm::Autolykos => Ok(autolykos_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-kheavyhash")]
        NativeAlgorithm::KHeavyHash => Ok(kheavyhash_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-equihash")]
        NativeAlgorithm::Equihash => Ok(equihash_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-progpow")]
        NativeAlgorithm::ProgPow => Ok(progpow_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-argon2d")]
        NativeAlgorithm::Argon2d => Ok(argon2d_ffi::benchmark(iterations)),
        
        #[cfg(feature = "native-blake3")]
        NativeAlgorithm::Blake3 => Ok(blake3_ffi::benchmark(iterations)),
        
        _ => Err(anyhow!("Algorithm {:?} not compiled", algo)),
    }
}

/// List available native algorithms
pub fn available_algorithms() -> Vec<NativeAlgorithm> {
    let mut algos = Vec::new();
    
    #[cfg(feature = "native-ethash")]
    algos.push(NativeAlgorithm::Ethash);
    
    #[cfg(feature = "native-kawpow")]
    algos.push(NativeAlgorithm::KawPow);
    
    #[cfg(feature = "native-kawpow-gpu")]
    algos.push(NativeAlgorithm::KawPowGpu);
    
    #[cfg(feature = "native-autolykos")]
    algos.push(NativeAlgorithm::Autolykos);
    
    #[cfg(feature = "native-kheavyhash")]
    algos.push(NativeAlgorithm::KHeavyHash);
    
    #[cfg(feature = "native-equihash")]
    algos.push(NativeAlgorithm::Equihash);
    
    #[cfg(feature = "native-progpow")]
    algos.push(NativeAlgorithm::ProgPow);
    
    #[cfg(feature = "native-argon2d")]
    algos.push(NativeAlgorithm::Argon2d);
    
    #[cfg(feature = "native-blake3")]
    algos.push(NativeAlgorithm::Blake3);
    
    algos
}

// ============================================================================
// GPU INITIALIZATION
// ============================================================================

#[cfg(feature = "native-kawpow-gpu")]
pub fn init_kawpow_gpu(device_id: i32) -> Result<()> {
    kawpow_gpu_ffi::init(device_id)
}

#[cfg(feature = "native-kawpow-gpu")]
pub fn shutdown_kawpow_gpu() {
    kawpow_gpu_ffi::shutdown()
}
