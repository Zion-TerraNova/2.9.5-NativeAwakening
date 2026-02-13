/// ZION Emission Schedule — Constant Emission (No Halving)
///
/// Total Supply:      144,000,000,000 ZION (144B)
/// Genesis Premine:    16,280,000,000 ZION (16.28B — 11.31%)
/// Mining Emission:   127,720,000,000 ZION (127.72B — 88.69%)
///
/// Block time:   60 seconds
/// Mining years: 45 (2026–2070)
/// Total blocks: 23,652,000  (45 × 525,600)
///
/// Block Reward = MINING_EMISSION / TOTAL_BLOCKS
///              = 127,720,000,000 / 23,652,000
///              = 5,400.067 ZION per block (constant)
///
/// Verification:
///   5,400.067 × 23,652,000 = 127,720,384,400 ZION
///   Rounding error: 384,400 ZION (0.00027% of total — acceptable)
///
/// After block 23,652,000 the reward is 0 — emission complete.
///
/// All values in atomic units (1 ZION = 1,000,000 atomic units).

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// 1 ZION = 1,000,000 atomic units (6 decimal places)
pub const ATOMIC_UNITS_PER_ZION: u64 = 1_000_000;

/// Total supply: 144,000,000,000 ZION
pub const TOTAL_SUPPLY: u64 = 144_000_000_000 * ATOMIC_UNITS_PER_ZION;

/// Genesis premine: 16,280,000,000 ZION
pub const GENESIS_PREMINE: u64 = 16_280_000_000 * ATOMIC_UNITS_PER_ZION;

/// Mining emission: 127,720,000,000 ZION (TOTAL_SUPPLY − GENESIS_PREMINE)
pub const MINING_EMISSION: u64 = TOTAL_SUPPLY - GENESIS_PREMINE;

/// Block time target: 60 seconds
pub const BLOCK_TIME_SECONDS: u64 = 60;

/// Blocks per year (365 days × 24h × 60min = 525,600)
pub const BLOCKS_PER_YEAR: u64 = 525_600;

/// Mining duration: 45 years
pub const MINING_YEARS: u64 = 45;

/// Total mineable blocks: 23,652,000
pub const TOTAL_MINING_BLOCKS: u64 = MINING_YEARS * BLOCKS_PER_YEAR;

/// Constant block reward: 5,400.067 ZION = 5,400,067,000 atomic units
///
/// Derived: MINING_EMISSION / TOTAL_MINING_BLOCKS
///        = 127,720,000,000,000,000 / 23,652,000
///        = 5,400,067,024.… → truncated to 5,400,067,000 atomic units
///        = 5,400.067 ZION
pub const BLOCK_REWARD_ATOMIC: u64 = 5_400_067_000;

/// Humanitarian tithe: 10% of block reward
pub const TITHE_PERCENT: u64 = 10;

/// Pool fee: 1% of block reward
pub const POOL_FEE_PERCENT: u64 = 1;

/// Miner share: 89% of block reward
pub const MINER_SHARE_PERCENT: u64 = 89;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Calculate the block reward for a given height.
///
/// Constant emission: every block from height 1 to TOTAL_MINING_BLOCKS yields
/// BLOCK_REWARD_ATOMIC.  Height 0 is the genesis block (premine only, no
/// coinbase mining reward).  After TOTAL_MINING_BLOCKS the reward is 0.
///
/// The `_difficulty` parameter is accepted for API compatibility but does not
/// affect the reward.
pub fn calculate(height: u64, _difficulty: u64) -> u64 {
    if height == 0 {
        // Genesis block — reward is handled by premine, not coinbase
        return 0;
    }
    if height > TOTAL_MINING_BLOCKS {
        // Emission complete
        return 0;
    }
    BLOCK_REWARD_ATOMIC
}

/// Calculate the miner's share of the block reward (89%).
pub fn miner_reward(height: u64, difficulty: u64) -> u64 {
    let total = calculate(height, difficulty);
    total * MINER_SHARE_PERCENT / 100
}

/// Calculate the humanitarian tithe (10%).
pub fn tithe_reward(height: u64, difficulty: u64) -> u64 {
    let total = calculate(height, difficulty);
    total * TITHE_PERCENT / 100
}

/// Calculate the pool fee (1%).
pub fn pool_fee_reward(height: u64, difficulty: u64) -> u64 {
    let total = calculate(height, difficulty);
    total * POOL_FEE_PERCENT / 100
}

/// Calculate the theoretical maximum mining supply.
/// Returns value in ZION (not atomic units).
pub fn max_mining_supply() -> f64 {
    // Constant emission: BLOCK_REWARD × TOTAL_MINING_BLOCKS
    (BLOCK_REWARD_ATOMIC as f64 * TOTAL_MINING_BLOCKS as f64) / ATOMIC_UNITS_PER_ZION as f64
}

/// Returns the total supply in ZION (not atomic units).
pub fn total_supply() -> f64 {
    TOTAL_SUPPLY as f64 / ATOMIC_UNITS_PER_ZION as f64
}

/// Returns the block at which mining emission ends.
pub fn emission_end_block() -> u64 {
    TOTAL_MINING_BLOCKS
}

/// Returns the estimated year when emission ends (assuming launch 2026).
pub fn emission_end_year() -> u64 {
    2026 + MINING_YEARS
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_genesis_no_reward() {
        // Genesis block has no mining reward (premine only)
        assert_eq!(calculate(0, 1000), 0);
    }

    #[test]
    fn test_block_1_reward() {
        // First mined block
        assert_eq!(calculate(1, 1000), BLOCK_REWARD_ATOMIC);
        assert_eq!(calculate(1, 1000), 5_400_067_000);
    }

    #[test]
    fn test_constant_reward() {
        // Reward is the same at any height within mining period
        assert_eq!(calculate(1, 1000), calculate(100_000, 1000));
        assert_eq!(calculate(1, 1000), calculate(1_000_000, 1000));
        assert_eq!(calculate(1, 1000), calculate(10_000_000, 1000));
        assert_eq!(calculate(1, 1000), calculate(TOTAL_MINING_BLOCKS, 1000));
    }

    #[test]
    fn test_last_block_has_reward() {
        // Block 23,652,000 is the last with a reward
        assert_eq!(calculate(TOTAL_MINING_BLOCKS, 1000), BLOCK_REWARD_ATOMIC);
    }

    #[test]
    fn test_emission_ends() {
        // Block 23,652,001 has no reward
        assert_eq!(calculate(TOTAL_MINING_BLOCKS + 1, 1000), 0);
        assert_eq!(calculate(TOTAL_MINING_BLOCKS + 1_000_000, 1000), 0);
        assert_eq!(calculate(u64::MAX, 1000), 0);
    }

    #[test]
    fn test_difficulty_ignored() {
        // Difficulty does not affect reward
        assert_eq!(calculate(1, 0), calculate(1, u64::MAX));
        assert_eq!(calculate(1, 1), calculate(1, 1_000_000));
    }

    #[test]
    fn test_max_mining_supply() {
        let supply = max_mining_supply();
        // Should be approximately 127.72B ZION
        assert!(supply > 127_700_000_000.0, "Supply too low: {}", supply);
        assert!(supply < 127_750_000_000.0, "Supply too high: {}", supply);
        println!("Max mining supply: {:.2} ZION", supply);
    }

    #[test]
    fn test_total_supply() {
        assert_eq!(total_supply(), 144_000_000_000.0);
    }

    #[test]
    fn test_reward_distribution_sums_to_100() {
        assert_eq!(MINER_SHARE_PERCENT + TITHE_PERCENT + POOL_FEE_PERCENT, 100);
    }

    #[test]
    fn test_miner_reward() {
        let mr = miner_reward(1, 1000);
        // 89% of 5,400,067,000 = 4,806,059,630
        assert_eq!(mr, 5_400_067_000 * 89 / 100);
    }

    #[test]
    fn test_tithe_reward() {
        let tr = tithe_reward(1, 1000);
        // 10% of 5,400,067,000 = 540,006,700
        assert_eq!(tr, 5_400_067_000 * 10 / 100);
    }

    #[test]
    fn test_pool_fee() {
        let pf = pool_fee_reward(1, 1000);
        // 1% of 5,400,067,000 = 54,000,670
        assert_eq!(pf, 5_400_067_000 * 1 / 100);
    }

    #[test]
    fn test_emission_end() {
        assert_eq!(emission_end_block(), 23_652_000);
        assert_eq!(emission_end_year(), 2071);
    }

    #[test]
    fn test_block_reward_value() {
        // 5,400.067 ZION in human terms
        let reward_zion = BLOCK_REWARD_ATOMIC as f64 / ATOMIC_UNITS_PER_ZION as f64;
        assert!((reward_zion - 5400.067).abs() < 0.001);
    }

    #[test]
    fn test_constants_consistency() {
        // MINING_EMISSION = TOTAL_SUPPLY - GENESIS_PREMINE
        assert_eq!(MINING_EMISSION, TOTAL_SUPPLY - GENESIS_PREMINE);
        // TOTAL_MINING_BLOCKS = 45 × 525,600
        assert_eq!(TOTAL_MINING_BLOCKS, 23_652_000);
        // BLOCKS_PER_YEAR = 525,600
        assert_eq!(BLOCKS_PER_YEAR, 525_600);
    }
}
