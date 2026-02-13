//! FFI exports for Metal GPU miner
//! 
//! C-compatible interface for Python/Swift integration

use std::ffi::{c_char, CString};
use std::ptr;

#[cfg(all(feature = "metal", target_os = "macos"))]
use super::metal_miner::MetalMiner;

/// Opaque miner handle for FFI
#[cfg(all(feature = "metal", target_os = "macos"))]
pub struct MetalMinerHandle {
    inner: MetalMiner,
    device_name: CString,
    last_hashrate: f64,
}

/// Create a new Metal miner
/// 
/// # Safety
/// Returns null if Metal is not available or initialization fails.
#[no_mangle]
#[cfg(all(feature = "metal", target_os = "macos"))]
pub unsafe extern "C" fn metal_miner_create(batch_size: u64) -> *mut MetalMinerHandle {
    match MetalMiner::new(batch_size as usize) {
        Ok(miner) => {
            let device_info = miner.device_info();
            let device_name = CString::new(device_info.name.clone()).unwrap_or_default();
            
            let handle = Box::new(MetalMinerHandle {
                inner: miner,
                device_name,
                last_hashrate: 0.0,
            });
            
            Box::into_raw(handle)
        }
        Err(e) => {
            eprintln!("Metal miner creation failed: {}", e);
            ptr::null_mut()
        }
    }
}

/// Destroy a Metal miner
/// 
/// # Safety
/// `miner` must be a valid pointer returned by `metal_miner_create`.
#[no_mangle]
#[cfg(all(feature = "metal", target_os = "macos"))]
pub unsafe extern "C" fn metal_miner_destroy(miner: *mut MetalMinerHandle) {
    if !miner.is_null() {
        drop(Box::from_raw(miner));
    }
}

/// Mine for a valid nonce
/// 
/// # Safety
/// - `miner` must be a valid pointer
/// - `header` must point to `header_len` valid bytes
/// - `target` must point to 32 valid bytes
/// - `out_nonce` and `out_hash` must be valid pointers
/// 
/// Returns true if a solution was found.
#[no_mangle]
#[cfg(all(feature = "metal", target_os = "macos"))]
pub unsafe extern "C" fn metal_miner_mine(
    miner: *mut MetalMinerHandle,
    header: *const u8,
    header_len: usize,
    target: *const u8,
    start_nonce: u64,
    out_nonce: *mut u64,
    out_hash: *mut u8,
) -> bool {
    if miner.is_null() || header.is_null() || target.is_null() {
        return false;
    }
    
    let handle = &mut *miner;
    
    // Read header
    let header_slice = std::slice::from_raw_parts(header, header_len);
    
    // Read target
    let target_arr: [u8; 32] = {
        let target_slice = std::slice::from_raw_parts(target, 32);
        let mut arr = [0u8; 32];
        arr.copy_from_slice(target_slice);
        arr
    };
    
    // Mine
    match handle.inner.mine(header_slice, &target_arr, start_nonce) {
        Some((nonce, hash)) => {
            if !out_nonce.is_null() {
                *out_nonce = nonce;
            }
            if !out_hash.is_null() {
                std::ptr::copy_nonoverlapping(hash.as_ptr(), out_hash, 32);
            }
            true
        }
        None => false,
    }
}

/// Run benchmark and return hashrate
/// 
/// # Safety
/// `miner` must be a valid pointer
#[no_mangle]
#[cfg(all(feature = "metal", target_os = "macos"))]
pub unsafe extern "C" fn metal_miner_benchmark(
    miner: *mut MetalMinerHandle,
    duration_secs: f64,
) -> f64 {
    if miner.is_null() {
        return 0.0;
    }
    
    let handle = &mut *miner;
    let hashrate = handle.inner.benchmark(duration_secs);
    handle.last_hashrate = hashrate;
    hashrate
}

/// Get current hashrate
/// 
/// # Safety
/// `miner` must be a valid pointer
#[no_mangle]
#[cfg(all(feature = "metal", target_os = "macos"))]
pub unsafe extern "C" fn metal_miner_get_hashrate(miner: *mut MetalMinerHandle) -> f64 {
    if miner.is_null() {
        return 0.0;
    }
    
    let handle = &*miner;
    handle.last_hashrate
}

/// Get device name
/// 
/// # Safety
/// `miner` must be a valid pointer.
/// Returns a pointer to a null-terminated string that is valid until the miner is destroyed.
#[no_mangle]
#[cfg(all(feature = "metal", target_os = "macos"))]
pub unsafe extern "C" fn metal_miner_get_device_name(miner: *mut MetalMinerHandle) -> *const c_char {
    if miner.is_null() {
        return ptr::null();
    }
    
    let handle = &*miner;
    handle.device_name.as_ptr()
}

/// Batch compute hashes (without mining target check)
/// 
/// # Safety
/// - `miner` must be a valid pointer
/// - `header` must point to `header_len` valid bytes  
/// - `out_hashes` must point to at least `count * 32` bytes
#[no_mangle]
#[cfg(all(feature = "metal", target_os = "macos"))]
pub unsafe extern "C" fn metal_miner_batch_hash(
    miner: *mut MetalMinerHandle,
    header: *const u8,
    header_len: usize,
    start_nonce: u64,
    count: usize,
    out_hashes: *mut u8,
) -> bool {
    if miner.is_null() || header.is_null() || out_hashes.is_null() {
        return false;
    }
    
    let handle = &mut *miner;
    let header_slice = std::slice::from_raw_parts(header, header_len);
    
    let hashes = handle.inner.batch_hash(header_slice, start_nonce, count);
    
    for (i, hash) in hashes.iter().enumerate() {
        std::ptr::copy_nonoverlapping(hash.as_ptr(), out_hashes.add(i * 32), 32);
    }
    
    true
}

// Stubs for non-macOS platforms
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub struct MetalMinerHandle;

#[no_mangle]
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub unsafe extern "C" fn metal_miner_create(_batch_size: u64) -> *mut MetalMinerHandle {
    eprintln!("Metal is only available on macOS");
    ptr::null_mut()
}

#[no_mangle]
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub unsafe extern "C" fn metal_miner_destroy(_miner: *mut MetalMinerHandle) {}

#[no_mangle]
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub unsafe extern "C" fn metal_miner_mine(
    _miner: *mut MetalMinerHandle,
    _header: *const u8,
    _header_len: usize,
    _target: *const u8,
    _start_nonce: u64,
    _out_nonce: *mut u64,
    _out_hash: *mut u8,
) -> bool {
    false
}

#[no_mangle]
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub unsafe extern "C" fn metal_miner_benchmark(
    _miner: *mut MetalMinerHandle,
    _duration_secs: f64,
) -> f64 {
    0.0
}

#[no_mangle]
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub unsafe extern "C" fn metal_miner_get_hashrate(_miner: *mut MetalMinerHandle) -> f64 {
    0.0
}

#[no_mangle]
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub unsafe extern "C" fn metal_miner_get_device_name(_miner: *mut MetalMinerHandle) -> *const c_char {
    ptr::null()
}

#[no_mangle]
#[cfg(not(all(feature = "metal", target_os = "macos")))]
pub unsafe extern "C" fn metal_miner_batch_hash(
    _miner: *mut MetalMinerHandle,
    _header: *const u8,
    _header_len: usize,
    _start_nonce: u64,
    _count: usize,
    _out_hashes: *mut u8,
) -> bool {
    false
}
