/// Redis Async Storage - Share persistence and miner stats
///
/// Handles:
/// - Share storage (shares:job_id:nonce)
/// - Miner stats (miner:address:shares, miner:address:blocks)
/// - Block tracking (blocks:height)
/// - PPLNS window (shares:window - last N shares)
///
/// Uses redis::aio::ConnectionManager for async operations
/// Mirrors Python src/pool/database/models.py logic

use anyhow::{anyhow, Result};
use chrono::Utc;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Share data stored in Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredShare {
    pub job_id: String,
    pub miner_address: String,
    pub nonce: String,
    pub hash: String,
    pub difficulty: u64,
    pub algorithm: String,
    pub timestamp: i64,
    pub is_block: bool,
    #[serde(default)]
    pub job_blob: Option<String>,
    #[serde(default)]
    pub height: Option<u64>,
}

/// Miner statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerStats {
    pub address: String,
    pub total_shares: u64,
    pub valid_shares: u64,
    pub invalid_shares: u64,
    pub blocks_found: u64,
    pub last_share_time: i64,
    pub hashrate_1h: f64,
    pub hashrate_24h: f64,
    pub total_paid: u64,
    pub pending_balance: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolStats {
    pub hash_rate: f64,
    pub miners: usize,
    pub miners_paid: usize,
    pub total_blocks: u64,
    pub network_diff: f64, // Placeholder for real network diff
}

/// Payout record stored in Redis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutRecord {
    pub id: u64,
    pub address: String,
    pub amount_atomic: u64,
    pub amount: f64,
    pub status: String,
    pub tx_id: Option<String>,
    pub created_ts: i64,
    pub updated_ts: i64,
    pub error: Option<String>,
}

/// Block found notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockFound {
    pub height: u64,
    pub hash: String,
    pub miner_address: String,
    pub reward: u64,
    pub timestamp: i64,
    pub difficulty: u64,
}

/// Redis storage manager
pub struct RedisStorage {
    client: redis::Client,
    connection: Arc<RwLock<Option<redis::aio::ConnectionManager>>>,
    pplns_window_shares: usize,
}

fn difficulty_to_hashrate(sum_difficulty: u64, window_secs: u64) -> f64 {
    if window_secs == 0 {
        return 0.0;
    }
    // Pool hashrate: each share at difficulty D represents D hashes of work.
    // hashrate = total_work / time_window (H/s).
    // NOTE: Do NOT multiply by 2^32 — that formula is for estimating network
    // hashrate from *block* difficulty, not pool share difficulty.
    sum_difficulty as f64 / window_secs as f64
}

impl RedisStorage {
    /// Create new Redis storage
    pub fn new(redis_url: &str, pplns_window_shares: usize) -> Result<Self> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| anyhow!("Failed to create Redis client: {}", e))?;

        Ok(Self {
            client,
            connection: Arc::new(RwLock::new(None)),
            pplns_window_shares: if pplns_window_shares == 0 { 100_000 } else { pplns_window_shares },
        })
    }

    /// Get async connection (lazy initialization)
    async fn get_connection(&self) -> Result<redis::aio::ConnectionManager> {
        let mut conn_guard = self.connection.write().await;

        if conn_guard.is_none() {
            let manager = self
                .client
                .get_connection_manager()
                .await
                .map_err(|e| anyhow!("Failed to get connection manager: {}", e))?;
            *conn_guard = Some(manager);
        }

        Ok(conn_guard.clone().unwrap())
    }

    /// Get connection manager (public for other modules)
    pub async fn get_connection_manager(&self) -> Result<redis::aio::ConnectionManager> {
        self.get_connection().await
    }

    /// Store a valid share
    pub async fn store_share(&self, share: &StoredShare) -> Result<()> {
        let mut conn = self.get_connection().await?;

        // Key: shares:job_id:nonce
        let share_key = format!("shares:{}:{}", share.job_id, share.nonce);

        // Serialize share to JSON
        let share_json = serde_json::to_string(share)
            .map_err(|e| anyhow!("Failed to serialize share: {}", e))?;

        // Store with 1-hour expiry (prevent memory bloat)
        conn.set_ex::<_, _, ()>(&share_key, share_json, 3600)
            .await
            .map_err(|e| anyhow!("Failed to store share: {}", e))?;

        // Increment miner stats
        let miner_shares_key = format!("miner:{}:shares", share.miner_address);
        conn.incr::<_, _, ()>(&miner_shares_key, 1)
            .await
            .map_err(|e| anyhow!("Failed to increment shares: {}", e))?;

        // Global valid share counter
        conn.incr::<_, _, ()>("shares:valid", 1)
            .await
            .map_err(|e| anyhow!("Failed to increment global valid shares: {}", e))?;

        // Update last share time
        let miner_last_key = format!("miner:{}:last_share", share.miner_address);
        conn.set::<_, _, ()>(&miner_last_key, share.timestamp)
            .await
            .map_err(|e| anyhow!("Failed to update last share: {}", e))?;

        // Track last share per miner (for active miner counts)
        conn.zadd::<_, _, _, ()>("miners:last_share", &share.miner_address, share.timestamp)
            .await
            .map_err(|e| anyhow!("Failed to update miners:last_share: {}", e))?;

        // Add to PPLNS window (sorted set by timestamp)
        conn.zadd::<_, _, _, ()>("shares:window", &share_key, share.timestamp)
            .await
            .map_err(|e| anyhow!("Failed to add to PPLNS window: {}", e))?;

        // Trim window to last configured shares
        let window_size: isize = -(self.pplns_window_shares as isize).max(1);
        let _: () = conn
            .zremrangebyrank("shares:window", 0, window_size)
            .await
            .map_err(|e| anyhow!("Failed to trim PPLNS window: {}", e))?;

        // Track per-miner hashrate samples (timestamp-scored)
        let miner_ts_key = format!("miner:{}:shares:ts", share.miner_address);
        let sample_val = format!("{}:{}:{}", share.timestamp, share.difficulty, share.nonce);
        conn.zadd::<_, _, _, ()>(&miner_ts_key, &sample_val, share.timestamp)
            .await
            .map_err(|e| anyhow!("Failed to add hashrate sample: {}", e))?;

        // Trim samples older than 24h
        let cutoff = share.timestamp.saturating_sub(86_400);
        redis::cmd("ZREMRANGEBYSCORE")
            .arg(&miner_ts_key)
            .arg(0)
            .arg(cutoff)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| anyhow!("Failed to trim hashrate samples: {}", e))?;

        // Set key TTL to 25h to avoid stale data
        let _: () = conn.expire(&miner_ts_key, 90_000).await.unwrap_or(());

        // Pool-wide hashrate samples
        let pool_ts_key = "shares:ts";
        conn.zadd::<_, _, _, ()>(pool_ts_key, &sample_val, share.timestamp)
            .await
            .map_err(|e| anyhow!("Failed to add pool hashrate sample: {}", e))?;
        redis::cmd("ZREMRANGEBYSCORE")
            .arg(pool_ts_key)
            .arg(0)
            .arg(cutoff)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| anyhow!("Failed to trim pool hashrate samples: {}", e))?;
        let _: () = conn.expire(pool_ts_key, 90_000).await.unwrap_or(());

        Ok(())
    }

    /// Store a found block
    pub async fn store_block(&self, block: &BlockFound) -> Result<()> {
        let mut conn = self.get_connection().await?;

        // Key: blocks:height
        let block_key = format!("blocks:{}", block.height);

        // Serialize block
        let block_json = serde_json::to_string(block)
            .map_err(|e| anyhow!("Failed to serialize block: {}", e))?;

        // Store permanently
        conn.set::<_, _, ()>(&block_key, block_json)
            .await
            .map_err(|e| anyhow!("Failed to store block: {}", e))?;

        // Increment miner's block count
        let miner_blocks_key = format!("miner:{}:blocks", block.miner_address);
        conn.incr::<_, _, ()>(&miner_blocks_key, 1)
            .await
            .map_err(|e| anyhow!("Failed to increment blocks: {}", e))?;

        // Add to global blocks list
        conn.lpush::<_, _, ()>("blocks:list", block.height)
            .await
            .map_err(|e| anyhow!("Failed to add to blocks list: {}", e))?;

        // Publish notification
        let notif = serde_json::json!({
            "type": "block_found",
            "height": block.height,
            "hash": block.hash,
            "miner": block.miner_address,
            "reward": block.reward,
        });

        conn.publish::<_, _, ()>("zion:blocks", notif.to_string())
            .await
            .map_err(|e| anyhow!("Failed to publish block notification: {}", e))?;

        Ok(())
    }

    /// Allocate a unique block height.
    ///
    /// Until real blockchain height is wired, we use a Redis counter to avoid
    /// overwriting blocks: `blocks:height_counter`.
    pub async fn allocate_block_height(&self) -> Result<u64> {
        let mut conn = self.get_connection().await?;
        let height: u64 = conn
            .incr("blocks:height_counter", 1u64)
            .await
            .map_err(|e| anyhow!("Failed to allocate block height: {}", e))?;
        Ok(height)
    }

    /// Snapshot current pool stats to history list
    pub async fn snapshot_pool_stats(&self) -> Result<()> {
        let mut conn = self.get_connection().await?;
        let now = Utc::now().timestamp();
        
        // Calculate current hashrate (1m window)
        let (hashrate, _) = self.get_pool_hashrate().await?;
        
        // Count active miners
        let miners = self.get_active_miners(600).await?;
        
        // Create snapshot point
        let snapshot = serde_json::json!({
            "timestamp": now,
            "hashrate": hashrate,
            "miners": miners,
            "workers": miners // simplified
        });

        // Add to history list (right push)
        let _: () = conn.rpush("history:pool", snapshot.to_string()).await?;
        
        // Trim history to last 24h (Assuming 1 min interval = 1440 items)
        let _: () = conn.ltrim("history:pool", -1440, -1).await?;
        
        Ok(())
    }

    /// Get pool history
    pub async fn get_pool_history(&self) -> Result<Vec<serde_json::Value>> {
        let mut conn = self.get_connection().await?;
        let history: Vec<String> = conn.lrange("history:pool", 0, -1).await?;
        
        let mut result = Vec::new();
        for item in history {
            if let Ok(val) = serde_json::from_str(&item) {
                result.push(val);
            }
        }
        Ok(result)
    }

    /// Get top miners by total shares
    pub async fn get_top_miners(&self, limit: usize) -> Result<Vec<MinerStats>> {
         // This is expensive in Redis without a sorted set of "total_shares".
         // But we assume "total_shares" doesn't decrease.
         // Better: Maintain a ZSET "leaderboard:shares" updated on every share?
         // Optimisation: We'll implement a scan-based approach or reuse existing keys if possible.
         // Real approach: We don't have a leaderboard ZSET.
         // fallback: Return top active miners from miners:last_share? No, that's by time.
         
         // Fix: For now, return empty list or implement ZSET maintenance in store_share.
         // I'll add ZSET maintenance to store_share later. 
         // For now, return empty.
         Ok(Vec::new())
    }

    /// Get miner statistics
    pub async fn get_miner_stats(&self, address: &str) -> Result<MinerStats> {
        let mut conn = self.get_connection().await?;

        // Get total shares
        let shares_key = format!("miner:{}:shares", address);
        let total_shares: u64 = conn
            .get(&shares_key)
            .await
            .unwrap_or(0);

        // Get blocks found
        let blocks_key = format!("miner:{}:blocks", address);
        let blocks_found: u64 = conn
            .get(&blocks_key)
            .await
            .unwrap_or(0);

        // Get last share time
        let last_key = format!("miner:{}:last_share", address);
        let last_share_time: i64 = conn
            .get(&last_key)
            .await
            .unwrap_or(0);

        // Get invalid shares
        let invalid_key = format!("miner:{}:invalid", address);
        let invalid_shares: u64 = conn
            .get(&invalid_key)
            .await
            .unwrap_or(0);

        // Get paid amount
        // Prefer PPLNS accounting keys, fallback to legacy miner:* keys.
        let total_paid: u64 = {
            let key = format!("payout:{}:total", address);
            let v: u64 = conn.get(&key).await.unwrap_or(0);
            if v > 0 {
                v
            } else {
                let legacy = format!("miner:{}:paid", address);
                conn.get(&legacy).await.unwrap_or(0)
            }
        };

        // Get pending balance
        // Prefer PPLNS accounting keys, fallback to legacy miner:* keys.
        let pending_balance: u64 = {
            let key = format!("payout:{}:pending", address);
            let v: u64 = conn.get(&key).await.unwrap_or(0);
            if v > 0 {
                v
            } else {
                let legacy = format!("miner:{}:balance", address);
                conn.get(&legacy).await.unwrap_or(0)
            }
        };

        // Calculate hashrate from recent shares (difficulty-based)
        let now = Utc::now().timestamp();
        let miner_ts_key = format!("miner:{}:shares:ts", address);
        let window_1h = self
            .sum_difficulty_since(&mut conn, &miner_ts_key, now.saturating_sub(3600))
            .await
            .unwrap_or(0);
        let window_24h = self
            .sum_difficulty_since(&mut conn, &miner_ts_key, now.saturating_sub(86_400))
            .await
            .unwrap_or(0);

        let hashrate_1h = difficulty_to_hashrate(window_1h, 3600);
        let hashrate_24h = difficulty_to_hashrate(window_24h, 86_400);

        Ok(MinerStats {
            address: address.to_string(),
            total_shares,
            valid_shares: total_shares - invalid_shares,
            invalid_shares,
            blocks_found,
            last_share_time,
            hashrate_1h,
            hashrate_24h,
            total_paid,
            pending_balance,
        })
    }

    async fn sum_difficulty_since(
        &self,
        conn: &mut redis::aio::ConnectionManager,
        key: &str,
        since_ts: i64,
    ) -> Result<u64> {
        let entries: Vec<String> = conn
            .zrangebyscore(key, since_ts, i64::MAX)
            .await
            .map_err(|e| anyhow!("Failed to read hashrate samples: {}", e))?;

        let mut sum: u64 = 0;
        for entry in entries {
            if let Some(diff) = entry.split(':').nth(1) {
                if let Ok(d) = diff.parse::<u64>() {
                    sum = sum.saturating_add(d);
                }
            }
        }

        Ok(sum)
    }

    /// Increment invalid share counter
    pub async fn increment_invalid(&self, address: &str) -> Result<()> {
        let mut conn = self.get_connection().await?;
        let key = format!("miner:{}:invalid", address);
        conn.incr::<_, _, ()>(&key, 1)
            .await
            .map_err(|e| anyhow!("Failed to increment invalid: {}", e))?;
        conn.incr::<_, _, ()>("shares:invalid", 1)
            .await
            .map_err(|e| anyhow!("Failed to increment global invalid shares: {}", e))?;
        Ok(())
    }

    pub async fn get_active_miners(&self, window_secs: i64) -> Result<u64> {
        let mut conn = self.get_connection().await?;
        let now = Utc::now().timestamp();
        let min_ts = now.saturating_sub(window_secs);
        let count: u64 = conn
            .zcount("miners:last_share", min_ts, i64::MAX)
            .await
            .unwrap_or(0);
        Ok(count)
    }

    pub async fn get_total_miners(&self) -> Result<u64> {
        let mut conn = self.get_connection().await?;
        let count: u64 = conn.zcard("miners:last_share").await.unwrap_or(0);
        Ok(count)
    }

    /// Get recent miners by last share time
    pub async fn get_recent_miners(&self, limit: usize) -> Result<Vec<(String, i64)>> {
        let mut conn = self.get_connection().await?;
        let mut cmd = redis::cmd("ZREVRANGE");
        cmd.arg("miners:last_share")
            .arg(0)
            .arg((limit.saturating_sub(1)) as isize)
            .arg("WITHSCORES");

        let results: Vec<(String, f64)> = cmd
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow!("Failed to read recent miners: {}", e))?;

        Ok(results
            .into_iter()
            .map(|(addr, score)| (addr, score as i64))
            .collect())
    }

    pub async fn get_pool_hashrate(&self) -> Result<(f64, f64)> {
        let mut conn = self.get_connection().await?;
        let now = Utc::now().timestamp();
        let window_1h = self
            .sum_difficulty_since(&mut conn, "shares:ts", now.saturating_sub(3600))
            .await
            .unwrap_or(0);
        let window_24h = self
            .sum_difficulty_since(&mut conn, "shares:ts", now.saturating_sub(86_400))
            .await
            .unwrap_or(0);

        Ok((
            difficulty_to_hashrate(window_1h, 3600),
            difficulty_to_hashrate(window_24h, 86_400),
        ))
    }

    pub async fn get_global_share_counts(&self) -> Result<(u64, u64)> {
        let mut conn = self.get_connection().await?;
        let valid: u64 = conn.get("shares:valid").await.unwrap_or(0);
        let invalid: u64 = conn.get("shares:invalid").await.unwrap_or(0);
        Ok((valid, invalid))
    }

    /// List payout candidates from pending balance index
    pub async fn list_payout_candidates(
        &self,
        min_amount: u64,
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        let mut conn = self.get_connection().await?;
        let mut cmd = redis::cmd("ZREVRANGEBYSCORE");
        cmd.arg("payout:pending:balances")
            .arg("+inf")
            .arg(min_amount)
            .arg("WITHSCORES")
            .arg("LIMIT")
            .arg(0)
            .arg(limit as i64);

        let results: Vec<(String, f64)> = cmd
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow!("Failed to list payout candidates: {}", e))?;

        Ok(results
            .into_iter()
            .map(|(addr, score)| (addr, score.max(0.0) as u64))
            .collect())
    }

    /// Sum pending payouts from index
    pub async fn get_pending_payout_totals(&self) -> Result<(u64, u64)> {
        let mut conn = self.get_connection().await?;
        let mut cmd = redis::cmd("ZRANGE");
        cmd.arg("payout:pending:balances")
            .arg(0)
            .arg(-1)
            .arg("WITHSCORES");

        let results: Vec<(String, f64)> = cmd
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow!("Failed to read pending payouts: {}", e))?;

        let mut total: u64 = 0;
        for (_addr, score) in results.iter() {
            if *score > 0.0 {
                total = total.saturating_add(*score as u64);
            }
        }

        Ok((total, results.len() as u64))
    }

    /// Get recent payout records
    pub async fn get_recent_payouts(&self, limit: usize) -> Result<Vec<PayoutRecord>> {
        let mut conn = self.get_connection().await?;
        let ids: Vec<u64> = conn
            .lrange("payout:records", 0, (limit.saturating_sub(1)) as isize)
            .await
            .unwrap_or_default();

        let mut records = Vec::new();
        for id in ids {
            let key = format!("payout:record:{}", id);
            let map: std::collections::HashMap<String, String> = conn
                .hgetall(&key)
                .await
                .unwrap_or_default();

            if map.is_empty() {
                continue;
            }

            let record = PayoutRecord {
                id,
                address: map.get("address").cloned().unwrap_or_default(),
                amount_atomic: map
                    .get("amount_atomic")
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0),
                amount: map
                    .get("amount")
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(0.0),
                status: map.get("status").cloned().unwrap_or_else(|| "unknown".to_string()),
                tx_id: map.get("tx_id").cloned().filter(|v| !v.is_empty()),
                created_ts: map
                    .get("created_ts")
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0),
                updated_ts: map
                    .get("updated_ts")
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0),
                error: map.get("error").cloned().filter(|v| !v.is_empty()),
            };

            records.push(record);
        }

        Ok(records)
    }

    /// Get shares in PPLNS window for miner
    pub async fn get_miner_pplns_shares(&self, address: &str) -> Result<Vec<String>> {
        let mut conn = self.get_connection().await?;

        // Get all shares in window
        let shares: Vec<String> = conn
            .zrange("shares:window", 0, -1)
            .await
            .map_err(|e| anyhow!("Failed to get PPLNS window: {}", e))?;

        // Filter by miner address (must do sequentially for async)
        let mut miner_shares = Vec::new();
        for key in shares {
            if let Ok(Some(json)) = conn.get::<_, Option<String>>(&key).await {
                if let Ok(share) = serde_json::from_str::<StoredShare>(&json) {
                    if share.miner_address == address {
                        miner_shares.push(key);
                    }
                }
            }
        }

        Ok(miner_shares)
    }

    /// Get total shares in PPLNS window
    pub async fn get_pplns_window_size(&self) -> Result<usize> {
        let mut conn = self.get_connection().await?;
        let size: usize = conn
            .zcard("shares:window")
            .await
            .map_err(|e| anyhow!("Failed to get window size: {}", e))?;
        Ok(size)
    }

    /// Get recent blocks
    pub async fn get_recent_blocks(&self, count: usize) -> Result<Vec<BlockFound>> {
        let mut conn = self.get_connection().await?;

        let heights: Vec<u64> = conn
            .lrange("blocks:list", 0, (count - 1) as isize)
            .await
            .map_err(|e| anyhow!("Failed to get blocks list: {}", e))?;

        let mut blocks = Vec::new();
        for height in heights {
            let key = format!("blocks:{}", height);
            if let Ok(Some(json)) = conn.get::<_, Option<String>>(&key).await {
                if let Ok(block) = serde_json::from_str::<BlockFound>(&json) {
                    blocks.push(block);
                }
            }
        }

        Ok(blocks)
    }

    pub async fn get_blocks_count(&self) -> Result<u64> {
        let mut conn = self.get_connection().await?;
        let count: u64 = conn.llen("blocks:list").await.unwrap_or(0);
        Ok(count)
    }

    /// Health check
    pub async fn ping(&self) -> Result<()> {
        let mut conn = self.get_connection().await?;
        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow!("Redis PING failed: {}", e))?;

        if pong != "PONG" {
            return Err(anyhow!("Redis health check failed: got {}", pong));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_storage_creation() {
        // Test with invalid URL (should fail gracefully)
        let result = RedisStorage::new("redis://invalid", 1000);
        assert!(result.is_ok()); // Client creation succeeds, connection fails later
    }

    #[test]
    fn test_share_serialization() {
        let share = StoredShare {
            job_id: "test_job".to_string(),
            miner_address: "ZION_TEST".to_string(),
            nonce: "12345678".to_string(),
            hash: "deadbeef".to_string(),
            difficulty: 1000,
            algorithm: "randomx".to_string(),
            timestamp: 1234567890,
            is_block: false,
            job_blob: None,
            height: None,
        };

        let json = serde_json::to_string(&share).unwrap();
        let deserialized: StoredShare = serde_json::from_str(&json).unwrap();
        assert_eq!(share.job_id, deserialized.job_id);
    }
}
