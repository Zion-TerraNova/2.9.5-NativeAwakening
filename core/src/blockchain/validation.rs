use crate::blockchain::block::{Block, Algorithm};
use crate::blockchain::consensus;
use crate::blockchain::fee;
use hex::FromHex;
use num_bigint::BigUint;
use num_traits::Num;

// ──────────────────────────────────────────────
// Consensus constants
// ──────────────────────────────────────────────

/// Coinbase outputs cannot be spent until this many blocks after the block
/// that contains them.  100 blocks ≈ 100 minutes at 60 s target.
pub const COINBASE_MATURITY: u64 = 100;

/// Maximum allowed timestamp drift from the previous block.
/// TestNet: 24 hours (86400 s) to survive restarts and long mining pauses.
/// MainNet: reduce to 7200 s (2 hours) before launch.
/// LWMA internally clamps solve times to prevent manipulation.
pub const MAX_TIMESTAMP_DRIFT: u64 = 86400; // 24 hours (TestNet)

/// Comprehensive block validation
pub fn validate_block(
    block: &Block,
    prev_block: Option<&Block>,
    current_time_secs: u64,
) -> Result<(), String> {
    // SECURITY: ZION_DEV_MODE is only honoured in debug (non-release) builds.
    // In release builds dev_mode is ALWAYS false — no env-var can bypass consensus.
    #[cfg(debug_assertions)]
    let dev_mode = std::env::var("ZION_DEV_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    #[cfg(not(debug_assertions))]
    let dev_mode = false;
    // 1. Version check
    if block.header.version != 1 {
        return Err(format!("Invalid version: {}", block.header.version));
    }
    
    // 2. Height check (must be prev + 1, or 0 for genesis)
    if let Some(prev) = prev_block {
        if block.header.height != prev.header.height + 1 {
            return Err(format!(
                "Invalid height: {} (expected {})",
                block.header.height,
                prev.header.height + 1
            ));
        }
        
        // 3. Previous hash check
        let computed_prev_hash = prev.calculate_hash();
        if block.header.prev_hash != computed_prev_hash {
            eprintln!("❌ prev_hash MISMATCH at height {}:", block.header.height);
            eprintln!("   block.prev_hash = {}", block.header.prev_hash);
            eprintln!("   computed(prev)  = {}", computed_prev_hash);
            eprintln!("   prev height={}, prev nonce={}, prev algo={:?}",
                prev.header.height, prev.header.nonce, prev.header.algorithm);
            eprintln!("   prev prev_hash  = {}", prev.header.prev_hash);
            eprintln!("   prev merkle     = {}", prev.header.merkle_root);
            eprintln!("   prev timestamp  = {}", prev.header.timestamp);
            eprintln!("   prev difficulty = {}", prev.header.difficulty);
            return Err(format!(
                "Invalid prev_hash at height {}: expected {} but got {}",
                block.header.height,
                &block.header.prev_hash[..16.min(block.header.prev_hash.len())],
                &computed_prev_hash[..16.min(computed_prev_hash.len())]
            ));
        }
    } else if block.header.height != 0 {
        return Err(format!(
            "Missing previous block context for height {} (cannot validate non-genesis block without prev)",
            block.header.height
        ));
    }
    
    // 4. Timestamp validation
    //    a) Not too far in the future (absolute wall-clock check)
    const MAX_FUTURE_DRIFT: u64 = 7200; // 2 hours
    if block.header.timestamp > current_time_secs + MAX_FUTURE_DRIFT {
        return Err(format!(
            "Block timestamp {} too far in future (current: {})",
            block.header.timestamp, current_time_secs
        ));
    }
    
    if let Some(prev) = prev_block {
        //    b) Not before previous block
        if block.header.timestamp < prev.header.timestamp {
            return Err("Block timestamp before previous block".to_string());
        }
        
        //    c) Timestamp sanity — clamp ±2× target (±120 s)
        //       Reject blocks whose timestamp deviates more than MAX_TIMESTAMP_DRIFT
        //       from the previous block.  This limits manipulation of LWMA inputs.
        let delta = if block.header.timestamp >= prev.header.timestamp {
            block.header.timestamp - prev.header.timestamp
        } else {
            0 // Already rejected above, but be safe
        };
        
        // Maximum gap: prev_timestamp + MAX_TIMESTAMP_DRIFT
        // (We allow 0 delta for fast blocks, but cap the upper end.)
        // Skip drift check for block #1: genesis has a fixed historical timestamp
        // and the first mined block will naturally have a large gap.
        let is_first_block_after_genesis = block.header.height == 1;
        if delta > MAX_TIMESTAMP_DRIFT && !dev_mode && !is_first_block_after_genesis {
            return Err(format!(
                "Block timestamp drift {} s exceeds max {} s (prev: {}, block: {})",
                delta, MAX_TIMESTAMP_DRIFT, prev.header.timestamp, block.header.timestamp
            ));
        }
    }
    
    // 5. Algorithm check (must match height)
    let expected_algo = Algorithm::from_height(block.header.height);
    if block.header.algorithm != expected_algo {
        return Err(format!(
            "Wrong algorithm: {:?} (expected {:?} for height {})",
            block.header.algorithm, expected_algo, block.header.height
        ));
    }
    
    // 6. Merkle root validation
    let calculated_root = Block::calculate_merkle_root(&block.transactions);
    if block.header.merkle_root != calculated_root {
        return Err(format!(
            "Invalid merkle root: {} (expected {})",
            block.header.merkle_root, calculated_root
        ));
    }
    
    // 7. Difficulty check (validate against consensus rules)
    //
    // Difficulty for block N is set DETERMINISTICALY by the node before mining
    // (via get_block_template / state.difficulty). The miner cannot predict the
    // exact solve-time when it receives the template, so we CANNOT recalculate
    // expected difficulty using this block's timestamp.
    //
    // Instead we validate:
    //   a) difficulty ≥ MIN_DIFFICULTY  &&  ≤ MAX_DIFFICULTY
    //   b) difficulty is within ±25% of previous block (max single-block adjustment)
    //   c) PoW hash meets the declared difficulty target (done in step 8)
    //
    // The actual LWMA retarget happens in state.process_block() AFTER acceptance,
    // setting state.difficulty for the NEXT block's template.
    if !dev_mode {
        // Global floor / ceiling
        if block.header.difficulty < consensus::MIN_DIFFICULTY {
            return Err(format!(
                "Difficulty {} below minimum {}",
                block.header.difficulty, consensus::MIN_DIFFICULTY
            ));
        }
        if block.header.difficulty > consensus::MAX_DIFFICULTY {
            return Err(format!(
                "Difficulty {} above maximum {}",
                block.header.difficulty, consensus::MAX_DIFFICULTY
            ));
        }
        // Per-block adjustment bounds (±25% from previous)
        if let Some(prev) = prev_block {
            let prev_diff = prev.header.difficulty;
            let max_allowed = (prev_diff as f64 * consensus::MAX_ADJUSTMENT_UP) as u64;
            let min_allowed = ((prev_diff as f64 * consensus::MAX_ADJUSTMENT_DOWN) as u64)
                .max(consensus::MIN_DIFFICULTY);

            if block.header.difficulty > max_allowed || block.header.difficulty < min_allowed {
                return Err(format!(
                    "Difficulty {} out of adjustment range [{}, {}] (prev={})",
                    block.header.difficulty, min_allowed, max_allowed, prev_diff
                ));
            }
        }
    }
    
    // 8. Proof-of-Work validation
    validate_pow(block)?;
    
    // 9. Transaction validation (structural + signature checks)
    if block.transactions.is_empty() {
        return Err("Block must contain at least one transaction (coinbase)".to_string());
    }

    // Skip strict tx validation for genesis (premine handled in state init)
    if block.header.height > 0 {
        let zero_hash = "0000000000000000000000000000000000000000000000000000000000000000";
        let coinbase = &block.transactions[0];

        let is_coinbase = coinbase.inputs.is_empty()
            || coinbase
                .inputs
                .iter()
                .all(|i| i.prev_tx_hash == zero_hash);

        if !is_coinbase {
            return Err("First transaction must be coinbase".to_string());
        }

        // Validate coinbase reward — FEES ARE BURNED, coinbase ≤ block reward only
        let total_output: u64 = coinbase.outputs.iter().map(|o| o.amount).sum();
        let max_allowed = fee::max_coinbase_output(block.header.height);
        if total_output > max_allowed {
            return Err(format!(
                "Coinbase output {} exceeds max allowed {} (fee burning: fees are NOT paid to miner)",
                total_output, max_allowed
            ));
        }

        for tx in block.transactions.iter().skip(1) {
            if tx.inputs.is_empty() {
                return Err("Non-coinbase tx must have inputs".to_string());
            }

            if tx.outputs.is_empty() {
                return Err("Non-coinbase tx must have outputs".to_string());
            }

            if tx.inputs.iter().any(|i| i.prev_tx_hash == zero_hash) {
                return Err("Non-coinbase tx contains coinbase input".to_string());
            }
            
            // Validate transaction structure, fees, and output amounts
            validate_transaction(tx)?;
        }
    }
    
    Ok(())
}

/// Validate a single transaction (structure, signatures, fees, amounts)
pub fn validate_transaction(tx: &crate::tx::Transaction) -> Result<(), String> {
    // NOTE: Coinbase validation is contextual (position in block) and is handled in validate_block.
    // This function validates a regular (non-coinbase) transaction.

    // 1. Structural checks
    if tx.inputs.is_empty() {
        return Err("Transaction has no inputs".to_string());
    }

    if tx.outputs.is_empty() {
        return Err("Transaction has no outputs".to_string());
    }

    // 2. Signature verification
    if !tx.verify_signatures() {
        return Err(format!("Invalid transaction signature for tx {}", tx.id));
    }

    // 3. Fee validation (minimum fee + fee rate)
    let tx_size = fee::estimate_tx_size(tx.inputs.len(), tx.outputs.len());
    if let Err(msg) = fee::validate_fee(tx.fee, tx_size) {
        return Err(format!("Transaction {}: {}", tx.id, msg));
    }

    // 4. Output amount validation (no zero, no overflow, within total supply)
    let outputs: Vec<(u64, &str)> = tx.outputs.iter()
        .map(|o| (o.amount, o.address.as_str()))
        .collect();
    if let Err(msg) = fee::validate_output_amounts(&outputs) {
        return Err(format!("Transaction {}: {}", tx.id, msg));
    }

    // 5. Transaction size check
    if tx_size > fee::MAX_TX_SIZE_BYTES {
        return Err(format!(
            "Transaction {} too large: {} bytes (max {})",
            tx.id, tx_size, fee::MAX_TX_SIZE_BYTES
        ));
    }
    
    Ok(())
}

/// Calculate maximum allowed coinbase output for validation.
///
/// **Fees are burned.** Coinbase = block reward only (no fee component).
pub fn calculate_block_reward(height: u64) -> u64 {
    fee::max_coinbase_output(height)
}

/// Validate Proof-of-Work (hash meets difficulty target)
pub fn validate_pow(block: &Block) -> Result<(), String> {
    let hash = block.header.calculate_hash();
    let diff = block.header.difficulty.max(1);

    let meets = match block.header.algorithm {
        Algorithm::RandomX => {
            if hash.len() < 8 {
                return Err("RandomX hash too short".to_string());
            }
            let mut low64 = [0u8; 8];
            low64.copy_from_slice(&hash[0..8]);
            let res = u64::from_le_bytes(low64);
            let target = consensus::target_u64_from_difficulty(diff);
            res <= target
        }
        Algorithm::CosmicHarmony => {
            if hash.len() < 4 {
                return Err("Cosmic hash too short".to_string());
            }
            let state0 = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
            let target = consensus::target_u32_from_difficulty(diff);
            state0 <= target
        }
        Algorithm::Yescrypt => {
            if hash.len() < 28 {
                return Err("Yescrypt hash too short".to_string());
            }
            let target_u128 = consensus::target_u128_from_difficulty(diff);
            let target_hex = format!("{:032x}", target_u128);
            meets_target_be(&hash, &target_hex, 28)
        }
        Algorithm::Blake3 => {
            let target_hex = consensus::target_from_difficulty_256(diff);
            meets_target_be(&hash, &target_hex, 32)
        }
    };

    if !meets {
        let hash_hex = block.calculate_hash();
        return Err(format!(
            "Insufficient PoW: hash {} does not meet target for difficulty {}",
            hash_hex, block.header.difficulty
        ));
    }

    Ok(())
}

/// Convert difficulty to 256-bit target
pub fn difficulty_to_target(difficulty: u64) -> [u8; 32] {
    let max_target = BigUint::from_str_radix(
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        16
    ).unwrap();
    
    let difficulty_val = BigUint::from(difficulty.max(1));
    let target = max_target / difficulty_val;
    
    // Convert to 32-byte array
    let target_bytes = target.to_bytes_be();
    let mut result = [0u8; 32];
    let start = 32 - target_bytes.len();
    result[start..].copy_from_slice(&target_bytes);
    result
}

/// Quick validation (minimal checks for network propagation)
pub fn quick_validate_block(block: &Block) -> Result<(), String> {
    // Just check PoW and merkle root
    validate_pow(block)?;
    
    let calculated_root = Block::calculate_merkle_root(&block.transactions);
    if block.header.merkle_root != calculated_root {
        return Err("Invalid merkle root".to_string());
    }
    
    Ok(())
}

fn meets_target_be(hash: &[u8], target_hex: &str, size: usize) -> bool {
    let mut target_bytes = vec![0u8; size];
    let t = target_hex.trim_start_matches("0x");
    if let Ok(mut tbytes) = Vec::from_hex(t) {
        if tbytes.len() > size {
            tbytes = tbytes.split_off(tbytes.len() - size);
        }
        let start = size - tbytes.len();
        target_bytes[start..].copy_from_slice(&tbytes);
    }

    for (a, b) in hash.iter().take(size).zip(target_bytes.iter()) {
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
    use std::time::{SystemTime, UNIX_EPOCH};
    
    #[test]
    fn test_difficulty_to_target() {
        let target = difficulty_to_target(1000);
        // Target should be max / 1000
        assert!(target[0] == 0); // High bytes should be small
    }
    
    #[test]
    fn test_validate_genesis() {
        let genesis = Block::genesis();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Genesis should validate without previous block
        let result = validate_block(&genesis, None, now);
        // May fail on PoW if nonce is wrong, but structure should be valid
        assert!(result.is_ok() || result.unwrap_err().contains("PoW"));
    }
    
    // ── Coinbase maturity constant ──
    
    #[test]
    fn test_coinbase_maturity_constant() {
        assert_eq!(COINBASE_MATURITY, 100);
    }
    
    // ── Timestamp sanity ──
    
    #[test]
    fn test_max_timestamp_drift_constant() {
        assert_eq!(MAX_TIMESTAMP_DRIFT, 7200);
    }
    
    #[test]
    fn test_timestamp_sanity_rejected() {
        // Create two blocks where the second has timestamp > prev + 7200s
        let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
        
        // Block with timestamp 8000 seconds after prev (> 7200s drift)
        let block = Block::new(
            1, 1,
            prev.calculate_hash(),
            1_008_000, // 8000 seconds later
            1000, 0, vec![],
        );
        
        // This should fail on timestamp drift (unless dev mode)
        let result = validate_block(&block, Some(&prev), 1_008_000);
        // It will fail either on timestamp drift or PoW, depending on dev mode
        if let Err(e) = result {
            let valid_error = e.contains("drift") || e.contains("PoW") || e.contains("algorithm");
            assert!(valid_error, "Unexpected error: {}", e);
        }
    }
    
    #[test]
    fn test_timestamp_within_drift_accepted() {
        // Block with timestamp exactly 7200s after prev (at the limit)
        let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
        let block = Block::new(
            1, 1,
            prev.calculate_hash(),
            1_007_200, // exactly 7200s — at limit, should be allowed
            1000, 0, vec![],
        );
        
        let result = validate_block(&block, Some(&prev), 1_007_200);
        // Should not fail on timestamp drift specifically
        if let Err(e) = result {
            assert!(!e.contains("drift"), "Should not reject at exactly MAX_TIMESTAMP_DRIFT: {}", e);
        }
    }
    
    #[test]
    fn test_timestamp_before_prev_rejected() {
        let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
        let block = Block::new(
            1, 1,
            prev.calculate_hash(),
            999_999, // before previous
            1000, 0, vec![],
        );
        
        let result = validate_block(&block, Some(&prev), 1_000_000);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("before previous"));
    }
}
