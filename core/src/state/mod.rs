use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, AtomicBool};
use std::path::Path;
use tokio::sync::broadcast;
use crate::mempool::Mempool;
use crate::tx::{Transaction, TxOutput};
use crate::storage::ZionStorage;
use crate::metrics::Metrics;
use crate::p2p::peers::PeerManager;

pub struct Inner {
    pub height: AtomicU64,
    pub difficulty: AtomicU64,
    pub tip: Mutex<String>,
    pub mempool: Mempool,
    pub storage: ZionStorage,
    pub tx_broadcaster: broadcast::Sender<Transaction>, // For notifying P2P about new txs
    pub block_broadcaster: broadcast::Sender<(u64, String)>, // For notifying P2P about new blocks (Height, Hash)
    pub metrics: Arc<Metrics>, // Performance metrics and health monitoring
    /// AUDIT-FIX P0-08: Mutex to serialize block processing — prevents concurrent
    /// process_block() calls from corrupting UTXO state or double-applying blocks.
    pub block_processing_lock: std::sync::Mutex<()>,
    /// Mutex to serialize all reorg operations — prevents concurrent reorgs from racing
    pub reorg_lock: tokio::sync::Mutex<()>,
    /// Flag to signal that a reorg is in progress — prevents duplicate fork requests
    pub reorging: AtomicBool,
    /// P2P peer manager — set after p2p::start() via set_peer_manager()
    pub peer_manager: Mutex<Option<Arc<PeerManager>>>,
}

pub type State = Arc<Inner>;

use crate::blockchain::block::Block;
use crate::blockchain::validation;

impl Inner {
    pub fn new(db_path: &str) -> State {
        println!("Opening storage at {}", db_path);
        let storage = ZionStorage::open(Path::new(db_path)).expect("Failed to open storage");
        
        // Channels
        let (tx_tx, _) = broadcast::channel(1000);
        let (block_tx, _) = broadcast::channel(100);
        
        // Check if Genesis exists
        let (height, tip) = storage.get_tip().unwrap_or((0, "0000000000000000000000000000000000000000000000000000000000000000".to_string()));
        
        let (final_height, final_tip) = if height == 0 {
             // Create and persist Genesis block
             let genesis_ts = crate::network::get_network().genesis_timestamp();
             println!("Genesis timestamp: {} ({})", genesis_ts, crate::network::get_network().name());
             let genesis_block = Block::new(
                 1, // version
                 0, // height
                 "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
                 genesis_ts,
                 1000, // initial difficulty (matches MIN_DIFFICULTY from consensus)
                 0, // nonce
                 vec![], // no transactions (premine is UTXO set only)
             );
             
             let genesis_hash = genesis_block.calculate_hash();
             storage.save_block(&genesis_block).expect("Failed to save Genesis block");
             
             // Inject Genesis Premine
             let all_premine = crate::premine::get_all_premine_addresses();
             let genesis_tx_hash = "0000000000000000000000000000000000000000000000000000000000000000";
             
             // Check if already injected (idempotency)
             let first_key = format!("{}:{}", genesis_tx_hash, 0);
             if storage.get_utxo(&first_key).unwrap().is_none() {
                 println!("Initializing Genesis Premine/UTXO Set...");
                 for (i, entry) in all_premine.iter().enumerate() {
                    let key = format!("{}:{}", genesis_tx_hash, i);
                    storage.add_utxo(&key, &TxOutput {
                        amount: entry.amount,
                        address: entry.address.clone(),
                    }).unwrap();
                 }
             }
             
             println!("Genesis block created: {}", genesis_hash);
             (0, genesis_hash)
        } else {
             (height, tip)
        };

        let initial_difficulty: u64 = if final_height == 0 {
            // Genesis block difficulty
            1000
        } else {
            // Match the stored tip block difficulty
            storage
                .get_block_by_height(final_height)
                .ok()
                .flatten()
                .map(|b| b.difficulty())
                .unwrap_or(10_000)
        };

        let metrics = Metrics::new();
        metrics.current_height.store(final_height, std::sync::atomic::Ordering::Relaxed);
        metrics.current_difficulty
            .store(initial_difficulty, std::sync::atomic::Ordering::Relaxed);

        Arc::new(Self {
            height: AtomicU64::new(final_height),
            difficulty: AtomicU64::new(initial_difficulty), // Eventually stored in DB/Block Header
            tip: Mutex::new(final_tip),
            mempool: Mempool::new(),
            storage,
            tx_broadcaster: tx_tx,
            block_broadcaster: block_tx,
            metrics,
            reorg_lock: tokio::sync::Mutex::new(()),
            block_processing_lock: std::sync::Mutex::new(()),
            reorging: AtomicBool::new(false),
            peer_manager: Mutex::new(None),
        })
    }

    pub fn process_block(&self, block: Block) -> Result<(u64, String), String> {
        // AUDIT-FIX P0-08: Serialize block processing to prevent concurrent UTXO corruption
        let _block_lock = self.block_processing_lock.lock()
            .map_err(|e| format!("Block processing lock poisoned: {}", e))?;
        
        let start_time = std::time::Instant::now();
        
        // 1. Validate Block Header & PoW
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Time error: {e}"))?
            .as_secs();

        let prev_block = if block.height() == 0 {
            None
        } else {
            self.storage
                .get_block_by_height(block.height() - 1)
                .map_err(|e| format!("Storage error loading previous block: {e}"))?
        };

        if block.height() > 0 && prev_block.is_none() {
            self.metrics
                .blocks_rejected
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Err(format!(
                "Previous block at height {} not found",
                block.height().saturating_sub(1)
            ));
        }

        if let Err(e) = validation::validate_block(&block, prev_block.as_ref(), now) {
            self.metrics.blocks_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Err(e);
        }

        // 2. Validate Transactions & Check Inputs (UTXO + Balance)
        // Track outpoints consumed within this block to detect intra-block double-spends
        let mut block_spent_outpoints = std::collections::HashSet::new();
        
        for (tx_index, tx) in block.transactions.iter().enumerate() {
             // Coinbase rules are contextual: only the first tx may be coinbase-like.
             let zero_hash = "0000000000000000000000000000000000000000000000000000000000000000";
             let is_coinbase_like =
                 tx.inputs.is_empty() || tx.inputs.iter().all(|i| i.prev_tx_hash == zero_hash);

             if tx_index == 0 {
                 if !is_coinbase_like {
                     return Err("First transaction must be coinbase".to_string());
                 }
                 continue;
             }

             if is_coinbase_like {
                 return Err("Non-coinbase tx must have inputs".to_string());
             }

             if tx.outputs.is_empty() {
                 return Err("Non-coinbase tx must have outputs".to_string());
             }

             if tx.inputs.iter().any(|i| i.prev_tx_hash == zero_hash) {
                 return Err("Non-coinbase tx contains coinbase input".to_string());
             }
             
             let mut input_sum: u64 = 0;
             
             for input in &tx.inputs {
                 let key = format!("{}:{}", input.prev_tx_hash, input.output_index);
                 
                 // Intra-block double-spend check: ensure no two txs in the same
                 // block consume the same outpoint
                 if !block_spent_outpoints.insert(key.clone()) {
                     return Err(format!(
                         "Intra-block double-spend: outpoint {} consumed by multiple txs in block at height {}",
                         key, block.height()
                     ));
                 }
                 
                 // Check UTXO exists
                 let utxo = match self.storage.get_utxo(&key).map_err(|e| format!("Storage error: {}", e))? {
                     Some(u) => u,
                     None => return Err(format!("Missing UTXO: {} for tx {}", key, tx.id)),
                 };
                 
                 // Verify ownership (address from UTXO must match signature)
                 let addr_from_pubkey = crate::crypto::keys::address_from_public_key_hex(&input.public_key);
                 if utxo.address != addr_from_pubkey {
                     return Err(format!(
                         "UTXO {} address mismatch: {} != {}",
                         key, utxo.address, addr_from_pubkey
                     ));
                 }
                 
                 // Burn address check: UTXOs at burn address are permanently unspendable
                 if crate::blockchain::burn::is_burn_address(&utxo.address) {
                     return Err(format!(
                         "Cannot spend UTXO {} — burn address is permanently unspendable (tx {})",
                         key, tx.id
                     ));
                 }
                 
                 // Coinbase maturity check: coinbase outputs must wait COINBASE_MATURITY blocks
                 if let Ok(Some(src_block_hash)) = self.storage.get_block_hash_for_tx(&input.prev_tx_hash) {
                     if let Ok(Some(src_height)) = self.storage.get_height_for_block_hash(&src_block_hash) {
                         // Check if the source transaction is the coinbase (first tx in block)
                         if let Ok(Some(src_block)) = self.storage.get_block_by_height(src_height) {
                             let is_coinbase_tx = !src_block.transactions.is_empty()
                                 && src_block.transactions[0].calculate_hash() == input.prev_tx_hash;
                             
                             if is_coinbase_tx {
                                 let maturity = block.height().saturating_sub(src_height);
                                 if maturity < validation::COINBASE_MATURITY {
                                     return Err(format!(
                                         "Coinbase UTXO {} not mature: {} confirmations, need {} (tx {})",
                                         key, maturity, validation::COINBASE_MATURITY, tx.id
                                     ));
                                 }
                             }
                         }
                     }
                 }
                 
                 input_sum = input_sum.checked_add(utxo.amount)
                     .ok_or_else(|| format!("Input sum overflow in tx {}", tx.id))?;
             }
             
             let output_sum: u64 = tx.outputs.iter().map(|o| o.amount).sum();
             
             // Check balance (input >= output + fee)
             if input_sum < output_sum.checked_add(tx.fee).unwrap_or(u64::MAX) {
                 return Err(format!(
                     "Insufficient balance in tx {}: input {} < output {} + fee {}",
                     tx.id, input_sum, output_sum, tx.fee
                 ));
             }
        }
        
        // AUDIT-FIX P0-09: Apply UTXOs and save block atomically in a single
        // LMDB write transaction. Prevents partial state on crash.
        if let Err(e) = self.storage.save_block_and_apply_utxos(&block) {
            return Err(format!("Database error saving block + UTXOs atomically: {}", e));
        }

        // 3. Update Tip
        let new_hash = block.calculate_hash();
        let new_height = block.height();
        
        // Calculate next block difficulty using LWMA (60-block window)
        let next_difficulty = if block.height() == 0 {
            block.difficulty() // genesis → keep initial difficulty
        } else {
            // Build LWMA window: collect last N+1 blocks (oldest first)
            use crate::blockchain::consensus::{BlockInfo, LWMA_WINDOW, lwma_next_difficulty};
            let window_start = block.height().saturating_sub(LWMA_WINDOW);
            let mut window: Vec<BlockInfo> = Vec::with_capacity((LWMA_WINDOW + 1) as usize);
            
            for h in window_start..=block.height() {
                if let Ok(Some(b)) = self.storage.get_block_by_height(h) {
                    window.push(BlockInfo {
                        timestamp: b.header.timestamp,
                        difficulty: b.difficulty(),
                    });
                }
            }
            
            if window.len() >= 2 {
                lwma_next_difficulty(&window)
            } else {
                block.difficulty()
            }
        };
        
        self.height.store(new_height, std::sync::atomic::Ordering::Relaxed);
        self.difficulty
            .store(next_difficulty, std::sync::atomic::Ordering::Relaxed);
        {  let mut tip = self.tip.lock().unwrap();
            *tip = new_hash.clone();
        }
        
        // 4. Clean Mempool
        for tx in &block.transactions {
            self.mempool.remove_transaction(&tx.id); 
        }
        
        // 5. Update metrics
        let validation_time_us = start_time.elapsed().as_micros() as u64;
        self.metrics.validation_time_us.store(validation_time_us, std::sync::atomic::Ordering::Relaxed);
        self.metrics.blocks_processed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.metrics.current_height.store(new_height, std::sync::atomic::Ordering::Relaxed);
        self.metrics.current_difficulty.store(block.difficulty(), std::sync::atomic::Ordering::Relaxed);
        self.metrics.last_block_time.store(now, std::sync::atomic::Ordering::Relaxed);
        self.metrics.storage_writes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.metrics.txs_in_mempool.store(self.mempool.size(), std::sync::atomic::Ordering::Relaxed);
        
        // 6. Notify Local Subscriptions (P2P will pick this up)
        let _ = self.block_broadcaster.send((new_height, new_hash.clone()));

        Ok((new_height, new_hash))
    }

    pub fn process_transaction(self: &Arc<Self>, tx: Transaction) -> Result<(), String> {
        self.metrics.txs_submitted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // 1. Check Mempool (already have it?)
        if self.mempool.get_transaction(&tx.id).is_some() {
            return Ok(());
        }
        
        // 2. Validate Inputs (UTXO existence + ownership)
        if !tx.inputs.is_empty() {
             for input in &tx.inputs {
                let key = format!("{}:{}", input.prev_tx_hash, input.output_index);
                match self.storage.get_utxo(&key) {
                    Ok(Some(utxo)) => {
                        // Verify ownership: address derived from public key must match UTXO
                        let addr_from_pubkey = crate::crypto::keys::address_from_public_key_hex(&input.public_key);
                        if utxo.address != addr_from_pubkey {
                            self.metrics.txs_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return Err(format!(
                                "UTXO {} address mismatch: expected {}, got {} (tx {})",
                                key, utxo.address, addr_from_pubkey, tx.id
                            ));
                        }
                        // Burn address UTXOs are permanently unspendable
                        if crate::blockchain::burn::is_burn_address(&utxo.address) {
                            self.metrics.txs_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            return Err(format!(
                                "Cannot spend UTXO {} — burn address is permanently unspendable (tx {})",
                                key, tx.id
                            ));
                        }
                    }
                    Ok(None) => {
                        self.metrics.txs_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return Err(format!("Missing UTXO: {} for tx {}", key, tx.id));
                    }
                    Err(e) => {
                        self.metrics.txs_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return Err(format!("Storage error checking UTXO {}: {}", key, e));
                    }
                }
             }
        }
        
        // 3. Add to Mempool with fee/double-spend validation
        match self.mempool.add_transaction_validated(tx.clone()) {
            Ok(()) => {
                self.metrics.txs_accepted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.metrics.txs_in_mempool.store(self.mempool.size(), std::sync::atomic::Ordering::Relaxed);
                // 4. Broadcast
                let _ = self.tx_broadcaster.send(tx);
            }
            Err(crate::mempool::pool::MempoolError::Duplicate) => {
                // Already have it — not an error
            }
            Err(e) => {
                self.metrics.txs_rejected.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return Err(e.to_string());
            }
        }
        
        Ok(())
    }

    /// Perform a chain reorganization: rollback blocks from current tip down to
    /// `fork_point_height`, then apply `new_blocks` on top of the fork point.
    ///
    /// This handles:
    /// 1. UTXO rollback for old blocks
    /// 2. Block deletion from storage
    /// 3. Transaction restoration to mempool (non-coinbase txs from old blocks)
    /// 4. Application of new fork blocks
    /// 5. State tip/height/difficulty update
    ///
    /// Returns the new (height, hash) or an error.
    ///
    /// When `force_allow` is `true` the `MAX_REORG_DEPTH` check is skipped.
    /// The IBD fork-resolution handler sets this to `true` because the global
    /// `is_ibd()` flag may already be cleared by the time we get here.
    pub fn reorg_to_fork(
        &self,
        fork_point_height: u64,
        new_blocks: Vec<Block>,
        force_allow: bool,
    ) -> Result<(u64, String), String> {
        use crate::blockchain::chain::MAX_REORG_DEPTH;
        use crate::p2p::get_sync_status;
        
        let current_height = self.height.load(std::sync::atomic::Ordering::Relaxed);
        let reorg_depth = current_height.saturating_sub(fork_point_height);
        
        // During IBD (initial block download) or when explicitly forced by
        // the IBD fork handler, allow deeper reorgs since we're syncing
        // from scratch and may receive overlapping block ranges.
        let is_ibd = force_allow || get_sync_status().is_ibd();
        let effective_max = if is_ibd { reorg_depth + 1 } else { MAX_REORG_DEPTH };
        
        // 1. Reject reorgs deeper than limit (disabled during IBD / forced)
        if reorg_depth > effective_max {
            return Err(format!(
                "Reorg depth {} exceeds maximum {} (fork at {}, tip at {})",
                reorg_depth, effective_max, fork_point_height, current_height
            ));
        }
        
        if is_ibd {
            println!("🔀 IBD reorg: rolling back {} blocks from tip={} to fork_point={}",
                reorg_depth, current_height, fork_point_height);
        }

        // Validate that new_blocks start at fork_point+1 and are contiguous.
        // Without this, a gap (e.g. missing block at fork_point+1) causes
        // process_block to fail with "Previous block not found" after rollback.
        if let Some(first_new) = new_blocks.first() {
            if first_new.height() != fork_point_height + 1 {
                return Err(format!(
                    "Fork blocks must start at fork_point+1 ({}), but first block is at height {}",
                    fork_point_height + 1, first_new.height()
                ));
            }
        }
        for w in new_blocks.windows(2) {
            if w[1].height() != w[0].height() + 1 {
                return Err(format!(
                    "Fork blocks not contiguous: gap between height {} and {}",
                    w[0].height(), w[1].height()
                ));
            }
        }

        // 2. Rollback old blocks (from tip down to fork_point + 1)
        let mut restored_txs: Vec<Transaction> = Vec::new();
        
        for h in (fork_point_height + 1..=current_height).rev() {
            if let Ok(Some(old_block)) = self.storage.get_block_by_height(h) {
                // Collect non-coinbase transactions for mempool restoration
                restored_txs.extend(old_block.transactions.clone());
                
                // Rollback UTXO changes
                if let Err(e) = self.storage.rollback_block_utxos(&old_block) {
                    return Err(format!("Failed to rollback UTXOs at height {}: {}", h, e));
                }
                
                // Delete block from storage
                if let Err(e) = self.storage.delete_block_at_height(h) {
                    return Err(format!("Failed to delete block at height {}: {}", h, e));
                }
            }
        }
        
        // 3. Restore transactions from old blocks to mempool
        self.mempool.restore_transactions(&restored_txs);
        
        // 4. Apply new fork blocks sequentially
        let mut last_result = Err("No new blocks to apply".to_string());
        for new_block in new_blocks {
            // Clean restored txs that are in the new block from mempool
            for tx in &new_block.transactions {
                self.mempool.remove_transaction(&tx.id);
            }
            last_result = self.process_block(new_block);
            if last_result.is_err() {
                return last_result;
            }
        }
        
        last_result
    }
}
