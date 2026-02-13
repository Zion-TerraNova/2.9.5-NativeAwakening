/// Share Validator - Algorithm-specific validation
///
/// Validates mining shares using native Rust cryptography
/// Supports: RandomX, Yescrypt, Cosmic Harmony, Autolykos v2
///
/// Implementation mirrors /src/pool/mining/share_validator.py exactly
/// for cross-compatibility with Python pool reference implementation.

use hex::FromHex;
use zion_core::algorithms::{blake3, cosmic_harmony, cosmic_harmony_v2, randomx, yescrypt};
use zion_cosmic_harmony_v3::algorithms_opt;
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::Arc;
use chrono::Utc;

/// Share validation result
#[derive(Debug, Clone)]
pub struct ShareResult {
    pub valid: bool,
    pub reason: String,
    pub hash_value: Option<String>,
    pub meets_target: bool,
    pub is_block: bool,
    pub difficulty: u64,
}

/// Algorithm types (matching Python)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Algorithm {
    RandomX,
    Yescrypt,
    CosmicHarmony,
    CosmicHarmonyV2,
    CosmicHarmonyV3,
    Blake3,
    AutolykovV2,
    Unknown,
}

impl Algorithm {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "randomx" | "rx/0" => Algorithm::RandomX,
            "yescrypt" => Algorithm::Yescrypt,
            // IMPORTANT: cosmic_harmony now defaults to V3 (the current production algorithm)
            "cosmic_harmony" | "cosmic" | "cosmic_harmony_v3" | "cosmic_v3" | "cosmic3" => Algorithm::CosmicHarmonyV3,
            "cosmic_harmony_v1" => Algorithm::CosmicHarmony, // Legacy v1 only if explicitly requested
            "cosmic_harmony_v2" | "cosmicharmonyv2" | "cosmic-harmony-v2" => Algorithm::CosmicHarmonyV2,
            "blake3" => Algorithm::Blake3,
            "autolykos" | "autolykos_v2" => Algorithm::AutolykovV2,
            // External algorithms — ethash/kawpow shares should normally be routed
            // to the external pool by StreamScheduler and never reach the validator.
            // Map them to Unknown as a safety net so logs show the actual algo name.
            "ethash" | "etchash" | "kawpow" => Algorithm::Unknown,
            _ => Algorithm::Unknown,
        }
    }
}

/// Submitted share from miner
#[derive(Debug, Clone)]
pub struct SubmittedShare {
    pub job_id: String,
    pub nonce: String,       // hex
    pub result: Option<String>, // hex (may not be provided)
    pub algorithm: String,
    pub job_blob: String,    // hex
    pub job_target: String,  // hex
    pub block_target: Option<String>, // hex
    pub height: Option<u64>,
}

/// Share cache entry (for duplicate detection)
struct CacheEntry {
    timestamp: i64,
}

/// Max age of share cache entries (10 minutes)
const SHARE_CACHE_MAX_AGE_SECS: i64 = 600;

/// Main share validator
pub struct ShareValidator {
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    cosmic_state0_endian: &'static str, // "big" or "little"
}

impl ShareValidator {
    pub fn new(cosmic_state0_endian: &'static str) -> Self {
        let validator = Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            cosmic_state0_endian,
        };
        // P1-18: Spawn periodic cache pruning task (only if tokio runtime is available)
        let cache_clone = validator.cache.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let now = Utc::now().timestamp();
                    let mut cache = cache_clone.write().await;
                    let before = cache.len();
                    cache.retain(|_, entry| now - entry.timestamp < SHARE_CACHE_MAX_AGE_SECS);
                    let pruned = before - cache.len();
                    if pruned > 0 {
                        tracing::debug!("Share cache pruned {} stale entries ({} remaining)", pruned, cache.len());
                    }
                }
            });
        }
        validator
    }

    /// Main validation entry point
    ///
    /// `miner_id` is included in the duplicate-detection cache key so that
    /// different miners (or the same miner after reconnect) sharing the same
    /// job_id + nonce are NOT falsely flagged as duplicates.  The pool does
    /// not embed a per-session extranonce into the blob for XMRig-protocol
    /// miners, so nonce-space overlap between miners is expected.
    pub async fn validate_share(&self, share: &SubmittedShare, miner_id: &str) -> ShareResult {
        // Check for duplicate — key includes miner identity to avoid
        // cross-miner collisions when nonce spaces overlap.
        let cache_key = format!("{}:{}:{}", share.job_id, share.nonce, miner_id);
        {
            let cache = self.cache.read().await;
            if cache.contains_key(&cache_key) {
                return ShareResult {
                    valid: false,
                    reason: "Duplicate share".to_string(),
                    hash_value: None,
                    meets_target: false,
                    is_block: false,
                    difficulty: 0,
                };
            }
        }

        // Parse nonce
        let _nonce_int = match u32::from_str_radix(&share.nonce, 16) {
            Ok(n) => n,
            Err(_) => {
                return ShareResult {
                    valid: false,
                    reason: "Invalid nonce format".to_string(),
                    hash_value: None,
                    meets_target: false,
                    is_block: false,
                    difficulty: 0,
                }
            }
        };

        // Determine algorithm
        let algo = Algorithm::from_str(&share.algorithm);

        // ALWAYS compute hash ourselves (don't trust miner result)
        let hash_hex = match Self::compute_hash(algo, share) {
            Some(hash) => hash,
            None => {
                return ShareResult {
                    valid: false,
                    reason: format!("Hash computation not supported for {:?}", algo),
                    hash_value: None,
                    meets_target: false,
                    is_block: false,
                    difficulty: 0,
                };
            }
        };
        // Miner-provided `result` is optional and not trusted.
        // We ALWAYS compute the hash ourselves; do not reject shares solely because
        // the miner-supplied hash differs (protocol / client variance).

        // Convert hash to bytes
        let hash_bytes = match Vec::from_hex(&hash_hex) {
            Ok(b) => b,
            Err(_) => {
                return ShareResult {
                    valid: false,
                    reason: "Invalid hash hex format".to_string(),
                    hash_value: Some(hash_hex),
                    meets_target: false,
                    is_block: false,
                    difficulty: 0,
                }
            }
        };

        // Check target - algorithm-specific
        let (meets_target, _job_difficulty) =
            self.check_target(&hash_bytes, algo, &share.job_target);

        if !meets_target {
            return ShareResult {
                valid: false,
                reason: "Does not meet target difficulty".to_string(),
                hash_value: Some(hash_hex),
                meets_target: false,
                is_block: false,
                difficulty: 0,
            };
        }

        // Check if it's a block
        let is_block = if let Some(block_target) = &share.block_target {
            let result = self.check_block_target(&hash_bytes, algo, block_target);
            tracing::info!(
                "🔍 Block check: algo={:?} hash={} block_target={} is_block={}",
                algo,
                hash_hex,
                block_target,
                result
            );
            result
        } else {
            tracing::warn!("⚠️ No block_target set for share - cannot detect blocks!");
            false
        };

        // Calculate difficulty achieved
        let difficulty = self.calculate_difficulty(&hash_bytes, algo);

        // Add to cache
        {
            let mut cache = self.cache.write().await;
            cache.insert(
                cache_key,
                CacheEntry {
                    timestamp: Utc::now().timestamp(),
                },
            );
        }

        ShareResult {
            valid: true,
            reason: "Valid share".to_string(),
            hash_value: Some(hash_hex),
            meets_target: true,
            is_block,
            difficulty,
        }
    }

    fn compute_hash(algo: Algorithm, share: &SubmittedShare) -> Option<String> {
        let blob = share.job_blob.trim_start_matches("0x");
        let nonce = u32::from_str_radix(&share.nonce, 16).ok()?;
        let full_blob = Vec::from_hex(blob).ok()?;
        let height = share.height.unwrap_or(0);
        
        // CRITICAL: Core calculate_hash uses:
        // version(4) + height(8) + prev_hash(64) + merkle_root(64) + timestamp(8) + difficulty(8) + nonce(8) = 156 bytes data, then hashes
        // But template blob is:
        // version(4) + height(8) + prev_hash(64) + merkle_root(64) + timestamp(8) + difficulty(8) + algo(1) + nonce_placeholder(8) = 165 bytes
        // 
        // We need to take first 156 bytes (without algo byte and nonce_placeholder) and add actual nonce
        // Offsets: 0-3 version, 4-11 height, 12-75 prev_hash, 76-139 merkle_root, 140-147 timestamp, 148-155 difficulty
        // For some algorithms (CH v1, RandomX, Yescrypt, Blake3) we mirror core's
        // calculate_hash(): header fields (156 bytes) + nonce (8 bytes).
        let header_len = 156; // 4+8+64+64+8+8
        let mut data: Vec<u8> = Vec::new();
        if matches!(
            algo,
            Algorithm::CosmicHarmony | Algorithm::RandomX | Algorithm::Yescrypt | Algorithm::Blake3
        ) {
            if full_blob.len() < header_len {
                tracing::warn!(
                    "Blob too short for {:?}: {} bytes, need at least {}",
                    algo,
                    full_blob.len(),
                    header_len
                );
                return None;
            }

            data = full_blob[..header_len].to_vec();
            data.extend_from_slice(&(nonce as u64).to_le_bytes());

            tracing::debug!(
                "POOL compute_hash: algo={:?} data_len={} nonce={} height={}",
                algo,
                data.len(),
                nonce,
                height
            );
        }

        match algo {
            Algorithm::CosmicHarmony => {
                // CRITICAL: Must match core's hash function which XORs nonce with height
                // cosmic_harmony::hash(data, nonce, height) does: nonce32 = (nonce as u32) ^ (height as u32)
                let hash = cosmic_harmony::hash(&data, nonce as u64, height);
                Some(hex::encode(hash))
            }
            Algorithm::CosmicHarmonyV2 => {
                // CH v2 is memory-hard and uses dynamic params derived from prev_hash + height.
                // Mirror universal miner behavior: use the first 32 bytes of the template blob as prev_hash seed.
                let prev_hash: [u8; 32] = if full_blob.len() >= 32 {
                    let mut tmp = [0u8; 32];
                    tmp.copy_from_slice(&full_blob[..32]);
                    tmp
                } else {
                    blake3::hash(&full_blob)
                };
                let mut hasher = cosmic_harmony_v2::CosmicHarmonyV2::new(&prev_hash, height);
                let hash = hasher.hash(&full_blob, nonce as u64);
                Some(hex::encode(hash))
            }
            Algorithm::CosmicHarmonyV3 => {
                // CH v3: canonical pipeline (80-byte header + 8-byte nonce), no height XOR.
                // Pass the raw template blob bytes; the hasher will only use the first 80 bytes.
                if full_blob.len() < 80 {
                    tracing::warn!("Blob too short for CHv3: {} bytes, need at least 80", full_blob.len());
                    return None;
                }
                let h = algorithms_opt::cosmic_harmony_v3(&full_blob, nonce as u64);
                Some(hex::encode(h.data))
            }
            Algorithm::RandomX => {
                let height = share.height.unwrap_or(0);
                let mut input = data.clone();
                input.extend_from_slice(&nonce.to_le_bytes());
                let hash = randomx::hash(&input, &height.to_le_bytes());
                Some(hex::encode(hash))
            }
            Algorithm::Yescrypt => {
                let hash = yescrypt::yescrypt_hash_mining(&data, nonce as u64).ok()?;
                Some(hex::encode(hash))
            }
            Algorithm::Blake3 => {
                let hash = blake3::hash_with_nonce(&data, nonce);
                Some(hex::encode(hash))
            }
            _ => None,
        }
    }

    /// Check if hash meets job target (algorithm-specific)
    fn check_target(&self, hash_bytes: &[u8], algo: Algorithm, job_target: &str) -> (bool, u64) {
        match algo {
            Algorithm::RandomX => {
                // RandomX: compare first 8 bytes (low 64 bits, little-endian) to 64-bit target
                if hash_bytes.len() < 8 {
                    return (false, 0);
                }
                let mut low64_bytes = [0u8; 8];
                low64_bytes.copy_from_slice(&hash_bytes[0..8]);
                let res_low64 = u64::from_le_bytes(low64_bytes);

                let target_int = u64::from_str_radix(job_target, 16).unwrap_or(0);
                let meets = res_low64 <= target_int;
                let difficulty = if res_low64 > 0 {
                    (u64::MAX / res_low64) as u64
                } else {
                    0
                };
                (meets, difficulty)
            }
            Algorithm::Yescrypt => {
                // Yescrypt: compare first 28 bytes (big-endian) to 224-bit target
                if hash_bytes.len() < 28 {
                    return (false, 0);
                }
                let hash_int = u256_from_be_slice(&hash_bytes[0..28]);
                let meets = meets_target_be(hash_bytes, job_target, 28);
                let difficulty = if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                };
                (meets, difficulty)
            }
            Algorithm::CosmicHarmony => {
                // Cosmic Harmony: compare first 4 bytes (32-bit, endian-configurable)
                if hash_bytes.len() < 4 {
                    return (false, 0);
                }
                let state0 = match self.cosmic_state0_endian {
                    "little" => u32::from_le_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                    _ => u32::from_be_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                };

                let target_int = if job_target.len() <= 8 {
                    u32::from_str_radix(job_target, 16).unwrap_or(0)
                } else {
                    u32::from_str_radix(&job_target[0..8], 16).unwrap_or(0)
                };

                let meets = state0 <= target_int;
                let difficulty = if state0 > 0 {
                    u32::MAX as u64 / state0 as u64
                } else {
                    0
                };
                
                // Target comparison logging (DEBUG level for production)
                tracing::debug!("POOL check_target: state0={} target={} meets={}",
                    state0, target_int, meets);
                
                (meets, difficulty)
            }
            Algorithm::CosmicHarmonyV2 => {
                // CH v2: use full 256-bit big-endian comparison (same model as blake3).
                if hash_bytes.len() < 32 {
                    return (false, 0);
                }
                let hash_int = u256_from_be_slice(&hash_bytes[0..32]);
                let meets = meets_target_be(hash_bytes, job_target, 32);
                let difficulty = if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                };
                (meets, difficulty)
            }
            Algorithm::CosmicHarmonyV3 => {
                // CH v3 uses the same share target model as Cosmic Harmony (state0 vs u32 target).
                if hash_bytes.len() < 4 {
                    return (false, 0);
                }
                let state0 = match self.cosmic_state0_endian {
                    "little" => u32::from_le_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                    _ => u32::from_be_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                };

                let target_int = if job_target.len() <= 8 {
                    u32::from_str_radix(job_target, 16).unwrap_or(0)
                } else {
                    u32::from_str_radix(&job_target[0..8], 16).unwrap_or(0)
                };

                let meets = state0 <= target_int;
                let difficulty = if state0 > 0 {
                    u32::MAX as u64 / state0 as u64
                } else {
                    0
                };

                tracing::debug!("POOL check_target(CHv3): state0={} target={} meets={}", state0, target_int, meets);

                (meets, difficulty)
            }
            Algorithm::Blake3 => {
                // Blake3: full 256-bit big-endian comparison
                if hash_bytes.len() < 32 {
                    return (false, 0);
                }
                let hash_int = u256_from_be_slice(&hash_bytes[0..32]);
                let meets = meets_target_be(hash_bytes, job_target, 32);
                let difficulty = if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                };
                (meets, difficulty)
            }
            Algorithm::AutolykovV2 => {
                // Autolykos: full 256-bit big-endian comparison
                let hash_int = u256_from_be_slice(hash_bytes);
                let meets = meets_target_be(hash_bytes, job_target, 32);
                let difficulty = if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                };
                (meets, difficulty)
            }
            Algorithm::Unknown => (false, 0),
        }
    }

    /// Check if hash qualifies as a block (network difficulty)
    fn check_block_target(&self, hash_bytes: &[u8], algo: Algorithm, block_target: &str) -> bool {
        match algo {
            Algorithm::RandomX => {
                if hash_bytes.len() < 8 {
                    return false;
                }
                let mut low64_bytes = [0u8; 8];
                low64_bytes.copy_from_slice(&hash_bytes[0..8]);
                let res_low64 = u64::from_le_bytes(low64_bytes);
                let target_int = u64::from_str_radix(block_target, 16).unwrap_or(0);
                res_low64 <= target_int
            }
            Algorithm::Yescrypt => {
                if hash_bytes.len() < 28 {
                    return false;
                }
                meets_target_be(hash_bytes, block_target, 28)
            }
            Algorithm::CosmicHarmony => {
                if hash_bytes.len() < 4 {
                    return false;
                }
                let state0 = match self.cosmic_state0_endian {
                    "little" => u32::from_le_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                    _ => u32::from_be_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                };
                let target_int = if block_target.len() > 8 {
                    u32::from_str_radix(&block_target[0..8], 16).unwrap_or(0)
                } else {
                    u32::from_str_radix(block_target, 16).unwrap_or(0)
                };
                state0 <= target_int
            }
            Algorithm::CosmicHarmonyV2 => {
                if hash_bytes.len() < 32 {
                    return false;
                }
                meets_target_be(hash_bytes, block_target, 32)
            }
            Algorithm::CosmicHarmonyV3 => {
                // Same block-target model as Cosmic Harmony (state0 vs u32).
                if hash_bytes.len() < 4 {
                    return false;
                }
                let state0 = match self.cosmic_state0_endian {
                    "little" => u32::from_le_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                    _ => u32::from_be_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                };
                let target_int = if block_target.len() > 8 {
                    u32::from_str_radix(&block_target[0..8], 16).unwrap_or(0)
                } else {
                    u32::from_str_radix(block_target, 16).unwrap_or(0)
                };
                state0 <= target_int
            }
            Algorithm::Blake3 => {
                if hash_bytes.len() < 32 {
                    return false;
                }
                meets_target_be(hash_bytes, block_target, 32)
            }
            Algorithm::AutolykovV2 => {
                meets_target_be(hash_bytes, block_target, 32)
            }
            Algorithm::Unknown => false,
        }
    }

    /// Calculate difficulty achieved (algorithm-specific)
    fn calculate_difficulty(&self, hash_bytes: &[u8], algo: Algorithm) -> u64 {
        match algo {
            Algorithm::RandomX => {
                if hash_bytes.len() < 8 {
                    return 0;
                }
                let mut low64_bytes = [0u8; 8];
                low64_bytes.copy_from_slice(&hash_bytes[0..8]);
                let res_low64 = u64::from_le_bytes(low64_bytes);
                if res_low64 > 0 {
                    u64::MAX / res_low64
                } else {
                    0
                }
            }
            Algorithm::Yescrypt => {
                if hash_bytes.len() < 28 {
                    return 0;
                }
                let hash_int = u256_from_be_slice(&hash_bytes[0..28]);
                if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                }
            }
            Algorithm::CosmicHarmony => {
                if hash_bytes.len() < 4 {
                    return 0;
                }
                let state0 = match self.cosmic_state0_endian {
                    "little" => u32::from_le_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                    _ => u32::from_be_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                };
                if state0 > 0 {
                    (u32::MAX as u64) / (state0 as u64)
                } else {
                    0
                }
            }
            Algorithm::CosmicHarmonyV2 => {
                if hash_bytes.len() < 32 {
                    return 0;
                }
                let hash_int = u256_from_be_slice(&hash_bytes[0..32]);
                if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                }
            }
            Algorithm::CosmicHarmonyV3 => {
                // Same difficulty model as Cosmic Harmony (state0).
                if hash_bytes.len() < 4 {
                    return 0;
                }
                let state0 = match self.cosmic_state0_endian {
                    "little" => u32::from_le_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                    _ => u32::from_be_bytes([
                        hash_bytes[0],
                        hash_bytes[1],
                        hash_bytes[2],
                        hash_bytes[3],
                    ]),
                };
                if state0 > 0 {
                    (u32::MAX as u64) / (state0 as u64)
                } else {
                    0
                }
            }
            Algorithm::Blake3 => {
                if hash_bytes.len() < 16 {
                    return 0;
                }
                let hash_int = u256_from_be_slice(&hash_bytes[0..32]);
                if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                }
            }
            Algorithm::AutolykovV2 => {
                let hash_int = u256_from_be_slice(hash_bytes);
                if hash_int > 0 {
                    ((u64::MAX as u128) / (hash_int as u128)) as u64
                } else {
                    0
                }
            }
            Algorithm::Unknown => 0,
        }
    }
}

/// Helper: parse big-endian u256 from byte slice (simplified)
fn u256_from_be_slice(bytes: &[u8]) -> u128 {
    let mut result: u128 = 0;
    for (_i, &byte) in bytes.iter().take(16).enumerate() {
        result = (result << 8) | (byte as u128);
    }
    result
}

/// Helper: parse u256 from hex string (simplified) - reserved for extended difficulty targets
#[allow(dead_code)]
fn u256_from_hex(hex: &str) -> u128 {
    u128::from_str_radix(hex, 16).unwrap_or(0)
}

fn meets_target_be(hash_bytes: &[u8], target_hex: &str, size: usize) -> bool {
    let mut target_bytes = vec![0u8; size];
    let t = target_hex.trim_start_matches("0x");
    if let Ok(mut tbytes) = Vec::from_hex(t) {
        if tbytes.len() > size {
            tbytes = tbytes.split_off(tbytes.len() - size);
        }
        let start = size - tbytes.len();
        target_bytes[start..].copy_from_slice(&tbytes);
    }

    for (a, b) in hash_bytes.iter().take(size).zip(target_bytes.iter()) {
        if a < b {
            return true;
        } else if a > b {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_algorithm_parsing() {
        assert_eq!(Algorithm::from_str("randomx"), Algorithm::RandomX);
        assert_eq!(Algorithm::from_str("rx/0"), Algorithm::RandomX);
        assert_eq!(Algorithm::from_str("yescrypt"), Algorithm::Yescrypt);
        // cosmic_harmony now defaults to v3 (production); v1 is explicit only.
        assert_eq!(Algorithm::from_str("cosmic_harmony"), Algorithm::CosmicHarmonyV3);
        assert_eq!(Algorithm::from_str("cosmic"), Algorithm::CosmicHarmonyV3);
        assert_eq!(Algorithm::from_str("cosmic_harmony_v1"), Algorithm::CosmicHarmony);
        assert_eq!(Algorithm::from_str("blake3"), Algorithm::Blake3);
    }

    #[tokio::test]
    async fn test_duplicate_detection() {
        let validator = ShareValidator::new("little");
        
        // Use CH v3 since it's the default production mapping.
        // Provide a minimally valid blob length.
        let share = SubmittedShare {
            job_id: "job1".to_string(),
            nonce: "00000001".to_string(),
            result: None, // Let validator compute
            algorithm: "cosmic_harmony_v3".to_string(),
            job_blob: "00".repeat(156),
            // Max target so any hash meets difficulty
            job_target: "ffffffff".to_string(),
            block_target: None,
            height: Some(0),
        };

        // First validation
        let result1 = validator.validate_share(&share, "miner_A").await;
        assert!(result1.valid, "First share should be valid: {}", result1.reason);
        
        // Second validation from SAME miner (duplicate)
        let result2 = validator.validate_share(&share, "miner_A").await;
        assert!(!result2.valid, "Second share from same miner should be rejected");
        assert!(result2.reason.contains("Duplicate"), "Reason should mention duplicate: {}", result2.reason);
        
        // Third validation from DIFFERENT miner (should NOT be duplicate)
        let result3 = validator.validate_share(&share, "miner_B").await;
        assert!(result3.valid, "Same nonce from different miner should be accepted: {}", result3.reason);
    }
}
