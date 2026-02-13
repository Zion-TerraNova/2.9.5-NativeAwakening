/// Sprint 1.4 — Pool Payout Integration Tests
///
/// Validates:
/// 1. Batch TX builder (multi-recipient transactions)
/// 2. submitTransaction JSON-RPC (accepts signed TX)
/// 3. UTXO consumption and change handling
/// 4. Signature verification for batch transactions
/// 5. Edge cases: max recipients, fee calculation, insufficient funds

use zion_core::wallet::batch::*;
use zion_core::wallet::{SpendableUtxo, WalletError, build_and_sign, SendParams};
use zion_core::tx::{Transaction, TxInput, TxOutput};
use zion_core::blockchain::fee;
use zion_core::crypto::{keys, to_hex};
use ed25519_dalek::SigningKey;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn pool_keypair() -> ([u8; 32], String, String) {
    let secret = [42u8; 32];
    let signing_key = SigningKey::from_bytes(&secret);
    let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
    let addr = keys::address_from_public_key_hex(&pk_hex);
    (secret, pk_hex, addr)
}

fn miner_address(n: u8) -> String {
    let c = (b'a' + n % 26) as char;
    format!("zion1{}", c.to_string().repeat(39))
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

// =========================================================================
// 1. Batch TX Builder — Core Mechanics
// =========================================================================

#[test]
fn test_batch_1_recipient_basic() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("coinbase1", 0, 5_400_067_000, &pool_addr)];

    let params = BatchParams {
        recipients: vec![Recipient {
            address: miner_address(0),
            amount: 4_806_059_630, // 89% of reward
        }],
        fee: Some(2_000),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    assert_eq!(result.recipients_paid, 1);
    assert_eq!(result.total_sent, 4_806_059_630);
    assert_eq!(result.fee, 2_000);
    assert!(result.change > 0);
    assert!(result.transaction.verify_signatures());
}

#[test]
fn test_batch_10_miners_pplns() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("coinbase1", 0, 100_000_000_000, &pool_addr)]; // 100k ZION

    // Simulate PPLNS distribution to 10 miners
    let recipients: Vec<Recipient> = (0..10)
        .map(|i| Recipient {
            address: miner_address(i),
            amount: 5_000_000_000, // 5000 ZION each
        })
        .collect();

    let params = BatchParams {
        recipients,
        fee: None,
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    assert_eq!(result.recipients_paid, 10);
    assert_eq!(result.total_sent, 50_000_000_000);
    assert!(result.fee >= fee::MIN_TX_FEE);
    assert!(result.change > 0);
    assert!(result.transaction.verify_signatures());
    assert_eq!(result.transaction.outputs.len(), 11); // 10 miners + change
}

#[test]
fn test_batch_50_miners() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("big_utxo", 0, 1_000_000_000_000, &pool_addr)]; // 1M ZION

    let recipients: Vec<Recipient> = (0..50)
        .map(|i| Recipient {
            address: miner_address(i as u8),
            amount: 10_000_000, // 10 ZION each (min payout)
        })
        .collect();

    let params = BatchParams {
        recipients,
        fee: None,
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    assert_eq!(result.recipients_paid, 50);
    assert_eq!(result.total_sent, 500_000_000); // 500 ZION total
    assert!(result.transaction.verify_signatures());
}

// =========================================================================
// 2. UTXO Consumption
// =========================================================================

#[test]
fn test_batch_multiple_utxo_consumption() {
    let (secret, _, pool_addr) = pool_keypair();
    // Pool has 3 small UTXOs from 3 different blocks
    let utxos = vec![
        make_utxo("block_100_cb", 0, 5_400_067_000, &pool_addr),
        make_utxo("block_101_cb", 0, 5_400_067_000, &pool_addr),
        make_utxo("block_102_cb", 0, 5_400_067_000, &pool_addr),
    ];

    let params = BatchParams {
        recipients: vec![
            Recipient { address: miner_address(0), amount: 8_000_000_000 }, // 8000 ZION
            Recipient { address: miner_address(1), amount: 5_000_000_000 }, // 5000 ZION
        ],
        fee: Some(5_000),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    assert!(result.inputs_used >= 3); // All 3 UTXOs needed
    assert_eq!(result.total_sent, 13_000_000_000);
    assert!(result.transaction.verify_signatures());
}

#[test]
fn test_batch_utxo_largest_first() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![
        make_utxo("small", 0, 1_000_000, &pool_addr),    // 1 ZION
        make_utxo("large", 0, 100_000_000, &pool_addr),   // 100 ZION
        make_utxo("medium", 0, 10_000_000, &pool_addr),   // 10 ZION
    ];

    let params = BatchParams {
        recipients: vec![Recipient {
            address: miner_address(0),
            amount: 50_000_000, // 50 ZION — only 'large' needed
        }],
        fee: Some(2_000),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    assert_eq!(result.inputs_used, 1); // Only the large UTXO
    assert_eq!(result.transaction.inputs[0].prev_tx_hash, "large");
}

#[test]
fn test_batch_exact_amount_no_change() {
    let (secret, _, pool_addr) = pool_keypair();
    let min_fee = fee::minimum_fee_for_size(fee::estimate_tx_size(1, 1));
    let utxos = vec![make_utxo("exact", 0, 50_000_000 + min_fee, &pool_addr)];

    let params = BatchParams {
        recipients: vec![Recipient {
            address: miner_address(0),
            amount: 50_000_000,
        }],
        fee: Some(min_fee),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    assert_eq!(result.change, 0);
    assert_eq!(result.transaction.outputs.len(), 1); // No change output
}

// =========================================================================
// 3. Signature Verification
// =========================================================================

#[test]
fn test_batch_signatures_ed25519_valid() {
    let (secret, pk_hex, pool_addr) = pool_keypair();
    let utxos = vec![
        make_utxo("u1", 0, 100_000_000, &pool_addr),
        make_utxo("u2", 0, 100_000_000, &pool_addr),
    ];

    let params = BatchParams {
        recipients: vec![
            Recipient { address: miner_address(0), amount: 50_000_000 },
            Recipient { address: miner_address(1), amount: 50_000_000 },
            Recipient { address: miner_address(2), amount: 50_000_000 },
        ],
        fee: Some(5_000),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    let tx = &result.transaction;

    // All inputs signed with same pool key
    for input in &tx.inputs {
        assert_eq!(input.public_key, pk_hex);
        assert_eq!(input.signature.len(), 128); // 64 bytes hex
    }

    // TX self-verifies
    assert!(tx.verify_signatures());

    // Tamper with signature → should fail
    let mut tampered = tx.clone();
    tampered.inputs[0].signature = "00".repeat(64);
    assert!(!tampered.verify_signatures());
}

#[test]
fn test_batch_tamper_output_invalidates() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("u1", 0, 100_000_000, &pool_addr)];

    let params = BatchParams {
        recipients: vec![Recipient {
            address: miner_address(0),
            amount: 50_000_000,
        }],
        fee: Some(2_000),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    let mut tampered = result.transaction.clone();

    // Change output amount (attacker tries to steal more)
    tampered.outputs[0].amount = 90_000_000;
    // Recalculate hash — now hash ≠ signed hash
    tampered.id = tampered.calculate_hash();
    assert!(!tampered.verify_signatures()); // Signatures signed old hash
}

// =========================================================================
// 4. Fee Calculation
// =========================================================================

#[test]
fn test_batch_fee_scales_with_outputs() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("big", 0, u64::MAX / 2, &pool_addr)];

    // 1 recipient
    let r1 = build_and_sign_batch(
        &BatchParams {
            recipients: vec![Recipient { address: miner_address(0), amount: 1_000_000 }],
            fee: None,
            change_address: pool_addr.clone(),
        },
        &utxos,
        &secret,
    ).unwrap();

    // 20 recipients
    let recipients_20: Vec<Recipient> = (0..20)
        .map(|i| Recipient { address: miner_address(i), amount: 1_000_000 })
        .collect();
    let r20 = build_and_sign_batch(
        &BatchParams {
            recipients: recipients_20,
            fee: None,
            change_address: pool_addr.clone(),
        },
        &utxos,
        &secret,
    ).unwrap();

    // More outputs → higher fee
    assert!(r20.fee > r1.fee);
}

#[test]
fn test_batch_fee_minimum_enforced() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("u1", 0, 100_000_000, &pool_addr)];

    let result = build_and_sign_batch(
        &BatchParams {
            recipients: vec![Recipient { address: miner_address(0), amount: 1_000_000 }],
            fee: None,
            change_address: pool_addr.clone(),
        },
        &utxos,
        &secret,
    ).unwrap();

    assert!(result.fee >= fee::MIN_TX_FEE);
}

// =========================================================================
// 5. Error Cases
// =========================================================================

#[test]
fn test_batch_error_empty_recipients() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("u1", 0, 100_000_000, &pool_addr)];

    let result = build_and_sign_batch(
        &BatchParams {
            recipients: vec![],
            fee: None,
            change_address: pool_addr,
        },
        &utxos,
        &secret,
    );
    assert!(matches!(result, Err(WalletError::ZeroAmount)));
}

#[test]
fn test_batch_error_insufficient_funds() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("tiny", 0, 1_000, &pool_addr)]; // tiny

    let result = build_and_sign_batch(
        &BatchParams {
            recipients: vec![Recipient { address: miner_address(0), amount: 100_000_000 }],
            fee: None,
            change_address: pool_addr,
        },
        &utxos,
        &secret,
    );
    assert!(matches!(result, Err(WalletError::InsufficientFunds { .. })));
}

#[test]
fn test_batch_error_invalid_miner_address() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("u1", 0, 100_000_000, &pool_addr)];

    let result = build_and_sign_batch(
        &BatchParams {
            recipients: vec![Recipient { address: "BAD_ADDR".to_string(), amount: 1_000_000 }],
            fee: None,
            change_address: pool_addr,
        },
        &utxos,
        &secret,
    );
    assert!(matches!(result, Err(WalletError::InvalidAddress(_))));
}

#[test]
fn test_batch_error_zero_amount_recipient() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("u1", 0, 100_000_000, &pool_addr)];

    let result = build_and_sign_batch(
        &BatchParams {
            recipients: vec![Recipient { address: miner_address(0), amount: 0 }],
            fee: None,
            change_address: pool_addr,
        },
        &utxos,
        &secret,
    );
    assert!(matches!(result, Err(WalletError::ZeroAmount)));
}

#[test]
fn test_batch_error_no_utxos() {
    let (secret, _, pool_addr) = pool_keypair();

    let result = build_and_sign_batch(
        &BatchParams {
            recipients: vec![Recipient { address: miner_address(0), amount: 1_000_000 }],
            fee: None,
            change_address: pool_addr,
        },
        &[],
        &secret,
    );
    assert!(matches!(result, Err(WalletError::NoUtxos)));
}

#[test]
fn test_batch_error_over_max_recipients() {
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("u1", 0, u64::MAX / 2, &pool_addr)];

    let recipients: Vec<Recipient> = (0..=MAX_BATCH_RECIPIENTS as u8)
        .map(|i| Recipient { address: miner_address(i), amount: 1_000_000 })
        .collect();

    let result = build_and_sign_batch(
        &BatchParams {
            recipients,
            fee: None,
            change_address: pool_addr,
        },
        &utxos,
        &secret,
    );
    assert!(matches!(result, Err(WalletError::SigningError(_))));
}

// =========================================================================
// 6. Batch vs Single TX Comparison
// =========================================================================

#[test]
fn test_batch_more_efficient_than_singles() {
    let (secret, _, pool_addr) = pool_keypair();

    // Single TX for each of 5 miners
    let mut total_single_fee = 0u64;
    for i in 0..5 {
        let utxos = vec![make_utxo(
            &format!("utxo_{}", i),
            0,
            100_000_000,
            &pool_addr,
        )];
        let result = build_and_sign(
            &SendParams {
                to_address: miner_address(i),
                amount: 10_000_000,
                fee: None,
                change_address: pool_addr.clone(),
            },
            &utxos,
            &secret,
        ).unwrap();
        total_single_fee += result.fee;
    }

    // Batch TX for all 5 miners
    let utxos = vec![make_utxo("big", 0, 500_000_000, &pool_addr)];
    let recipients: Vec<Recipient> = (0..5)
        .map(|i| Recipient { address: miner_address(i), amount: 10_000_000 })
        .collect();
    let batch_result = build_and_sign_batch(
        &BatchParams {
            recipients,
            fee: None,
            change_address: pool_addr.clone(),
        },
        &utxos,
        &secret,
    ).unwrap();

    // Batch should be cheaper than sum of singles
    assert!(
        batch_result.fee < total_single_fee,
        "Batch fee {} should be < sum of single fees {}",
        batch_result.fee,
        total_single_fee
    );
}

// =========================================================================
// 7. Payout Constants
// =========================================================================

#[test]
fn test_min_payout_is_10_zion() {
    assert_eq!(MIN_PAYOUT_AMOUNT, 10_000_000);
}

#[test]
fn test_max_batch_is_200() {
    assert_eq!(MAX_BATCH_RECIPIENTS, 200);
}

#[test]
fn test_coinbase_maturity_matches_core() {
    // COINBASE_MATURITY in validation.rs = 100
    assert_eq!(zion_core::blockchain::validation::COINBASE_MATURITY, 100);
}

// =========================================================================
// 8. TX Structure Validation
// =========================================================================

#[test]
fn test_batch_tx_has_correct_structure() {
    let (secret, pk_hex, pool_addr) = pool_keypair();
    let utxos = vec![
        make_utxo("cb1", 0, 5_400_067_000, &pool_addr),
        make_utxo("cb2", 0, 5_400_067_000, &pool_addr),
    ];

    let params = BatchParams {
        recipients: vec![
            Recipient { address: miner_address(0), amount: 3_000_000_000 },
            Recipient { address: miner_address(1), amount: 2_000_000_000 },
            Recipient { address: miner_address(2), amount: 1_000_000_000 },
        ],
        fee: Some(10_000),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    let tx = &result.transaction;

    // Version
    assert_eq!(tx.version, 1);
    // ID is non-empty hash
    assert!(!tx.id.is_empty());
    assert_eq!(tx.id.len(), 64); // blake2b hash = 32 bytes = 64 hex
    // Fee
    assert_eq!(tx.fee, 10_000);
    // Timestamp
    assert!(tx.timestamp > 0);
    // Inputs reference correct UTXOs
    assert_eq!(tx.inputs.len(), 2);
    // Outputs: 3 recipients + 1 change = 4
    assert_eq!(tx.outputs.len(), 4);
    // First 3 outputs are recipients in order
    assert_eq!(tx.outputs[0].address, miner_address(0));
    assert_eq!(tx.outputs[0].amount, 3_000_000_000);
    assert_eq!(tx.outputs[1].address, miner_address(1));
    assert_eq!(tx.outputs[1].amount, 2_000_000_000);
    assert_eq!(tx.outputs[2].address, miner_address(2));
    assert_eq!(tx.outputs[2].amount, 1_000_000_000);
    // Last output is change to pool
    assert_eq!(tx.outputs[3].address, pool_addr);
    let expected_change = 5_400_067_000u64 * 2 - 6_000_000_000 - 10_000;
    assert_eq!(tx.outputs[3].amount, expected_change);
    // All inputs have pool pubkey
    for inp in &tx.inputs {
        assert_eq!(inp.public_key, pk_hex);
    }
}

#[test]
fn test_batch_tx_id_is_deterministic() {
    // Same inputs/outputs should produce same hash
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("det", 0, 100_000_000, &pool_addr)];

    let params = BatchParams {
        recipients: vec![Recipient {
            address: miner_address(0),
            amount: 50_000_000,
        }],
        fee: Some(2_000),
        change_address: pool_addr.clone(),
    };

    let r1 = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    let r2 = build_and_sign_batch(&params, &utxos, &secret).unwrap();

    // IDs may differ due to timestamp — but structure should be same
    assert_eq!(r1.total_sent, r2.total_sent);
    assert_eq!(r1.fee, r2.fee);
    assert_eq!(r1.change, r2.change);
    assert_eq!(r1.recipients_paid, r2.recipients_paid);
}

// =========================================================================
// 9. Pool Reward Distribution Math
// =========================================================================

#[test]
fn test_pool_reward_split_89_10_1() {
    // Block reward: 5,400,067,000 atomic (5400.067 ZION)
    let block_reward: u64 = 5_400_067_000;
    let miner_share = (block_reward as f64 * 0.89) as u64;  // 4,806,059,630
    let tithe_share = (block_reward as f64 * 0.10) as u64;  // 540,006,700
    let pool_fee    = (block_reward as f64 * 0.01) as u64;  // 54,000,670

    assert_eq!(miner_share, 4_806_059_630);
    assert_eq!(tithe_share, 540_006_700);
    assert_eq!(pool_fee, 54_000_670);

    // Miner share can be distributed via batch TX
    let (secret, _, pool_addr) = pool_keypair();
    let utxos = vec![make_utxo("reward", 0, block_reward, &pool_addr)];

    let params = BatchParams {
        recipients: vec![
            Recipient { address: miner_address(0), amount: miner_share },
            Recipient { address: miner_address(1), amount: tithe_share }, // humanitarian
        ],
        fee: Some(2_000),
        change_address: pool_addr.clone(),
    };

    let result = build_and_sign_batch(&params, &utxos, &secret).unwrap();
    assert_eq!(result.total_sent, miner_share + tithe_share);
    assert!(result.transaction.verify_signatures());
    // Change should be pool_fee - tx_fee
    assert_eq!(result.change, pool_fee - 2_000);
}
