/// ZION Coinbase Maturity Tracker
///
/// Tracks mined blocks and their confirmation count. Coinbase outputs
/// cannot be spent until they have COINBASE_MATURITY (100) confirmations.
///
/// The pool must wait for a block's coinbase to mature before distributing
/// rewards to miners. This prevents payouts being invalidated by reorgs.
///
/// Flow:
/// 1. Block found → record in Redis with height + hash
/// 2. On each payout cycle, check current chain height
/// 3. If current_height - block_height >= COINBASE_MATURITY → mature
/// 4. Only mature blocks' rewards are eligible for distribution

use anyhow::{anyhow, Result};
use redis::AsyncCommands;
use std::sync::Arc;

use crate::blockchain::ZionRPCClient;
use crate::shares::RedisStorage;

/// Number of confirmations required before coinbase can be spent.
/// Matches `COINBASE_MATURITY` in `core/src/blockchain/validation.rs`.
pub const COINBASE_MATURITY: u64 = 100;

/// Redis key for tracking found blocks pending maturity
const PENDING_BLOCKS_KEY: &str = "pool:blocks:pending_maturity";

/// A block found by the pool, awaiting coinbase maturity.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingBlock {
    /// Block height
    pub height: u64,
    /// Block hash
    pub hash: String,
    /// Coinbase reward in atomic units
    pub reward_atomic: u64,
    /// Timestamp when block was found
    pub found_at: u64,
    /// Whether the block is still on the main chain
    pub valid: bool,
}

pub struct MaturityTracker {
    storage: Arc<RedisStorage>,
    rpc: Arc<ZionRPCClient>,
}

impl MaturityTracker {
    pub fn new(storage: Arc<RedisStorage>, rpc: Arc<ZionRPCClient>) -> Self {
        Self { storage, rpc }
    }

    /// Record a newly found block as pending maturity.
    pub async fn record_found_block(
        &self,
        height: u64,
        hash: &str,
        reward_atomic: u64,
    ) -> Result<()> {
        let block = PendingBlock {
            height,
            hash: hash.to_string(),
            reward_atomic,
            found_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            valid: true,
        };

        let json = serde_json::to_string(&block)?;
        let mut conn = self.storage.get_connection_manager().await?;

        // Store in sorted set with height as score (for easy range queries)
        conn.zadd::<_, _, _, ()>(PENDING_BLOCKS_KEY, &json, height as f64)
            .await
            .map_err(|e| anyhow!("Failed to record pending block: {}", e))?;

        tracing::info!(
            "⏳ Block recorded for maturity tracking: height={}, hash={}, reward={} ZION",
            height,
            &hash[..16.min(hash.len())],
            reward_atomic as f64 / 1_000_000.0
        );

        Ok(())
    }

    /// Get the current chain height from the node.
    async fn get_current_height(&self) -> Result<u64> {
        let info = self.rpc.call("getInfo", serde_json::json!({})).await?;
        let height = info
            .get("height")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow!("getInfo did not return height"))?;
        Ok(height)
    }

    /// Check which pending blocks have reached maturity.
    ///
    /// Returns list of mature blocks and removes them from the pending set.
    pub async fn check_maturity(&self) -> Result<Vec<PendingBlock>> {
        let current_height = self.get_current_height().await?;

        if current_height < COINBASE_MATURITY {
            // Chain too short for any block to be mature
            return Ok(vec![]);
        }

        let mature_cutoff = current_height - COINBASE_MATURITY;
        let mut conn = self.storage.get_connection_manager().await?;

        // Get all blocks with height <= mature_cutoff
        let entries: Vec<String> = conn
            .zrangebyscore(PENDING_BLOCKS_KEY, 0f64, mature_cutoff as f64)
            .await
            .map_err(|e| anyhow!("Failed to query pending blocks: {}", e))?;

        if entries.is_empty() {
            return Ok(vec![]);
        }

        let mut mature_blocks = Vec::new();

        for entry in &entries {
            let block: PendingBlock = match serde_json::from_str(entry) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!("Failed to parse pending block: {}", e);
                    continue;
                }
            };

            // Verify block is still on the main chain (not orphaned)
            let still_valid = match self.rpc.call("getBlockByHeight", serde_json::json!([block.height])).await {
                Ok(chain_block) => {
                    let chain_hash = chain_block
                        .get("hash")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    chain_hash == block.hash
                }
                Err(_) => {
                    // Can't verify — assume still valid but don't remove
                    tracing::warn!(
                        "Cannot verify block {} at height {} — skipping",
                        &block.hash[..16.min(block.hash.len())],
                        block.height
                    );
                    continue;
                }
            };

            if still_valid {
                mature_blocks.push(block);
            } else {
                tracing::warn!(
                    "⚠️ Block {} at height {} was orphaned — marking invalid",
                    &entry,
                    mature_cutoff
                );
                // Remove orphaned block from pending
                conn.zrem::<_, _, ()>(PENDING_BLOCKS_KEY, entry)
                    .await
                    .unwrap_or(());
            }
        }

        // Remove mature blocks from pending set
        for entry in &entries {
            conn.zrem::<_, _, ()>(PENDING_BLOCKS_KEY, entry)
                .await
                .unwrap_or(());
        }

        if !mature_blocks.is_empty() {
            tracing::info!(
                "✅ {} blocks matured (current_height={}, cutoff={})",
                mature_blocks.len(),
                current_height,
                mature_cutoff
            );
        }

        Ok(mature_blocks)
    }

    /// Get count of pending (not yet mature) blocks.
    pub async fn pending_count(&self) -> Result<u64> {
        let mut conn = self.storage.get_connection_manager().await?;
        let count: u64 = conn
            .zcard(PENDING_BLOCKS_KEY)
            .await
            .map_err(|e| anyhow!("Failed to count pending blocks: {}", e))?;
        Ok(count)
    }

    /// Get all pending blocks (for API/debugging).
    pub async fn get_pending_blocks(&self) -> Result<Vec<PendingBlock>> {
        let mut conn = self.storage.get_connection_manager().await?;
        let entries: Vec<String> = conn
            .zrangebyscore(PENDING_BLOCKS_KEY, "-inf", "+inf")
            .await
            .map_err(|e| anyhow!("Failed to list pending blocks: {}", e))?;

        let blocks: Vec<PendingBlock> = entries
            .iter()
            .filter_map(|e| serde_json::from_str(e).ok())
            .collect();

        Ok(blocks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coinbase_maturity_constant() {
        assert_eq!(COINBASE_MATURITY, 100);
    }

    #[test]
    fn test_pending_block_serialization() {
        let block = PendingBlock {
            height: 42,
            hash: "abc123".to_string(),
            reward_atomic: 5_400_067_000,
            found_at: 1700000000,
            valid: true,
        };

        let json = serde_json::to_string(&block).unwrap();
        let deserialized: PendingBlock = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.height, 42);
        assert_eq!(deserialized.hash, "abc123");
        assert_eq!(deserialized.reward_atomic, 5_400_067_000);
        assert!(deserialized.valid);
    }
}
