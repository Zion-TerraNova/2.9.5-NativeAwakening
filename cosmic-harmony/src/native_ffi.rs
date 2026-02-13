//! Native Algorithm FFI - Clean Static Linking
//!
//! Direct C FFI bindings to native libraries in ../native-libs/
//! Enabled via feature flags: native-randomx, native-yescrypt, native-cosmic-harmony
//!
//! Usage:
//!   cargo build --features native-randomx    # Link RandomX
//!   cargo build --features native-all        # Link all native libs


// ============================================================================
// RANDOMX FFI (only when native-randomx feature enabled)
// ============================================================================

#[cfg(feature = "native-randomx")]
mod randomx_ffi {
    use std::ffi::{c_char, CString};
    use std::sync::Once;
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn zion_randomx_init(key_hex: *const c_char, threads: i32) -> i32;
        pub fn zion_randomx_hash_bytes(input: *const u8, len: usize, output: *mut u8);
        pub fn zion_randomx_hash_bytes_vm(vm_index: i32, input: *const u8, len: usize, output: *mut u8);
        pub fn zion_randomx_get_num_threads() -> i32;
        pub fn zion_randomx_check_difficulty(hash: *const u8, difficulty: i32) -> i32;
        pub fn zion_randomx_cleanup();
        pub fn zion_randomx_version() -> *const c_char;
    }
    
    static INIT: Once = Once::new();
    static mut INITIALIZED: bool = false;
    
    pub fn init(pool_key: &str, threads: i32) -> anyhow::Result<bool> {
        let mut result = false;
        
        INIT.call_once(|| {
            let key_hex = hex::encode(pool_key.as_bytes());
            if let Ok(key_cstr) = CString::new(key_hex) {
                unsafe {
                    let ret = zion_randomx_init(key_cstr.as_ptr(), threads);
                    result = ret != 0;
                    INITIALIZED = result;
                }
            }
        });
        
        unsafe {
            if INITIALIZED { Ok(true) }
            else { Err(anyhow::anyhow!("RandomX initialization failed")) }
        }
    }
    
    pub fn hash(input: &[u8]) -> anyhow::Result<HashOutput> {
        unsafe {
            if !INITIALIZED {
                return Err(anyhow::anyhow!("RandomX not initialized - call init first"));
            }
            let mut output = [0u8; 32];
            zion_randomx_hash_bytes(input.as_ptr(), input.len(), output.as_mut_ptr());
            Ok(HashOutput { hash: output.to_vec() })
        }
    }
    
    pub fn hash_vm(vm_index: i32, input: &[u8]) -> anyhow::Result<HashOutput> {
        unsafe {
            if !INITIALIZED {
                return Err(anyhow::anyhow!("RandomX not initialized"));
            }
            let mut output = [0u8; 32];
            zion_randomx_hash_bytes_vm(vm_index, input.as_ptr(), input.len(), output.as_mut_ptr());
            Ok(HashOutput { hash: output.to_vec() })
        }
    }
    
    pub fn threads() -> i32 {
        unsafe { zion_randomx_get_num_threads() }
    }
    
    pub fn check_diff(hash: &[u8], difficulty: i32) -> bool {
        if hash.len() != 32 { return false; }
        unsafe { zion_randomx_check_difficulty(hash.as_ptr(), difficulty) != 0 }
    }
}

#[cfg(feature = "native-randomx")]
pub use randomx_ffi::{init as randomx_init, hash as randomx_hash, hash_vm as randomx_hash_vm, threads as randomx_threads, check_diff as randomx_check_diff};

// ============================================================================
// YESCRYPT FFI (only when native-yescrypt feature enabled)
// ============================================================================

#[cfg(feature = "native-yescrypt")]
mod yescrypt_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn zion_yescrypt_hash(input: *const u8, len: usize, output: *mut u8) -> i32;
    }
    
    pub fn hash(input: &[u8]) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe {
            let ret = zion_yescrypt_hash(input.as_ptr(), input.len(), output.as_mut_ptr());
            if ret != 0 {
                return Err(anyhow::anyhow!("Yescrypt hash failed with code {}", ret));
            }
        }
        Ok(HashOutput { hash: output.to_vec() })
    }
}

#[cfg(feature = "native-yescrypt")]
pub use yescrypt_ffi::hash as yescrypt_hash;

// ============================================================================
// COSMIC HARMONY V2 FFI (only when native-cosmic-harmony feature enabled)
// ============================================================================

#[cfg(feature = "native-cosmic-harmony")]
mod cosmic_harmony_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn zion_cosmic_harmony_hash(input: *const u8, len: usize, output: *mut u8) -> i32;
    }
    
    pub fn hash(input: &[u8]) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe {
            let ret = zion_cosmic_harmony_hash(input.as_ptr(), input.len(), output.as_mut_ptr());
            if ret != 0 {
                return Err(anyhow::anyhow!("Cosmic Harmony v2 hash failed with code {}", ret));
            }
        }
        Ok(HashOutput { hash: output.to_vec() })
    }
}

#[cfg(feature = "native-cosmic-harmony")]
pub use cosmic_harmony_ffi::hash as cosmic_harmony_v2_hash;

// ============================================================================
// AUTOLYKOS V2 FFI (only when native-autolykos feature enabled)
// ============================================================================

#[cfg(feature = "native-autolykos")]
mod autolykos_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        /// Compute Autolykos v2 hash
        pub fn autolykos_hash(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            output: *mut u8
        ) -> u64;
        
        /// Verify Autolykos solution
        pub fn autolykos_verify(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            target: u64
        ) -> i32;
        
        /// Benchmark CPU performance
        pub fn autolykos_benchmark_cpu(iterations: i32) -> f64;
    }
    
    pub fn hash(input: &[u8], nonce: u64, height: u32) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe {
            autolykos_hash(
                input.as_ptr(),
                input.len(),
                nonce,
                height,
                output.as_mut_ptr()
            );
        }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn verify(input: &[u8], nonce: u64, height: u32, target: u64) -> bool {
        unsafe {
            autolykos_verify(input.as_ptr(), input.len(), nonce, height, target) != 0
        }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { autolykos_benchmark_cpu(iterations) }
    }
}

#[cfg(feature = "native-autolykos")]
pub use autolykos_ffi::{hash as autolykos_hash, verify as autolykos_verify, benchmark as autolykos_benchmark};

// ============================================================================
// KAWPOW FFI (only when native-kawpow feature enabled) - RVN/CLORE CRITICAL
// ============================================================================

#[cfg(feature = "native-kawpow")]
mod kawpow_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        /// Compute KawPow hash
        pub fn kawpow_hash(
            header: *const u8,       // 32-byte header hash
            nonce: u64,              // 8-byte nonce
            height: u32,             // Block height
            epoch: u32,              // DAG epoch
            mix_out: *mut u8,        // 32-byte mix hash output
            hash_out: *mut u8        // 32-byte final hash output
        );
        
        /// Verify KawPow solution
        pub fn kawpow_verify(
            header: *const u8,
            nonce: u64,
            height: u32,
            epoch: u32,
            expected_mix: *const u8,
            target: *const u8
        ) -> i32;
        
        /// Get epoch for block height
        pub fn kawpow_get_epoch(height: u32) -> u32;
        
        /// Benchmark CPU performance
        pub fn kawpow_benchmark_cpu(iterations: i32) -> f64;
        
        /// Get version string
        pub fn kawpow_version() -> *const std::ffi::c_char;
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> anyhow::Result<(HashOutput, Vec<u8>)> {
        if header.len() != 32 {
            return Err(anyhow::anyhow!("KawPow requires 32-byte header hash"));
        }
        
        let epoch = unsafe { kawpow_get_epoch(height) };
        let mut mix = [0u8; 32];
        let mut hash = [0u8; 32];
        
        unsafe {
            kawpow_hash(
                header.as_ptr(),
                nonce,
                height,
                epoch,
                mix.as_mut_ptr(),
                hash.as_mut_ptr()
            );
        }
        
        Ok((HashOutput { hash: hash.to_vec() }, mix.to_vec()))
    }
    
    pub fn verify(header: &[u8], nonce: u64, height: u32, expected_mix: &[u8], target: &[u8]) -> bool {
        if header.len() != 32 || expected_mix.len() != 32 || target.len() != 32 {
            return false;
        }
        
        let epoch = unsafe { kawpow_get_epoch(height) };
        
        unsafe {
            kawpow_verify(
                header.as_ptr(),
                nonce,
                height,
                epoch,
                expected_mix.as_ptr(),
                target.as_ptr()
            ) != 0
        }
    }
    
    pub fn get_epoch(height: u32) -> u32 {
        unsafe { kawpow_get_epoch(height) }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { kawpow_benchmark_cpu(iterations) }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = kawpow_version();
            if ptr.is_null() {
                "Unknown".to_string()
            } else {
                std::ffi::CStr::from_ptr(ptr)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }
}

#[cfg(feature = "native-kawpow")]
pub use kawpow_ffi::{
    hash as kawpow_hash, 
    verify as kawpow_verify, 
    get_epoch as kawpow_get_epoch,
    benchmark as kawpow_benchmark,
    version as kawpow_version
};

// ============================================================================
// KAWPOW GPU FFI (only when native-kawpow-gpu feature enabled)
// ============================================================================

#[cfg(feature = "native-kawpow-gpu")]
mod kawpow_gpu_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        /// Initialize GPU context
        pub fn kawpow_gpu_init(device_id: i32, platform_id: i32) -> i32;
        
        /// Shutdown GPU
        pub fn kawpow_gpu_shutdown();
        
        /// Set epoch (regenerate DAG if needed)
        pub fn kawpow_gpu_set_epoch(epoch: u32) -> i32;
        
        /// Compute KawPow hash on GPU
        pub fn kawpow_gpu_hash(
            header: *const u8,
            nonce: u64,
            height: u32,
            mix_out: *mut u8,
            hash_out: *mut u8
        );
        
        /// Benchmark GPU performance
        pub fn kawpow_gpu_benchmark(iterations: i32) -> f64;
        
        /// Get current hashrate
        pub fn kawpow_gpu_get_hashrate() -> f64;
        
        /// Get device name
        pub fn kawpow_gpu_get_device_name() -> *const std::ffi::c_char;
        
        /// Get current epoch
        pub fn kawpow_gpu_get_epoch() -> u32;
        
        /// Run test
        pub fn kawpow_gpu_test();
        
        /// Get version
        pub fn kawpow_gpu_version() -> *const std::ffi::c_char;
    }
    
    pub fn init(device_id: i32, platform_id: i32) -> anyhow::Result<()> {
        let result = unsafe { kawpow_gpu_init(device_id, platform_id) };
        if result == 0 {
            Ok(())
        } else {
            Err(anyhow::anyhow!("KawPow GPU init failed with code {}", result))
        }
    }
    
    pub fn shutdown() {
        unsafe { kawpow_gpu_shutdown() }
    }
    
    pub fn set_epoch(epoch: u32) -> anyhow::Result<()> {
        let result = unsafe { kawpow_gpu_set_epoch(epoch) };
        if result == 0 {
            Ok(())
        } else {
            Err(anyhow::anyhow!("Failed to set epoch {} (code {})", epoch, result))
        }
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> anyhow::Result<(HashOutput, Vec<u8>)> {
        if header.len() != 32 {
            return Err(anyhow::anyhow!("KawPow GPU requires 32-byte header"));
        }
        
        let mut mix = [0u8; 32];
        let mut hash = [0u8; 32];
        
        unsafe {
            kawpow_gpu_hash(
                header.as_ptr(),
                nonce,
                height,
                mix.as_mut_ptr(),
                hash.as_mut_ptr()
            );
        }
        
        Ok((HashOutput { hash: hash.to_vec() }, mix.to_vec()))
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { kawpow_gpu_benchmark(iterations) }
    }
    
    pub fn get_hashrate() -> f64 {
        unsafe { kawpow_gpu_get_hashrate() }
    }
    
    pub fn get_device_name() -> String {
        unsafe {
            let ptr = kawpow_gpu_get_device_name();
            if ptr.is_null() {
                "Unknown".to_string()
            } else {
                std::ffi::CStr::from_ptr(ptr)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }
    
    pub fn get_epoch() -> u32 {
        unsafe { kawpow_gpu_get_epoch() }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = kawpow_gpu_version();
            if ptr.is_null() {
                "Unknown".to_string()
            } else {
                std::ffi::CStr::from_ptr(ptr)
                    .to_string_lossy()
                    .into_owned()
            }
        }
    }
}

#[cfg(feature = "native-kawpow-gpu")]
pub use kawpow_gpu_ffi::{
    init as kawpow_gpu_init,
    shutdown as kawpow_gpu_shutdown,
    set_epoch as kawpow_gpu_set_epoch,
    hash as kawpow_gpu_hash,
    benchmark as kawpow_gpu_benchmark,
    get_hashrate as kawpow_gpu_get_hashrate,
    get_device_name as kawpow_gpu_get_device_name,
    get_epoch as kawpow_gpu_get_epoch,
    version as kawpow_gpu_version
};

// ============================================================================
// FEATURE FLAG CONSTANTS
// ============================================================================

#[cfg(feature = "native-randomx")]
pub const HAS_NATIVE_RANDOMX: bool = true;
#[cfg(not(feature = "native-randomx"))]
pub const HAS_NATIVE_RANDOMX: bool = false;

#[cfg(feature = "native-yescrypt")]
pub const HAS_NATIVE_YESCRYPT: bool = true;
#[cfg(not(feature = "native-yescrypt"))]
pub const HAS_NATIVE_YESCRYPT: bool = false;

#[cfg(feature = "native-cosmic-harmony")]
pub const HAS_NATIVE_COSMIC_HARMONY: bool = true;
#[cfg(not(feature = "native-cosmic-harmony"))]
pub const HAS_NATIVE_COSMIC_HARMONY: bool = false;

#[cfg(feature = "native-autolykos")]
pub const HAS_NATIVE_AUTOLYKOS: bool = true;
#[cfg(not(feature = "native-autolykos"))]
pub const HAS_NATIVE_AUTOLYKOS: bool = false;

#[cfg(feature = "native-kawpow")]
pub const HAS_NATIVE_KAWPOW: bool = true;
#[cfg(not(feature = "native-kawpow"))]
pub const HAS_NATIVE_KAWPOW: bool = false;

#[cfg(feature = "native-kawpow-gpu")]
pub const HAS_NATIVE_KAWPOW_GPU: bool = true;
#[cfg(not(feature = "native-kawpow-gpu"))]
pub const HAS_NATIVE_KAWPOW_GPU: bool = false;

#[cfg(feature = "native-ethash")]
pub const HAS_NATIVE_ETHASH: bool = true;
#[cfg(not(feature = "native-ethash"))]
pub const HAS_NATIVE_ETHASH: bool = false;

#[cfg(feature = "native-kheavyhash")]
pub const HAS_NATIVE_KHEAVYHASH: bool = true;
#[cfg(not(feature = "native-kheavyhash"))]
pub const HAS_NATIVE_KHEAVYHASH: bool = false;

#[cfg(feature = "native-equihash")]
pub const HAS_NATIVE_EQUIHASH: bool = true;
#[cfg(not(feature = "native-equihash"))]
pub const HAS_NATIVE_EQUIHASH: bool = false;

#[cfg(feature = "native-progpow")]
pub const HAS_NATIVE_PROGPOW: bool = true;
#[cfg(not(feature = "native-progpow"))]
pub const HAS_NATIVE_PROGPOW: bool = false;

#[cfg(feature = "native-argon2d")]
pub const HAS_NATIVE_ARGON2D: bool = true;
#[cfg(not(feature = "native-argon2d"))]
pub const HAS_NATIVE_ARGON2D: bool = false;

#[cfg(feature = "native-blake3")]
pub const HAS_NATIVE_BLAKE3: bool = true;
#[cfg(not(feature = "native-blake3"))]
pub const HAS_NATIVE_BLAKE3: bool = false;

/// Check what native algorithms are available at compile time
pub fn available_native_algorithms() -> Vec<&'static str> {
    let mut algos = Vec::new();
    if HAS_NATIVE_RANDOMX { algos.push("randomx"); }
    if HAS_NATIVE_YESCRYPT { algos.push("yescrypt"); }
    if HAS_NATIVE_COSMIC_HARMONY { algos.push("cosmic_harmony_v2"); }
    if HAS_NATIVE_AUTOLYKOS { algos.push("autolykos"); }
    if HAS_NATIVE_KAWPOW { algos.push("kawpow"); }
    if HAS_NATIVE_KAWPOW_GPU { algos.push("kawpow_gpu"); }
    if HAS_NATIVE_ETHASH { algos.push("ethash"); }
    if HAS_NATIVE_KHEAVYHASH { algos.push("kheavyhash"); }
    if HAS_NATIVE_EQUIHASH { algos.push("equihash"); }
    if HAS_NATIVE_PROGPOW { algos.push("progpow"); }
    if HAS_NATIVE_ARGON2D { algos.push("argon2d"); }
    if HAS_NATIVE_BLAKE3 { algos.push("blake3"); }
    algos
}

// ============================================================================
// ETHASH FFI (only when native-ethash feature enabled) - ETC
// ============================================================================

#[cfg(feature = "native-ethash")]
mod ethash_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn ethash_init();
        pub fn ethash_hash(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            output: *mut u8
        );
        pub fn ethash_verify(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            height: u32,
            target: *const u8
        ) -> i32;
        pub fn ethash_get_epoch(block_number: u32) -> u32;
        pub fn ethash_benchmark(iterations: i32) -> f64;
        pub fn ethash_version() -> *const std::ffi::c_char;
    }
    
    pub fn init() {
        unsafe { ethash_init() }
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe {
            ethash_hash(
                header.as_ptr(),
                header.len(),
                nonce,
                height,
                output.as_mut_ptr()
            );
        }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn verify(header: &[u8], nonce: u64, height: u32, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe {
            ethash_verify(
                header.as_ptr(),
                header.len(),
                nonce,
                height,
                target.as_ptr()
            ) != 0
        }
    }
    
    pub fn get_epoch(block_number: u32) -> u32 {
        unsafe { ethash_get_epoch(block_number) }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { ethash_benchmark(iterations) }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = ethash_version();
            if ptr.is_null() { "Unknown".to_string() }
            else { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }
}

#[cfg(feature = "native-ethash")]
pub use ethash_ffi::{
    init as ethash_init, hash as ethash_hash, verify as ethash_verify,
    get_epoch as ethash_get_epoch, benchmark as ethash_benchmark, version as ethash_version
};

// ============================================================================
// KHEAVYHASH FFI (only when native-kheavyhash feature enabled) - KAS
// ============================================================================

#[cfg(feature = "native-kheavyhash")]
mod kheavyhash_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn kheavyhash_hash(input: *const u8, len: usize, output: *mut u8);
        pub fn kheavyhash_mine(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        pub fn kheavyhash_verify(header: *const u8, header_len: usize, nonce: u64, target: *const u8) -> i32;
        pub fn kheavyhash_benchmark(iterations: i32) -> f64;
        pub fn kheavyhash_version() -> *const std::ffi::c_char;
    }
    
    pub fn hash(input: &[u8]) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe { kheavyhash_hash(input.as_ptr(), input.len(), output.as_mut_ptr()); }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn mine(header: &[u8], nonce: u64) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe { kheavyhash_mine(header.as_ptr(), header.len(), nonce, output.as_mut_ptr()); }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn verify(header: &[u8], nonce: u64, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { kheavyhash_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { kheavyhash_benchmark(iterations) }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = kheavyhash_version();
            if ptr.is_null() { "Unknown".to_string() }
            else { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }
}

#[cfg(feature = "native-kheavyhash")]
pub use kheavyhash_ffi::{
    hash as kheavyhash_hash, mine as kheavyhash_mine, verify as kheavyhash_verify,
    benchmark as kheavyhash_benchmark, version as kheavyhash_version
};

// ============================================================================
// EQUIHASH FFI (only when native-equihash feature enabled) - ZEC
// ============================================================================

#[cfg(feature = "native-equihash")]
mod equihash_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn equihash_solve(header: *const u8, header_len: usize, nonce: u64, solution: *mut u8) -> i32;
        pub fn equihash_verify(header: *const u8, header_len: usize, solution: *const u8) -> i32;
        pub fn equihash_mine(header: *const u8, header_len: usize, start_nonce: u64, target: *const u8, found_nonce: *mut u64, solution: *mut u8) -> i32;
        pub fn equihash_benchmark(iterations: i32) -> f64;
        pub fn equihash_version() -> *const std::ffi::c_char;
    }
    
    pub fn solve(header: &[u8], nonce: u64) -> anyhow::Result<Vec<u8>> {
        let mut solution = vec![0u8; 1344]; // Equihash(200,9) solution size
        let result = unsafe { equihash_solve(header.as_ptr(), header.len(), nonce, solution.as_mut_ptr()) };
        if result == 0 { Ok(solution) }
        else { Err(anyhow::anyhow!("No solution found")) }
    }
    
    pub fn verify(header: &[u8], solution: &[u8]) -> bool {
        unsafe { equihash_verify(header.as_ptr(), header.len(), solution.as_ptr()) != 0 }
    }
    
    pub fn mine(header: &[u8], start_nonce: u64, target: &[u8]) -> anyhow::Result<(u64, Vec<u8>)> {
        let mut found_nonce = 0u64;
        let mut solution = vec![0u8; 1344];
        let result = unsafe {
            equihash_mine(
                header.as_ptr(), header.len(), start_nonce,
                target.as_ptr(), &mut found_nonce, solution.as_mut_ptr()
            )
        };
        if result == 0 { Ok((found_nonce, solution)) }
        else { Err(anyhow::anyhow!("Mining failed")) }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { equihash_benchmark(iterations) }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = equihash_version();
            if ptr.is_null() { "Unknown".to_string() }
            else { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }
}

#[cfg(feature = "native-equihash")]
pub use equihash_ffi::{
    solve as equihash_solve, verify as equihash_verify, mine as equihash_mine,
    benchmark as equihash_benchmark, version as equihash_version
};

// ============================================================================
// PROGPOW FFI (only when native-progpow feature enabled) - VEIL
// ============================================================================

#[cfg(feature = "native-progpow")]
mod progpow_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn progpow_hash(header: *const u8, header_len: usize, nonce: u64, height: u32, output: *mut u8);
        pub fn progpow_verify(header: *const u8, header_len: usize, nonce: u64, height: u32, target: *const u8) -> i32;
        pub fn progpow_benchmark(iterations: i32) -> f64;
        pub fn progpow_version() -> *const std::ffi::c_char;
    }
    
    pub fn hash(header: &[u8], nonce: u64, height: u32) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe { progpow_hash(header.as_ptr(), header.len(), nonce, height, output.as_mut_ptr()); }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn verify(header: &[u8], nonce: u64, height: u32, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { progpow_verify(header.as_ptr(), header.len(), nonce, height, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { progpow_benchmark(iterations) }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = progpow_version();
            if ptr.is_null() { "Unknown".to_string() }
            else { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }
}

#[cfg(feature = "native-progpow")]
pub use progpow_ffi::{
    hash as progpow_hash, verify as progpow_verify,
    benchmark as progpow_benchmark, version as progpow_version
};

// ============================================================================
// ARGON2D FFI (only when native-argon2d feature enabled) - DYN
// ============================================================================

#[cfg(feature = "native-argon2d")]
mod argon2d_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn argon2d_hash(
            password: *const u8, pwdlen: usize,
            salt: *const u8, saltlen: usize,
            t_cost: u32, m_cost: u32, parallelism: u32,
            output: *mut u8, outlen: usize
        ) -> i32;
        pub fn argon2d_mine(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        pub fn argon2d_verify(header: *const u8, header_len: usize, nonce: u64, target: *const u8) -> i32;
        pub fn argon2d_benchmark(iterations: i32) -> f64;
        pub fn argon2d_version() -> *const std::ffi::c_char;
    }
    
    pub fn hash(password: &[u8], salt: &[u8], t_cost: u32, m_cost: u32, parallelism: u32, outlen: usize) -> anyhow::Result<Vec<u8>> {
        let mut output = vec![0u8; outlen];
        let result = unsafe {
            argon2d_hash(
                password.as_ptr(), password.len(),
                salt.as_ptr(), salt.len(),
                t_cost, m_cost, parallelism,
                output.as_mut_ptr(), outlen
            )
        };
        if result == 0 { Ok(output) }
        else { Err(anyhow::anyhow!("Argon2d hash failed with code {}", result)) }
    }
    
    pub fn mine(header: &[u8], nonce: u64) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe { argon2d_mine(header.as_ptr(), header.len(), nonce, output.as_mut_ptr()); }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn verify(header: &[u8], nonce: u64, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { argon2d_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { argon2d_benchmark(iterations) }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = argon2d_version();
            if ptr.is_null() { "Unknown".to_string() }
            else { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }
}

#[cfg(feature = "native-argon2d")]
pub use argon2d_ffi::{
    hash as argon2d_hash, mine as argon2d_mine, verify as argon2d_verify,
    benchmark as argon2d_benchmark, version as argon2d_version
};

// ============================================================================
// BLAKE3 FFI (only when native-blake3 feature enabled) - ALPH
// ============================================================================

#[cfg(feature = "native-blake3")]
mod blake3_ffi {
    use crate::algorithms::HashOutput;
    
    extern "C" {
        pub fn blake3_hash(input: *const u8, len: usize, output: *mut u8);
        pub fn blake3_mine(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        pub fn blake3_alph(header: *const u8, header_len: usize, nonce: u64, output: *mut u8);
        pub fn blake3_verify(header: *const u8, header_len: usize, nonce: u64, target: *const u8) -> i32;
        pub fn blake3_benchmark(iterations: i32) -> f64;
        pub fn blake3_version() -> *const std::ffi::c_char;
    }
    
    pub fn hash(input: &[u8]) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe { blake3_hash(input.as_ptr(), input.len(), output.as_mut_ptr()); }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn mine(header: &[u8], nonce: u64) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe { blake3_mine(header.as_ptr(), header.len(), nonce, output.as_mut_ptr()); }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    /// Alephium-style double Blake3
    pub fn alph(header: &[u8], nonce: u64) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        unsafe { blake3_alph(header.as_ptr(), header.len(), nonce, output.as_mut_ptr()); }
        Ok(HashOutput { hash: output.to_vec() })
    }
    
    pub fn verify(header: &[u8], nonce: u64, target: &[u8]) -> bool {
        if target.len() != 32 { return false; }
        unsafe { blake3_verify(header.as_ptr(), header.len(), nonce, target.as_ptr()) != 0 }
    }
    
    pub fn benchmark(iterations: i32) -> f64 {
        unsafe { blake3_benchmark(iterations) }
    }
    
    pub fn version() -> String {
        unsafe {
            let ptr = blake3_version();
            if ptr.is_null() { "Unknown".to_string() }
            else { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }
}

#[cfg(feature = "native-blake3")]
pub use blake3_ffi::{
    hash as native_blake3_hash, mine as blake3_mine, alph as blake3_alph,
    verify as blake3_verify, benchmark as blake3_benchmark, version as blake3_version
};

// ============================================================================
// COSMIC HARMONY V3 FFI (native C library - libcosmic_harmony_v3.dylib/.so)
// Full CHv3 pipeline: Keccak-256 → SHA3-512 → Golden Matrix → Cosmic Fusion
// ============================================================================

#[cfg(feature = "native-cosmic-harmony-v3")]
mod cosmic_harmony_v3_ffi {
    use crate::algorithms::HashOutput;

    extern "C" {
        /// Compute CHv3 hash: header + nonce → 32-byte hash
        pub fn cosmic_harmony_v3_hash(
            header: *const u8,
            header_len: usize,
            nonce: u64,
            output: *mut u8
        ) -> i32;

        /// Compute CHv3 hash from raw input (no nonce appended)
        pub fn cosmic_harmony_v3_hash_raw(
            input: *const u8,
            input_len: usize,
            output: *mut u8
        ) -> i32;

        /// GPU device count
        pub fn cosmic_harmony_v3_gpu_count() -> u32;

        /// Initialize GPU mining context
        pub fn cosmic_harmony_v3_gpu_init(device_id: u32, batch_size: u32) -> i32;

        /// Mine a batch of nonces (CPU fallback or Metal)
        pub fn cosmic_harmony_v3_gpu_mine(
            header: *const u8,
            header_len: usize,
            nonce_start: u64,
            target: *const u8,
            found_nonce: *mut u64,
            found_hash: *mut u8
        ) -> i32;

        /// Cleanup GPU resources
        pub fn cosmic_harmony_v3_gpu_cleanup();

        /// Individual step: Keccak-256
        pub fn cosmic_harmony_v3_keccak256(input: *const u8, len: usize, output: *mut u8);

        /// Individual step: SHA3-512
        pub fn cosmic_harmony_v3_sha3_512(input: *const u8, len: usize, output: *mut u8);

        /// Individual step: Golden Matrix
        pub fn cosmic_harmony_v3_golden_matrix(input: *const u8, output: *mut u8);

        /// Individual step: Cosmic Fusion
        pub fn cosmic_harmony_v3_cosmic_fusion(input: *const u8, output: *mut u8);

        /// Library info string
        pub fn cosmic_harmony_v3_get_info() -> *const std::ffi::c_char;

        /// NEON support check
        pub fn cosmic_harmony_v3_has_neon() -> i32;

        /// AVX2 support check
        pub fn cosmic_harmony_v3_has_avx2() -> i32;

        /// Benchmark (returns H/s)
        pub fn cosmic_harmony_v3_benchmark(duration_seconds: i32) -> f64;
    }

    /// Hash with header + nonce
    pub fn hash(header: &[u8], nonce: u64) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        let ret = unsafe {
            cosmic_harmony_v3_hash(header.as_ptr(), header.len(), nonce, output.as_mut_ptr())
        };
        if ret != 0 {
            return Err(anyhow::anyhow!("CHv3 hash failed with code {}", ret));
        }
        Ok(HashOutput { hash: output.to_vec() })
    }

    /// Hash raw input (no nonce)
    pub fn hash_raw(input: &[u8]) -> anyhow::Result<HashOutput> {
        let mut output = [0u8; 32];
        let ret = unsafe {
            cosmic_harmony_v3_hash_raw(input.as_ptr(), input.len(), output.as_mut_ptr())
        };
        if ret != 0 {
            return Err(anyhow::anyhow!("CHv3 raw hash failed with code {}", ret));
        }
        Ok(HashOutput { hash: output.to_vec() })
    }

    /// GPU mine a batch
    pub fn gpu_mine(header: &[u8], nonce_start: u64, target: &[u8]) -> anyhow::Result<Option<(u64, Vec<u8>)>> {
        if target.len() != 32 {
            return Err(anyhow::anyhow!("Target must be 32 bytes"));
        }
        let mut found_nonce = 0u64;
        let mut found_hash = [0u8; 32];
        let ret = unsafe {
            cosmic_harmony_v3_gpu_mine(
                header.as_ptr(), header.len(),
                nonce_start, target.as_ptr(),
                &mut found_nonce, found_hash.as_mut_ptr()
            )
        };
        match ret {
            1 => Ok(Some((found_nonce, found_hash.to_vec()))),
            0 => Ok(None),
            _ => Err(anyhow::anyhow!("GPU mine failed with code {}", ret)),
        }
    }

    /// GPU init
    pub fn gpu_init(device_id: u32, batch_size: u32) -> anyhow::Result<()> {
        let ret = unsafe { cosmic_harmony_v3_gpu_init(device_id, batch_size) };
        if ret != 0 {
            return Err(anyhow::anyhow!("GPU init failed with code {}", ret));
        }
        Ok(())
    }

    /// GPU device count
    pub fn gpu_count() -> u32 {
        unsafe { cosmic_harmony_v3_gpu_count() }
    }

    /// GPU cleanup
    pub fn gpu_cleanup() {
        unsafe { cosmic_harmony_v3_gpu_cleanup(); }
    }

    /// Library info
    pub fn info() -> String {
        unsafe {
            let ptr = cosmic_harmony_v3_get_info();
            if ptr.is_null() { "Unknown".to_string() }
            else { std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned() }
        }
    }

    /// Benchmark (returns H/s)
    pub fn benchmark(duration: i32) -> f64 {
        unsafe { cosmic_harmony_v3_benchmark(duration) }
    }
}

#[cfg(feature = "native-cosmic-harmony-v3")]
pub use cosmic_harmony_v3_ffi::{
    hash as chv3_hash, hash_raw as chv3_hash_raw,
    gpu_mine as chv3_gpu_mine, gpu_init as chv3_gpu_init,
    gpu_count as chv3_gpu_count, gpu_cleanup as chv3_gpu_cleanup,
    info as chv3_info, benchmark as chv3_benchmark
};
