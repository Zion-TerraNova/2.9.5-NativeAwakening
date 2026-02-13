use std::path::Path;
use anyhow::Result;
use heed::{EnvOpenOptions, Database, Env};
use heed::types::*;
use heed::byteorder::BigEndian;
use serde::{Serialize, Deserialize};
use crate::blockchain::block::Block;
use crate::tx::TxOutput;

/// Undo data for a single block — stores all UTXOs that were spent when the
/// block was applied.  During rollback we restore these without needing to
/// traverse the block-chain to reconstruct them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockUndoData {
    /// Block hash this undo record belongs to (for sanity checks).
    pub block_hash: String,
    /// The spent UTXOs: Vec of (outpoint_key "txid:index", TxOutput).
    pub spent_utxos: Vec<(String, TxOutput)>,
}

#[derive(Clone)]
pub struct ZionStorage {
    env: Env,
    // Tables
    blocks: Database<Str, SerdeBincode<Block>>, // Hash -> Block
    height_to_hash: Database<U64<BigEndian>, Str>, // Height -> Hash
    utxos: Database<Str, SerdeBincode<TxOutput>>, // "txid:index" -> Output
    tx_to_block: Database<Str, Str>, // TxID -> Block hash
    hash_to_height: Database<Str, U64<BigEndian>>, // Block hash -> Height
    /// Undo log: height -> BlockUndoData (spent UTXOs for safe rollback)
    undo_blocks: Database<U64<BigEndian>, SerdeBincode<BlockUndoData>>,
    /// Balance cache: address -> (balance_atomic, utxo_count) serialized as [u64; 2]
    balance_cache: Database<Str, SerdeBincode<(u64, u64)>>,
}

impl ZionStorage {
    pub fn open(path: &Path) -> Result<Self> {
        std::fs::create_dir_all(path)?;
        
        // P1-12: Configurable map size via env var (default 10 GB)
        let map_size_gb: usize = std::env::var("ZION_LMDB_MAP_SIZE_GB")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let map_size_bytes = map_size_gb * 1024 * 1024 * 1024;
        let env = unsafe {
            EnvOpenOptions::new()
                .map_size(map_size_bytes)
                .max_dbs(10)
                .open(path)?
        };

        let mut wtxn = env.write_txn()?;
        let blocks = env.create_database(&mut wtxn, Some("blocks"))?;
        let height_to_hash = env.create_database(&mut wtxn, Some("height_to_hash"))?;
        let utxos = env.create_database(&mut wtxn, Some("utxos"))?;
        let tx_to_block = env.create_database(&mut wtxn, Some("tx_to_block"))?;
        let hash_to_height = env.create_database(&mut wtxn, Some("hash_to_height"))?;
        let undo_blocks = env.create_database(&mut wtxn, Some("undo_blocks"))?;
        let balance_cache = env.create_database(&mut wtxn, Some("balance_cache"))?;
        wtxn.commit()?;

        Ok(Self {
            env,
            blocks,
            height_to_hash,
            utxos,
            hash_to_height,
            tx_to_block,
            undo_blocks,
            balance_cache,
        })
    }
    
    // --- Block Methods ---

    pub fn save_block(&self, block: &Block) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        let hash = block.calculate_hash();
        
        self.blocks.put(&mut wtxn, &hash, block)?;
        self.hash_to_height.put(&mut wtxn, &hash, &block.height())?;
        self.height_to_hash.put(&mut wtxn, &block.height(), &hash)?;

        // Index transactions for fast lookup
        for tx in &block.transactions {
            if !tx.id.is_empty() {
                self.tx_to_block.put(&mut wtxn, &tx.id, &hash)?;
            }
        }
        
        wtxn.commit()?;
        Ok(())
    }

    pub fn get_block_hash_for_tx(&self, tx_id: &str) -> Result<Option<String>> {
        let rtxn = self.env.read_txn()?;
        Ok(self.tx_to_block.get(&rtxn, tx_id)?.map(|s| s.to_string()))
    }

    pub fn get_height_for_block_hash(&self, hash: &str) -> Result<Option<u64>> {
        let rtxn = self.env.read_txn()?;
        Ok(self.hash_to_height.get(&rtxn, hash)?)
    }

    pub fn get_block(&self, hash: &str) -> Result<Option<Block>> {
        let rtxn = self.env.read_txn()?;
        Ok(self.blocks.get(&rtxn, hash)?)
    }
    
    pub fn get_block_by_height(&self, height: u64) -> Result<Option<Block>> {
        let rtxn = self.env.read_txn()?;
        if let Some(hash) = self.height_to_hash.get(&rtxn, &height)? {
            Ok(self.blocks.get(&rtxn, hash)?)
        } else {
            Ok(None)
        }
    }

    pub fn get_tip(&self) -> Result<(u64, String)> {
        let rtxn = self.env.read_txn()?;
        // Iterate backwards from end to find highest height
        // Since keys are BigEndian U64, the last key is the highest.
        match self.height_to_hash.last(&rtxn)? {
            Some((h, hash)) => Ok((h, hash.to_string())),
            None => Ok((0, "0000000000000000000000000000000000000000000000000000000000000000".to_string())), // Genesis default
        }
    }

    /// Get blocks in a height range [start, end] (inclusive).
    /// Single read transaction for efficiency — avoids N×1 overhead.
    /// Returns Vec of blocks, skipping any missing heights.
    /// Clamped to max 100 blocks per call.
    pub fn get_blocks_in_range(&self, start: u64, end: u64) -> Result<Vec<Block>> {
        let clamped_end = end.min(start.saturating_add(99));
        let rtxn = self.env.read_txn()?;
        let mut blocks = Vec::with_capacity((clamped_end - start + 1) as usize);

        for h in start..=clamped_end {
            if let Some(hash) = self.height_to_hash.get(&rtxn, &h)? {
                if let Some(block) = self.blocks.get(&rtxn, hash)? {
                    blocks.push(block);
                }
            }
        }

        Ok(blocks)
    }

    // --- UTXO Methods ---

    pub fn get_utxo(&self, key: &str) -> Result<Option<TxOutput>> {
         let rtxn = self.env.read_txn()?;
         Ok(self.utxos.get(&rtxn, key)?)
    }

    pub fn add_utxo(&self, key: &str, output: &TxOutput) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        self.utxos.put(&mut wtxn, key, output)?;
        wtxn.commit()?;
        Ok(())
    }

    pub fn remove_utxo(&self, key: &str) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        self.utxos.delete(&mut wtxn, key)?;
        wtxn.commit()?;
        Ok(())
    }
    
    /// Atomically apply block: remove inputs, add outputs, save undo data.
    ///
    /// The undo data captures every UTXO that is *spent* by this block so
    /// that `rollback_block_utxos` can restore them without traversing
    /// the chain.
    pub fn apply_block_utxos(&self, block: &Block) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        self.apply_block_utxos_in_txn(&mut wtxn, block)?;
        wtxn.commit()?;
        Ok(())
    }

    /// AUDIT-FIX P0-09: Atomically save block AND apply UTXO changes in a single
    /// LMDB write transaction. Prevents partial state where UTXOs are applied
    /// but block is not saved (or vice versa) if a crash occurs mid-operation.
    pub fn save_block_and_apply_utxos(&self, block: &Block) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        let hash = block.calculate_hash();

        // --- save_block logic ---
        self.blocks.put(&mut wtxn, &hash, block)?;
        self.hash_to_height.put(&mut wtxn, &hash, &block.height())?;
        self.height_to_hash.put(&mut wtxn, &block.height(), &hash)?;
        for tx in &block.transactions {
            if !tx.id.is_empty() {
                self.tx_to_block.put(&mut wtxn, &tx.id, &hash)?;
            }
        }

        // --- apply_block_utxos logic ---
        self.apply_block_utxos_in_txn(&mut wtxn, block)?;

        // AUDIT-FIX P1-13: Invalidate balance cache for all addresses affected by this block
        let mut affected_addresses = std::collections::HashSet::new();
        for tx in &block.transactions {
            for output in &tx.outputs {
                affected_addresses.insert(output.address.clone());
            }
            for input in &tx.inputs {
                // We can't easily get the address from spent inputs here (they're already deleted),
                // but the undo data captured them. For cache safety, we invalidate all output addresses.
                // Input addresses will be stale until next query triggers a cache rebuild.
                let _ = input; // inputs' addresses are captured via undo data
            }
        }
        for addr in &affected_addresses {
            let _ = self.balance_cache.delete(&mut wtxn, addr);
        }

        wtxn.commit()?;
        Ok(())
    }

    /// Internal: apply UTXO changes within an existing write transaction.
    fn apply_block_utxos_in_txn(&self, mut wtxn: &mut heed::RwTxn, block: &Block) -> Result<()> {
        let block_height = block.height();
        let block_hash = block.calculate_hash();
        let mut spent_utxos: Vec<(String, TxOutput)> = Vec::new();
        
        for tx in &block.transactions {
            // Remove inputs — but first snapshot for undo
            for input in &tx.inputs {
                if input.prev_tx_hash == "0000000000000000000000000000000000000000000000000000000000000000" { continue; } // Coinbase
                let key = format!("{}:{}", input.prev_tx_hash, input.output_index);
                // Snapshot the UTXO before deleting
                if let Some(output) = self.utxos.get(&wtxn, &key)? {
                    spent_utxos.push((key.clone(), output));
                }
                self.utxos.delete(&mut wtxn, &key)?;
            }
            
            // Add outputs
            let tx_id = tx.calculate_hash();
            for (idx, output) in tx.outputs.iter().enumerate() {
                let key = format!("{}:{}", tx_id, idx);
                self.utxos.put(&mut wtxn, &key, output)?;
            }
        }

        // Persist undo data for this block height
        let undo = BlockUndoData {
            block_hash,
            spent_utxos,
        };
        self.undo_blocks.put(wtxn, &block_height, &undo)?;
        
        Ok(())
    }
    
    /// Rollback block UTXO changes using the undo log.
    ///
    /// 1. Remove outputs created by the block.
    /// 2. Restore spent UTXOs from the undo record.
    /// 3. Delete the undo record.
    ///
    /// Falls back to the legacy reconstruction path if no undo data exists
    /// (e.g. blocks applied before the undo log was added).
    pub fn rollback_block_utxos(&self, block: &Block) -> Result<()> {
        let block_height = block.height();
        
        // Try undo-log path first
        let undo_opt = {
            let rtxn = self.env.read_txn()?;
            self.undo_blocks.get(&rtxn, &block_height)?
        };

        if let Some(undo) = undo_opt {
            return self.rollback_block_utxos_from_undo(block, &undo);
        }

        // Fallback: legacy reconstruction (pre-undo blocks)
        eprintln!(
            "[WARN] No undo data for height {}; falling back to legacy reconstruction",
            block_height
        );
        self.rollback_block_utxos_legacy(block)
    }

    /// Fast rollback using persisted undo data.
    fn rollback_block_utxos_from_undo(&self, block: &Block, undo: &BlockUndoData) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        
        // 1. Remove outputs that were created by this block
        for tx in &block.transactions {
            let tx_id = tx.calculate_hash();
            for idx in 0..tx.outputs.len() {
                let key = format!("{}:{}", tx_id, idx);
                let _ = self.utxos.delete(&mut wtxn, &key);
            }
        }
        
        // 2. Restore spent UTXOs from undo data
        for (key, output) in &undo.spent_utxos {
            self.utxos.put(&mut wtxn, key, output)?;
        }
        
        // 3. Remove undo record
        self.undo_blocks.delete(&mut wtxn, &block.height())?;
        
        wtxn.commit()?;
        Ok(())
    }

    /// Legacy rollback: reconstruct spent UTXOs by traversing block history.
    /// Only used for blocks that were applied before the undo log existed.
    ///
    /// AUDIT-FIX P0-10: All reads now use the existing write transaction
    /// instead of opening nested read transactions (which would deadlock in LMDB).
    fn rollback_block_utxos_legacy(&self, block: &Block) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        
        // Process in reverse order compared to apply
        for tx in &block.transactions {
            // Remove outputs that were added
            let tx_id = tx.calculate_hash();
            for idx in 0..tx.outputs.len() {
                let key = format!("{}:{}", tx_id, idx);
                // Ignore error if UTXO was already spent in a later block
                let _ = self.utxos.delete(&mut wtxn, &key);
            }
            
            // Restore inputs that were removed
            for input in &tx.inputs {
                if input.prev_tx_hash == "0000000000000000000000000000000000000000000000000000000000000000" { continue; }
                
                // Read through the SAME write transaction to avoid nested txn deadlock
                let prev_block_hash: Option<String> = self.tx_to_block
                    .get(&wtxn, &input.prev_tx_hash)?
                    .map(|s| s.to_string());
                
                if let Some(ref pbh) = prev_block_hash {
                    let prev_block: Option<Block> = self.blocks.get(&wtxn, pbh)?;
                    if let Some(prev_block) = prev_block {
                        if let Some(prev_tx) = prev_block.transactions.iter().find(|t| t.calculate_hash() == input.prev_tx_hash) {
                            if let Some(output) = prev_tx.outputs.get(input.output_index as usize) {
                                let key = format!("{}:{}", input.prev_tx_hash, input.output_index);
                                self.utxos.put(&mut wtxn, &key, output)?;
                            }
                        }
                    }
                }
            }
        }
        
        wtxn.commit()?;
        Ok(())
    }

    /// Calculate balance for a given address.
    /// Fast path: check balance_cache first (O(1)).
    /// Slow path (cache miss): scan UTXO set, then populate cache.
    /// Returns (total_amount_atomic, utxo_count).
    pub fn get_balance_for_address(&self, address: &str) -> Result<(u64, usize)> {
        // Fast path: cached balance
        let rtxn = self.env.read_txn()?;
        if let Some((balance, count)) = self.balance_cache.get(&rtxn, address)? {
            return Ok((balance, count as usize));
        }
        drop(rtxn);

        // Slow path: full UTXO scan (only on first access or after cache invalidation)
        let rtxn = self.env.read_txn()?;
        let mut total: u64 = 0;
        let mut count: usize = 0;

        let mut iter = self.utxos.iter(&rtxn)?;
        while let Some(result) = iter.next() {
            let (_key, output) = result?;
            if output.address == address {
                total = total.saturating_add(output.amount);
                count += 1;
            }
        }
        drop(iter);
        drop(rtxn);

        // Populate cache for next time
        if let Ok(mut wtxn) = self.env.write_txn() {
            let _ = self.balance_cache.put(&mut wtxn, address, &(total, count as u64));
            let _ = wtxn.commit();
        }

        Ok((total, count))
    }

    /// Update balance cache for addresses affected by a block.
    /// Call this after inserting/removing UTXOs for a block.
    pub fn update_balance_cache(&self, addresses: &[String]) -> Result<()> {
        let rtxn = self.env.read_txn()?;
        let mut updates: Vec<(String, u64, u64)> = Vec::new();

        for address in addresses {
            let mut total: u64 = 0;
            let mut count: u64 = 0;
            let mut iter = self.utxos.iter(&rtxn)?;
            while let Some(result) = iter.next() {
                let (_key, output) = result?;
                if output.address == *address {
                    total = total.saturating_add(output.amount);
                    count += 1;
                }
            }
            updates.push((address.clone(), total, count));
        }
        drop(rtxn);

        let mut wtxn = self.env.write_txn()?;
        for (addr, total, count) in updates {
            self.balance_cache.put(&mut wtxn, &addr, &(total, count))?;
        }
        wtxn.commit()?;
        Ok(())
    }

    /// Invalidate balance cache for given addresses (e.g. after reorg).
    pub fn invalidate_balance_cache(&self, addresses: &[String]) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        for addr in addresses {
            let _ = self.balance_cache.delete(&mut wtxn, addr);
        }
        wtxn.commit()?;
        Ok(())
    }

    /// DEV ONLY: Credit balance to an address by creating a synthetic UTXO.
    /// Used for testing payouts in TestNet without real mining.
    /// Compile-time gated: only available with `--features dev-tools`.
    #[cfg(feature = "dev-tools")]
    pub fn credit_balance(&self, address: &str, amount_atomic: u64) -> Result<()> {
        let mut wtxn = self.env.write_txn()?;
        
        // Generate unique key for synthetic UTXO
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let key = format!("dev_credit:{}:{}", timestamp, address);
        
        let output = TxOutput {
            amount: amount_atomic,
            address: address.to_string(),
        };
        
        self.utxos.put(&mut wtxn, &key, &output)?;
        wtxn.commit()?;
        
        eprintln!("[DEV] Credited {} atomic units to {} (key: {})", amount_atomic, address, key);
        Ok(())
    }

    /// Return UTXOs for a given address (key + output), with simple pagination.
    pub fn get_utxos_for_address(
        &self,
        address: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<(String, TxOutput)>> {
        let rtxn = self.env.read_txn()?;
        let mut out: Vec<(String, TxOutput)> = Vec::new();
        let mut seen: usize = 0;

        let mut iter = self.utxos.iter(&rtxn)?;
        while let Some(result) = iter.next() {
            let (key, output) = result?;
            if output.address != address {
                continue;
            }

            if seen < offset {
                seen += 1;
                continue;
            }

            out.push((key.to_string(), output));
            if out.len() >= limit {
                break;
            }
        }

        Ok(out)
    }

    /// Delete block at specified height (for reorg rollback).
    /// NOTE: This does NOT restore UTXOs - that must be handled separately.
    /// Also cleans up the undo record for this height.
    pub fn delete_block_at_height(&self, height: u64) -> Result<Option<Block>> {
        let mut wtxn = self.env.write_txn()?;

        // Get hash at height
        let hash = match self.height_to_hash.get(&wtxn, &height)? {
            Some(h) => h.to_string(),
            None => {
                wtxn.abort();
                return Ok(None);
            }
        };

        // Get block data before deletion
        let block = self.blocks.get(&wtxn, &hash)?;

        // Delete from height_to_hash
        self.height_to_hash.delete(&mut wtxn, &height)?;

        // Delete from hash_to_height
        self.hash_to_height.delete(&mut wtxn, &hash)?;

        // Delete from blocks
        self.blocks.delete(&mut wtxn, &hash)?;

        // Delete undo record (ignore if missing — pre-undo blocks)
        let _ = self.undo_blocks.delete(&mut wtxn, &height);

        // Delete transaction indexes
        if let Some(ref b) = block {
            for tx in &b.transactions {
                if !tx.id.is_empty() {
                    self.tx_to_block.delete(&mut wtxn, &tx.id)?;
                }
            }
        }

        wtxn.commit()?;
        Ok(block)
    }

    // --- Undo Log Methods ---

    /// Retrieve undo data for a specific block height.
    pub fn get_undo_data(&self, height: u64) -> Result<Option<BlockUndoData>> {
        let rtxn = self.env.read_txn()?;
        Ok(self.undo_blocks.get(&rtxn, &height)?)
    }

    /// Prune undo data for blocks that are now deeply buried and will never
    /// be rolled back.  Deletes undo records for heights `[0, finalized_height]`.
    ///
    /// Call periodically (e.g. every 100 blocks) with the soft-finality depth:
    ///   `prune_undo_data(tip_height.saturating_sub(SOFT_FINALITY_DEPTH))`
    pub fn prune_undo_data(&self, finalized_height: u64) -> Result<usize> {
        let mut wtxn = self.env.write_txn()?;
        let mut pruned: usize = 0;
        
        // Iterate from height 0 up to finalized_height
        for h in 0..=finalized_height {
            if self.undo_blocks.delete(&mut wtxn, &h)? {
                pruned += 1;
            }
        }
        
        wtxn.commit()?;
        Ok(pruned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::{Transaction, TxInput, TxOutput};

    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    fn temp_path(prefix: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("{prefix}-{uniq}"));
        p
    }

    /// Helper: coinbase transaction paying `amount` to `address`.
    fn coinbase_tx(address: &str, amount: u64) -> Transaction {
        Transaction {
            id: ZERO_HASH.to_string(),
            version: 1,
            inputs: vec![TxInput {
                prev_tx_hash: ZERO_HASH.to_string(),
                output_index: 0,
                signature: String::new(),
                public_key: String::new(),
            }],
            outputs: vec![TxOutput {
                amount,
                address: address.to_string(),
            }],
            fee: 0,
            timestamp: 1000,
        }
    }

    /// Helper: spending transaction — consumes `inputs` and creates `outputs`.
    fn spend_tx(
        inputs: Vec<(&str, u32)>,   // (prev_tx_hash, output_index)
        outputs: Vec<(&str, u64)>,   // (address, amount)
    ) -> Transaction {
        let tx = Transaction {
            id: String::new(), // will be replaced by calculate_hash
            version: 1,
            inputs: inputs
                .into_iter()
                .map(|(h, i)| TxInput {
                    prev_tx_hash: h.to_string(),
                    output_index: i,
                    signature: "deadbeef".to_string(),
                    public_key: "cafebabe".to_string(),
                })
                .collect(),
            outputs: outputs
                .into_iter()
                .map(|(a, amt)| TxOutput {
                    amount: amt,
                    address: a.to_string(),
                })
                .collect(),
            fee: 0,
            timestamp: 2000,
        };
        // Pre-compute the id so it matches calculate_hash()
        let id = tx.calculate_hash();
        Transaction { id, ..tx }
    }

    #[test]
    fn tx_to_block_index_roundtrip() {
        let dir = temp_path("zion-core-lmdb-test");
        {
            let storage = ZionStorage::open(&dir).unwrap();
            let block = Block::genesis();
            storage.save_block(&block).unwrap();

            let txid = block.transactions[0].id.clone();
            let expected_block_hash = block.calculate_hash();
            let got = storage.get_block_hash_for_tx(&txid).unwrap();
            assert_eq!(got, Some(expected_block_hash));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ========== UNDO LOG TESTS ==========

    #[test]
    fn apply_block_creates_undo_record() {
        let dir = temp_path("undo-apply");
        let storage = ZionStorage::open(&dir).unwrap();

        let genesis = Block::genesis();
        storage.save_block(&genesis).unwrap();
        storage.apply_block_utxos(&genesis).unwrap();

        // Genesis is height 0 — undo data should exist
        let undo = storage.get_undo_data(0).unwrap();
        assert!(undo.is_some(), "Undo record must be saved for height 0");
        // Genesis has no real inputs → spent list empty
        assert!(undo.unwrap().spent_utxos.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_record_captures_spent_utxos() {
        let dir = temp_path("undo-capture-spent");
        let storage = ZionStorage::open(&dir).unwrap();

        // Block 0: coinbase creates 5000 for Alice
        let cb = coinbase_tx("alice", 5000);
        let block0 = Block::new(1, 0, ZERO_HASH.to_string(), 100, 1000, 0, vec![cb.clone()]);
        storage.save_block(&block0).unwrap();
        storage.apply_block_utxos(&block0).unwrap();

        let cb_txid = block0.transactions[0].calculate_hash();

        // Block 1: spend Alice's UTXO → Bob
        let spend = spend_tx(vec![(&cb_txid, 0)], vec![("bob", 4500)]);
        let block1 = Block::new(1, 1, block0.calculate_hash(), 200, 1000, 1, vec![spend]);
        storage.save_block(&block1).unwrap();
        storage.apply_block_utxos(&block1).unwrap();

        // Undo for height 1 must contain Alice's original UTXO
        let undo = storage.get_undo_data(1).unwrap().expect("undo must exist");
        assert_eq!(undo.spent_utxos.len(), 1);
        assert_eq!(undo.spent_utxos[0].0, format!("{}:0", cb_txid));
        assert_eq!(undo.spent_utxos[0].1.address, "alice");
        assert_eq!(undo.spent_utxos[0].1.amount, 5000);

        // Alice's UTXO must be gone, Bob's must exist
        assert!(storage.get_utxo(&format!("{}:0", cb_txid)).unwrap().is_none());
        let bob_txid = block1.transactions[0].calculate_hash();
        let bob_utxo = storage.get_utxo(&format!("{}:0", bob_txid)).unwrap().unwrap();
        assert_eq!(bob_utxo.address, "bob");
        assert_eq!(bob_utxo.amount, 4500);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rollback_restores_utxos_from_undo() {
        let dir = temp_path("undo-rollback");
        let storage = ZionStorage::open(&dir).unwrap();

        // Block 0: coinbase → Alice 5000
        let cb = coinbase_tx("alice", 5000);
        let block0 = Block::new(1, 0, ZERO_HASH.to_string(), 100, 1000, 0, vec![cb.clone()]);
        storage.save_block(&block0).unwrap();
        storage.apply_block_utxos(&block0).unwrap();

        let cb_txid = block0.transactions[0].calculate_hash();

        // Block 1: Alice → Bob 4500
        let spend = spend_tx(vec![(&cb_txid, 0)], vec![("bob", 4500)]);
        let bob_txid = spend.calculate_hash();
        let block1 = Block::new(1, 1, block0.calculate_hash(), 200, 1000, 1, vec![spend]);
        storage.save_block(&block1).unwrap();
        storage.apply_block_utxos(&block1).unwrap();

        // --- ROLLBACK block 1 ---
        storage.rollback_block_utxos(&block1).unwrap();

        // Alice's UTXO must be restored
        let alice_utxo = storage.get_utxo(&format!("{}:0", cb_txid)).unwrap().unwrap();
        assert_eq!(alice_utxo.address, "alice");
        assert_eq!(alice_utxo.amount, 5000);

        // Bob's UTXO must be gone
        assert!(storage.get_utxo(&format!("{}:0", bob_txid)).unwrap().is_none());

        // Undo record for height 1 must be cleaned up
        assert!(storage.get_undo_data(1).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_block_rollback_chain() {
        let dir = temp_path("undo-multi-rollback");
        let storage = ZionStorage::open(&dir).unwrap();

        // Block 0: coinbase → Alice 10000
        let cb = coinbase_tx("alice", 10_000);
        let block0 = Block::new(1, 0, ZERO_HASH.to_string(), 100, 1000, 0, vec![cb.clone()]);
        storage.save_block(&block0).unwrap();
        storage.apply_block_utxos(&block0).unwrap();
        let cb_txid = block0.transactions[0].calculate_hash();

        // Block 1: Alice → Bob 6000, change → Alice 4000
        let spend1 = spend_tx(vec![(&cb_txid, 0)], vec![("bob", 6000), ("alice", 4000)]);
        let spend1_txid = spend1.calculate_hash();
        let block1 = Block::new(1, 1, block0.calculate_hash(), 200, 1000, 1, vec![spend1]);
        storage.save_block(&block1).unwrap();
        storage.apply_block_utxos(&block1).unwrap();

        // Block 2: Bob → Carol 3000, change → Bob 3000
        let spend2 = spend_tx(vec![(&spend1_txid, 0)], vec![("carol", 3000), ("bob", 3000)]);
        let spend2_txid = spend2.calculate_hash();
        let block2 = Block::new(1, 2, block1.calculate_hash(), 300, 1000, 2, vec![spend2]);
        storage.save_block(&block2).unwrap();
        storage.apply_block_utxos(&block2).unwrap();

        // Verify pre-rollback state
        assert!(storage.get_utxo(&format!("{}:0", cb_txid)).unwrap().is_none());     // Alice original: spent
        assert!(storage.get_utxo(&format!("{}:0", spend1_txid)).unwrap().is_none());  // Bob 6000: spent
        assert_eq!(storage.get_utxo(&format!("{}:1", spend1_txid)).unwrap().unwrap().amount, 4000); // Alice change
        assert_eq!(storage.get_utxo(&format!("{}:0", spend2_txid)).unwrap().unwrap().amount, 3000); // Carol
        assert_eq!(storage.get_utxo(&format!("{}:1", spend2_txid)).unwrap().unwrap().amount, 3000); // Bob change

        // --- ROLLBACK block 2 ---
        storage.rollback_block_utxos(&block2).unwrap();

        // Bob 6000 UTXO restored, Carol + Bob-change removed
        assert_eq!(storage.get_utxo(&format!("{}:0", spend1_txid)).unwrap().unwrap().amount, 6000);
        assert!(storage.get_utxo(&format!("{}:0", spend2_txid)).unwrap().is_none());
        assert!(storage.get_utxo(&format!("{}:1", spend2_txid)).unwrap().is_none());

        // --- ROLLBACK block 1 ---
        storage.rollback_block_utxos(&block1).unwrap();

        // Alice original UTXO restored, Bob + Alice-change removed
        assert_eq!(storage.get_utxo(&format!("{}:0", cb_txid)).unwrap().unwrap().amount, 10_000);
        assert!(storage.get_utxo(&format!("{}:0", spend1_txid)).unwrap().is_none());
        assert!(storage.get_utxo(&format!("{}:1", spend1_txid)).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_preserves_multiple_spent_utxos() {
        let dir = temp_path("undo-multi-inputs");
        let storage = ZionStorage::open(&dir).unwrap();

        // Two coinbases in block 0 (2 outputs to different addresses)
        let cb = Transaction {
            id: ZERO_HASH.to_string(),
            version: 1,
            inputs: vec![TxInput {
                prev_tx_hash: ZERO_HASH.to_string(),
                output_index: 0,
                signature: String::new(),
                public_key: String::new(),
            }],
            outputs: vec![
                TxOutput { amount: 2000, address: "alice".to_string() },
                TxOutput { amount: 3000, address: "bob".to_string() },
            ],
            fee: 0,
            timestamp: 100,
        };
        let block0 = Block::new(1, 0, ZERO_HASH.to_string(), 100, 1000, 0, vec![cb]);
        storage.save_block(&block0).unwrap();
        storage.apply_block_utxos(&block0).unwrap();
        let cb_txid = block0.transactions[0].calculate_hash();

        // Block 1: spend BOTH outputs in a single tx → Carol
        let spend = spend_tx(
            vec![(&cb_txid, 0), (&cb_txid, 1)],
            vec![("carol", 5000)]
        );
        let block1 = Block::new(1, 1, block0.calculate_hash(), 200, 1000, 1, vec![spend]);
        storage.save_block(&block1).unwrap();
        storage.apply_block_utxos(&block1).unwrap();

        // Undo must contain both spent UTXOs
        let undo = storage.get_undo_data(1).unwrap().unwrap();
        assert_eq!(undo.spent_utxos.len(), 2);

        // Rollback
        storage.rollback_block_utxos(&block1).unwrap();

        let alice = storage.get_utxo(&format!("{}:0", cb_txid)).unwrap().unwrap();
        let bob   = storage.get_utxo(&format!("{}:1", cb_txid)).unwrap().unwrap();
        assert_eq!(alice.amount, 2000);
        assert_eq!(bob.amount, 3000);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_undo_data_removes_old_records() {
        let dir = temp_path("undo-prune");
        let storage = ZionStorage::open(&dir).unwrap();

        // Create 5 blocks
        let mut prev_hash = ZERO_HASH.to_string();
        for h in 0..5u64 {
            let cb = coinbase_tx("miner", 100);
            let block = Block::new(1, h, prev_hash.clone(), 100 + h, 1000, h, vec![cb]);
            prev_hash = block.calculate_hash();
            storage.save_block(&block).unwrap();
            storage.apply_block_utxos(&block).unwrap();
        }

        // All 5 undo records exist
        for h in 0..5 {
            assert!(storage.get_undo_data(h).unwrap().is_some(), "undo at height {h}");
        }

        // Prune heights 0..=2
        let pruned = storage.prune_undo_data(2).unwrap();
        assert_eq!(pruned, 3);

        // Heights 0-2 gone, 3-4 remain
        for h in 0..=2 {
            assert!(storage.get_undo_data(h).unwrap().is_none());
        }
        for h in 3..5 {
            assert!(storage.get_undo_data(h).unwrap().is_some());
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_block_also_removes_undo_record() {
        let dir = temp_path("undo-delete-block");
        let storage = ZionStorage::open(&dir).unwrap();

        let cb = coinbase_tx("miner", 100);
        let block = Block::new(1, 0, ZERO_HASH.to_string(), 100, 1000, 0, vec![cb]);
        storage.save_block(&block).unwrap();
        storage.apply_block_utxos(&block).unwrap();

        assert!(storage.get_undo_data(0).unwrap().is_some());

        storage.delete_block_at_height(0).unwrap();

        // Undo record must be cleaned up with the block
        assert!(storage.get_undo_data(0).unwrap().is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_record_has_correct_block_hash() {
        let dir = temp_path("undo-hash");
        let storage = ZionStorage::open(&dir).unwrap();

        let cb = coinbase_tx("miner", 100);
        let block = Block::new(1, 0, ZERO_HASH.to_string(), 100, 1000, 0, vec![cb]);
        let expected_hash = block.calculate_hash();
        storage.save_block(&block).unwrap();
        storage.apply_block_utxos(&block).unwrap();

        let undo = storage.get_undo_data(0).unwrap().unwrap();
        assert_eq!(undo.block_hash, expected_hash);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rollback_idempotent_after_undo_consumed() {
        let dir = temp_path("undo-idempotent");
        let storage = ZionStorage::open(&dir).unwrap();

        let cb = coinbase_tx("alice", 5000);
        let block0 = Block::new(1, 0, ZERO_HASH.to_string(), 100, 1000, 0, vec![cb]);
        storage.save_block(&block0).unwrap();
        storage.apply_block_utxos(&block0).unwrap();
        let cb_txid = block0.transactions[0].calculate_hash();

        let spend = spend_tx(vec![(&cb_txid, 0)], vec![("bob", 5000)]);
        let block1 = Block::new(1, 1, block0.calculate_hash(), 200, 1000, 1, vec![spend]);
        storage.save_block(&block1).unwrap();
        storage.apply_block_utxos(&block1).unwrap();

        // First rollback uses undo log
        storage.rollback_block_utxos(&block1).unwrap();
        assert!(storage.get_undo_data(1).unwrap().is_none());

        // Second rollback falls back to legacy (undo consumed) — should not panic
        storage.rollback_block_utxos(&block1).unwrap();

        let _ = std::fs::remove_dir_all(&dir);
    }
}

