use crate::mempool::Mempool;
use crate::blockchain::fee;

/// Evict transactions until the mempool size is <= `max_txs`.
///
/// Policy: evict **lowest fee rate** first; ties break by oldest timestamp.
/// Freed outpoints are removed from the spent-outpoints set.
/// Returns number of evicted transactions.
pub fn evict_to_limit(mempool: &Mempool, max_txs: usize) -> usize {
	let mut pool = mempool.transactions.write().unwrap();
	if pool.len() <= max_txs {
		return 0;
	}

	let mut candidates: Vec<(String, u64, u64)> = pool
		.values()
		.map(|tx| {
			let size = fee::estimate_tx_size(tx.inputs.len(), tx.outputs.len()) as u64;
			let rate = if size > 0 { tx.fee / size } else { 0 };
			(tx.id.clone(), rate, tx.timestamp)
		})
		.collect();

	// Sort ascending by fee rate, then by timestamp (oldest first)
	candidates.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)));

	let to_remove = pool.len().saturating_sub(max_txs);
	let mut spent = mempool.spent_outpoints.write().unwrap();

	for (tx_id, _, _) in candidates.into_iter().take(to_remove) {
		if let Some(tx) = pool.remove(&tx_id) {
			// Free up outpoints
			for input in &tx.inputs {
				let outpoint = format!("{}:{}", input.prev_tx_hash, input.output_index);
				spent.remove(&outpoint);
			}
		}
	}

	to_remove
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::tx::{Transaction, TxInput, TxOutput};

	fn tx(id: &str, fee: u64, timestamp: u64) -> Transaction {
		Transaction {
			id: id.to_string(),
			version: 1,
			inputs: vec![TxInput {
				prev_tx_hash: format!("utxo_{}", id),
				output_index: 0,
				signature: "a".repeat(128),
				public_key: "b".repeat(64),
			}],
			outputs: vec![TxOutput {
				amount: 1_000_000,
				address: "zion1test".to_string(),
			}],
			fee,
			timestamp,
		}
	}

	#[test]
	fn evicts_lowest_fee_rate_first() {
		let mempool = Mempool::new();
		mempool.add_transaction(tx("a", 1_000, 100)); // low fee rate
		mempool.add_transaction(tx("b", 50_000, 100)); // high fee rate
		mempool.add_transaction(tx("c", 5_000, 100)); // medium fee rate

		let evicted = evict_to_limit(&mempool, 2);
		assert_eq!(evicted, 1);
		assert!(mempool.get_transaction("a").is_none()); // lowest rate evicted
		assert!(mempool.get_transaction("b").is_some());
		assert!(mempool.get_transaction("c").is_some());
	}

	#[test]
	fn breaks_fee_rate_ties_by_oldest() {
		let mempool = Mempool::new();
		mempool.add_transaction(tx("old", 1_000, 10));
		mempool.add_transaction(tx("new", 1_000, 20));

		let evicted = evict_to_limit(&mempool, 1);
		assert_eq!(evicted, 1);
		assert!(mempool.get_transaction("old").is_none()); // oldest evicted
		assert!(mempool.get_transaction("new").is_some());
	}

	#[test]
	fn no_eviction_when_under_limit() {
		let mempool = Mempool::new();
		mempool.add_transaction(tx("a", 1_000, 100));
		let evicted = evict_to_limit(&mempool, 10);
		assert_eq!(evicted, 0);
		assert_eq!(mempool.size(), 1);
	}

	#[test]
	fn eviction_clears_outpoints() {
		let mempool = Mempool::new();
		mempool.add_transaction(tx("a", 1_000, 100));
		mempool.add_transaction(tx("b", 50_000, 100));

		// Before eviction: outpoint for "a" is tracked
		assert!(mempool.is_outpoint_spent("utxo_a", 0));

		evict_to_limit(&mempool, 1);

		// After eviction: outpoint for "a" should be freed
		assert!(!mempool.is_outpoint_spent("utxo_a", 0));
		// "b" still tracked
		assert!(mempool.is_outpoint_spent("utxo_b", 0));
	}
}


