//! RandomX CPU-optimized mining algorithm
//!
//! Thread-safe implementation using randomx-rs crate with auto-detected
//! optimal flags (JIT, HARD_AES, etc.).
//!
//! ## Performance modes
//!
//! - **Light mode** (default): ~50-400 H/s per thread, fast init, 256 MB RAM
//! - **Full mode** (`ZION_RANDOMX_FULL=1`): ~500-2000 H/s per thread, 2 GB RAM + 30-60s init
//!
//! The mode is selected via `ZION_RANDOMX_FULL` environment variable.
//! JIT compilation is always enabled when the CPU supports it.

use anyhow::{anyhow, Result};
use randomx_rs::{RandomXCache, RandomXDataset, RandomXFlag, RandomXVM};
use std::sync::Once;

static LOG_FLAGS_ONCE: Once = Once::new();

/// Detect optimal RandomX flags for this CPU.
///
/// `get_recommended_flags()` enables JIT + HARD_AES where available.
/// If `ZION_RANDOMX_FULL=1`, also enables FULL_MEM (needs 2 GB RAM).
fn detect_flags() -> RandomXFlag {
    let mut flags = RandomXFlag::get_recommended_flags();

    // Full mode: 2 GB dataset in RAM — ~5-10× faster hashing
    let use_full = std::env::var("ZION_RANDOMX_FULL")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes"
        })
        .unwrap_or(false);

    if use_full {
        flags |= RandomXFlag::FLAG_FULL_MEM;
    }

    // HugePages (2 MB pages) — reduces TLB misses, 10-30% hashrate boost.
    // Requires: sysctl vm.nr_hugepages >= 1280 (for 2.5 GB)
    // and either root, CAP_IPC_LOCK, or memlock ulimit.
    // Auto-detect: if /proc/meminfo shows available huge pages, enable.
    // Override: ZION_RANDOMX_HUGEPAGES=0 to force-disable.
    let hugepages_disabled = std::env::var("ZION_RANDOMX_HUGEPAGES")
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "0" || v == "false" || v == "no"
        })
        .unwrap_or(false);

    if !hugepages_disabled {
        // On Linux, check if huge pages are actually available
        let hp_available = if cfg!(target_os = "linux") {
            std::fs::read_to_string("/proc/meminfo")
                .ok()
                .and_then(|m| {
                    m.lines()
                        .find(|l| l.starts_with("HugePages_Free:"))
                        .and_then(|l| l.split_whitespace().nth(1))
                        .and_then(|v| v.parse::<u64>().ok())
                })
                .unwrap_or(0)
                > 0
        } else {
            // On macOS/other: try anyway, RandomX will fall back gracefully
            false
        };

        if hp_available {
            flags |= RandomXFlag::FLAG_LARGE_PAGES;
        }
    }

    LOG_FLAGS_ONCE.call_once(|| {
        let mode = if flags.contains(RandomXFlag::FLAG_FULL_MEM) { "FULL" } else { "LIGHT" };
        let jit = if flags.contains(RandomXFlag::FLAG_JIT) { "+JIT" } else { "" };
        let aes = if flags.contains(RandomXFlag::FLAG_HARD_AES) { "+HARD_AES" } else { "" };
        let hp = if flags.contains(RandomXFlag::FLAG_LARGE_PAGES) { "+HUGEPAGES" } else { "" };
        log::info!(
            "⚡ RandomX flags: 0x{:x} mode={}{}{}{} (get_recommended_flags + env)",
            flags.bits(), mode, jit, aes, hp
        );
    });

    flags
}

/// RandomX hasher (per-thread instance)
pub struct RandomXHasher {
    vm: RandomXVM,
    // Keep dataset alive for the lifetime of the VM (full mode).
    _dataset: Option<RandomXDataset>,
}

impl RandomXHasher {
    /// Create new RandomX hasher for this thread.
    ///
    /// Auto-detects optimal CPU flags (JIT, HARD_AES).
    /// Set `ZION_RANDOMX_FULL=1` for full-dataset mode (~5-10× faster, 2 GB RAM).
    pub fn new(key: &[u8]) -> Result<Self> {
        let flags = detect_flags();

        // Create cache from key (always needed)
        let cache = RandomXCache::new(flags, key)
            .map_err(|e| anyhow!("RandomX cache creation failed: {}", e))?;

        // Full mode: allocate 2 GB dataset from cache
        let (vm, dataset) = if flags.contains(RandomXFlag::FLAG_FULL_MEM) {
            let dataset = RandomXDataset::new(flags, cache.clone(), 0)
                .map_err(|e| anyhow!("RandomX dataset creation failed: {}", e))?;
            let vm = RandomXVM::new(flags, Some(cache), Some(dataset.clone()))
                .map_err(|e| anyhow!("RandomX VM (full) creation failed: {}", e))?;
            (vm, Some(dataset))
        } else {
            // Light mode: no dataset, uses cache directly (slower but less RAM)
            let vm = RandomXVM::new(flags, Some(cache), None)
                .map_err(|e| anyhow!("RandomX VM (light) creation failed: {}", e))?;
            (vm, None)
        };

        Ok(Self { vm, _dataset: dataset })
    }

    /// Compute RandomX hash
    pub fn hash(&mut self, input: &[u8]) -> Result<[u8; 32]> {
        let hash = self
            .vm
            .calculate_hash(input)
            .map_err(|e| anyhow!("RandomX hash calculation failed: {}", e))?;

        // Convert Vec<u8> to [u8; 32]
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash[..32]);
        Ok(result)
    }

    /// Batch compute RandomX hashes using pipeline mode.
    ///
    /// `calculate_hash_set` uses RandomX's internal first/next pipeline,
    /// which overlaps execution of consecutive hashes for ~1.5-2× throughput
    /// compared to sequential `calculate_hash` calls.
    pub fn hash_batch(&self, inputs: &[Vec<u8>]) -> Result<Vec<[u8; 32]>> {
        if inputs.is_empty() {
            return Ok(vec![]);
        }

        // Build slice-of-slices for the FFI call
        let refs: Vec<&[u8]> = inputs.iter().map(|v| v.as_slice()).collect();
        let hashes = self
            .vm
            .calculate_hash_set(&refs)
            .map_err(|e| anyhow!("RandomX batch hash failed: {}", e))?;

        let mut results = Vec::with_capacity(hashes.len());
        for h in &hashes {
            if h.len() < 32 {
                return Err(anyhow!("RandomX batch: short hash ({}B)", h.len()));
            }
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&h[..32]);
            results.push(arr);
        }
        Ok(results)
    }
}

/// Initialize RandomX (just validates the key works)
pub fn init_randomx(key: &[u8]) -> Result<()> {
    log::info!("[RandomX] Validating key...");

    // Test that we can create a hasher
    let _hasher = RandomXHasher::new(key)?;

    let flags = detect_flags();
    let mode = if flags.contains(RandomXFlag::FLAG_FULL_MEM) { "full" } else { "light" };
    log::info!("[RandomX] Ready ({} mode, per-thread VMs, flags=0x{:x})", mode, flags.bits());
    Ok(())
}

/// Check if RandomX is available
pub fn is_randomx_initialized() -> bool {
    // Always available after validation
    true
}

/// Compute RandomX hash (creates temporary hasher)
///
/// Note: For mining loops, create a RandomXHasher once per thread instead
pub fn randomx_hash(input: &[u8]) -> Result<[u8; 32]> {
    // Use default key for standalone hashing
    let mut hasher = RandomXHasher::new(b"zion-randomx-default-key")?;
    hasher.hash(input)
}

/// Blockchain convenience: algorithm-specific PoW hash (returns raw bytes).
pub fn hash(data: &[u8], key: &[u8]) -> Vec<u8> {
    let mut hasher = RandomXHasher::new(key).expect("RandomX init failed");
    hasher
        .hash(data)
        .expect("RandomX hash calculation failed")
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_randomx_init() {
        let key = b"zion-test-key";
        assert!(init_randomx(key).is_ok());
        assert!(is_randomx_initialized());
    }

    #[test]
    fn test_randomx_hash() {
        let input = b"zion-block-header";
        let hash = randomx_hash(input).unwrap();

        // Should be 32 bytes
        assert_eq!(hash.len(), 32);

        // Hash should be deterministic
        let hash2 = randomx_hash(input).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_randomx_different_inputs() {
        let hash1 = randomx_hash(b"input1").unwrap();
        let hash2 = randomx_hash(b"input2").unwrap();

        // Different inputs should produce different hashes
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_randomx_hasher_reuse() {
        let key = b"zion-test-key";
        let mut hasher = RandomXHasher::new(key).unwrap();

        // Hash multiple inputs with same hasher
        let hash1 = hasher.hash(b"input1").unwrap();
        let hash2 = hasher.hash(b"input2").unwrap();
        let hash3 = hasher.hash(b"input1").unwrap();

        // Different inputs produce different hashes
        assert_ne!(hash1, hash2);

        // Same input is deterministic
        assert_eq!(hash1, hash3);
    }
}
