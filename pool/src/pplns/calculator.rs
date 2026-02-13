/// PPLNS (Pay Per Last N Shares) Distribution Calculator
///
/// Calculates fair reward distribution based on weighted shares
/// in the last N shares window (default: 100,000 shares).
///
/// ## Algorithm
/// 1. Get all shares in PPLNS window (shares:window sorted set)
/// 2. Calculate weighted shares per miner (share_difficulty × count)
/// 3. Calculate each miner's proportion of total weighted shares
/// 4. Distribute miner_share (89% of total reward) proportionally
///
/// ## Redis Keys
/// - shares:window → ZSET (score = timestamp, member = share_id)
/// - shares:share_id → HASH (miner, difficulty, timestamp)
/// - payout:queue:address → LIST (pending payouts)
/// - payout:address:total → INTEGER (total paid to address)
///
/// Mirrors Python logic from src/pool/database/models.py

use anyhow::{anyhow, Result};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use crate::shares::storage::{RedisStorage, StoredShare};
use crate::metrics::prometheus as metrics;

/// Payout entry for a miner
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payout {
    pub miner_address: String,
    pub amount: u64, // atomic units
    pub block_height: u64,
    pub block_hash: String,
    pub timestamp: i64,
    pub shares_count: u64,
    pub weighted_shares: f64,
}

/// PPLNS window share
#[derive(Debug, Clone)]
struct WindowShare {
    _share_id: String,
    miner_address: String,
    difficulty: u64,
    _timestamp: i64,
}

/// PPLNS calculator
pub struct PPLNSCalculator {
    redis: Arc<RedisStorage>,
    window_size: u64,
}

impl PPLNSCalculator {
    /// Create new PPLNS calculator
    pub fn new(redis: Arc<RedisStorage>, window_size: u64) -> Self {
        Self { redis, window_size }
    }

    /// Create with default window size (100k shares)
    pub fn default(redis: Arc<RedisStorage>) -> Self {
        Self::new(redis, 100_000)
    }

    /// Calculate PPLNS distribution for a block
    pub async fn calculate_distribution(
        &self,
        block_height: u64,
        block_hash: String,
        miner_share_reward: u64, // 89% of total reward (atomic units)
        _finder_address: String,
        timestamp: i64,
    ) -> Result<Vec<Payout>> {
        // Get shares from PPLNS window
        let window_shares = self.get_window_shares().await?;

        if window_shares.is_empty() {
            tracing::warn!("PPLNS window empty - no shares to distribute");
            return Ok(Vec::new());
        }

        // Calculate weighted shares per miner
        let mut miner_weighted_shares: HashMap<String, (f64, u64)> = HashMap::new();
        let mut total_weighted_shares = 0.0;

        for share in &window_shares {
            let weight = share.difficulty as f64;
            total_weighted_shares += weight;

            let entry = miner_weighted_shares
                .entry(share.miner_address.clone())
                .or_insert((0.0, 0));
            entry.0 += weight;
            entry.1 += 1;
        }

        // Calculate payout for each miner
        let mut payouts = Vec::new();

        for (miner_address, (weighted_shares, share_count)) in miner_weighted_shares {
            let proportion = weighted_shares / total_weighted_shares;
            let amount = (miner_share_reward as f64 * proportion) as u64;

            if amount > 0 {
                payouts.push(Payout {
                    miner_address,
                    amount,
                    block_height,
                    block_hash: block_hash.clone(),
                    timestamp,
                    shares_count: share_count,
                    weighted_shares,
                });
            }
        }

        tracing::info!(
            "PPLNS calculated: {} miners, {} total shares, {:.2} total weighted shares",
            payouts.len(),
            window_shares.len(),
            total_weighted_shares
        );

        Ok(payouts)
    }

    /// Queue payouts in Redis
    pub async fn queue_payouts(&self, payouts: &[Payout]) -> Result<()> {
        let mut conn = self.redis.get_connection_manager().await?;

        if !payouts.is_empty() {
            metrics::inc_payouts_queued_by(payouts.len() as u64);
        }

        for payout in payouts {
            // Serialize payout to JSON
            let payout_json = serde_json::to_string(payout)
                .map_err(|e| anyhow!("Failed to serialize payout: {}", e))?;

            // Add to payout queue
            let queue_key = format!("payout:queue:{}", payout.miner_address);
            conn.rpush::<_, _, ()>(&queue_key, &payout_json)
                .await
                .map_err(|e| anyhow!("Failed to queue payout: {}", e))?;

            // Update pending balance
            let total_key = format!("payout:{}:pending", payout.miner_address);
            let pending: u64 = conn
                .incr(&total_key, payout.amount)
                .await
                .map_err(|e| anyhow!("Failed to update pending balance: {}", e))?;

            // Maintain pending balances index
            let pending_index = "payout:pending:balances";
            conn.zadd::<_, _, _, ()>(pending_index, &payout.miner_address, pending)
                .await
                .map_err(|e| anyhow!("Failed to update pending index: {}", e))?;

            metrics::add_payout_pending_atomic(payout.amount);
            metrics::inc_payout_queue_len();

            tracing::debug!(
                "Queued payout: {} → {} atomic units (block {})",
                payout.miner_address,
                payout.amount,
                payout.block_height
            );
        }

        Ok(())
    }

    /// Get pending payouts for a miner
    pub async fn get_pending_payouts(&self, miner_address: &str) -> Result<Vec<Payout>> {
        let mut conn = self.redis.get_connection_manager().await?;

        let queue_key = format!("payout:queue:{}", miner_address);
        let payout_jsons: Vec<String> = conn
            .lrange(&queue_key, 0, -1)
            .await
            .map_err(|e| anyhow!("Failed to get payout queue: {}", e))?;

        let mut payouts = Vec::new();
        for json in payout_jsons {
            if let Ok(payout) = serde_json::from_str::<Payout>(&json) {
                payouts.push(payout);
            }
        }

        Ok(payouts)
    }

    /// Get pending balance for a miner
    pub async fn get_pending_balance(&self, miner_address: &str) -> Result<u64> {
        let mut conn = self.redis.get_connection_manager().await?;

        let balance_key = format!("payout:{}:pending", miner_address);
        let balance: u64 = conn.get(&balance_key).await.unwrap_or(0);

        Ok(balance)
    }

    /// Mark payout as paid (remove from queue)
    pub async fn mark_paid(
        &self,
        miner_address: &str,
        payout: &Payout,
        txid: String,
    ) -> Result<()> {
        let mut conn = self.redis.get_connection_manager().await?;

        // Remove from queue
        let queue_key = format!("payout:queue:{}", miner_address);
        let payout_json = serde_json::to_string(payout)?;
        conn.lrem::<_, _, ()>(&queue_key, 1, &payout_json)
            .await
            .map_err(|e| anyhow!("Failed to remove from queue: {}", e))?;

        // Decrement pending balance
        let pending_key = format!("payout:{}:pending", miner_address);
        let pending: i64 = conn
            .decr(&pending_key, payout.amount)
            .await
            .map_err(|e| anyhow!("Failed to update pending balance: {}", e))?;

        // Maintain pending balances index
        let pending_index = "payout:pending:balances";
        if pending <= 0 {
            conn.zrem::<_, _, ()>(pending_index, miner_address)
                .await
                .map_err(|e| anyhow!("Failed to update pending index: {}", e))?;
            let _: () = conn.set(&pending_key, 0u64).await.unwrap_or(());
        } else {
            conn.zadd::<_, _, _, ()>(pending_index, miner_address, pending)
                .await
                .map_err(|e| anyhow!("Failed to update pending index: {}", e))?;
        }

        // Increment total paid
        let total_key = format!("payout:{}:total", miner_address);
        conn.incr::<_, _, ()>(&total_key, payout.amount)
            .await
            .map_err(|e| anyhow!("Failed to update total paid: {}", e))?;

        metrics::inc_payouts_paid();
        metrics::sub_payout_pending_atomic(payout.amount);
        metrics::dec_payout_queue_len();

        // Store payment record
        let payment_key = format!("payment:{}:{}", txid, miner_address);
        let payment_data = serde_json::json!({
            "miner": miner_address,
            "amount": payout.amount,
            "block": payout.block_height,
            "txid": txid,
            "timestamp": chrono::Utc::now().timestamp(),
        });
        conn.set::<_, _, ()>(&payment_key, payment_data.to_string())
            .await
            .map_err(|e| anyhow!("Failed to store payment record: {}", e))?;

        tracing::info!(
            "Marked paid: {} → {} atomic units (txid: {})",
            miner_address,
            payout.amount,
            txid
        );

        Ok(())
    }

    /// Calculate how much can be paid from the queue (full or capped by max_amount)
    pub async fn calculate_payable_amount(
        &self,
        miner_address: &str,
        max_amount: u64,
    ) -> Result<u64> {
        let mut conn = self.redis.get_connection_manager().await?;

        let queue_key = format!("payout:queue:{}", miner_address);
        let payout_jsons: Vec<String> = conn
            .lrange(&queue_key, 0, -1)
            .await
            .map_err(|e| anyhow!("Failed to get payout queue: {}", e))?;

        let mut total: u64 = 0;
        for json in payout_jsons {
            if let Ok(payout) = serde_json::from_str::<Payout>(&json) {
                if max_amount > 0 && total.saturating_add(payout.amount) > max_amount {
                    break;
                }
                total = total.saturating_add(payout.amount);
            }
        }

        Ok(total)
    }

    /// Settle payouts from the queue up to paid_amount and move them to history
    pub async fn settle_pending_amount(
        &self,
        miner_address: &str,
        paid_amount: u64,
        txid: &str,
    ) -> Result<u64> {
        if paid_amount == 0 {
            return Ok(0);
        }

        let mut conn = self.redis.get_connection_manager().await?;
        let queue_key = format!("payout:queue:{}", miner_address);
        let payout_jsons: Vec<String> = conn
            .lrange(&queue_key, 0, -1)
            .await
            .map_err(|e| anyhow!("Failed to get payout queue: {}", e))?;

        let mut to_remove: Vec<String> = Vec::new();
        let mut total: u64 = 0;
        for json in payout_jsons.iter() {
            if let Ok(payout) = serde_json::from_str::<Payout>(json) {
                if total.saturating_add(payout.amount) > paid_amount {
                    break;
                }
                total = total.saturating_add(payout.amount);
                to_remove.push(json.clone());
            }
        }

        if total == 0 {
            return Ok(0);
        }

        let history_key = format!("payout:history:{}", miner_address);
        for json in to_remove.iter() {
            let mut entry = serde_json::from_str::<serde_json::Value>(json)
                .unwrap_or_else(|_| json!({"raw": json}));
            entry["txid"] = json!(txid);
            let entry_json = entry.to_string();

            conn.lrem::<_, _, ()>(&queue_key, 1, json)
                .await
                .map_err(|e| anyhow!("Failed to remove from queue: {}", e))?;
            conn.rpush::<_, _, ()>(&history_key, entry_json)
                .await
                .map_err(|e| anyhow!("Failed to store payout history: {}", e))?;

            metrics::inc_payouts_paid();
            metrics::dec_payout_queue_len();
        }

        // Update pending and total paid
        let pending_key = format!("payout:{}:pending", miner_address);
        let pending: i64 = conn
            .decr(&pending_key, total)
            .await
            .map_err(|e| anyhow!("Failed to update pending balance: {}", e))?;

        let total_key = format!("payout:{}:total", miner_address);
        conn.incr::<_, _, ()>(&total_key, total)
            .await
            .map_err(|e| anyhow!("Failed to update total paid: {}", e))?;

        metrics::sub_payout_pending_atomic(total);

        let pending_index = "payout:pending:balances";
        if pending <= 0 {
            conn.zrem::<_, _, ()>(pending_index, miner_address)
                .await
                .map_err(|e| anyhow!("Failed to update pending index: {}", e))?;
            let _: () = conn.set(&pending_key, 0u64).await.unwrap_or(());
        } else {
            conn.zadd::<_, _, _, ()>(pending_index, miner_address, pending)
                .await
                .map_err(|e| anyhow!("Failed to update pending index: {}", e))?;
        }

        Ok(total)
    }

    /// Get shares from PPLNS window
    async fn get_window_shares(&self) -> Result<Vec<WindowShare>> {
        let mut conn = self.redis.get_connection_manager().await?;

        // Get share IDs from window (last N shares)
        let share_ids: Vec<String> = conn
            .zrevrange("shares:window", 0, (self.window_size - 1) as isize)
            .await
            .map_err(|e| anyhow!("Failed to get PPLNS window: {}", e))?;

        let mut shares = Vec::new();

        // Get share data for each ID
        for share_id in share_ids {
            let share_json: Option<String> = conn.get(&share_id).await.ok();
            let Some(share_json) = share_json else {
                continue;
            };

            let parsed: Result<StoredShare, _> = serde_json::from_str(&share_json);
            if let Ok(stored) = parsed {
                shares.push(WindowShare {
                    _share_id: share_id,
                    miner_address: stored.miner_address,
                    difficulty: stored.difficulty,
                    _timestamp: stored.timestamp,
                });
            }
        }

        Ok(shares)
    }
}

/// Pure-logic PPLNS distribution — no I/O, fully testable.
///
/// Given a slice of `(miner_address, difficulty)` tuples and the total miner
/// reward, returns a `Vec<Payout>` proportional to each miner's weighted shares.
pub fn compute_pplns_payouts(
    shares: &[(String, u64)],
    miner_share_reward: u64,
    block_height: u64,
    block_hash: &str,
    timestamp: i64,
) -> Vec<Payout> {
    if shares.is_empty() {
        return Vec::new();
    }

    let mut miner_weighted: HashMap<String, (f64, u64)> = HashMap::new();
    let mut total_weight = 0.0f64;

    for (addr, diff) in shares {
        let w = *diff as f64;
        total_weight += w;
        let e = miner_weighted.entry(addr.clone()).or_insert((0.0, 0));
        e.0 += w;
        e.1 += 1;
    }

    let mut payouts = Vec::new();
    for (miner_address, (weighted, count)) in miner_weighted {
        let proportion = weighted / total_weight;
        let amount = (miner_share_reward as f64 * proportion) as u64;
        if amount > 0 {
            payouts.push(Payout {
                miner_address,
                amount,
                block_height,
                block_hash: block_hash.to_string(),
                timestamp,
                shares_count: count,
                weighted_shares: weighted,
            });
        }
    }
    payouts
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // Inline arithmetic (pre-existing)
    // =========================================================================

    #[test]
    fn test_pplns_distribution_equal_shares() {
        let total_reward: u64 = 1_000_000_000_000;
        let miner_a_weight = 10_000.0;
        let miner_b_weight = 10_000.0;
        let total_weight = 20_000.0;

        let payout_a = (total_reward as f64 * (miner_a_weight / total_weight)) as u64;
        let payout_b = (total_reward as f64 * (miner_b_weight / total_weight)) as u64;

        assert_eq!(payout_a, 500_000_000_000);
        assert_eq!(payout_b, 500_000_000_000);
    }

    #[test]
    fn test_pplns_distribution_unequal_difficulty() {
        let total_reward: u64 = 1_000_000_000_000;
        let miner_a_weight = 20_000.0;
        let miner_b_weight = 10_000.0;
        let total_weight = 30_000.0;

        let payout_a = (total_reward as f64 * (miner_a_weight / total_weight)) as u64;
        let payout_b = (total_reward as f64 * (miner_b_weight / total_weight)) as u64;

        assert_eq!(payout_a, 666_666_666_666);
        assert_eq!(payout_b, 333_333_333_333);
    }

    // =========================================================================
    // Pure-logic PPLNS distribution (compute_pplns_payouts)
    // =========================================================================

    fn make_shares(spec: &[(&str, u64, u64)]) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        for &(addr, diff, count) in spec {
            for _ in 0..count {
                out.push((addr.to_string(), diff));
            }
        }
        out
    }

    #[test]
    fn test_compute_empty_shares() {
        let payouts = compute_pplns_payouts(&[], 1_000_000, 1, "hash", 0);
        assert!(payouts.is_empty());
    }

    #[test]
    fn test_compute_single_miner() {
        let shares = make_shares(&[("zion1alice", 1000, 10)]);
        let payouts = compute_pplns_payouts(&shares, 1_000_000_000_000, 100, "blockhash", 12345);
        assert_eq!(payouts.len(), 1);
        assert_eq!(payouts[0].miner_address, "zion1alice");
        assert_eq!(payouts[0].amount, 1_000_000_000_000); // 100% to sole miner
        assert_eq!(payouts[0].shares_count, 10);
        assert_eq!(payouts[0].block_height, 100);
    }

    #[test]
    fn test_compute_two_miners_equal() {
        let shares = make_shares(&[
            ("zion1alice", 1000, 10), // 10,000 weight
            ("zion1bob",   1000, 10), // 10,000 weight
        ]);
        let payouts = compute_pplns_payouts(&shares, 1_000_000_000_000, 1, "h", 0);
        assert_eq!(payouts.len(), 2);

        let total: u64 = payouts.iter().map(|p| p.amount).sum();
        // Due to f64 truncation, total may be slightly less than reward
        assert!(total <= 1_000_000_000_000);
        assert!(total >= 999_999_999_998);

        for p in &payouts {
            assert_eq!(p.amount, 500_000_000_000);
        }
    }

    #[test]
    fn test_compute_two_miners_unequal_difficulty() {
        // Alice: 10 shares @ diff 2000 = 20,000
        // Bob:   10 shares @ diff 1000 = 10,000
        // Alice gets 2/3, Bob gets 1/3
        let shares = make_shares(&[
            ("zion1alice", 2000, 10),
            ("zion1bob",   1000, 10),
        ]);
        let payouts = compute_pplns_payouts(&shares, 900_000_000_000, 2, "h2", 0);

        let alice = payouts.iter().find(|p| p.miner_address == "zion1alice").unwrap();
        let bob   = payouts.iter().find(|p| p.miner_address == "zion1bob").unwrap();

        assert_eq!(alice.amount, 600_000_000_000); // 2/3 of 900B
        assert_eq!(bob.amount,   300_000_000_000); // 1/3 of 900B
    }

    #[test]
    fn test_compute_many_miners_proportional() {
        // 5 miners with different difficulties
        let shares = make_shares(&[
            ("m1", 100,  50),
            ("m2", 200,  25),
            ("m3", 400,  10),
            ("m4", 50,  100),
            ("m5", 1000,  5),
        ]);
        let reward = 10_000_000_000_000u64; // 10 ZION
        let payouts = compute_pplns_payouts(&shares, reward, 3, "h3", 0);
        assert_eq!(payouts.len(), 5);

        // Total weight = 50*100 + 25*200 + 10*400 + 100*50 + 5*1000
        //              = 5000 + 5000 + 4000 + 5000 + 5000 = 24000
        // Each miner's expected share:
        //   m1: 5000/24000 ≈ 20.83%
        //   m2: 5000/24000 ≈ 20.83%
        //   m3: 4000/24000 ≈ 16.67%
        //   m4: 5000/24000 ≈ 20.83%
        //   m5: 5000/24000 ≈ 20.83%

        let total: u64 = payouts.iter().map(|p| p.amount).sum();
        // Truncation means total <= reward
        assert!(total <= reward);
        // But very close (within 5 atomic units max for 5 miners)
        assert!(reward - total <= 5, "Rounding loss too high: {}", reward - total);
    }

    #[test]
    fn test_compute_dust_amounts_filtered() {
        // Miner with negligible share → amount rounds to 0 → excluded
        let shares = make_shares(&[
            ("whale", 1_000_000, 100),
            ("dust",  1,          1),
        ]);
        let payouts = compute_pplns_payouts(&shares, 1_000_000, 1, "h", 0);
        // dust miner's share: 1 / (100*1M + 1) ≈ 1e-8, payout ≈ 0.01 → truncated to 0
        assert!(payouts.iter().all(|p| p.amount > 0), "No zero-amount payouts allowed");
    }

    #[test]
    fn test_compute_preserves_block_metadata() {
        let shares = make_shares(&[("zion1miner", 500, 5)]);
        let payouts = compute_pplns_payouts(&shares, 1000, 999, "abc123", 1700000000);
        let p = &payouts[0];
        assert_eq!(p.block_height, 999);
        assert_eq!(p.block_hash, "abc123");
        assert_eq!(p.timestamp, 1700000000);
    }
}
