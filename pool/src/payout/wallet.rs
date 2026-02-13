/// ZION Pool Wallet — Local Signing & Batch Payout
///
/// The pool wallet holds a dedicated Ed25519 keypair and builds/signs
/// payout transactions locally, then submits them to Core via
/// `submitTransaction` JSON-RPC (which validates signatures + UTXOs).
///
/// This is the secure approach:
/// - Pool never sends secret keys over the network
/// - Core never needs to manage pool keys
/// - All signing happens in-process
///
/// Flow:
/// 1. Fetch pool wallet UTXOs via `getUtxos` RPC
/// 2. Build batch TX (N recipients) using `zion_core::wallet::batch`
/// 3. Sign with pool secret key
/// 4. Submit signed TX via `submitTransaction` RPC

use anyhow::{anyhow, Result};
use std::sync::Arc;
use zion_core::crypto::{keys, to_hex};
use zion_core::wallet::batch::{BatchParams, BatchResult, Recipient, build_and_sign_batch};
use zion_core::wallet::SpendableUtxo;
use ed25519_dalek::SigningKey;

use crate::blockchain::ZionRPCClient;

pub struct PoolWallet {
    secret_key: [u8; 32],
    pub address: String,
    pub public_key_hex: String,
    rpc: Arc<ZionRPCClient>,
}

impl PoolWallet {
    /// Create a new PoolWallet from a 64-char hex secret key.
    ///
    /// The secret key is the raw 32-byte Ed25519 signing key in hex.
    /// The address and public key are derived deterministically.
    pub fn new(secret_key_hex: &str, rpc: Arc<ZionRPCClient>) -> Result<Self> {
        let sk_bytes = keys::from_hex(secret_key_hex)
            .ok_or_else(|| anyhow!("Invalid pool wallet secret key hex"))?;

        if sk_bytes.len() != 32 {
            return Err(anyhow!(
                "Pool wallet secret key must be 32 bytes (64 hex chars), got {}",
                sk_bytes.len()
            ));
        }

        let mut secret_key = [0u8; 32];
        secret_key.copy_from_slice(&sk_bytes);

        let signing_key = SigningKey::from_bytes(&secret_key);
        let public_key_hex = to_hex(signing_key.verifying_key().as_bytes());
        let address = keys::address_from_public_key_hex(&public_key_hex);

        tracing::info!(
            "🔑 Pool wallet initialized: address={}, pubkey={}...",
            address,
            &public_key_hex[..16]
        );

        Ok(Self {
            secret_key,
            address,
            public_key_hex,
            rpc,
        })
    }

    /// Fetch spendable UTXOs from the blockchain.
    pub async fn fetch_utxos(&self) -> Result<Vec<SpendableUtxo>> {
        let mut all_utxos = Vec::new();
        let mut offset = 0usize;
        let limit = 500usize;

        loop {
            let result = self.rpc.get_utxos(&self.address, limit, offset).await?;

            let utxos_arr = result
                .get("utxos")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if utxos_arr.is_empty() {
                break;
            }

            for u in &utxos_arr {
                let key = u.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let amount = u
                    .get("amount_atomic")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let addr = u
                    .get("address")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                // Parse key "txhash:index"
                if let Some((tx_hash, output_index)) = zion_core::wallet::parse_utxo_key(&key) {
                    all_utxos.push(SpendableUtxo {
                        key,
                        tx_hash,
                        output_index,
                        amount,
                        address: addr,
                    });
                }
            }

            if utxos_arr.len() < limit {
                break; // No more pages
            }
            offset += limit;
        }

        tracing::info!(
            "💰 Pool wallet UTXOs: {} found, total balance = {} ZION",
            all_utxos.len(),
            all_utxos.iter().map(|u| u.amount).sum::<u64>() as f64 / 1_000_000.0
        );

        Ok(all_utxos)
    }

    /// Get pool wallet balance (sum of all UTXOs).
    pub async fn get_balance(&self) -> Result<u64> {
        let result = self.rpc.get_balance(&self.address).await?;
        let balance = result
            .get("balance_atomic")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        Ok(balance)
    }

    /// Build, sign, and submit a batch payout transaction.
    ///
    /// Returns the tx_id of the submitted transaction.
    pub async fn send_batch_payout(
        &self,
        recipients: Vec<Recipient>,
    ) -> Result<BatchResult> {
        if recipients.is_empty() {
            return Err(anyhow!("No recipients for batch payout"));
        }

        // 1. Fetch UTXOs
        let utxos = self.fetch_utxos().await?;
        if utxos.is_empty() {
            return Err(anyhow!("Pool wallet has no UTXOs"));
        }

        // 2. Build and sign batch TX
        let params = BatchParams {
            recipients,
            fee: None, // Auto-calculate
            change_address: self.address.clone(),
        };

        let result = build_and_sign_batch(&params, &utxos, &self.secret_key)
            .map_err(|e| anyhow!("Batch TX build failed: {}", e))?;

        tracing::info!(
            "📦 Batch payout TX built: {} recipients, {} inputs, total={} ZION, fee={} atomic, txid={}",
            result.recipients_paid,
            result.inputs_used,
            result.total_sent as f64 / 1_000_000.0,
            result.fee,
            result.transaction.id,
        );

        // 3. Submit signed TX to Core
        let submit_result = self
            .rpc
            .submit_signed_transaction(&result.transaction)
            .await
            .map_err(|e| anyhow!("Submit signed TX failed: {}", e))?;

        let status = submit_result
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if status != "OK" {
            let msg = submit_result
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(anyhow!("Core rejected payout TX: {}", msg));
        }

        tracing::info!(
            "✅ Batch payout TX submitted: txid={}, recipients={}",
            result.transaction.id,
            result.recipients_paid
        );

        Ok(result)
    }

    /// Send a single payout (wraps batch with 1 recipient).
    pub async fn send_single_payout(
        &self,
        to_address: &str,
        amount_atomic: u64,
    ) -> Result<String> {
        let recipients = vec![Recipient {
            address: to_address.to_string(),
            amount: amount_atomic,
        }];

        let result = self.send_batch_payout(recipients).await?;
        Ok(result.transaction.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_wallet_key_derivation() {
        // Use a known secret key and verify address derivation
        let secret_hex = "2a".repeat(32); // 32 bytes of 0x2a
        let sk_bytes = keys::from_hex(&secret_hex).unwrap();
        let mut secret = [0u8; 32];
        secret.copy_from_slice(&sk_bytes);

        let signing_key = SigningKey::from_bytes(&secret);
        let pk_hex = to_hex(signing_key.verifying_key().as_bytes());
        let addr = keys::address_from_public_key_hex(&pk_hex);

        // Address should be deterministic and valid
        assert!(keys::is_valid_zion1_address(&addr));
        assert!(addr.starts_with("zion1"));
        assert_eq!(addr.len(), 44);
    }

    #[test]
    fn test_pool_wallet_invalid_key() {
        // Can't create without RPC, but we can test key validation
        let bad_hex = "not_hex";
        let result = keys::from_hex(bad_hex);
        assert!(result.is_none());

        let short_hex = "aabb";
        let result = keys::from_hex(short_hex);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 2); // Too short for 32 bytes
    }
}
