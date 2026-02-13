/// ZION Fee Market — Fee Burning Model
///
/// Design principles:
/// 1. **All transaction fees are burned** — they are NOT paid to the miner.
///    The coinbase output is capped at the block reward; fees are destroyed.
/// 2. Minimum fee prevents dust spam and zero-cost DoS.
/// 3. Fee rate = fee / tx_size_bytes — mempool sorts by highest fee rate.
/// 4. Total supply is *deflationary* over time thanks to fee burning.
///
/// Fee burning rationale:
///   - Miner incentive comes from the constant 5,400.067 ZION block reward.
///   - Burning prevents miner-extractable fee games (MEV).
///   - Long-term: as emission ends (2071), burned supply creates scarcity.
///
/// All values in atomic units (1 ZION = 1,000,000 atomic units).

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Minimum transaction fee: 0.001 ZION = 1,000 atomic units
///
/// This is the absolute floor — any transaction with fee < MIN_TX_FEE
/// is rejected by both mempool and block validation.
pub const MIN_TX_FEE: u64 = 1_000;

/// Minimum fee rate: 1 atomic unit per byte
///
/// For a typical 250-byte transaction this means ≥ 250 atomic units (0.00025 ZION).
/// The effective minimum is max(MIN_TX_FEE, size_bytes × MIN_FEE_RATE).
pub const MIN_FEE_RATE: u64 = 1;

/// Maximum transaction size: 100 KB
///
/// Larger transactions are rejected. This limits resource usage and bounds
/// the maximum absolute fee needed (100,000 × MIN_FEE_RATE = 100,000 atomic).
pub const MAX_TX_SIZE_BYTES: usize = 100_000;

/// Maximum total output amount per transaction: equal to total supply.
///
/// No single transaction output can exceed the total supply. This prevents
/// overflow exploits and clearly invalid amounts.
pub const MAX_OUTPUT_AMOUNT: u64 = 144_000_000_000 * 1_000_000; // 144B × 10^6

// ---------------------------------------------------------------------------
// Fee Calculation
// ---------------------------------------------------------------------------

/// Estimate the serialized size of a transaction (in bytes).
///
/// Rough formula:
///   - Base: 8 (id_overhead) + 4 (version) + 8 (fee) + 8 (timestamp) = 28
///   - Per input: 64 (prev_hash) + 4 (index) + 64 (signature) + 64 (pubkey) = 196
///   - Per output: 8 (amount) + 64 (address) = 72
pub fn estimate_tx_size(num_inputs: usize, num_outputs: usize) -> usize {
    28 + num_inputs * 196 + num_outputs * 72
}

/// Calculate the fee rate (atomic units per byte).
///
/// Returns 0 if tx_size is 0 (should never happen for valid transactions).
pub fn fee_rate(fee: u64, tx_size_bytes: usize) -> u64 {
    if tx_size_bytes == 0 {
        return 0;
    }
    fee / tx_size_bytes as u64
}

/// Calculate the minimum required fee for a transaction of the given size.
///
/// Returns max(MIN_TX_FEE, size × MIN_FEE_RATE).
pub fn minimum_fee_for_size(tx_size_bytes: usize) -> u64 {
    let rate_based = tx_size_bytes as u64 * MIN_FEE_RATE;
    rate_based.max(MIN_TX_FEE)
}

// ---------------------------------------------------------------------------
// Fee Validation
// ---------------------------------------------------------------------------

/// Validate that a transaction's fee meets minimum requirements.
///
/// Returns Ok(()) if the fee is sufficient, or an error string explaining why not.
pub fn validate_fee(fee: u64, tx_size_bytes: usize) -> Result<(), String> {
    // 1. Absolute minimum
    if fee < MIN_TX_FEE {
        return Err(format!(
            "Transaction fee {} is below minimum {} atomic units (0.001 ZION)",
            fee, MIN_TX_FEE
        ));
    }

    // 2. Fee rate minimum
    let min_for_size = minimum_fee_for_size(tx_size_bytes);
    if fee < min_for_size {
        return Err(format!(
            "Transaction fee {} is below minimum for size {} bytes (need at least {})",
            fee, tx_size_bytes, min_for_size
        ));
    }

    Ok(())
}

/// Validate that all transaction outputs are within bounds.
///
/// - No output can be zero.
/// - No output can exceed MAX_OUTPUT_AMOUNT.
/// - Total outputs cannot exceed MAX_OUTPUT_AMOUNT.
pub fn validate_output_amounts(outputs: &[(u64, &str)]) -> Result<(), String> {
    let mut total: u128 = 0;

    for (i, (amount, _addr)) in outputs.iter().enumerate() {
        if *amount == 0 {
            return Err(format!("Output {} has zero amount", i));
        }
        if *amount > MAX_OUTPUT_AMOUNT {
            return Err(format!(
                "Output {} amount {} exceeds maximum {}",
                i, amount, MAX_OUTPUT_AMOUNT
            ));
        }
        total += *amount as u128;
    }

    if total > MAX_OUTPUT_AMOUNT as u128 {
        return Err(format!(
            "Total output amount {} exceeds maximum {}",
            total, MAX_OUTPUT_AMOUNT
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Fee Burning
// ---------------------------------------------------------------------------

/// Calculate the maximum allowed coinbase output for a block.
///
/// **Fees are burned** — the coinbase may only contain the block reward.
/// The miner does NOT receive transaction fees.
///
/// coinbase_max = block_reward (no fee component)
pub fn max_coinbase_output(block_height: u64) -> u64 {
    use super::reward;
    reward::calculate(block_height, 0)
}

/// Calculate total fees burned in a block.
///
/// Sum of all transaction fees (excluding coinbase) in the block.
/// These fees are destroyed — they reduce the effective circulating supply.
pub fn total_fees_burned(transactions: &[crate::tx::Transaction]) -> u64 {
    // Skip coinbase (index 0)
    transactions.iter().skip(1).map(|tx| tx.fee).sum()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_min_fee_constant() {
        // 0.001 ZION = 1,000 atomic units
        assert_eq!(MIN_TX_FEE, 1_000);
    }

    #[test]
    fn test_fee_rate_calculation() {
        assert_eq!(fee_rate(1000, 250), 4); // 1000/250 = 4
        assert_eq!(fee_rate(500, 250), 2);  // 500/250 = 2
        assert_eq!(fee_rate(100, 250), 0);  // 100/250 = 0 (integer division)
        assert_eq!(fee_rate(0, 250), 0);
        assert_eq!(fee_rate(1000, 0), 0);   // Edge case: zero size
    }

    #[test]
    fn test_minimum_fee_for_size() {
        // Small tx: MIN_TX_FEE dominates
        assert_eq!(minimum_fee_for_size(100), MIN_TX_FEE); // 100 < 1000
        assert_eq!(minimum_fee_for_size(500), MIN_TX_FEE); // 500 < 1000

        // Large tx: rate-based dominates
        assert_eq!(minimum_fee_for_size(2000), 2000); // 2000 > 1000
        assert_eq!(minimum_fee_for_size(10_000), 10_000);
    }

    #[test]
    fn test_estimate_tx_size() {
        // 1 input, 2 outputs (typical send with change)
        let size = estimate_tx_size(1, 2);
        assert_eq!(size, 28 + 196 + 144); // = 368 bytes
        assert_eq!(size, 368);

        // 2 inputs, 1 output (consolidation)
        let size = estimate_tx_size(2, 1);
        assert_eq!(size, 28 + 392 + 72); // = 492 bytes

        // 0 inputs, 0 outputs (degenerate, would be rejected elsewhere)
        assert_eq!(estimate_tx_size(0, 0), 28);
    }

    #[test]
    fn test_validate_fee_ok() {
        // 1000 atomic fee, 250 byte tx → ok (1000 >= 1000 min, 1000 >= 250 rate-based)
        assert!(validate_fee(1_000, 250).is_ok());

        // Generous fee
        assert!(validate_fee(100_000, 500).is_ok());
    }

    #[test]
    fn test_validate_fee_too_low() {
        // Below absolute minimum
        assert!(validate_fee(999, 250).is_err());
        assert!(validate_fee(0, 250).is_err());
    }

    #[test]
    fn test_validate_fee_rate_too_low() {
        // Fee meets absolute min but not rate-based for large tx
        // 1000 fee, 2000 byte tx → need 2000 atomic minimum
        assert!(validate_fee(1_000, 2_000).is_err());
    }

    #[test]
    fn test_validate_output_amounts_ok() {
        let outputs = vec![
            (1_000_000u64, "addr1"),     // 1 ZION
            (5_000_000u64, "addr2"),     // 5 ZION
        ];
        assert!(validate_output_amounts(&outputs).is_ok());
    }

    #[test]
    fn test_validate_output_zero() {
        let outputs = vec![
            (0u64, "addr1"),
        ];
        assert!(validate_output_amounts(&outputs).is_err());
    }

    #[test]
    fn test_validate_output_exceeds_max() {
        let outputs = vec![
            (MAX_OUTPUT_AMOUNT + 1, "addr1"),
        ];
        assert!(validate_output_amounts(&outputs).is_err());
    }

    #[test]
    fn test_validate_output_total_exceeds_max() {
        // Each output is within bounds, but sum exceeds max
        let half = MAX_OUTPUT_AMOUNT / 2 + 1;
        let outputs = vec![
            (half, "addr1"),
            (half, "addr2"),
        ];
        assert!(validate_output_amounts(&outputs).is_err());
    }

    #[test]
    fn test_max_coinbase_output() {
        // Block 1: should equal block reward
        let max = max_coinbase_output(1);
        assert_eq!(max, 5_400_067_000);

        // Genesis: no coinbase reward
        assert_eq!(max_coinbase_output(0), 0);

        // Post-emission: no coinbase
        assert_eq!(max_coinbase_output(23_652_001), 0);
    }

    #[test]
    fn test_fees_are_burned_not_added_to_coinbase() {
        // The max coinbase output does NOT include fees
        // This is the key test — proving fee burning
        let block_reward = super::super::reward::calculate(1, 0);
        let max_cb = max_coinbase_output(1);
        assert_eq!(max_cb, block_reward); // coinbase = reward only, no fees
    }

    #[test]
    fn test_total_fees_burned() {
        use crate::tx::{Transaction, TxOutput};

        let coinbase = Transaction {
            id: "cb".to_string(),
            version: 1,
            inputs: vec![],
            outputs: vec![TxOutput { amount: 5_400_067_000, address: "miner".to_string() }],
            fee: 0,
            timestamp: 0,
        };
        let tx1 = Transaction {
            id: "t1".to_string(),
            version: 1,
            inputs: vec![],
            outputs: vec![],
            fee: 5_000,
            timestamp: 0,
        };
        let tx2 = Transaction {
            id: "t2".to_string(),
            version: 1,
            inputs: vec![],
            outputs: vec![],
            fee: 3_000,
            timestamp: 0,
        };

        let txs = vec![coinbase, tx1, tx2];
        assert_eq!(total_fees_burned(&txs), 8_000);
    }

    #[test]
    fn test_max_tx_size() {
        assert_eq!(MAX_TX_SIZE_BYTES, 100_000);
    }

    #[test]
    fn test_max_output_amount_equals_total_supply() {
        assert_eq!(MAX_OUTPUT_AMOUNT, 144_000_000_000_000_000);
    }
}
