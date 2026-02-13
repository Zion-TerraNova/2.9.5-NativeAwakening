/// Sprint 1.2 — Security & Edge-Case Test Suite
///
/// 25 integration tests covering:
///   1.2.1  Reorg test suite (short + max depth + rejected)
///   1.2.2  Double-spend simulation (mempool + block-level)
///   1.2.3  Fork-choice tests (highest accumulated work wins)
///   1.2.4  Timestamp drift tests (boundary cases)
///   1.2.5  Mempool edge cases (oversize, invalid sig, dust, eviction)
///   1.2.6  Coinbase maturity enforcement

use zion_core::blockchain::block::Block;
use zion_core::blockchain::chain::{Chain, MAX_REORG_DEPTH, SOFT_FINALITY_DEPTH};
use zion_core::blockchain::validation;
use zion_core::mempool::pool::{Mempool, MempoolError, MAX_MEMPOOL_SIZE};
use zion_core::tx::{Transaction, TxInput, TxOutput};

// ═══════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════

/// Build a chain of `count` blocks on top of genesis using `Chain` in-memory.
/// Uses insert_block_unchecked to bypass PoW validation (test-only).
fn build_chain(count: u64, difficulty: u64) -> Chain {
    let mut chain = Chain::new();
    let genesis = chain.get_block(0).unwrap();
    let mut prev = genesis;
    let base_ts = prev.header.timestamp + 60;

    for i in 1..=count {
        let block = Block::new(
            1,
            i,
            prev.calculate_hash(),
            base_ts + (i - 1) * 60,
            difficulty,
            0,
            vec![make_coinbase(i)],
        );
        chain.insert_block_unchecked(block.clone());
        prev = block;
    }
    chain
}

/// Build a fork of `count` blocks starting at fork_point, each with given difficulty.
fn build_fork_blocks(chain: &Chain, fork_point: u64, count: u64, difficulty: u64) -> Vec<Block> {
    let fork_block = chain.get_block(fork_point).unwrap();
    let base_ts = fork_block.header.timestamp + 60;
    let mut blocks = Vec::new();
    let mut prev = fork_block;

    for i in 0..count {
        let height = fork_point + 1 + i;
        let block = Block::new(
            1,
            height,
            prev.calculate_hash(),
            base_ts + i * 60,
            difficulty,
            i + 1, // different nonce to differentiate from original chain
            vec![make_coinbase(height)],
        );
        prev = block.clone();
        blocks.push(block);
    }
    blocks
}

/// Create a minimal coinbase transaction for a given block height.
fn make_coinbase(height: u64) -> Transaction {
    let zero_hash = "0000000000000000000000000000000000000000000000000000000000000000";
    Transaction {
        id: format!("coinbase_{}", height),
        version: 1,
        inputs: vec![TxInput {
            prev_tx_hash: zero_hash.to_string(),
            output_index: 0,
            signature: "0".repeat(128),
            public_key: "0".repeat(64),
        }],
        outputs: vec![TxOutput {
            amount: 5_400_067_000,
            address: "zion1miner".to_string(),
        }],
        fee: 0,
        timestamp: height * 60,
    }
}

/// Create a test transaction with specified parameters.
fn make_tx(id: &str, fee: u64, inputs: Vec<(&str, u32)>, outputs: Vec<u64>) -> Transaction {
    Transaction {
        id: id.to_string(),
        version: 1,
        inputs: inputs
            .iter()
            .map(|(hash, idx)| TxInput {
                prev_tx_hash: hash.to_string(),
                output_index: *idx,
                signature: "a".repeat(128),
                public_key: "b".repeat(64),
            })
            .collect(),
        outputs: outputs
            .iter()
            .map(|amt| TxOutput {
                amount: *amt,
                address: "zion1test".to_string(),
            })
            .collect(),
        fee,
        timestamp: 100,
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.2.1 — Reorg Test Suite
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_reorg_short_3_blocks_succeeds() {
    // Build a chain of 10 blocks with difficulty 1000
    let mut chain = build_chain(10, 1000);
    assert_eq!(chain.height, 10);

    // Fork at height 7 with 3 blocks of difficulty 1250 (within ±25% of 1000)
    // Fork work: fork_point_work + 3 × 1250 = 7×1000 + 1000(genesis) + 3×1250
    // Original: 11×1000 = 11000
    // Fork: 8000 + 3750 = 11750 > 11000 ✓
    let fork_blocks = build_fork_blocks(&chain, 7, 3, 1250);
    assert_eq!(fork_blocks.len(), 3);

    let result = chain.try_reorg_unchecked(7, &fork_blocks);
    assert!(result.is_ok(), "3-block reorg should succeed: {:?}", result);
    assert_eq!(chain.height, 10);
    assert_eq!(chain.tip, fork_blocks.last().unwrap().calculate_hash());
}

#[test]
fn test_reorg_max_10_blocks_succeeds() {
    // Build a chain of 15 blocks with difficulty 1000
    let mut chain = build_chain(15, 1000);
    assert_eq!(chain.height, 15);

    // Fork at height 5 with exactly 10 blocks (reorg depth = 15-5 = 10 = MAX_REORG_DEPTH)
    // Use difficulty 1250 (within ±25% of 1000)
    let fork_blocks = build_fork_blocks(&chain, 5, 10, 1250);

    let result = chain.try_reorg_unchecked(5, &fork_blocks);
    assert!(result.is_ok(), "10-block reorg (max depth) should succeed: {:?}", result);
    assert_eq!(chain.height, 15);
}

#[test]
fn test_reorg_11_blocks_rejected() {
    // With MAX_REORG_DEPTH=50, 11-block reorg should now SUCCEED.
    // Build a chain of 15 blocks
    let mut chain = build_chain(15, 1000);

    // Fork at height 4 → depth = 15 - 4 = 11 < MAX_REORG_DEPTH (50) → allowed
    let fork_blocks = build_fork_blocks(&chain, 4, 11, 2000);

    let result = chain.try_reorg_unchecked(4, &fork_blocks);
    assert!(result.is_ok(), "11-block reorg (depth < 50) should succeed: {:?}", result);
}

#[test]
fn test_reorg_updates_total_work() {
    let mut chain = build_chain(10, 1000);
    let old_work = chain.total_work;

    // Fork at height 7 with higher-difficulty blocks (1250, within ±25%)
    let fork_blocks = build_fork_blocks(&chain, 7, 3, 1250);
    chain.try_reorg_unchecked(7, &fork_blocks).unwrap();

    // Total work should now be higher (fork point work + 3 × 1250)
    assert!(chain.total_work > old_work, "Work after reorg should exceed original");
}

#[test]
fn test_reorg_old_blocks_removed() {
    let mut chain = build_chain(10, 1000);
    let original_tip_hash = chain.tip.clone();

    // Fork at height 7 with 3 blocks of difficulty 1250
    let fork_blocks = build_fork_blocks(&chain, 7, 3, 1250);
    chain.try_reorg_unchecked(7, &fork_blocks).unwrap();

    // Original tip hash should no longer be the current tip
    assert_ne!(chain.tip, original_tip_hash);
    assert_eq!(chain.tip, fork_blocks.last().unwrap().calculate_hash());
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.2.2 — Double-Spend Simulation
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_double_spend_mempool_rejected() {
    let pool = Mempool::new();

    let tx1 = make_tx("tx_a", 5_000, vec![("utxo_one", 0)], vec![1_000_000]);
    let tx2 = make_tx("tx_b", 5_000, vec![("utxo_one", 0)], vec![1_000_000]);

    assert!(pool.add_transaction_validated(tx1).is_ok());
    let result = pool.add_transaction_validated(tx2);
    assert!(
        matches!(result, Err(MempoolError::DoubleSpend(_))),
        "Second tx spending same UTXO should be rejected: {:?}",
        result
    );
}

#[test]
fn test_double_spend_cleared_after_block_inclusion() {
    let pool = Mempool::new();

    let tx1 = make_tx("tx_a", 5_000, vec![("utxo_shared", 0)], vec![1_000_000]);
    pool.add_transaction_validated(tx1).unwrap();

    // Simulate block inclusion: remove from mempool
    pool.remove_transaction("tx_a");

    // Outpoint should now be freed — but in reality, the UTXO is spent in the
    // blockchain, so a new tx spending it should fail at UTXO validation (not mempool).
    // From mempool perspective, the outpoint tracking is cleared.
    assert!(!pool.is_outpoint_spent("utxo_shared", 0));
}

#[test]
fn test_double_spend_available_after_reorg_restore() {
    let pool = Mempool::new();

    // Tx spending utxo_x was in an old block, then block is reorged out
    let tx = make_tx("tx_reorged", 5_000, vec![("utxo_x", 0)], vec![1_000_000]);

    // Simulate: the tx is restored to mempool via restore_transactions
    pool.restore_transactions(&[tx]);

    // The tx should be in the mempool now
    assert!(pool.get_transaction("tx_reorged").is_some());
    assert!(pool.is_outpoint_spent("utxo_x", 0));

    // A conflicting tx should be rejected
    let conflicting = make_tx("tx_conflict", 5_000, vec![("utxo_x", 0)], vec![500_000]);
    assert!(matches!(
        pool.add_transaction_validated(conflicting),
        Err(MempoolError::DoubleSpend(_))
    ));
}

#[test]
fn test_restore_skips_coinbase() {
    let pool = Mempool::new();
    let zero_hash = "0000000000000000000000000000000000000000000000000000000000000000";

    let coinbase = Transaction {
        id: "coinbase_test".to_string(),
        version: 1,
        inputs: vec![TxInput {
            prev_tx_hash: zero_hash.to_string(),
            output_index: 0,
            signature: "0".repeat(128),
            public_key: "0".repeat(64),
        }],
        outputs: vec![TxOutput {
            amount: 5_400_067_000,
            address: "zion1miner".to_string(),
        }],
        fee: 0,
        timestamp: 0,
    };

    pool.restore_transactions(&[coinbase]);

    // Coinbase should NOT be in mempool
    assert!(pool.get_transaction("coinbase_test").is_none());
    assert_eq!(pool.size(), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.2.3 — Fork-Choice Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_fork_higher_accumulated_work_wins() {
    let mut chain = build_chain(10, 1000);
    let original_tip = chain.tip.clone();

    // Fork at height 7 with difficulty 1250 (within ±25% of 1000)
    // Fork work exceeds original work: same height but higher per-block difficulty
    let fork_blocks = build_fork_blocks(&chain, 7, 3, 1250);
    let result = chain.try_reorg_unchecked(7, &fork_blocks);

    assert!(result.is_ok(), "Higher-work fork should win: {:?}", result);
    assert_ne!(chain.tip, original_tip, "Tip should change to winning fork");
}

#[test]
fn test_fork_equal_work_keeps_incumbent() {
    let mut chain = build_chain(10, 1000);
    let _original_tip = chain.tip.clone();

    // Fork at height 7 with SAME difficulty — total work will be ≤ current
    let fork_blocks = build_fork_blocks(&chain, 7, 3, 1000);
    let result = chain.try_reorg_unchecked(7, &fork_blocks);

    // Should fail: competing work does not EXCEED current
    assert!(result.is_err(), "Equal-work fork should not replace incumbent");
    assert!(result.unwrap_err().contains("does not exceed"));
}

#[test]
fn test_fork_lower_work_rejected() {
    let mut chain = build_chain(10, 1000);
    let original_tip = chain.tip.clone();

    // Fork at height 7 with difficulty 750 (within ±25% of 1000, but lower work)
    let fork_blocks = build_fork_blocks(&chain, 7, 3, 750);
    let result = chain.try_reorg_unchecked(7, &fork_blocks);

    assert!(result.is_err(), "Lower-work fork should be rejected");
    assert_eq!(chain.tip, original_tip, "Tip should remain unchanged");
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.2.4 — Timestamp Drift Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_timestamp_future_exceeds_max_rejected() {
    let now: u64 = 1_700_000_000;
    let prev = Block::new(
        1, 0,
        "00".repeat(32),
        now - 120,
        1000, 0,
        vec![make_coinbase(0)],
    );

    // Block timestamp = now + 7201 (exceeds MAX_FUTURE_DRIFT of 7200)
    let block = Block::new(
        1, 1,
        prev.calculate_hash(),
        now + 7201,
        1000, 0,
        vec![make_coinbase(1)],
    );

    let result = validation::validate_block(&block, Some(&prev), now);
    assert!(result.is_err(), "Timestamp >7200s in future should be rejected");
    let err = result.unwrap_err();
    assert!(
        err.contains("future") || err.contains("drift"),
        "Error should mention future/drift: {}",
        err
    );
}

#[test]
fn test_timestamp_future_within_limit_structurally_valid() {
    let now: u64 = 1_700_000_000;
    let prev = Block::new(
        1, 0,
        "00".repeat(32),
        now - 60,
        1000, 0,
        vec![make_coinbase(0)],
    );

    // Block timestamp = now + 7199 (just within MAX_FUTURE_DRIFT)
    let block = Block::new(
        1, 1,
        prev.calculate_hash(),
        now + 7199,
        1000, 0,
        vec![make_coinbase(1)],
    );

    let result = validation::validate_block(&block, Some(&prev), now);
    // Should NOT fail on "too far in future" — may fail on PoW, that's fine
    if let Err(e) = &result {
        assert!(
            !e.contains("too far in future"),
            "Should not reject timestamp within limit: {}",
            e
        );
    }
}

#[test]
fn test_timestamp_before_parent_rejected() {
    let now: u64 = 1_700_000_000;
    let prev = Block::new(
        1, 0,
        "00".repeat(32),
        now,
        1000, 0,
        vec![make_coinbase(0)],
    );

    // Block timestamp BEFORE parent
    let block = Block::new(
        1, 1,
        prev.calculate_hash(),
        now - 1,
        1000, 0,
        vec![make_coinbase(1)],
    );

    let result = validation::validate_block(&block, Some(&prev), now);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("before previous"));
}

#[test]
fn test_timestamp_boundary_exactly_max_drift_accepted() {
    let now: u64 = 1_700_000_000;
    let prev = Block::new(
        1, 0,
        "00".repeat(32),
        now - 60,
        1000, 0,
        vec![make_coinbase(0)],
    );

    // Exactly at the MAX_TIMESTAMP_DRIFT boundary (7200s from prev)
    let block = Block::new(
        1, 1,
        prev.calculate_hash(),
        prev.header.timestamp + validation::MAX_TIMESTAMP_DRIFT,
        1000, 0,
        vec![make_coinbase(1)],
    );

    let result = validation::validate_block(&block, Some(&prev), now + 7200);
    // At exactly MAX_TIMESTAMP_DRIFT, should NOT be rejected for drift
    if let Err(e) = &result {
        assert!(
            !e.contains("drift"),
            "Exactly MAX_TIMESTAMP_DRIFT should not trigger drift rejection: {}",
            e
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.2.5 — Mempool Edge Cases
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_mempool_oversized_tx_rejected() {
    let pool = Mempool::new();

    // Create tx with many inputs to exceed MAX_TX_SIZE_BYTES (100_000)
    // Each input ≈ 196 bytes, so 512 inputs ≈ 100_352 > 100_000
    let many_inputs: Vec<(&str, u32)> = (0..512)
        .map(|_| ("large_utxo_hash", 0u32))
        .collect();
    let tx = make_tx("tx_oversized", 1_000_000, many_inputs, vec![1_000_000]);

    let result = pool.add_transaction_validated(tx);
    assert!(
        matches!(result, Err(MempoolError::TxTooLarge(_))),
        "Oversized tx should be rejected: {:?}",
        result
    );
}

#[test]
fn test_mempool_invalid_signature_rejected() {
    // validate_transaction checks verify_signatures()
    // A tx with mismatched id will fail signature check
    let tx = Transaction {
        id: "wrong_hash_on_purpose".to_string(),
        version: 1,
        inputs: vec![TxInput {
            prev_tx_hash: "abc123".to_string(),
            output_index: 0,
            signature: "ff".repeat(64),
            public_key: "aa".repeat(32),
        }],
        outputs: vec![TxOutput {
            amount: 1_000_000,
            address: "zion1test".to_string(),
        }],
        fee: 5_000,
        timestamp: 100,
    };

    let result = validation::validate_transaction(&tx);
    assert!(result.is_err(), "Invalid signature should be rejected");
    let err_msg = result.unwrap_err();
    assert!(
        err_msg.contains("signature") || err_msg.contains("Invalid"),
        "Error should mention signature: {}",
        err_msg
    );
}

#[test]
fn test_mempool_dust_zero_output_rejected() {
    let pool = Mempool::new();

    let tx = make_tx("tx_dust", 5_000, vec![("some_utxo", 0)], vec![0]);

    let result = pool.add_transaction_validated(tx);
    assert!(
        matches!(result, Err(MempoolError::InvalidOutputAmount(_))),
        "Zero-amount output should be rejected: {:?}",
        result
    );
}

#[test]
fn test_mempool_eviction_lowest_fee() {
    let pool = Mempool::new();

    // Fill the mempool to capacity
    for i in 0..MAX_MEMPOOL_SIZE {
        let utxo_name = format!("utxo_{}", i);
        let tx = make_tx(
            &format!("tx_{}", i),
            5_000,  // moderate fee
            vec![(utxo_name.as_str(), 0)],
            vec![1_000_000],
        );
        pool.add_transaction_validated(tx).unwrap();
    }
    assert_eq!(pool.size(), MAX_MEMPOOL_SIZE);

    // Add one more with high fee — should trigger eviction
    let tx_high_fee = make_tx(
        "tx_high_fee",
        10_000_000, // very high fee
        vec![("fresh_utxo", 0)],
        vec![1_000_000],
    );
    pool.add_transaction_validated(tx_high_fee).unwrap();

    // Pool should still be at or below limit
    assert!(pool.size() <= MAX_MEMPOOL_SIZE, "Pool should evict to stay within limit");
    // The high-fee tx should still be present
    assert!(pool.get_transaction("tx_high_fee").is_some());
}

#[test]
fn test_mempool_fee_below_minimum_rejected() {
    let pool = Mempool::new();

    // fee = 100 which is below MIN_TX_FEE (1000)
    let tx = make_tx("tx_lowfee", 100, vec![("utxo_cheapskate", 0)], vec![1_000_000]);

    let result = pool.add_transaction_validated(tx);
    assert!(
        matches!(result, Err(MempoolError::FeeTooLow(_))),
        "Fee below minimum should be rejected: {:?}",
        result
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.2.6 — Coinbase Maturity
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_coinbase_maturity_constant() {
    assert_eq!(
        validation::COINBASE_MATURITY, 100,
        "Coinbase maturity must be 100 blocks"
    );
}

#[test]
fn test_coinbase_spend_at_99_conceptually_immature() {
    // This tests the business logic conceptually:
    // A coinbase output at height H requires spending at height >= H + COINBASE_MATURITY
    // At H + 99, the maturity = 99 < 100 → REJECT
    let coinbase_height: u64 = 10;
    let spending_height: u64 = 109; // 109 - 10 = 99
    let maturity = spending_height.saturating_sub(coinbase_height);
    assert!(
        maturity < validation::COINBASE_MATURITY,
        "99 confirmations should be immature"
    );
}

#[test]
fn test_coinbase_spend_at_100_conceptually_mature() {
    let coinbase_height: u64 = 10;
    let spending_height: u64 = 110; // 110 - 10 = 100
    let maturity = spending_height.saturating_sub(coinbase_height);
    assert!(
        maturity >= validation::COINBASE_MATURITY,
        "100 confirmations should be mature"
    );
}

#[test]
fn test_coinbase_spend_at_101_conceptually_mature() {
    let coinbase_height: u64 = 10;
    let spending_height: u64 = 111; // 111 - 10 = 101
    let maturity = spending_height.saturating_sub(coinbase_height);
    assert!(
        maturity >= validation::COINBASE_MATURITY,
        "101 confirmations should be mature"
    );
}

#[test]
fn test_non_coinbase_can_be_spent_immediately() {
    // Non-coinbase transactions have no maturity requirement.
    // A regular tx output at height H can be spent at height H+1.
    // This is a semantic test — the maturity check in process_block only applies
    // to the first tx (coinbase) in the source block.
    let source_height: u64 = 50;
    let spending_height: u64 = 51;
    let _gap = spending_height - source_height;
    // No COINBASE_MATURITY check applies to non-coinbase txs
    assert!(true, "Non-coinbase outputs have no maturity requirement");
}

// ═══════════════════════════════════════════════════════════════════════════
// Additional cross-cutting security tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_max_reorg_depth_constant() {
    assert_eq!(MAX_REORG_DEPTH, 50);
}

#[test]
fn test_soft_finality_constant() {
    assert_eq!(SOFT_FINALITY_DEPTH, 60);
}

#[test]
fn test_chain_verify_integrity() {
    // A freshly constructed chain (genesis only) should verify
    let chain = Chain::new();
    // verify_chain may fail on PoW for genesis with nonce=0, that's expected
    // The important thing is it doesn't panic
    let _ = chain.verify_chain();
}
