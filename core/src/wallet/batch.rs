/// ZION Wallet — Batch Transaction Builder
///
/// Builds a single transaction with multiple recipients (N outputs).
/// Used by the mining pool for efficient PPLNS payout distribution.
///
/// Instead of N separate transactions (each with own fee + UTXO overhead),
/// a single batch TX pays all miners in one go:
///
///   inputs: [pool_utxo_1, pool_utxo_2, ...]
///   outputs: [miner_1: 50 ZION, miner_2: 120 ZION, ..., change: remainder]
///   fee: auto-calculated based on size
///
/// Benefits:
/// - Lower total fees (1 tx vs N txs)
/// - Atomic: all payouts confirmed in one block
/// - Simpler tracking: one txid for entire payout round

use crate::tx::{Transaction, TxInput, TxOutput};
use crate::blockchain::fee;
use crate::crypto::{keys, to_hex};
use crate::wallet::{SpendableUtxo, WalletError};
use ed25519_dalek::{Signer, SigningKey};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single recipient in a batch payout.
#[derive(Debug, Clone)]
pub struct Recipient {
    /// Destination zion1 address
    pub address: String,
    /// Amount in atomic units
    pub amount: u64,
}

/// Parameters for building a batch transaction.
#[derive(Debug, Clone)]
pub struct BatchParams {
    /// List of recipients (miner payouts)
    pub recipients: Vec<Recipient>,
    /// Optional explicit fee (atomic units). If None, auto-calculated.
    pub fee: Option<u64>,
    /// Change address (pool wallet's own address)
    pub change_address: String,
}

/// Result of building a batch transaction.
#[derive(Debug, Clone)]
pub struct BatchResult {
    /// The signed, ready-to-broadcast transaction
    pub transaction: Transaction,
    /// Total input amount consumed
    pub total_input: u64,
    /// Total amount sent to all recipients (sum of outputs, excl. change)
    pub total_sent: u64,
    /// Fee paid (burned)
    pub fee: u64,
    /// Change returned to sender
    pub change: u64,
    /// Number of UTXOs consumed
    pub inputs_used: usize,
    /// Number of recipients paid
    pub recipients_paid: usize,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum recipients per batch transaction.
/// With MAX_TX_SIZE=100KB, each output ~72 bytes → ~1300 outputs max.
/// We cap at 200 for safety (leaving room for inputs + overhead).
pub const MAX_BATCH_RECIPIENTS: usize = 200;

/// Minimum payout amount: 10 ZION = 10_000_000 atomic
pub const MIN_PAYOUT_AMOUNT: u64 = 10_000_000;

// ---------------------------------------------------------------------------
// UTXO Selection for Batch
// ---------------------------------------------------------------------------

/// Select UTXOs to cover a batch payout (total_amount + fee).
/// Uses largest-first strategy (same as single tx).
fn select_utxos_for_batch(
    available: &[SpendableUtxo],
    total_amount: u64,
    num_recipients: usize,
    explicit_fee: Option<u64>,
) -> Result<(Vec<SpendableUtxo>, u64), WalletError> {
    if available.is_empty() {
        return Err(WalletError::NoUtxos);
    }

    let mut sorted: Vec<SpendableUtxo> = available.to_vec();
    sorted.sort_by(|a, b| b.amount.cmp(&a.amount));

    let mut selected: Vec<SpendableUtxo> = Vec::new();
    let mut total: u64 = 0;

    for utxo in &sorted {
        selected.push(utxo.clone());
        total += utxo.amount;

        // num_outputs = recipients + potentially 1 change
        let fee_no_change = estimate_batch_fee(selected.len(), num_recipients, explicit_fee);
        let fee_with_change = estimate_batch_fee(selected.len(), num_recipients + 1, explicit_fee);

        if total == total_amount + fee_no_change {
            // Exact match, no change needed
            return Ok((selected, fee_no_change));
        }
        if total >= total_amount + fee_with_change {
            // Enough to cover amount + fee + change output
            return Ok((selected, fee_with_change));
        }
    }

    let needed = total_amount + estimate_batch_fee(sorted.len(), num_recipients + 1, explicit_fee);
    Err(WalletError::InsufficientFunds {
        available: total,
        needed,
    })
}

/// Estimate fee for a batch transaction.
fn estimate_batch_fee(num_inputs: usize, num_outputs: usize, explicit_fee: Option<u64>) -> u64 {
    if let Some(f) = explicit_fee {
        return f;
    }
    let size = fee::estimate_tx_size(num_inputs, num_outputs);
    fee::minimum_fee_for_size(size)
}

// ---------------------------------------------------------------------------
// Batch Transaction Building
// ---------------------------------------------------------------------------

/// Build and sign a batch payout transaction.
///
/// Creates a single transaction paying multiple recipients at once.
/// All inputs are signed with the same secret key (pool wallet).
///
/// # Arguments
/// * `params` — batch parameters (recipients, fee, change address)
/// * `available_utxos` — pool wallet's spendable UTXOs
/// * `secret_key_bytes` — 32-byte Ed25519 secret key
///
/// # Returns
/// * `BatchResult` with the signed transaction and summary stats
pub fn build_and_sign_batch(
    params: &BatchParams,
    available_utxos: &[SpendableUtxo],
    secret_key_bytes: &[u8; 32],
) -> Result<BatchResult, WalletError> {
    // 1. Validate
    if params.recipients.is_empty() {
        return Err(WalletError::ZeroAmount);
    }
    if params.recipients.len() > MAX_BATCH_RECIPIENTS {
        return Err(WalletError::SigningError(format!(
            "Too many recipients: {} (max {})",
            params.recipients.len(),
            MAX_BATCH_RECIPIENTS
        )));
    }

    // Validate all addresses
    for r in &params.recipients {
        if !keys::is_valid_zion1_address(&r.address) {
            return Err(WalletError::InvalidAddress(r.address.clone()));
        }
        if r.amount == 0 {
            return Err(WalletError::ZeroAmount);
        }
    }
    if !keys::is_valid_zion1_address(&params.change_address) {
        return Err(WalletError::InvalidAddress(params.change_address.clone()));
    }

    // 2. Calculate total payout amount
    let total_payout: u64 = params.recipients.iter().map(|r| r.amount).sum();

    // 3. Select UTXOs
    let (selected, tx_fee) = select_utxos_for_batch(
        available_utxos,
        total_payout,
        params.recipients.len(),
        params.fee,
    )?;
    let total_input: u64 = selected.iter().map(|u| u.amount).sum();
    let change = total_input - total_payout - tx_fee;

    // 4. Validate fee
    let num_outputs = params.recipients.len() + if change > 0 { 1 } else { 0 };
    let tx_size = fee::estimate_tx_size(selected.len(), num_outputs);
    let min_required = fee::minimum_fee_for_size(tx_size);
    if tx_fee < min_required {
        return Err(WalletError::FeeTooLow {
            fee: tx_fee,
            minimum: min_required,
        });
    }

    // Validate total tx size
    if tx_size > fee::MAX_TX_SIZE_BYTES {
        return Err(WalletError::SigningError(format!(
            "Batch transaction too large: {} bytes (max {})",
            tx_size, fee::MAX_TX_SIZE_BYTES
        )));
    }

    // 5. Derive public key
    let signing_key = SigningKey::from_bytes(secret_key_bytes);
    let public_key_hex = to_hex(signing_key.verifying_key().as_bytes());

    // 6. Build outputs: all recipients first, then change
    let mut outputs: Vec<TxOutput> = params
        .recipients
        .iter()
        .map(|r| TxOutput {
            amount: r.amount,
            address: r.address.clone(),
        })
        .collect();

    if change > 0 {
        outputs.push(TxOutput {
            amount: change,
            address: params.change_address.clone(),
        });
    }

    // 7. Build inputs (empty signatures — filled after hash)
    let inputs: Vec<TxInput> = selected
        .iter()
        .map(|utxo| TxInput {
            prev_tx_hash: utxo.tx_hash.clone(),
            output_index: utxo.output_index,
            signature: String::new(),
            public_key: public_key_hex.clone(),
        })
        .collect();

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 8. Create unsigned transaction and compute hash
    let mut tx = Transaction {
        id: String::new(),
        version: 1,
        inputs,
        outputs,
        fee: tx_fee,
        timestamp,
    };

    let tx_hash = tx.calculate_hash();
    tx.id = tx_hash.clone();

    // 9. Sign each input
    let msg_bytes = keys::from_hex(&tx_hash)
        .ok_or_else(|| WalletError::SigningError("Failed to decode tx hash".into()))?;

    for input in &mut tx.inputs {
        let sig = signing_key.sign(&msg_bytes);
        input.signature = to_hex(&sig.to_bytes());
    }

    // 10. Self-verify
    if !tx.verify_signatures() {
        return Err(WalletError::SigningError("Self-verification failed".into()));
    }

    Ok(BatchResult {
        transaction: tx,
        total_input,
        total_sent: total_payout,
        fee: tx_fee,
        change,
        inputs_used: selected.len(),
        recipients_paid: params.recipients.len(),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn test_address(suffix: char) -> String {
        // Generate a valid checksummed address from a deterministic seed
        let seed = suffix as u8;
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
        keys::address_from_public_key_hex(&pk_hex)
    }

    fn pool_address_and_key() -> (String, [u8; 32]) {
        let secret = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);
        let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
        let addr = keys::address_from_public_key_hex(&pk_hex);
        (addr, secret)
    }

    fn make_utxo(tx_hash: &str, index: u32, amount: u64, address: &str) -> SpendableUtxo {
        SpendableUtxo {
            key: format!("{}:{}", tx_hash, index),
            tx_hash: tx_hash.to_string(),
            output_index: index,
            amount,
            address: address.to_string(),
        }
    }

    // --- Happy Path ---

    #[test]
    fn test_batch_single_recipient() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, 100_000_000, &pool_addr)];
        let params = BatchParams {
            recipients: vec![Recipient {
                address: test_address('a'),
                amount: 50_000_000,
            }],
            fee: Some(2_000),
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
        assert_eq!(result.total_sent, 50_000_000);
        assert_eq!(result.fee, 2_000);
        assert_eq!(result.change, 100_000_000 - 50_000_000 - 2_000);
        assert_eq!(result.recipients_paid, 1);
        assert!(result.transaction.verify_signatures());
        assert_eq!(result.transaction.outputs.len(), 2); // recipient + change
    }

    #[test]
    fn test_batch_multiple_recipients() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, 500_000_000, &pool_addr)]; // 500 ZION

        let recipients: Vec<Recipient> = (0..5)
            .map(|i| Recipient {
                address: test_address((b'a' + i) as char),
                amount: 50_000_000, // 50 ZION each
            })
            .collect();

        let params = BatchParams {
            recipients,
            fee: Some(5_000),
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
        assert_eq!(result.total_sent, 250_000_000);
        assert_eq!(result.recipients_paid, 5);
        assert_eq!(result.fee, 5_000);
        assert_eq!(result.change, 500_000_000 - 250_000_000 - 5_000);
        assert!(result.transaction.verify_signatures());
        assert_eq!(result.transaction.outputs.len(), 6); // 5 recipients + change
    }

    #[test]
    fn test_batch_multiple_utxos() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![
            make_utxo("tx1", 0, 100_000_000, &pool_addr),
            make_utxo("tx2", 0, 100_000_000, &pool_addr),
            make_utxo("tx3", 0, 100_000_000, &pool_addr),
        ];

        let params = BatchParams {
            recipients: vec![
                Recipient { address: test_address('a'), amount: 120_000_000 },
                Recipient { address: test_address('b'), amount: 80_000_000 },
            ],
            fee: Some(3_000),
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
        assert_eq!(result.total_sent, 200_000_000);
        assert!(result.inputs_used >= 2); // Need at least 2 UTXOs
        assert!(result.transaction.verify_signatures());
    }

    #[test]
    fn test_batch_no_change() {
        let (pool_addr, secret) = pool_address_and_key();
        let fee_amount = fee::minimum_fee_for_size(fee::estimate_tx_size(1, 1));
        let utxos = vec![make_utxo("tx1", 0, 50_000_000 + fee_amount, &pool_addr)];

        let params = BatchParams {
            recipients: vec![Recipient {
                address: test_address('a'),
                amount: 50_000_000,
            }],
            fee: Some(fee_amount),
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
        assert_eq!(result.change, 0);
        assert_eq!(result.transaction.outputs.len(), 1); // Only recipient, no change
    }

    #[test]
    fn test_batch_auto_fee() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, 500_000_000, &pool_addr)];

        let params = BatchParams {
            recipients: vec![
                Recipient { address: test_address('a'), amount: 50_000_000 },
                Recipient { address: test_address('b'), amount: 50_000_000 },
            ],
            fee: None, // Auto-calculate
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
        assert!(result.fee >= fee::MIN_TX_FEE);
        assert_eq!(result.total_sent, 100_000_000);
        assert!(result.transaction.verify_signatures());
    }

    // --- Error Cases ---

    #[test]
    fn test_batch_empty_recipients() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, 100_000_000, &pool_addr)];

        let params = BatchParams {
            recipients: vec![],
            fee: None,
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret);
        assert!(matches!(result, Err(WalletError::ZeroAmount)));
    }

    #[test]
    fn test_batch_insufficient_funds() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, 1_000, &pool_addr)]; // Tiny

        let params = BatchParams {
            recipients: vec![Recipient {
                address: test_address('a'),
                amount: 100_000_000,
            }],
            fee: None,
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret);
        assert!(matches!(
            result,
            Err(WalletError::InsufficientFunds { .. })
        ));
    }

    #[test]
    fn test_batch_invalid_recipient_address() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, 100_000_000, &pool_addr)];

        let params = BatchParams {
            recipients: vec![Recipient {
                address: "invalid_address".to_string(),
                amount: 50_000_000,
            }],
            fee: None,
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret);
        assert!(matches!(result, Err(WalletError::InvalidAddress(_))));
    }

    #[test]
    fn test_batch_zero_amount_recipient() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, 100_000_000, &pool_addr)];

        let params = BatchParams {
            recipients: vec![Recipient {
                address: test_address('a'),
                amount: 0,
            }],
            fee: None,
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret);
        assert!(matches!(result, Err(WalletError::ZeroAmount)));
    }

    #[test]
    fn test_batch_no_utxos() {
        let (pool_addr, secret) = pool_address_and_key();
        let params = BatchParams {
            recipients: vec![Recipient {
                address: test_address('a'),
                amount: 50_000_000,
            }],
            fee: None,
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &[], &secret);
        assert!(matches!(result, Err(WalletError::NoUtxos)));
    }

    #[test]
    fn test_batch_too_many_recipients() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![make_utxo("tx1", 0, u64::MAX / 2, &pool_addr)];

        let recipients: Vec<Recipient> = (0..MAX_BATCH_RECIPIENTS + 1)
            .map(|_| Recipient {
                address: test_address('a'),
                amount: 1_000_000,
            })
            .collect();

        let params = BatchParams {
            recipients,
            fee: None,
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret);
        assert!(matches!(result, Err(WalletError::SigningError(_))));
    }

    // --- Signature Verification ---

    #[test]
    fn test_batch_all_signatures_valid() {
        let (pool_addr, secret) = pool_address_and_key();
        let utxos = vec![
            make_utxo("tx1", 0, 200_000_000, &pool_addr),
            make_utxo("tx2", 0, 200_000_000, &pool_addr),
        ];

        let recipients: Vec<Recipient> = (0..10)
            .map(|i| Recipient {
                address: test_address((b'a' + i) as char),
                amount: 30_000_000, // 30 ZION each
            })
            .collect();

        let params = BatchParams {
            recipients,
            fee: Some(10_000),
            change_address: pool_addr.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();

        // Verify all signatures
        assert!(result.transaction.verify_signatures());

        // Verify each input has correct public key
        let signing_key = SigningKey::from_bytes(&secret);
        let expected_pk = to_hex(signing_key.verifying_key().as_bytes());
        for input in &result.transaction.inputs {
            assert_eq!(input.public_key, expected_pk);
            assert_eq!(input.signature.len(), 128); // 64 bytes = 128 hex chars
        }
    }

    // --- Constants ---

    #[test]
    fn test_min_payout_amount() {
        assert_eq!(MIN_PAYOUT_AMOUNT, 10_000_000); // 10 ZION
    }

    #[test]
    fn test_max_batch_recipients() {
        assert_eq!(MAX_BATCH_RECIPIENTS, 200);
    }
}
