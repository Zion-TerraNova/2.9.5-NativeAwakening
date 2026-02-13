//! C-compatible FFI interface for Cosmic Harmony v3
//! 
//! This module provides C ABI functions for use in:
//! - Python miners (via ctypes/cffi)
//! - Node.js addons (via N-API)
//! - Any language with C FFI support
//!
//! # Example (Python)
//! ```python
//! import ctypes
//! 
//! lib = ctypes.CDLL("libzion_cosmic_harmony_v3.so")
//! lib.cosmic_harmony_v3_hash.argtypes = [
//!     ctypes.POINTER(ctypes.c_uint8),  # input
//!     ctypes.c_size_t,                  # input_len
//!     ctypes.c_uint64,                  # nonce
//!     ctypes.POINTER(ctypes.c_uint8),  # output (32 bytes)
//! ]
//! lib.cosmic_harmony_v3_hash.restype = ctypes.c_int
//! ```

use crate::algorithms_opt::{cosmic_harmony_v3, cosmic_harmony_v3_batch, Hash32};
use std::slice;

/// Version of the FFI interface
pub const FFI_VERSION: u32 = 1;

// ============================================================================
// SINGLE HASH FUNCTIONS
// ============================================================================

/// Compute Cosmic Harmony v3 hash
/// 
/// # Arguments
/// * `input_ptr` - Pointer to input data (block header)
/// * `input_len` - Length of input data
/// * `nonce` - Mining nonce
/// * `output_ptr` - Pointer to 32-byte output buffer
/// 
/// # Returns
/// * 0 on success
/// * -1 on null pointer
/// * -2 on invalid input length
#[no_mangle]
pub unsafe extern "C" fn cosmic_harmony_v3_hash(
    input_ptr: *const u8,
    input_len: usize,
    nonce: u64,
    output_ptr: *mut u8,
) -> i32 {
    // Validate pointers
    if input_ptr.is_null() || output_ptr.is_null() {
        return -1;
    }
    
    // Validate input length
    if input_len == 0 || input_len > 1024 {
        return -2;
    }
    
    // Create slice from raw pointer
    let input = slice::from_raw_parts(input_ptr, input_len);
    
    // Compute hash
    let result = cosmic_harmony_v3(input, nonce);
    
    // Copy result to output
    let output = slice::from_raw_parts_mut(output_ptr, 32);
    output.copy_from_slice(&result.data);
    
    0 // Success
}

/// Compute Cosmic Harmony v3 hash with block height (for PoW compatibility with core)
/// 
/// IMPORTANT: This function XORs nonce with height to match core's hash algorithm.
/// Use this for mining shares that need to be validated by ZION Core.
/// 
/// # Arguments
/// * `input_ptr` - Pointer to input data (block header without nonce)
/// * `input_len` - Length of input data
/// * `nonce` - Mining nonce (will be XORed with height)
/// * `height` - Block height (used to XOR with nonce for difficulty variation)
/// * `output_ptr` - Pointer to 32-byte output buffer
/// 
/// # Returns
/// * 0 on success
/// * -1 on null pointer
/// * -2 on invalid input length
#[no_mangle]
pub unsafe extern "C" fn cosmic_harmony_v3_hash_with_height(
    input_ptr: *const u8,
    input_len: usize,
    nonce: u64,
    height: u64,
    output_ptr: *mut u8,
) -> i32 {
    // Validate pointers
    if input_ptr.is_null() || output_ptr.is_null() {
        return -1;
    }
    
    // Validate input length
    if input_len == 0 || input_len > 1024 {
        return -2;
    }
    
    // Create slice from raw pointer
    let input = slice::from_raw_parts(input_ptr, input_len);
    
    // XOR nonce with height to match core's algorithm (nonce32 = nonce ^ height)
    let effective_nonce = (nonce as u32) ^ (height as u32);
    
    // Compute hash with effective nonce
    let result = cosmic_harmony_v3(input, effective_nonce as u64);
    
    // Copy result to output
    let output = slice::from_raw_parts_mut(output_ptr, 32);
    output.copy_from_slice(&result.data);
    
    0 // Success
}

/// Check if hash meets difficulty target
/// 
/// # Arguments
/// * `hash_ptr` - Pointer to 32-byte hash
/// * `target_ptr` - Pointer to 32-byte target (big-endian)
/// 
/// # Returns
/// * 1 if hash <= target (valid block)
/// * 0 if hash > target (invalid)
/// * -1 on null pointer
#[no_mangle]
pub unsafe extern "C" fn cosmic_harmony_v3_check_difficulty(
    hash_ptr: *const u8,
    target_ptr: *const u8,
) -> i32 {
    if hash_ptr.is_null() || target_ptr.is_null() {
        return -1;
    }
    
    let hash = slice::from_raw_parts(hash_ptr, 32);
    let target = slice::from_raw_parts(target_ptr, 32);
    
    // Compare hash to target (big-endian comparison)
    for i in 0..32 {
        if hash[i] < target[i] {
            return 1; // Hash is smaller = valid
        }
        if hash[i] > target[i] {
            return 0; // Hash is bigger = invalid
        }
    }
    
    1 // Equal = valid
}

// ============================================================================
// BATCH HASH FUNCTIONS
// ============================================================================

/// Compute batch of Cosmic Harmony v3 hashes
/// 
/// # Arguments
/// * `input_ptr` - Pointer to input data (block header)
/// * `input_len` - Length of input data
/// * `start_nonce` - Starting nonce
/// * `count` - Number of hashes to compute
/// * `output_ptr` - Pointer to output buffer (32 * count bytes)
/// 
/// # Returns
/// * 0 on success
/// * -1 on null pointer
/// * -2 on invalid input length
/// * -3 on count too large (max 1M)
#[no_mangle]
pub unsafe extern "C" fn cosmic_harmony_v3_batch_hash(
    input_ptr: *const u8,
    input_len: usize,
    start_nonce: u64,
    count: usize,
    output_ptr: *mut u8,
) -> i32 {
    // Validate pointers
    if input_ptr.is_null() || output_ptr.is_null() {
        return -1;
    }
    
    // Validate input length
    if input_len == 0 || input_len > 1024 {
        return -2;
    }
    
    // Limit batch size
    if count > 1_000_000 {
        return -3;
    }
    
    let input = slice::from_raw_parts(input_ptr, input_len);
    
    // Allocate results
    let mut results = vec![Hash32::new(); count];
    
    // Compute batch
    cosmic_harmony_v3_batch(input, start_nonce, count, &mut results);
    
    // Copy results to output
    let output = slice::from_raw_parts_mut(output_ptr, 32 * count);
    for (i, hash) in results.iter().enumerate() {
        output[i * 32..(i + 1) * 32].copy_from_slice(&hash.data);
    }
    
    0 // Success
}

/// Find nonce that meets difficulty target (batch search)
/// 
/// # Arguments
/// * `input_ptr` - Pointer to input data (block header)
/// * `input_len` - Length of input data
/// * `start_nonce` - Starting nonce
/// * `max_iterations` - Maximum iterations to try
/// * `target_ptr` - Pointer to 32-byte target (big-endian)
/// * `found_nonce_ptr` - Output: found nonce (if return value is 1)
/// * `found_hash_ptr` - Output: found hash (32 bytes, if return value is 1)
/// 
/// # Returns
/// * 1 if solution found
/// * 0 if no solution in range
/// * -1 on null pointer
#[no_mangle]
pub unsafe extern "C" fn cosmic_harmony_v3_find_nonce(
    input_ptr: *const u8,
    input_len: usize,
    start_nonce: u64,
    max_iterations: u64,
    target_ptr: *const u8,
    found_nonce_ptr: *mut u64,
    found_hash_ptr: *mut u8,
) -> i32 {
    // Validate pointers
    if input_ptr.is_null() || target_ptr.is_null() || 
       found_nonce_ptr.is_null() || found_hash_ptr.is_null() {
        return -1;
    }
    
    if input_len == 0 || input_len > 1024 {
        return -1;
    }
    
    let input = slice::from_raw_parts(input_ptr, input_len);
    let target = slice::from_raw_parts(target_ptr, 32);
    
    // Batch size for efficient searching
    const BATCH_SIZE: usize = 1024;
    let mut results = vec![Hash32::new(); BATCH_SIZE];
    
    let mut current_nonce = start_nonce;
    let end_nonce = start_nonce.saturating_add(max_iterations);
    
    while current_nonce < end_nonce {
        let batch_count = std::cmp::min(BATCH_SIZE, (end_nonce - current_nonce) as usize);
        
        cosmic_harmony_v3_batch(input, current_nonce, batch_count, &mut results);
        
        // Check each hash
        for (i, hash) in results[..batch_count].iter().enumerate() {
            if check_hash_meets_target(&hash.data, target) {
                *found_nonce_ptr = current_nonce + i as u64;
                let output = slice::from_raw_parts_mut(found_hash_ptr, 32);
                output.copy_from_slice(&hash.data);
                return 1; // Found!
            }
        }
        
        current_nonce += batch_count as u64;
    }
    
    0 // Not found
}

// ============================================================================
// PARALLEL BATCH FUNCTIONS (requires feature "parallel")
// ============================================================================

#[cfg(feature = "parallel")]
use rayon::prelude::*;

/// Compute parallel batch of hashes (multi-threaded)
/// 
/// Requires the "parallel" feature to be enabled.
/// Falls back to single-threaded if parallel feature not available.
#[no_mangle]
#[cfg(feature = "parallel")]
pub unsafe extern "C" fn cosmic_harmony_v3_parallel_hash(
    input_ptr: *const u8,
    input_len: usize,
    start_nonce: u64,
    count: usize,
    output_ptr: *mut u8,
    thread_count: usize,
) -> i32 {
    // Validate pointers
    if input_ptr.is_null() || output_ptr.is_null() {
        return -1;
    }
    
    if input_len == 0 || input_len > 1024 {
        return -2;
    }
    
    if count > 10_000_000 {
        return -3;
    }
    
    // Configure thread pool
    #[cfg(feature = "parallel")]
    let thread_ct = if thread_count == 0 { num_cpus::get() } else { thread_count };
    #[cfg(not(feature = "parallel"))]
    let thread_ct = if thread_count == 0 { 4 } else { thread_count };
    
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(thread_ct)
        .build()
        .unwrap();
    
    let input = slice::from_raw_parts(input_ptr, input_len);
    
    // Parallel compute
    let results: Vec<Hash32> = pool.install(|| {
        (0..count as u64)
            .into_par_iter()
            .map(|i| cosmic_harmony_v3(input, start_nonce + i))
            .collect()
    });
    
    // Copy results
    let output = slice::from_raw_parts_mut(output_ptr, 32 * count);
    for (i, hash) in results.iter().enumerate() {
        output[i * 32..(i + 1) * 32].copy_from_slice(&hash.data);
    }
    
    0
}

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

/// Get FFI interface version
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_version() -> u32 {
    FFI_VERSION
}

/// Get number of CPU cores (for parallel mining)
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_cpu_count() -> u32 {
    #[cfg(feature = "parallel")]
    { num_cpus::get() as u32 }
    #[cfg(not(feature = "parallel"))]
    { 4 }
}

/// Get library info string
/// 
/// Returns pointer to static null-terminated string.
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_info() -> *const std::ffi::c_char {
    static INFO: &[u8] = b"ZION Cosmic Harmony v3 - Native Mining Library\0";
    INFO.as_ptr() as *const std::ffi::c_char
}

// ============================================================================
// GPU MINING FUNCTIONS (OpenCL)
// ============================================================================

#[cfg(feature = "gpu")]
use crate::gpu::{GpuMiner, GpuConfig};

#[cfg(feature = "gpu")]
use std::sync::Mutex;

#[cfg(feature = "gpu")]
static GPU_MINER: Mutex<Option<GpuMiner>> = Mutex::new(None);

/// Initialize GPU miner
/// 
/// # Arguments
/// * `device_id` - GPU device index (0 for first GPU)
/// * `batch_size` - Hashes per batch (recommended: 1000000)
/// 
/// # Returns
/// * 0 on success
/// * -1 if no GPU found
/// * -2 on initialization error
#[cfg(feature = "gpu")]
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_gpu_init(device_id: u32, batch_size: u32) -> i32 {
    let config = GpuConfig {
        device_id: device_id as usize,
        batch_size: batch_size as usize,
        work_group_size: 256,
        profiling: false,
    };
    
    match GpuMiner::new(config) {
        Ok(miner) => {
            if let Ok(mut guard) = GPU_MINER.lock() {
                *guard = Some(miner);
                0
            } else {
                -2
            }
        }
        Err(_) => -1
    }
}

/// Get GPU device count
/// 
/// # Returns
/// Number of available GPU devices
#[cfg(feature = "gpu")]
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_gpu_count() -> u32 {
    match GpuMiner::list_devices() {
        Ok(devices) => devices.len() as u32,
        Err(_) => 0
    }
}

/// Mine on GPU
/// 
/// # Arguments
/// * `header_ptr` - Block header (max 136 bytes)
/// * `header_len` - Header length
/// * `start_nonce` - Starting nonce
/// * `target_ptr` - 32-byte difficulty target
/// * `found_nonce_ptr` - Output: found nonce (if successful)
/// * `found_hash_ptr` - Output: found hash (32 bytes, if successful)
/// 
/// # Returns
/// * 1 if solution found (nonce/hash written to outputs)
/// * 0 if no solution in this batch
/// * -1 on error
#[cfg(feature = "gpu")]
#[no_mangle]
pub unsafe extern "C" fn cosmic_harmony_v3_gpu_mine(
    header_ptr: *const u8,
    header_len: usize,
    start_nonce: u64,
    target_ptr: *const u8,
    found_nonce_ptr: *mut u64,
    found_hash_ptr: *mut u8,
) -> i32 {
    if header_ptr.is_null() || target_ptr.is_null() {
        return -1;
    }
    
    let header = slice::from_raw_parts(header_ptr, header_len);
    let target: [u8; 32] = slice::from_raw_parts(target_ptr, 32).try_into().unwrap();
    
    if let Ok(mut guard) = GPU_MINER.lock() {
        if let Some(ref mut miner) = *guard {
            match miner.mine(header, start_nonce, &target) {
                Ok(Some((nonce, hash))) => {
                    if !found_nonce_ptr.is_null() {
                        *found_nonce_ptr = nonce;
                    }
                    if !found_hash_ptr.is_null() {
                        let out = slice::from_raw_parts_mut(found_hash_ptr, 32);
                        out.copy_from_slice(&hash);
                    }
                    return 1;
                }
                Ok(None) => return 0,
                Err(_) => return -1,
            }
        }
    }
    -1
}

/// Cleanup GPU miner
#[cfg(feature = "gpu")]
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_gpu_cleanup() {
    if let Ok(mut guard) = GPU_MINER.lock() {
        *guard = None;
    }
}

// Stub functions when GPU feature is disabled
#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_gpu_init(_device_id: u32, _batch_size: u32) -> i32 { -1 }

#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_gpu_count() -> u32 { 0 }

#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub unsafe extern "C" fn cosmic_harmony_v3_gpu_mine(
    _header_ptr: *const u8, _header_len: usize, _start_nonce: u64,
    _target_ptr: *const u8, _found_nonce_ptr: *mut u64, _found_hash_ptr: *mut u8,
) -> i32 { -1 }

#[cfg(not(feature = "gpu"))]
#[no_mangle]
pub extern "C" fn cosmic_harmony_v3_gpu_cleanup() {}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

#[inline]
fn check_hash_meets_target(hash: &[u8; 32], target: &[u8]) -> bool {
    for i in 0..32 {
        if hash[i] < target[i] {
            return true;
        }
        if hash[i] > target[i] {
            return false;
        }
    }
    true // Equal
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_ffi_hash() {
        let input = b"ZION block header test";
        let mut output = [0u8; 32];
        
        unsafe {
            let result = cosmic_harmony_v3_hash(
                input.as_ptr(),
                input.len(),
                12345,
                output.as_mut_ptr(),
            );
            
            assert_eq!(result, 0);
            // Verify output is not all zeros
            assert!(output.iter().any(|&b| b != 0));
        }
    }
    
    #[test]
    fn test_ffi_batch() {
        let input = b"ZION block header test";
        let mut output = vec![0u8; 32 * 10];
        
        unsafe {
            let result = cosmic_harmony_v3_batch_hash(
                input.as_ptr(),
                input.len(),
                0,
                10,
                output.as_mut_ptr(),
            );
            
            assert_eq!(result, 0);
            
            // Each hash should be different (different nonces)
            let hash1 = &output[0..32];
            let hash2 = &output[32..64];
            assert_ne!(hash1, hash2);
        }
    }
    
    #[test]
    fn test_ffi_difficulty_check() {
        let hash = [0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
                    0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
        
        // Easy target (0x00 01 ...)
        let easy_target = [0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                          0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                          0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                          0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        
        // Hard target (0x00 00 00 01 ...)
        let hard_target = [0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00,
                          0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                          0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
                          0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        
        unsafe {
            // Hash should meet easy target
            assert_eq!(cosmic_harmony_v3_check_difficulty(hash.as_ptr(), easy_target.as_ptr()), 1);
            
            // Hash should NOT meet hard target (0x00 00 FF > 0x00 00 00 01)
            assert_eq!(cosmic_harmony_v3_check_difficulty(hash.as_ptr(), hard_target.as_ptr()), 0);
        }
    }
    
    #[test]
    fn test_version() {
        assert_eq!(cosmic_harmony_v3_version(), FFI_VERSION);
    }
    
    #[test]
    fn test_cpu_count() {
        assert!(cosmic_harmony_v3_cpu_count() >= 1);
    }
}
