use crate::algorithms::{cosmic_harmony, cosmic_harmony_v2, blake3_algo, Algorithm};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub height: u64,
    pub prev_hash: String,
    pub difficulty: u64,
    pub target: String,
    pub coinbase_address: String,
    #[serde(default)]
    pub algorithm: Option<String>,
}

impl BlockTemplate {
    /// Get algorithm for this template (default: CosmicHarmony)
    pub fn get_algorithm(&self) -> Algorithm {
        self.algorithm
            .as_ref()
            .and_then(|a| Algorithm::from_str(a))
            .unwrap_or_default()
    }
}

#[derive(Debug)]
pub struct MiningResult {
    pub nonce: u64,
    pub hash: String,
    pub iterations: u64,
    pub elapsed_secs: f64,
    pub hashrate: f64,
    pub algorithm: Algorithm,
}

/// Multi-algorithm CPU miner
/// Returns when a valid hash is found or max_iterations is reached
pub fn mine_block(
    template: &BlockTemplate,
    max_iterations: u64,
    algorithm: Option<Algorithm>,
) -> Option<MiningResult> {
    let start = Instant::now();
    let algo = algorithm.unwrap_or_else(|| template.get_algorithm());
    let target = parse_target(&template.target);
    
    println!("🌟 Mining with {} algorithm", algo);
    
    // Create block header prefix (everything except nonce)
    let prefix = format!(
        "{}:{}:{}:{}",
        template.height,
        template.prev_hash,
        template.coinbase_address,
        template.difficulty
    );
    let prefix_bytes = prefix.as_bytes();
    
    for nonce in 0..max_iterations {
        // Compute hash based on algorithm
        let hash = match algo {
            Algorithm::CosmicHarmony => {
                cosmic_harmony::cosmic_hash(prefix_bytes, nonce as u32)
            }
            Algorithm::CosmicHarmonyV2 => {
                // v2 requires prev_hash as [u8; 32] and block height
                let mut prev_hash_bytes = [0u8; 32];
                if let Ok(decoded) = hex::decode(&template.prev_hash) {
                    let len = decoded.len().min(32);
                    prev_hash_bytes[..len].copy_from_slice(&decoded[..len]);
                }
                cosmic_harmony_v2::cosmic_hash_v2(
                    prefix_bytes,
                    nonce,  // u64 nonce
                    &prev_hash_bytes,
                    template.height
                )
            }
            Algorithm::Blake3 => {
                blake3_algo::blake3_hash_with_nonce(prefix_bytes, nonce as u32)
            }
            Algorithm::RandomX => {
                // RandomX needs full header with nonce
                let mut full_input = prefix_bytes.to_vec();
                full_input.extend_from_slice(&nonce.to_le_bytes());
                
                match crate::algorithms::randomx::randomx_hash(&full_input) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("❌ RandomX error: {}", e);
                        return None;
                    }
                }
            }
            Algorithm::Yescrypt => {
                match crate::algorithms::yescrypt::yescrypt_hash_mining(prefix_bytes, nonce as u64) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("❌ Yescrypt error: {}", e);
                        return None;
                    }
                }
            }
        };
        
        let hash_hex = hex::encode(&hash);
        
        // Check if hash meets target
        if hash_meets_target(&hash_hex, &target) {
            let elapsed = start.elapsed().as_secs_f64();
            let hashrate = (nonce + 1) as f64 / elapsed;
            
            return Some(MiningResult {
                nonce,
                hash: hash_hex,
                iterations: nonce + 1,
                elapsed_secs: elapsed,
                hashrate,
                algorithm: algo,
            });
        }
        
        // Progress update every million iterations
        if nonce > 0 && nonce % 1_000_000 == 0 {
            let elapsed = start.elapsed().as_secs_f64();
            let hashrate = nonce as f64 / elapsed;
            println!("⛏️  {} iterations, {:.2} kH/s", nonce, hashrate / 1000.0);
        }
    }
    
    None
}

/// Parse target from hex string to bytes for comparison
fn parse_target(target_hex: &str) -> Vec<u8> {
    // Remove "0x" prefix if present
    let hex = target_hex.trim_start_matches("0x");
    
    // Pad to 64 characters (32 bytes) if needed
    let padded = format!("{:0>64}", hex);
    
    hex::decode(&padded).unwrap_or_else(|_| vec![0xff; 32])
}

/// Check if hash (as hex string) is less than target (as bytes)
fn hash_meets_target(hash_hex: &str, target: &[u8]) -> bool {
    let hash_bytes = match hex::decode(hash_hex) {
        Ok(b) => b,
        Err(_) => return false,
    };
    
    // Compare byte by byte (big-endian comparison)
    for i in 0..32 {
        let h = hash_bytes.get(i).unwrap_or(&0xff);
        let t = target.get(i).unwrap_or(&0xff);
        
        if h < t {
            return true;
        } else if h > t {
            return false;
        }
        // If equal, continue to next byte
    }
    
    // Hashes are equal, which counts as meeting target
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_comparison() {
        // Low hash should meet high target
        let low_hash = "0000000000000001000000000000000000000000000000000000000000000000";
        let high_target = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10];
        let high_target_padded = [high_target, vec![0xff; 24]].concat();
        
        assert!(hash_meets_target(low_hash, &high_target_padded));
        
        // High hash should not meet low target
        let high_hash = "f000000000000000000000000000000000000000000000000000000000000000";
        let low_target = vec![0x01; 32];
        
        assert!(!hash_meets_target(high_hash, &low_target));
    }

    #[test]
    fn test_mine_simple_block() {
        let template = BlockTemplate {
            height: 1,
            prev_hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            difficulty: 100,
            target: "00ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            coinbase_address: "test_address".to_string(),
            algorithm: Some("cosmic_harmony".to_string()),
        };
        
        // Try mining with limited iterations
        let result = mine_block(&template, 100_000, None);
        
        // May or may not find a block depending on luck
        if let Some(res) = result {
            println!("Found block! Nonce: {}, Hash: {}", res.nonce, res.hash);
            assert!(res.iterations <= 100_000);
        }
    }

    #[test]
    fn test_cosmic_harmony_mining() {
        use crate::algorithms::Algorithm;
        
        let template = BlockTemplate {
            height: 1,
            prev_hash: "0".repeat(64),
            difficulty: 8, // Low difficulty for testing
            target: "0fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff".to_string(),
            coinbase_address: "ZION_TEST".to_string(),
            algorithm: None,
        };

        let result = mine_block(&template, 10_000, Some(Algorithm::CosmicHarmony));
        assert!(result.is_some(), "Should find block with low difficulty");
        
        if let Some(res) = result {
            assert_eq!(res.algorithm, Algorithm::CosmicHarmony);
            println!("✅ Cosmic Harmony found block at nonce {}", res.nonce);
        }
    }
}
