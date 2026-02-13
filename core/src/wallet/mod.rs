/// ZION Wallet Core — UTXO Selection, Transaction Building, Signing
///
/// This module provides the library-level building blocks for sending ZION:
///
/// 1. **UTXO Selection** — greedy largest-first coin selection
/// 2. **Transaction Building** — construct unsigned tx with change output
/// 3. **Signing** — Ed25519 sign each input
/// 4. **Fee Estimation** — automatic fee calculation
/// 5. **Batch Transactions** — multi-recipient payouts (pool)
///
/// The CLI binary (`zion-wallet send`) and RPC endpoint both use this module.

pub mod batch;

use crate::tx::{Transaction, TxInput, TxOutput};
use crate::blockchain::fee;
use crate::crypto::{keys, to_hex};
use ed25519_dalek::{Signer, SigningKey};
use zeroize::Zeroize;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A spendable UTXO with its storage key.
#[derive(Debug, Clone)]
pub struct SpendableUtxo {
    /// Storage key: "txid:output_index"
    pub key: String,
    /// The previous transaction hash
    pub tx_hash: String,
    /// The output index within that transaction
    pub output_index: u32,
    /// Amount in atomic units
    pub amount: u64,
    /// Owner address
    pub address: String,
}

/// Parameters for building a transaction.
#[derive(Debug, Clone)]
pub struct SendParams {
    /// Destination address
    pub to_address: String,
    /// Amount to send (atomic units)
    pub amount: u64,
    /// Optional explicit fee (atomic units). If None, auto-calculated.
    pub fee: Option<u64>,
    /// Change address (sender's own address)
    pub change_address: String,
}

/// Result of building a transaction.
#[derive(Debug, Clone)]
pub struct BuildResult {
    /// The signed, ready-to-broadcast transaction
    pub transaction: Transaction,
    /// Total input amount
    pub total_input: u64,
    /// Amount sent to recipient
    pub amount_sent: u64,
    /// Fee paid (burned)
    pub fee: u64,
    /// Change returned to sender
    pub change: u64,
    /// Number of UTXOs consumed
    pub inputs_used: usize,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum WalletError {
    /// Not enough balance to cover amount + fee
    InsufficientFunds { available: u64, needed: u64 },
    /// No UTXOs available
    NoUtxos,
    /// Invalid destination address
    InvalidAddress(String),
    /// Fee too low (would be rejected by mempool)
    FeeTooLow { fee: u64, minimum: u64 },
    /// Amount is zero
    ZeroAmount,
    /// Signing failed
    SigningError(String),
}

impl std::fmt::Display for WalletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WalletError::InsufficientFunds { available, needed } =>
                write!(f, "Insufficient funds: have {} atomic, need {}", available, needed),
            WalletError::NoUtxos =>
                write!(f, "No spendable UTXOs found"),
            WalletError::InvalidAddress(addr) =>
                write!(f, "Invalid address: {}", addr),
            WalletError::FeeTooLow { fee, minimum } =>
                write!(f, "Fee {} is below minimum {}", fee, minimum),
            WalletError::ZeroAmount =>
                write!(f, "Send amount cannot be zero"),
            WalletError::SigningError(msg) =>
                write!(f, "Signing error: {}", msg),
        }
    }
}

// ---------------------------------------------------------------------------
// UTXO Selection
// ---------------------------------------------------------------------------

/// Select UTXOs to cover the target amount + fee using largest-first strategy.
///
/// Returns selected UTXOs and the estimated fee.
/// The fee is recalculated based on the number of inputs/outputs selected.
pub fn select_utxos(
    available: &[SpendableUtxo],
    target_amount: u64,
    explicit_fee: Option<u64>,
) -> Result<(Vec<SpendableUtxo>, u64), WalletError> {
    if available.is_empty() {
        return Err(WalletError::NoUtxos);
    }

    // Sort by amount descending (largest first = fewer inputs = lower fee)
    let mut sorted: Vec<SpendableUtxo> = available.to_vec();
    sorted.sort_by(|a, b| b.amount.cmp(&a.amount));

    // Iteratively select UTXOs until we cover amount + fee
    let mut selected: Vec<SpendableUtxo> = Vec::new();
    let mut total: u64 = 0;

    for utxo in &sorted {
        selected.push(utxo.clone());
        total += utxo.amount;

        // Estimate fee based on current selection
        // Outputs: 1 (recipient) + potentially 1 (change)
        let has_change = total > target_amount + estimate_fee(selected.len(), 2, explicit_fee);
        let num_outputs = if has_change { 2 } else { 1 };
        let current_fee = estimate_fee(selected.len(), num_outputs, explicit_fee);

        if total >= target_amount + current_fee {
            return Ok((selected, current_fee));
        }
    }

    // Not enough
    let needed = target_amount + estimate_fee(sorted.len(), 2, explicit_fee);
    Err(WalletError::InsufficientFunds {
        available: total,
        needed,
    })
}

/// Estimate transaction fee based on input/output count.
fn estimate_fee(num_inputs: usize, num_outputs: usize, explicit_fee: Option<u64>) -> u64 {
    if let Some(f) = explicit_fee {
        return f;
    }
    let size = fee::estimate_tx_size(num_inputs, num_outputs);
    fee::minimum_fee_for_size(size)
}

// ---------------------------------------------------------------------------
// Transaction Building
// ---------------------------------------------------------------------------

/// Build and sign a transaction.
///
/// Steps:
/// 1. Validate params
/// 2. Select UTXOs (coin selection)
/// 3. Build unsigned transaction (with placeholder sigs)
/// 4. Calculate transaction hash (ID)
/// 5. Sign each input with the secret key
/// 6. Set final ID
pub fn build_and_sign(
    params: &SendParams,
    available_utxos: &[SpendableUtxo],
    secret_key_bytes: &[u8; 32],
) -> Result<BuildResult, WalletError> {
    // 1. Validate
    if params.amount == 0 {
        return Err(WalletError::ZeroAmount);
    }
    if !keys::is_valid_zion1_address(&params.to_address) {
        return Err(WalletError::InvalidAddress(params.to_address.clone()));
    }
    if !keys::is_valid_zion1_address(&params.change_address) {
        return Err(WalletError::InvalidAddress(params.change_address.clone()));
    }

    // 2. Select UTXOs
    let (selected, tx_fee) = select_utxos(available_utxos, params.amount, params.fee)?;
    let total_input: u64 = selected.iter().map(|u| u.amount).sum();
    let change = total_input - params.amount - tx_fee;

    // 3. Validate fee
    let num_outputs = if change > 0 { 2 } else { 1 };
    let tx_size = fee::estimate_tx_size(selected.len(), num_outputs);
    let min_required = fee::minimum_fee_for_size(tx_size);
    if tx_fee < min_required {
        return Err(WalletError::FeeTooLow { fee: tx_fee, minimum: min_required });
    }

    // 4. Derive public key
    // AUDIT-FIX P1-17: Copy key bytes so we can zeroize after signing
    let mut key_bytes = *secret_key_bytes;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let public_key_hex = to_hex(signing_key.verifying_key().as_bytes());

    // 5. Build outputs
    let mut outputs = vec![
        TxOutput {
            amount: params.amount,
            address: params.to_address.clone(),
        },
    ];
    if change > 0 {
        outputs.push(TxOutput {
            amount: change,
            address: params.change_address.clone(),
        });
    }

    // 6. Build inputs (with empty signatures — will be filled after hash)
    let inputs: Vec<TxInput> = selected.iter().map(|utxo| TxInput {
        prev_tx_hash: utxo.tx_hash.clone(),
        output_index: utxo.output_index,
        signature: String::new(),
        public_key: public_key_hex.clone(),
    }).collect();

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // 7. Create unsigned transaction and compute hash
    let mut tx = Transaction {
        id: String::new(),
        version: 1,
        inputs,
        outputs,
        fee: tx_fee,
        timestamp,
    };

    // Calculate hash (ID excludes signatures, so this is safe)
    let tx_hash = tx.calculate_hash();
    tx.id = tx_hash.clone();

    // 8. Sign: the message is the raw bytes of the tx hash
    let msg_bytes = keys::from_hex(&tx_hash)
        .ok_or_else(|| WalletError::SigningError("Failed to decode tx hash".into()))?;

    for input in &mut tx.inputs {
        let sig = signing_key.sign(&msg_bytes);
        input.signature = to_hex(&sig.to_bytes());
    }

    // 9. Verify our own signatures
    if !tx.verify_signatures() {
        key_bytes.zeroize();
        return Err(WalletError::SigningError("Self-verification failed".into()));
    }

    // AUDIT-FIX P1-17: Zeroize private key material from memory
    key_bytes.zeroize();

    Ok(BuildResult {
        transaction: tx,
        total_input,
        amount_sent: params.amount,
        fee: tx_fee,
        change,
        inputs_used: selected.len(),
    })
}

// ---------------------------------------------------------------------------
// Helper: parse UTXO key
// ---------------------------------------------------------------------------

/// Parse a storage UTXO key "txhash:index" into (tx_hash, output_index).
pub fn parse_utxo_key(key: &str) -> Option<(String, u32)> {
    let parts: Vec<&str> = key.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        return None;
    }
    let index: u32 = parts[0].parse().ok()?;
    let tx_hash = parts[1].to_string();
    Some((tx_hash, index))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Generate a valid checksummed zion1 address from a deterministic seed.
    fn make_test_address(seed: u8) -> String {
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
        keys::address_from_public_key_hex(&pk_hex)
    }

    fn make_utxo(key: &str, tx_hash: &str, index: u32, amount: u64) -> SpendableUtxo {
        SpendableUtxo {
            key: key.to_string(),
            tx_hash: tx_hash.to_string(),
            output_index: index,
            amount,
            address: make_test_address(1),
        }
    }

    fn test_address() -> String {
        make_test_address(99)
    }

    fn test_address_2() -> String {
        make_test_address(100)
    }

    #[test]
    fn test_select_utxos_simple() {
        let utxos = vec![
            make_utxo("tx1:0", "tx1", 0, 10_000_000), // 10 ZION
            make_utxo("tx2:0", "tx2", 0, 5_000_000),  // 5 ZION
        ];
        let (selected, fee) = select_utxos(&utxos, 3_000_000, None).unwrap();
        // Should select the 5M UTXO (smallest that covers 3M + fee)
        // Actually largest-first: 10M first
        assert!(!selected.is_empty());
        assert!(fee >= fee::MIN_TX_FEE);
    }

    #[test]
    fn test_select_utxos_insufficient() {
        let utxos = vec![
            make_utxo("tx1:0", "tx1", 0, 1_000), // tiny
        ];
        let result = select_utxos(&utxos, 1_000_000, None);
        assert!(matches!(result, Err(WalletError::InsufficientFunds { .. })));
    }

    #[test]
    fn test_select_utxos_empty() {
        let result = select_utxos(&[], 1_000_000, None);
        assert!(matches!(result, Err(WalletError::NoUtxos)));
    }

    #[test]
    fn test_select_utxos_exact() {
        // UTXO exactly covers amount + fee
        let min_fee = fee::minimum_fee_for_size(fee::estimate_tx_size(1, 1));
        let utxos = vec![
            make_utxo("tx1:0", "tx1", 0, 1_000_000 + min_fee),
        ];
        let (selected, fee_paid) = select_utxos(&utxos, 1_000_000, None).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(fee_paid, min_fee);
    }

    #[test]
    fn test_select_utxos_multiple_needed() {
        let utxos = vec![
            make_utxo("tx1:0", "tx1", 0, 500_000),
            make_utxo("tx2:0", "tx2", 0, 500_000),
            make_utxo("tx3:0", "tx3", 0, 500_000),
        ];
        let (selected, _fee) = select_utxos(&utxos, 1_200_000, None).unwrap();
        assert!(selected.len() >= 3); // Need all three
    }

    #[test]
    fn test_select_with_explicit_fee() {
        let utxos = vec![
            make_utxo("tx1:0", "tx1", 0, 10_000_000),
        ];
        let explicit = 5_000u64;
        let (selected, fee) = select_utxos(&utxos, 1_000_000, Some(explicit)).unwrap();
        assert_eq!(selected.len(), 1);
        assert_eq!(fee, explicit);
    }

    #[test]
    fn test_build_and_sign() {
        // Generate a real Ed25519 keypair
        let secret = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);
        let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
        let sender_addr = keys::address_from_public_key_hex(&pk_hex);

        let utxos = vec![
            SpendableUtxo {
                key: "tx1:0".to_string(),
                tx_hash: "tx1".to_string(),
                output_index: 0,
                amount: 100_000_000, // 100 ZION
                address: sender_addr.clone(),
            },
        ];

        let params = SendParams {
            to_address: test_address(),
            amount: 50_000_000, // 50 ZION
            fee: Some(2_000),
            change_address: sender_addr.clone(),
        };

        let result = build_and_sign(&params, &utxos, &secret).unwrap();

        assert_eq!(result.amount_sent, 50_000_000);
        assert_eq!(result.fee, 2_000);
        assert_eq!(result.change, 100_000_000 - 50_000_000 - 2_000);
        assert_eq!(result.inputs_used, 1);
        assert!(result.transaction.verify_signatures());
        assert_eq!(result.transaction.outputs.len(), 2); // recipient + change
    }

    #[test]
    fn test_build_no_change() {
        let secret = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);
        let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
        let sender_addr = keys::address_from_public_key_hex(&pk_hex);

        let fee_amount = fee::minimum_fee_for_size(fee::estimate_tx_size(1, 1));
        let utxos = vec![
            SpendableUtxo {
                key: "tx1:0".to_string(),
                tx_hash: "tx1".to_string(),
                output_index: 0,
                amount: 50_000_000 + fee_amount,
                address: sender_addr.clone(),
            },
        ];

        let params = SendParams {
            to_address: test_address(),
            amount: 50_000_000,
            fee: Some(fee_amount),
            change_address: sender_addr.clone(),
        };

        let result = build_and_sign(&params, &utxos, &secret).unwrap();
        assert_eq!(result.change, 0);
        assert_eq!(result.transaction.outputs.len(), 1); // only recipient, no change
    }

    #[test]
    fn test_build_zero_amount() {
        let secret = [42u8; 32];
        let utxos = vec![make_utxo("tx1:0", "tx1", 0, 10_000_000)];
        let params = SendParams {
            to_address: test_address(),
            amount: 0,
            fee: None,
            change_address: test_address_2(),
        };
        let result = build_and_sign(&params, &utxos, &secret);
        assert!(matches!(result, Err(WalletError::ZeroAmount)));
    }

    #[test]
    fn test_build_invalid_address() {
        let secret = [42u8; 32];
        let utxos = vec![make_utxo("tx1:0", "tx1", 0, 10_000_000)];
        let params = SendParams {
            to_address: "invalid".to_string(),
            amount: 1_000_000,
            fee: None,
            change_address: test_address(),
        };
        let result = build_and_sign(&params, &utxos, &secret);
        assert!(matches!(result, Err(WalletError::InvalidAddress(_))));
    }

    #[test]
    fn test_build_insufficient_funds() {
        let secret = [42u8; 32];
        let utxos = vec![make_utxo("tx1:0", "tx1", 0, 1_000)];
        let params = SendParams {
            to_address: test_address(),
            amount: 1_000_000_000,
            fee: None,
            change_address: test_address_2(),
        };
        let result = build_and_sign(&params, &utxos, &secret);
        assert!(matches!(result, Err(WalletError::InsufficientFunds { .. })));
    }

    #[test]
    fn test_parse_utxo_key() {
        let (hash, idx) = parse_utxo_key("abc123def:5").unwrap();
        assert_eq!(hash, "abc123def");
        assert_eq!(idx, 5);

        // Long hash
        let (hash, idx) = parse_utxo_key("0000000000000000000000000000000000000000000000000000000000000000:0").unwrap();
        assert_eq!(hash, "0000000000000000000000000000000000000000000000000000000000000000");
        assert_eq!(idx, 0);

        // Invalid
        assert!(parse_utxo_key("nocolon").is_none());
        assert!(parse_utxo_key("hash:notanumber").is_none());
    }

    #[test]
    fn test_signatures_are_valid_ed25519() {
        // Full end-to-end: build, sign, verify
        let secret = [7u8; 32];
        let signing_key = SigningKey::from_bytes(&secret);
        let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
        let addr = keys::address_from_public_key_hex(&pk_hex);

        let utxos = vec![
            SpendableUtxo {
                key: "aabbcc:0".to_string(),
                tx_hash: "aabbcc".to_string(),
                output_index: 0,
                amount: 200_000_000,
                address: addr.clone(),
            },
            SpendableUtxo {
                key: "ddeeff:1".to_string(),
                tx_hash: "ddeeff".to_string(),
                output_index: 1,
                amount: 100_000_000,
                address: addr.clone(),
            },
        ];

        let params = SendParams {
            to_address: test_address(),
            amount: 250_000_000,
            fee: Some(5_000),
            change_address: addr.clone(),
        };

        let result = build_and_sign(&params, &utxos, &secret).unwrap();
        let tx = &result.transaction;

        // All inputs should have valid signatures
        assert!(tx.verify_signatures());
        assert_eq!(tx.inputs.len(), 2);

        // Each signature should be 64 bytes (128 hex chars)
        for input in &tx.inputs {
            assert_eq!(input.signature.len(), 128);
            assert_eq!(input.public_key, pk_hex);
        }
    }
}
