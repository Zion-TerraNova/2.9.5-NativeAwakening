/// Sprint 1.1 — Config Validation Tests
///
/// Validates that hardcoded Rust constants match TOML config files and
/// MAINNET_CONSTITUTION specifications.  These tests act as a "spec freeze"
/// guard: if any constant changes, the corresponding config must be updated
/// and vice-versa.

use zion_core::blockchain::consensus;
use zion_core::blockchain::reward;
use zion_core::blockchain::premine;
use zion_core::blockchain::validation;
use zion_core::network::NetworkType;

// ═══════════════════════════════════════════════════════════════════════════
// 1. Emission / Reward constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_block_reward_matches_spec() {
    // MAINNET_CONSTITUTION: 5,400.067 ZION per block
    assert_eq!(reward::BLOCK_REWARD_ATOMIC, 5_400_067_000);
    let reward_zion = reward::BLOCK_REWARD_ATOMIC as f64 / reward::ATOMIC_UNITS_PER_ZION as f64;
    assert!((reward_zion - 5400.067).abs() < 0.001);
}

#[test]
fn test_total_supply_matches_spec() {
    // 144,000,000,000 ZION × 1,000,000 atomic units
    assert_eq!(reward::TOTAL_SUPPLY, 144_000_000_000_000_000);
    assert_eq!(reward::total_supply(), 144_000_000_000.0);
}

#[test]
fn test_premine_total_matches_spec() {
    // 16,280,000,000 ZION
    assert_eq!(reward::GENESIS_PREMINE, 16_280_000_000_000_000);
    assert_eq!(premine::PREMINE_TOTAL, 16_280_000_000_000_000);
    // reward.rs and premine.rs must agree
    assert_eq!(reward::GENESIS_PREMINE, premine::PREMINE_TOTAL);
}

#[test]
fn test_mining_emission_equals_supply_minus_premine() {
    assert_eq!(reward::MINING_EMISSION, reward::TOTAL_SUPPLY - reward::GENESIS_PREMINE);
    assert_eq!(reward::MINING_EMISSION, 127_720_000_000_000_000);
    // premine.rs must also agree
    assert_eq!(premine::MINING_EMISSION, reward::MINING_EMISSION);
}

#[test]
fn test_emission_timeline() {
    // 45 years × 525,600 blocks/year = 23,652,000 total blocks
    assert_eq!(reward::BLOCKS_PER_YEAR, 525_600);
    assert_eq!(reward::MINING_YEARS, 45);
    assert_eq!(reward::TOTAL_MINING_BLOCKS, 23_652_000);
    assert_eq!(reward::emission_end_block(), 23_652_000);
    assert_eq!(reward::emission_end_year(), 2071); // 2026 + 45
}

#[test]
fn test_reward_distribution_percentages() {
    assert_eq!(reward::MINER_SHARE_PERCENT, 89);
    assert_eq!(reward::TITHE_PERCENT, 10);
    assert_eq!(reward::POOL_FEE_PERCENT, 1);
    assert_eq!(
        reward::MINER_SHARE_PERCENT + reward::TITHE_PERCENT + reward::POOL_FEE_PERCENT,
        100
    );
}

#[test]
fn test_atomic_units_per_zion() {
    // 6 decimal places
    assert_eq!(reward::ATOMIC_UNITS_PER_ZION, 1_000_000);
}

// ═══════════════════════════════════════════════════════════════════════════
// 2. Consensus constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_target_block_time() {
    assert_eq!(consensus::TARGET_BLOCK_TIME, 60);
}

#[test]
fn test_lwma_window() {
    assert_eq!(consensus::LWMA_WINDOW, 60);
}

#[test]
fn test_difficulty_adjustment_bounds() {
    assert!((consensus::MAX_ADJUSTMENT_UP - 1.25).abs() < f64::EPSILON);
    assert!((consensus::MAX_ADJUSTMENT_DOWN - 0.75).abs() < f64::EPSILON);
}

#[test]
fn test_solve_time_clamps() {
    // TARGET / 2 and TARGET × 2
    assert_eq!(consensus::MIN_SOLVE_TIME, 30);
    assert_eq!(consensus::MAX_SOLVE_TIME, 120);
}

#[test]
fn test_difficulty_floor_and_ceiling() {
    assert_eq!(consensus::MIN_DIFFICULTY, 1_000);
    assert_eq!(consensus::MAX_DIFFICULTY, u64::MAX / 1_000);
    assert!(consensus::MIN_DIFFICULTY < consensus::MAX_DIFFICULTY);
}

#[test]
fn test_coinbase_maturity() {
    // 100 blocks ≈ 100 minutes
    assert_eq!(validation::COINBASE_MATURITY, 100);
}

#[test]
fn test_timestamp_drift_limit() {
    // 7200 seconds = 2 hours (same as Bitcoin)
    assert_eq!(validation::MAX_TIMESTAMP_DRIFT, 7200);
}

// ═══════════════════════════════════════════════════════════════════════════
// 3. Network identity — ports, magic, genesis timestamps
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_network_ports_differ_between_networks() {
    // Testnet and mainnet must use different ports
    assert_ne!(
        NetworkType::Testnet.default_p2p_port(),
        NetworkType::Mainnet.default_p2p_port()
    );
    assert_ne!(
        NetworkType::Testnet.default_rpc_port(),
        NetworkType::Mainnet.default_rpc_port()
    );
}

#[test]
fn test_testnet_ports() {
    assert_eq!(NetworkType::Testnet.default_p2p_port(), 8334);
    assert_eq!(NetworkType::Testnet.default_rpc_port(), 8444);
}

#[test]
fn test_mainnet_ports() {
    assert_eq!(NetworkType::Mainnet.default_p2p_port(), 8333);
    assert_eq!(NetworkType::Mainnet.default_rpc_port(), 8443);
}

#[test]
fn test_magic_bytes_differ() {
    assert_ne!(NetworkType::Testnet.magic(), NetworkType::Mainnet.magic());
    assert_eq!(NetworkType::Testnet.magic(), "ZION-TESTNET-V1");
    assert_eq!(NetworkType::Mainnet.magic(), "ZION-MAINNET-V1");
}

#[test]
fn test_genesis_timestamps_differ() {
    assert_ne!(
        NetworkType::Testnet.genesis_timestamp(),
        NetworkType::Mainnet.genesis_timestamp()
    );
}

#[test]
fn test_testnet_genesis_timestamp() {
    // Feb 8, 2026 12:00:00 UTC
    assert_eq!(NetworkType::Testnet.genesis_timestamp(), 1_770_552_000);
}

#[test]
fn test_mainnet_genesis_timestamp() {
    // Jan 1, 2024 00:00:00 UTC
    assert_eq!(NetworkType::Mainnet.genesis_timestamp(), 1_704_067_200);
}

#[test]
fn test_network_names() {
    assert_eq!(NetworkType::Testnet.name(), "testnet");
    assert_eq!(NetworkType::Mainnet.name(), "mainnet");
    assert_eq!(format!("{}", NetworkType::Testnet), "testnet");
    assert_eq!(format!("{}", NetworkType::Mainnet), "mainnet");
}

#[test]
fn test_network_parsing_case_insensitive() {
    assert_eq!(NetworkType::from_str("testnet").unwrap(), NetworkType::Testnet);
    assert_eq!(NetworkType::from_str("TESTNET").unwrap(), NetworkType::Testnet);
    assert_eq!(NetworkType::from_str("Mainnet").unwrap(), NetworkType::Mainnet);
    assert_eq!(NetworkType::from_str("MAIN").unwrap(), NetworkType::Mainnet);
    assert_eq!(NetworkType::from_str("test").unwrap(), NetworkType::Testnet);
    assert!(NetworkType::from_str("invalid").is_err());
    assert!(NetworkType::from_str("").is_err());
}

// ═══════════════════════════════════════════════════════════════════════════
// 4. Cross-module consistency
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_reward_and_premine_supply_constants_match() {
    // reward.rs and premine.rs define these independently — they MUST agree
    assert_eq!(reward::TOTAL_SUPPLY, premine::TOTAL_SUPPLY);
    assert_eq!(reward::GENESIS_PREMINE, premine::PREMINE_TOTAL);
    assert_eq!(reward::MINING_EMISSION, premine::MINING_EMISSION);
}

#[test]
fn test_genesis_has_no_mining_reward() {
    // Height 0 = genesis block, reward must be 0
    assert_eq!(reward::calculate(0, 1000), 0);
    assert_eq!(reward::calculate(0, 0), 0);
    assert_eq!(reward::calculate(0, u64::MAX), 0);
}

#[test]
fn test_emission_complete_after_last_block() {
    assert_eq!(reward::calculate(reward::TOTAL_MINING_BLOCKS, 1000), reward::BLOCK_REWARD_ATOMIC);
    assert_eq!(reward::calculate(reward::TOTAL_MINING_BLOCKS + 1, 1000), 0);
}

#[test]
fn test_difficulty_does_not_affect_reward() {
    let r1 = reward::calculate(1, 1);
    let r2 = reward::calculate(1, 1_000_000);
    let r3 = reward::calculate(1, u64::MAX);
    assert_eq!(r1, r2);
    assert_eq!(r2, r3);
}
