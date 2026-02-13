use crate::blockchain::block::Block;
use crate::blockchain::validation;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

// ──────────────────────────────────────────────
// Consensus constants
// ──────────────────────────────────────────────

/// Maximum reorg depth.  Any reorganisation deeper than this is rejected
/// outright.  50 blocks ≈ 50 minutes at 60 s target.
/// AUDIT-FIX P0-06: Hardened from 50 to 10 for mainnet safety.
/// HOTFIX: Reverted to 50 for testnet — 10 was too restrictive, caused
/// permanent fork when Germany diverged by 11 blocks.  Reduce to 10 again
/// only after MainNet launch with more peers.
pub const MAX_REORG_DEPTH: u64 = 50;

/// Soft finality depth.  Blocks this deep are considered settled for
/// wallet / API purposes.  60 blocks ≈ 1 hour.
pub const SOFT_FINALITY_DEPTH: u64 = 60;

/// In-memory blockchain state
pub struct Chain {
    /// Current chain height
    pub height: u64,
    /// Hash of the tip block
    pub tip: String,
    /// Total accumulated work of the best chain
    pub total_work: u128,
    /// Block storage (height -> Block)
    blocks: Arc<RwLock<HashMap<u64, Block>>>,
    /// Block hash index (hash -> height)
    hash_index: Arc<RwLock<HashMap<String, u64>>>,
    /// Accumulated work at each height
    work_at_height: Arc<RwLock<HashMap<u64, u128>>>,
}

impl Chain {
    pub fn new() -> Self {
        let genesis = Block::genesis();
        let genesis_hash = genesis.calculate_hash();
        let genesis_work = genesis.header.difficulty as u128;
        
        let mut blocks = HashMap::new();
        blocks.insert(0, genesis);
        
        let mut hash_index = HashMap::new();
        hash_index.insert(genesis_hash.clone(), 0);
        
        let mut work_at_height = HashMap::new();
        work_at_height.insert(0, genesis_work);
        
        Self {
            height: 0,
            tip: genesis_hash,
            total_work: genesis_work,
            blocks: Arc::new(RwLock::new(blocks)),
            hash_index: Arc::new(RwLock::new(hash_index)),
            work_at_height: Arc::new(RwLock::new(work_at_height)),
        }
    }
    
    /// Add a validated block to the chain
    pub fn add_block(&mut self, block: Block) -> Result<(), String> {
        // Get previous block for validation
        let prev_block = if block.height() > 0 {
            Some(self.get_block(block.height() - 1)?)
        } else {
            None
        };
        
        // Get current time
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        // Validate block
        validation::validate_block(&block, prev_block.as_ref(), now)?;
        
        // Calculate accumulated work for this chain
        let prev_work = if block.height() > 0 {
            let work_map = self.work_at_height.read().unwrap();
            *work_map.get(&(block.height() - 1)).unwrap_or(&0)
        } else {
            0
        };
        let block_work = block.header.difficulty as u128;
        let accumulated_work = prev_work + block_work;
        
        // Add to storage
        let block_hash = block.calculate_hash();
        let block_height = block.height();
        
        {
            let mut blocks = self.blocks.write().unwrap();
            blocks.insert(block_height, block);
        }
        
        {
            let mut hash_index = self.hash_index.write().unwrap();
            hash_index.insert(block_hash.clone(), block_height);
        }
        
        {
            let mut work_map = self.work_at_height.write().unwrap();
            work_map.insert(block_height, accumulated_work);
        }
        
        // Update tip — fork-choice: strictly more accumulated work wins
        // AUDIT-FIX P1-01: Changed >= to > to prevent tip-thrashing on equal work
        if accumulated_work > self.total_work {
            self.height = block_height;
            self.tip = block_hash;
            self.total_work = accumulated_work;
        }
        
        Ok(())
    }
    
    /// Attempt a chain reorganisation.  Returns `Err` if the reorg is
    /// deeper than `MAX_REORG_DEPTH` blocks.
    ///
    /// `fork_point_height` is the height of the common ancestor.
    /// `new_blocks` are the blocks of the competing fork, ordered by height.
    pub fn try_reorg(
        &mut self,
        fork_point_height: u64,
        new_blocks: &[Block],
    ) -> Result<(), String> {
        // 1. Reject reorgs deeper than MAX_REORG_DEPTH
        let reorg_depth = self.height.saturating_sub(fork_point_height);
        if reorg_depth > MAX_REORG_DEPTH {
            return Err(format!(
                "Reorg depth {} exceeds maximum {} (fork at height {}, current tip {})",
                reorg_depth, MAX_REORG_DEPTH, fork_point_height, self.height
            ));
        }
        
        // 2. Reject reorgs that try to modify finalized blocks
        let finality_height = self.height.saturating_sub(SOFT_FINALITY_DEPTH);
        if fork_point_height < finality_height && self.height > SOFT_FINALITY_DEPTH {
            return Err(format!(
                "Reorg fork point {} is below finality horizon {} (depth {})",
                fork_point_height, finality_height, self.height - fork_point_height
            ));
        }
        
        // 3. Calculate competing chain's total work
        let fork_point_work = {
            let work_map = self.work_at_height.read().unwrap();
            *work_map.get(&fork_point_height).unwrap_or(&0)
        };
        
        let mut competing_work = fork_point_work;
        for b in new_blocks {
            competing_work += b.header.difficulty as u128;
        }
        
        // 4. Fork-choice: highest accumulated work wins
        if competing_work <= self.total_work {
            return Err(format!(
                "Competing chain work {} does not exceed current work {}",
                competing_work, self.total_work
            ));
        }
        
        // 5. Remove old blocks from fork point + 1 onwards
        {
            let mut blocks = self.blocks.write().unwrap();
            let mut hash_index = self.hash_index.write().unwrap();
            let mut work_map = self.work_at_height.write().unwrap();
            
            for h in (fork_point_height + 1)..=self.height {
                if let Some(old_block) = blocks.remove(&h) {
                    let old_hash = old_block.calculate_hash();
                    hash_index.remove(&old_hash);
                }
                work_map.remove(&h);
            }
        }
        
        // 6. Apply new blocks
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let mut running_work = fork_point_work;
        
        for new_block in new_blocks {
            let prev = if new_block.height() > 0 {
                Some(self.get_block(new_block.height() - 1)?)
            } else {
                None
            };
            
            validation::validate_block(new_block, prev.as_ref(), now)?;
            
            let bh = new_block.height();
            let bk = new_block.calculate_hash();
            running_work += new_block.header.difficulty as u128;
            
            {
                let mut blocks = self.blocks.write().unwrap();
                blocks.insert(bh, new_block.clone());
            }
            {
                let mut hash_index = self.hash_index.write().unwrap();
                hash_index.insert(bk.clone(), bh);
            }
            {
                let mut work_map = self.work_at_height.write().unwrap();
                work_map.insert(bh, running_work);
            }
        }
        
        // 7. Update tip
        if let Some(last) = new_blocks.last() {
            self.height = last.height();
            self.tip = last.calculate_hash();
            self.total_work = running_work;
        }
        
        Ok(())
    }
    
    /// Attempt a chain reorganisation without PoW/timestamp validation.
    ///
    /// **Test-only** — same logic as `try_reorg` but skips `validate_block`
    /// for the new blocks. Still enforces MAX_REORG_DEPTH, finality, and
    /// fork-choice (highest accumulated work wins).
    /// AUDIT-FIX P1-06: DO NOT use in production — test/dev only.
    #[cfg(any(test, feature = "dev-tools"))]
    pub fn try_reorg_unchecked(
        &mut self,
        fork_point_height: u64,
        new_blocks: &[Block],
    ) -> Result<(), String> {
        // 1. Reject reorgs deeper than MAX_REORG_DEPTH
        let reorg_depth = self.height.saturating_sub(fork_point_height);
        if reorg_depth > MAX_REORG_DEPTH {
            return Err(format!(
                "Reorg depth {} exceeds maximum {} (fork at height {}, current tip {})",
                reorg_depth, MAX_REORG_DEPTH, fork_point_height, self.height
            ));
        }

        // 2. Reject reorgs that try to modify finalized blocks
        let finality_height = self.height.saturating_sub(SOFT_FINALITY_DEPTH);
        if fork_point_height < finality_height && self.height > SOFT_FINALITY_DEPTH {
            return Err(format!(
                "Reorg fork point {} is below finality horizon {} (depth {})",
                fork_point_height, finality_height, self.height - fork_point_height
            ));
        }

        // 3. Calculate competing chain's total work
        let fork_point_work = {
            let work_map = self.work_at_height.read().unwrap();
            *work_map.get(&fork_point_height).unwrap_or(&0)
        };

        let mut competing_work = fork_point_work;
        for b in new_blocks {
            competing_work += b.header.difficulty as u128;
        }

        // 4. Fork-choice: highest accumulated work wins
        if competing_work <= self.total_work {
            return Err(format!(
                "Competing chain work {} does not exceed current work {}",
                competing_work, self.total_work
            ));
        }

        // 5. Remove old blocks from fork point + 1 onwards
        {
            let mut blocks = self.blocks.write().unwrap();
            let mut hash_index = self.hash_index.write().unwrap();
            let mut work_map = self.work_at_height.write().unwrap();

            for h in (fork_point_height + 1)..=self.height {
                if let Some(old_block) = blocks.remove(&h) {
                    let old_hash = old_block.calculate_hash();
                    hash_index.remove(&old_hash);
                }
                work_map.remove(&h);
            }
        }

        // 6. Apply new blocks (without PoW validation)
        let mut running_work = fork_point_work;

        for new_block in new_blocks {
            let bh = new_block.height();
            let bk = new_block.calculate_hash();
            running_work += new_block.header.difficulty as u128;

            {
                let mut blocks = self.blocks.write().unwrap();
                blocks.insert(bh, new_block.clone());
            }
            {
                let mut hash_index = self.hash_index.write().unwrap();
                hash_index.insert(bk.clone(), bh);
            }
            {
                let mut work_map = self.work_at_height.write().unwrap();
                work_map.insert(bh, running_work);
            }
        }

        // 7. Update tip
        if let Some(last) = new_blocks.last() {
            self.height = last.height();
            self.tip = last.calculate_hash();
            self.total_work = running_work;
        }

        Ok(())
    }

    /// Check if a given height is finalized (SOFT_FINALITY_DEPTH confirmations).
    pub fn is_finalized(&self, height: u64) -> bool {
        if self.height < SOFT_FINALITY_DEPTH {
            // Chain is too short for any block to be finalized
            return height == 0; // Only genesis is always final
        }
        height <= self.height.saturating_sub(SOFT_FINALITY_DEPTH)
    }
    
    /// Get the latest finalized height.
    pub fn finalized_height(&self) -> u64 {
        self.height.saturating_sub(SOFT_FINALITY_DEPTH)
    }
    
    /// Get block by height
    pub fn get_block(&self, height: u64) -> Result<Block, String> {
        let blocks = self.blocks.read().unwrap();
        blocks
            .get(&height)
            .cloned()
            .ok_or_else(|| format!("Block at height {} not found", height))
    }
    
    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &str) -> Result<Block, String> {
        let hash_index = self.hash_index.read().unwrap();
        let height = hash_index
            .get(hash)
            .ok_or_else(|| format!("Block with hash {} not found", hash))?;
        
        self.get_block(*height)
    }
    
    /// Get current tip block
    pub fn get_tip_block(&self) -> Result<Block, String> {
        self.get_block(self.height)
    }
    
    /// Get chain info
    pub fn get_info(&self) -> ChainInfo {
        ChainInfo {
            height: self.height,
            tip: self.tip.clone(),
            block_count: self.blocks.read().unwrap().len(),
            total_work: self.total_work,
        }
    }
    
    /// Insert a block without PoW/timestamp validation.
    ///
    /// **Test-only** — used to build synthetic chains for reorg / fork-choice tests.
    /// Updates height, tip, total_work, and all internal indexes.
    /// AUDIT-FIX P1-06: Gated behind test/dev-tools — not available in release builds.
    #[cfg(any(test, feature = "dev-tools"))]
    pub fn insert_block_unchecked(&mut self, block: Block) {
        let block_hash = block.calculate_hash();
        let block_height = block.height();
        let block_work = block.header.difficulty as u128;

        let prev_work = if block_height > 0 {
            let work_map = self.work_at_height.read().unwrap();
            *work_map.get(&(block_height - 1)).unwrap_or(&0)
        } else {
            0
        };
        let accumulated_work = prev_work + block_work;

        {
            let mut blocks = self.blocks.write().unwrap();
            blocks.insert(block_height, block);
        }
        {
            let mut hash_index = self.hash_index.write().unwrap();
            hash_index.insert(block_hash.clone(), block_height);
        }
        {
            let mut work_map = self.work_at_height.write().unwrap();
            work_map.insert(block_height, accumulated_work);
        }

        // AUDIT-FIX P1-01: Changed >= to > (consistent fork-choice)
        if accumulated_work > self.total_work {
            self.height = block_height;
            self.tip = block_hash;
            self.total_work = accumulated_work;
        }
    }

    /// Verify entire chain integrity
    pub fn verify_chain(&self) -> Result<(), String> {
        let blocks = self.blocks.read().unwrap();
        
        // Verify each block sequentially
        for height in 0..=self.height {
            let block = blocks
                .get(&height)
                .ok_or_else(|| format!("Missing block at height {}", height))?;
            
            let prev_block = if height > 0 {
                Some(blocks.get(&(height - 1)).unwrap())
            } else {
                None
            };
            
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            validation::validate_block(block, prev_block, now)?;
        }
        
        Ok(())
    }
}

impl Default for Chain {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ChainInfo {
    pub height: u64,
    pub tip: String,
    pub block_count: usize,
    pub total_work: u128,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chain_initialization() {
        let chain = Chain::new();
        assert_eq!(chain.height, 0);
        assert_eq!(chain.get_info().block_count, 1);
        assert!(chain.total_work > 0, "Genesis has work");
    }
    
    #[test]
    fn test_get_genesis() {
        let chain = Chain::new();
        let genesis = chain.get_block(0);
        assert!(genesis.is_ok());
        assert_eq!(genesis.unwrap().height(), 0);
    }
    
    #[test]
    fn test_get_tip() {
        let chain = Chain::new();
        let tip = chain.get_tip_block();
        assert!(tip.is_ok());
        assert_eq!(tip.unwrap().height(), 0);
    }
    
    // ── Reorg depth tests ──
    
    #[test]
    fn test_max_reorg_depth_constant() {
        assert_eq!(MAX_REORG_DEPTH, 50);
    }
    
    #[test]
    fn test_soft_finality_constant() {
        assert_eq!(SOFT_FINALITY_DEPTH, 60);
    }
    
    #[test]
    fn test_finality_short_chain() {
        let chain = Chain::new();
        // Chain height=0, only genesis is final
        assert!(chain.is_finalized(0));
        assert!(!chain.is_finalized(1));
    }
    
    #[test]
    fn test_reorg_rejected_too_deep() {
        let mut chain = Chain::new();
        // Simulate chain at height 60
        chain.height = 60;
        chain.total_work = 60_000;
        
        // Try reorg from fork point 5 → depth = 55 > MAX_REORG_DEPTH (50)
        let result = chain.try_reorg(5, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exceeds maximum"));
    }
    
    #[test]
    fn test_reorg_accepted_within_limit() {
        let mut chain = Chain::new();
        chain.height = 15;
        chain.total_work = 15_000;
        
        // Fork point at 10 → depth = 5, within MAX_REORG_DEPTH (50)
        // But no competing blocks and no higher work → rejected on work basis
        let result = chain.try_reorg(10, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exceed"));
    }
    
    #[test]
    fn test_fork_choice_requires_more_work() {
        let mut chain = Chain::new();
        chain.height = 5;
        chain.total_work = 5_000;
        
        // Empty competing chain with 0 additional work
        let result = chain.try_reorg(4, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exceed"));
    }
    
    #[test]
    fn test_chain_info_includes_work() {
        let chain = Chain::new();
        let info = chain.get_info();
        assert!(info.total_work > 0);
    }
}
