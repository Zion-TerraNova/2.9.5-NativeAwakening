use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use crate::tx::Transaction;
use crate::blockchain::fee;

/// Maximum number of transactions in the mempool.
/// Beyond this limit, lowest-fee-rate transactions are evicted.
pub const MAX_MEMPOOL_SIZE: usize = 10_000;

/// AUDIT-FIX P1-15: Maximum total byte size of the mempool.
/// Prevents memory exhaustion via many large transactions.
pub const MAX_MEMPOOL_BYTES: usize = 20 * 1024 * 1024; // 20 MB

/// Result of attempting to add a transaction to the mempool.
#[derive(Debug, Clone, PartialEq)]
pub enum MempoolError {
    /// Transaction already in pool
    Duplicate,
    /// Fee too low (below MIN_TX_FEE or below min fee rate)
    FeeTooLow(String),
    /// Transaction size exceeds MAX_TX_SIZE_BYTES
    TxTooLarge(usize),
    /// One or more inputs are already spent by another mempool tx (double-spend)
    DoubleSpend(String),
    /// Output amount validation failed
    InvalidOutputAmount(String),
}

impl std::fmt::Display for MempoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MempoolError::Duplicate => write!(f, "Transaction already in mempool"),
            MempoolError::FeeTooLow(msg) => write!(f, "Fee too low: {}", msg),
            MempoolError::TxTooLarge(size) => write!(f, "Transaction too large: {} bytes (max {})", size, fee::MAX_TX_SIZE_BYTES),
            MempoolError::DoubleSpend(outpoint) => write!(f, "Double-spend detected: input {} already spent", outpoint),
            MempoolError::InvalidOutputAmount(msg) => write!(f, "Invalid output amount: {}", msg),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Mempool {
    /// Map transaction ID → Transaction
    pub transactions: Arc<RwLock<HashMap<String, Transaction>>>,
    /// Set of spent outpoints: "prev_tx_hash:output_index"
    /// Used for O(1) double-spend detection.
    pub(crate) spent_outpoints: Arc<RwLock<HashSet<String>>>,
}

impl Mempool {
    pub fn new() -> Self {
        Self {
            transactions: Arc::new(RwLock::new(HashMap::new())),
            spent_outpoints: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Add a transaction to the mempool with full validation.
    ///
    /// Checks:
    /// 1. Not a duplicate
    /// 2. Fee meets minimum (absolute + fee rate)
    /// 3. Transaction size within bounds
    /// 4. No double-spend with existing mempool txs
    /// 5. Output amounts within bounds
    /// 6. Evicts lowest-fee-rate tx if pool is full
    pub fn add_transaction_validated(&self, tx: Transaction) -> Result<(), MempoolError> {
        let tx_size = fee::estimate_tx_size(tx.inputs.len(), tx.outputs.len());

        // 1. Size check
        if tx_size > fee::MAX_TX_SIZE_BYTES {
            return Err(MempoolError::TxTooLarge(tx_size));
        }

        // 2. Fee check
        if let Err(msg) = fee::validate_fee(tx.fee, tx_size) {
            return Err(MempoolError::FeeTooLow(msg));
        }

        // 3. Output amount check
        let outputs: Vec<(u64, &str)> = tx.outputs.iter()
            .map(|o| (o.amount, o.address.as_str()))
            .collect();
        if let Err(msg) = fee::validate_output_amounts(&outputs) {
            return Err(MempoolError::InvalidOutputAmount(msg));
        }

        let mut pool = self.transactions.write().unwrap();
        let mut spent = self.spent_outpoints.write().unwrap();

        // 4. Duplicate check
        if pool.contains_key(&tx.id) {
            return Err(MempoolError::Duplicate);
        }

        // 5. Double-spend check
        for input in &tx.inputs {
            let outpoint = format!("{}:{}", input.prev_tx_hash, input.output_index);
            if spent.contains(&outpoint) {
                return Err(MempoolError::DoubleSpend(outpoint));
            }
        }

        // 6. Register spent outpoints
        for input in &tx.inputs {
            let outpoint = format!("{}:{}", input.prev_tx_hash, input.output_index);
            spent.insert(outpoint);
        }

        pool.insert(tx.id.clone(), tx);

        // 7. Evict if over count limit
        if pool.len() > MAX_MEMPOOL_SIZE {
            drop(pool);
            drop(spent);
            self.evict_to_limit(MAX_MEMPOOL_SIZE);
        } else {
            // AUDIT-FIX P1-15: Also evict if over byte size limit
            let estimated_bytes: usize = pool.values()
                .map(|t| fee::estimate_tx_size(t.inputs.len(), t.outputs.len()))
                .sum();
            if estimated_bytes > MAX_MEMPOOL_BYTES {
                drop(pool);
                drop(spent);
                // Evict ~10% to avoid constant eviction churn
                let target = MAX_MEMPOOL_SIZE.min(self.size().saturating_sub(self.size() / 10));
                self.evict_to_limit(target);
            }
        }

        Ok(())
    }

    /// Legacy add (no fee/double-spend checks). Used for dev RPC and test ops.
    /// AUDIT-FIX P1-16: Should be replaced with add_transaction_validated() in
    /// production paths. Retained for dev-tools RPC sendTransaction endpoint.
    /// TODO: Remove once RPC sendTransaction builds proper signed TXs.
    #[deprecated(note = "Use add_transaction_validated() for production code")]
    pub fn add_transaction(&self, tx: Transaction) -> bool {
        let mut pool = self.transactions.write().unwrap();
        if pool.contains_key(&tx.id) {
            return false;
        }
        // Register outpoints
        let mut spent = self.spent_outpoints.write().unwrap();
        for input in &tx.inputs {
            let outpoint = format!("{}:{}", input.prev_tx_hash, input.output_index);
            spent.insert(outpoint);
        }
        pool.insert(tx.id.clone(), tx);
        true
    }

    pub fn get_transaction(&self, tx_id: &str) -> Option<Transaction> {
        let pool = self.transactions.read().unwrap();
        pool.get(tx_id).cloned()
    }

    pub fn remove_transaction(&self, tx_id: &str) {
        let mut pool = self.transactions.write().unwrap();
        if let Some(tx) = pool.remove(tx_id) {
            let mut spent = self.spent_outpoints.write().unwrap();
            for input in &tx.inputs {
                let outpoint = format!("{}:{}", input.prev_tx_hash, input.output_index);
                spent.remove(&outpoint);
            }
        }
    }

    pub fn size(&self) -> usize {
        let pool = self.transactions.read().unwrap();
        pool.len()
    }

    /// Get all transactions sorted by fee rate (highest first) for block template.
    pub fn get_sorted_by_fee_rate(&self) -> Vec<Transaction> {
        let pool = self.transactions.read().unwrap();
        let mut txs: Vec<Transaction> = pool.values().cloned().collect();

        txs.sort_by(|a, b| {
            let size_a = fee::estimate_tx_size(a.inputs.len(), a.outputs.len()) as u64;
            let size_b = fee::estimate_tx_size(b.inputs.len(), b.outputs.len()) as u64;
            let rate_a = if size_a > 0 { a.fee / size_a } else { 0 };
            let rate_b = if size_b > 0 { b.fee / size_b } else { 0 };
            // Descending: highest fee rate first
            rate_b.cmp(&rate_a).then(a.timestamp.cmp(&b.timestamp))
        });

        txs
    }

    /// Get all transactions (unsorted).
    pub fn get_all(&self) -> Vec<Transaction> {
        let pool = self.transactions.read().unwrap();
        pool.values().cloned().collect()
    }

    /// Check if an outpoint is already spent by a mempool transaction.
    pub fn is_outpoint_spent(&self, prev_tx_hash: &str, output_index: u32) -> bool {
        let spent = self.spent_outpoints.read().unwrap();
        let outpoint = format!("{}:{}", prev_tx_hash, output_index);
        spent.contains(&outpoint)
    }

    pub fn evict_to_limit(&self, max_txs: usize) -> usize {
        crate::mempool::eviction::evict_to_limit(self, max_txs)
    }

    /// Restore transactions from rolled-back blocks back into the mempool.
    ///
    /// Used during chain reorganization: transactions from old blocks that are
    /// not present in the new fork are returned to the mempool so they can be
    /// re-mined. Coinbase transactions are skipped (they are block-specific).
    ///
    /// Double-spend detection applies: if a restored tx conflicts with an
    /// existing mempool tx, the restored tx is silently dropped.
    pub fn restore_transactions(&self, transactions: &[crate::tx::Transaction]) {
        let mut pool = self.transactions.write().unwrap();
        let mut spent = self.spent_outpoints.write().unwrap();
        let zero_hash = "0000000000000000000000000000000000000000000000000000000000000000";

        for tx in transactions {
            // Skip coinbase transactions
            let is_coinbase = tx.inputs.is_empty()
                || tx.inputs.iter().all(|i| i.prev_tx_hash == zero_hash);
            if is_coinbase {
                continue;
            }

            // Skip if already in mempool
            if pool.contains_key(&tx.id) {
                continue;
            }

            // Check for double-spend conflicts with existing mempool txs
            let has_conflict = tx.inputs.iter().any(|input| {
                let outpoint = format!("{}:{}", input.prev_tx_hash, input.output_index);
                spent.contains(&outpoint)
            });
            if has_conflict {
                continue;
            }

            // Register outpoints and add to pool
            for input in &tx.inputs {
                let outpoint = format!("{}:{}", input.prev_tx_hash, input.output_index);
                spent.insert(outpoint);
            }
            pool.insert(tx.id.clone(), tx.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::{Transaction, TxInput, TxOutput};

    fn make_tx(id: &str, fee: u64, inputs: Vec<(&str, u32)>, outputs: Vec<u64>) -> Transaction {
        Transaction {
            id: id.to_string(),
            version: 1,
            inputs: inputs.iter().map(|(hash, idx)| TxInput {
                prev_tx_hash: hash.to_string(),
                output_index: *idx,
                signature: "a".repeat(128),
                public_key: "b".repeat(64),
            }).collect(),
            outputs: outputs.iter().map(|amt| TxOutput {
                amount: *amt,
                address: "zion1test".to_string(),
            }).collect(),
            fee,
            timestamp: 100,
        }
    }

    #[test]
    fn test_add_valid_tx() {
        let pool = Mempool::new();
        let tx = make_tx("tx1", 5_000, vec![("aaa", 0)], vec![1_000_000]);
        assert!(pool.add_transaction_validated(tx).is_ok());
        assert_eq!(pool.size(), 1);
    }

    #[test]
    fn test_reject_duplicate() {
        let pool = Mempool::new();
        let tx = make_tx("tx1", 5_000, vec![("aaa", 0)], vec![1_000_000]);
        assert!(pool.add_transaction_validated(tx.clone()).is_ok());
        assert_eq!(pool.add_transaction_validated(tx), Err(MempoolError::Duplicate));
    }

    #[test]
    fn test_reject_fee_too_low() {
        let pool = Mempool::new();
        let tx = make_tx("tx1", 100, vec![("aaa", 0)], vec![1_000_000]); // fee=100 < 1000
        assert!(matches!(pool.add_transaction_validated(tx), Err(MempoolError::FeeTooLow(_))));
    }

    #[test]
    fn test_reject_double_spend() {
        let pool = Mempool::new();
        let tx1 = make_tx("tx1", 5_000, vec![("utxo1", 0)], vec![1_000_000]);
        let tx2 = make_tx("tx2", 5_000, vec![("utxo1", 0)], vec![1_000_000]); // same input!
        assert!(pool.add_transaction_validated(tx1).is_ok());
        assert!(matches!(pool.add_transaction_validated(tx2), Err(MempoolError::DoubleSpend(_))));
    }

    #[test]
    fn test_double_spend_cleared_on_remove() {
        let pool = Mempool::new();
        let tx1 = make_tx("tx1", 5_000, vec![("utxo1", 0)], vec![1_000_000]);
        pool.add_transaction_validated(tx1).unwrap();

        // Remove tx1 — outpoint should be freed
        pool.remove_transaction("tx1");

        // Now a tx spending the same input should succeed
        let tx2 = make_tx("tx2", 5_000, vec![("utxo1", 0)], vec![1_000_000]);
        assert!(pool.add_transaction_validated(tx2).is_ok());
    }

    #[test]
    fn test_reject_zero_output() {
        let pool = Mempool::new();
        let tx = make_tx("tx1", 5_000, vec![("aaa", 0)], vec![0]);
        assert!(matches!(pool.add_transaction_validated(tx), Err(MempoolError::InvalidOutputAmount(_))));
    }

    #[test]
    fn test_sorted_by_fee_rate() {
        let pool = Mempool::new();

        // tx1: low fee
        let tx1 = make_tx("tx1", 1_000, vec![("a", 0)], vec![1_000_000]);
        // tx2: high fee
        let tx2 = make_tx("tx2", 100_000, vec![("b", 0)], vec![1_000_000]);
        // tx3: medium fee
        let tx3 = make_tx("tx3", 10_000, vec![("c", 0)], vec![1_000_000]);

        pool.add_transaction_validated(tx1).unwrap();
        pool.add_transaction_validated(tx2).unwrap();
        pool.add_transaction_validated(tx3).unwrap();

        let sorted = pool.get_sorted_by_fee_rate();
        assert_eq!(sorted.len(), 3);
        assert_eq!(sorted[0].id, "tx2"); // highest fee rate first
        assert_eq!(sorted[1].id, "tx3");
        assert_eq!(sorted[2].id, "tx1");
    }

    #[test]
    fn test_outpoint_tracking() {
        let pool = Mempool::new();
        let tx = make_tx("tx1", 5_000, vec![("utxo_abc", 2)], vec![1_000_000]);
        pool.add_transaction_validated(tx).unwrap();

        assert!(pool.is_outpoint_spent("utxo_abc", 2));
        assert!(!pool.is_outpoint_spent("utxo_abc", 0));
        assert!(!pool.is_outpoint_spent("other", 2));
    }
}

