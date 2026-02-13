/// Sprint 1.1 — Chain Consensus Validation Tests
///
/// Tests the consensus rules that protect chain integrity:
/// difficulty bounds, timestamp sanity, reward schedule, and
/// target calculations.

use zion_core::blockchain::consensus::{self, BlockInfo};
use zion_core::blockchain::reward;
use zion_core::blockchain::validation;
use zion_core::blockchain::block::{Block, Algorithm};

// ═══════════════════════════════════════════════════════════════════════════
// 1. Difficulty target calculations
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_target_u32_inversely_proportional() {
    let t1 = consensus::target_u32_from_difficulty(1000);
    let t2 = consensus::target_u32_from_difficulty(2000);
    // Higher difficulty → lower target
    assert!(t1 > t2, "target(1000)={} should be > target(2000)={}", t1, t2);
}

#[test]
fn test_target_u64_inversely_proportional() {
    let t1 = consensus::target_u64_from_difficulty(1000);
    let t2 = consensus::target_u64_from_difficulty(2000);
    assert!(t1 > t2);
}

#[test]
fn test_target_u128_inversely_proportional() {
    let t1 = consensus::target_u128_from_difficulty(1000);
    let t2 = consensus::target_u128_from_difficulty(2000);
    assert!(t1 > t2);
}

#[test]
fn test_target_min_difficulty_is_max_target() {
    // Difficulty 1 should give the largest target (easiest)
    let t = consensus::target_u64_from_difficulty(1);
    assert_eq!(t, u64::MAX);
}

#[test]
fn test_target_zero_difficulty_handled() {
    // Difficulty 0 should be clamped to 1 (no division by zero)
    let t = consensus::target_u64_from_difficulty(0);
    assert_eq!(t, u64::MAX);
}

#[test]
fn test_target_256_format() {
    let target = consensus::target_from_difficulty_256(1000);
    assert_eq!(target.len(), 64, "Target hex should be 64 chars (256 bits)");
    // Should start with zeros (high difficulty = small target)
    assert!(target.starts_with("00"), "High difficulty target should start with zeros");
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. LWMA difficulty adjustment
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_lwma_stable_difficulty() {
    // When all blocks arrive exactly on target (60s apart),
    // difficulty should remain roughly the same
    let base_diff = 100_000u64;
    let mut window: Vec<BlockInfo> = Vec::new();
    let start_ts = 1_000_000u64;

    // Create LWMA_WINDOW + 1 entries, each 60s apart
    for i in 0..=(consensus::LWMA_WINDOW as usize) {
        window.push(BlockInfo {
            timestamp: start_ts + (i as u64) * consensus::TARGET_BLOCK_TIME,
            difficulty: base_diff,
        });
    }

    let next = consensus::lwma_next_difficulty(&window);
    // Should be within ±5% of base_diff
    let ratio = next as f64 / base_diff as f64;
    assert!(
        (0.95..=1.05).contains(&ratio),
        "Stable chain should maintain difficulty: next={}, base={}, ratio={}",
        next, base_diff, ratio
    );
}

#[test]
fn test_lwma_fast_blocks_increase_difficulty() {
    // When blocks arrive faster than target (30s instead of 60s),
    // difficulty should increase
    let base_diff = 100_000u64;
    let mut window: Vec<BlockInfo> = Vec::new();
    let start_ts = 1_000_000u64;

    for i in 0..=(consensus::LWMA_WINDOW as usize) {
        window.push(BlockInfo {
            timestamp: start_ts + (i as u64) * 30, // 30s intervals (too fast)
            difficulty: base_diff,
        });
    }

    let next = consensus::lwma_next_difficulty(&window);
    assert!(
        next > base_diff,
        "Fast blocks should increase difficulty: next={}, base={}",
        next, base_diff
    );
}

#[test]
fn test_lwma_slow_blocks_decrease_difficulty() {
    // When blocks arrive slower than target (120s instead of 60s),
    // difficulty should decrease
    let base_diff = 100_000u64;
    let mut window: Vec<BlockInfo> = Vec::new();
    let start_ts = 1_000_000u64;

    for i in 0..=(consensus::LWMA_WINDOW as usize) {
        window.push(BlockInfo {
            timestamp: start_ts + (i as u64) * 120, // 120s intervals (too slow)
            difficulty: base_diff,
        });
    }

    let next = consensus::lwma_next_difficulty(&window);
    assert!(
        next < base_diff,
        "Slow blocks should decrease difficulty: next={}, base={}",
        next, base_diff
    );
}

#[test]
fn test_lwma_never_below_min_difficulty() {
    // Even with extremely slow blocks, difficulty should never go below MIN
    let base_diff = consensus::MIN_DIFFICULTY;
    let mut window: Vec<BlockInfo> = Vec::new();
    let start_ts = 1_000_000u64;

    for i in 0..=(consensus::LWMA_WINDOW as usize) {
        window.push(BlockInfo {
            timestamp: start_ts + (i as u64) * 3600, // 1h intervals (very slow)
            difficulty: base_diff,
        });
    }

    let next = consensus::lwma_next_difficulty(&window);
    assert!(
        next >= consensus::MIN_DIFFICULTY,
        "Difficulty {} should not go below MIN {}",
        next, consensus::MIN_DIFFICULTY
    );
}

#[test]
fn test_lwma_clamped_per_block() {
    // Single-block adjustment should be capped at ±25%
    let current_diff = 100_000u64;
    let next = consensus::calculate_next_difficulty(current_diff, 10, 60);
    // 10s actual vs 60s target → should try to increase a lot, but clamped
    let max_allowed = (current_diff as f64 * consensus::MAX_ADJUSTMENT_UP) as u64;
    assert!(
        next <= max_allowed,
        "Difficulty {} exceeds max adjustment {} (from {})",
        next, max_allowed, current_diff
    );
}

#[test]
fn test_calculate_next_difficulty_stable() {
    // When actual_time == target_time, difficulty stays the same
    let d = 100_000u64;
    let next = consensus::calculate_next_difficulty(d, 60, 60);
    assert_eq!(next, d, "Same timing should keep same difficulty");
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Block validation rules
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_genesis_block_validates() {
    let genesis = Block::genesis();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let result = validation::validate_block(&genesis, None, now);
    // Genesis should validate structurally (may fail on PoW for test genesis)
    assert!(
        result.is_ok() || result.as_ref().unwrap_err().contains("PoW"),
        "Genesis validation failed: {:?}",
        result
    );
}

#[test]
fn test_block_version_must_be_1() {
    let block = Block::new(2, 1, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
    let prev = Block::new(1, 0, "00".repeat(32), 999_940, 1000, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_000);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("version"));
}

#[test]
fn test_block_height_must_be_sequential() {
    let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
    // Height 5 after height 0 — invalid
    let block = Block::new(1, 5, prev.calculate_hash(), 1_000_060, 1000, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_060);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("height"));
}

#[test]
fn test_block_prev_hash_must_match() {
    let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
    // Wrong prev_hash
    let block = Block::new(1, 1, "ff".repeat(32), 1_000_060, 1000, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_060);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("prev_hash"));
}

#[test]
fn test_timestamp_before_prev_rejected() {
    let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
    let block = Block::new(1, 1, prev.calculate_hash(), 999_999, 1000, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_000);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("before previous"));
}

#[test]
fn test_timestamp_far_future_rejected() {
    let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
    // Timestamp 10 hours in the future (> 7200s)
    let future_ts = 1_000_000 + 36_000;
    let block = Block::new(1, 1, prev.calculate_hash(), future_ts, 1000, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_000);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("future") || err.contains("drift"),
        "Expected future/drift rejection, got: {}",
        err
    );
}

#[test]
fn test_difficulty_below_minimum_rejected() {
    let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
    // Difficulty 500 < MIN_DIFFICULTY 1000
    let block = Block::new(1, 1, prev.calculate_hash(), 1_000_060, 500, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_060);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("below minimum"));
}

#[test]
fn test_difficulty_adjustment_too_large_rejected() {
    let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 10_000, 0, vec![]);
    // Difficulty 20_000 = 2× prev = way beyond +25% allowed
    let block = Block::new(1, 1, prev.calculate_hash(), 1_000_060, 20_000, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_060);
    assert!(result.is_err());
    assert!(
        result.unwrap_err().contains("adjustment range"),
        "Expected difficulty adjustment rejection"
    );
}

#[test]
fn test_difficulty_within_25pct_accepted() {
    // 1000 × 1.20 = 1200 (within +25%)
    let prev = Block::new(1, 0, "00".repeat(32), 1_000_000, 1000, 0, vec![]);
    let block = Block::new(1, 1, prev.calculate_hash(), 1_000_060, 1200, 0, vec![]);
    let result = validation::validate_block(&block, Some(&prev), 1_000_060);
    // Should not fail on difficulty specifically
    if let Err(e) = result {
        assert!(
            !e.contains("adjustment range") && !e.contains("below minimum"),
            "Difficulty 1200 should be within ±25% of 1000, got: {}",
            e
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Reward schedule integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_reward_at_various_heights() {
    // Height 0: no reward (genesis)
    assert_eq!(reward::calculate(0, 1000), 0);

    // Height 1: first mined block
    assert_eq!(reward::calculate(1, 1000), 5_400_067_000);

    // Height 1,000,000: same reward (constant emission)
    assert_eq!(reward::calculate(1_000_000, 1000), 5_400_067_000);

    // Height 23,652,000: last rewarded block
    assert_eq!(reward::calculate(23_652_000, 1000), 5_400_067_000);

    // Height 23,652,001: emission complete
    assert_eq!(reward::calculate(23_652_001, 1000), 0);
}

#[test]
fn test_miner_tithe_pool_distribution() {
    let total = reward::calculate(1, 1000);
    let miner = reward::miner_reward(1, 1000);
    let tithe = reward::tithe_reward(1, 1000);
    let pool = reward::pool_fee_reward(1, 1000);

    // All three should sum close to total (integer division truncation may lose a few units)
    let sum = miner + tithe + pool;
    assert!(
        sum <= total,
        "Distribution sum {} exceeds total {}",
        sum, total
    );
    // Allow max 3 atomic units rounding error
    assert!(
        total - sum <= 3,
        "Rounding error too large: total={}, sum={}",
        total, sum
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 5. Algorithm assignment
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_algorithm_cosmic_harmony_from_genesis() {
    // Current config: CosmicHarmony for all heights
    assert_eq!(Algorithm::from_height(0), Algorithm::CosmicHarmony);
    assert_eq!(Algorithm::from_height(1), Algorithm::CosmicHarmony);
    assert_eq!(Algorithm::from_height(100), Algorithm::CosmicHarmony);
    assert_eq!(Algorithm::from_height(1_000_000), Algorithm::CosmicHarmony);
}

#[test]
fn test_algorithm_names() {
    assert_eq!(Algorithm::CosmicHarmony.name(), "cosmic_harmony");
    assert_eq!(Algorithm::Blake3.name(), "blake3");
    assert_eq!(Algorithm::RandomX.name(), "randomx");
    assert_eq!(Algorithm::Yescrypt.name(), "yescrypt");
}
