use anyhow::{anyhow, Result};
use chrono::Utc;
use redis::AsyncCommands;
use std::sync::Arc;
use std::time::Duration;

use crate::blockchain::ZionRPCClient;
use crate::metrics::prometheus as metrics;
use crate::pplns::PPLNSCalculator;
use crate::shares::RedisStorage;

// ZION uses 6 decimal places (1 ZION = 1,000,000 atomic units)
const ATOMIC_UNITS: f64 = 1_000_000.0;

fn atomic_to_zion(amount: u64) -> f64 {
    (amount as f64) / ATOMIC_UNITS
}

fn zion_to_atomic(amount: f64) -> u64 {
    if amount <= 0.0 {
        0
    } else {
        (amount * ATOMIC_UNITS) as u64
    }
}

pub struct PayoutManager {
    storage: Arc<RedisStorage>,
    pplns: Arc<PPLNSCalculator>,
    rpc: Arc<ZionRPCClient>,
    pool_wallet: String,
    min_payout_atomic: u64,
    max_payout_atomic: u64,
    payout_interval: Duration,
    payout_batch_limit: usize,
    confirm_timeout_secs: u64,
}

impl PayoutManager {
    pub fn new(
        storage: Arc<RedisStorage>,
        pplns: Arc<PPLNSCalculator>,
        rpc: Arc<ZionRPCClient>,
        pool_wallet: String,
        min_payout: f64,
        max_payout_per_tx: f64,
        payout_interval_seconds: u64,
        payout_batch_limit: usize,
        confirm_timeout_secs: u64,
    ) -> Self {
        Self {
            storage,
            pplns,
            rpc,
            pool_wallet,
            min_payout_atomic: zion_to_atomic(min_payout),
            max_payout_atomic: zion_to_atomic(max_payout_per_tx),
            payout_interval: Duration::from_secs(payout_interval_seconds.max(1)),
            payout_batch_limit: payout_batch_limit.max(1),
            confirm_timeout_secs,
        }
    }

    pub fn start(self) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(self.payout_interval);
            loop {
                interval.tick().await;
                if let Err(e) = self.confirm_sent_payouts().await {
                    tracing::warn!("Payout confirm loop error: {}", e);
                }
                if let Err(e) = self.process_payouts().await {
                    tracing::warn!("Payout process loop error: {}", e);
                }
            }
        });
    }

    async fn process_payouts(&self) -> Result<()> {
        if self.pool_wallet.is_empty() {
            tracing::debug!("Payout skipped: pool_wallet is empty");
            return Ok(());
        }

        let candidates = self
            .storage
            .list_payout_candidates(self.min_payout_atomic, self.payout_batch_limit)
            .await?;
        
        tracing::info!(
            "💰 Payout check: {} candidates, min_payout={} atomic",
            candidates.len(),
            self.min_payout_atomic
        );
        
        if candidates.is_empty() {
            return Ok(());
        }

        let pool_balance = self
            .rpc
            .get_balance(&self.pool_wallet)
            .await
            .ok()
            .and_then(|v| {
                // Try both balance_zion (ZION Core) and balance (legacy)
                v.get("balance_zion")
                    .and_then(|b| b.as_f64())
                    .or_else(|| v.get("balance").and_then(|b| b.as_f64()))
            })
            .unwrap_or(0.0);
        let mut pool_balance_atomic = zion_to_atomic(pool_balance);
        
        tracing::info!(
            "💰 Pool balance: {} ZION = {} atomic",
            pool_balance,
            pool_balance_atomic
        );
        
        if pool_balance_atomic == 0 {
            tracing::warn!("Payout skipped: pool balance is 0");
            return Ok(());
        }

        for (addr, pending_atomic) in candidates {
            tracing::info!(
                "💰 Processing candidate: {} with {} atomic pending",
                addr,
                pending_atomic
            );
            
            if pending_atomic < self.min_payout_atomic {
                tracing::debug!("Skipping {}: below min_payout", addr);
                continue;
            }

            // If max_payout is 0 (unlimited), use full pending amount
            let cap = if self.max_payout_atomic > 0 {
                self.max_payout_atomic.min(pending_atomic)
            } else {
                pending_atomic  // No cap, use full pending
            };

            let payable = self
                .pplns
                .calculate_payable_amount(&addr, cap)
                .await
                .unwrap_or(0);
            
            tracing::info!(
                "💰 PPLNS result for {}: cap={}, payable={}",
                addr,
                cap,
                payable
            );
            
            if payable < self.min_payout_atomic {
                tracing::debug!("Skipping {}: payable {} < min {}", addr, payable, self.min_payout_atomic);
                continue;
            }

            if pool_balance_atomic < payable {
                // Skip this candidate, try next (don't break - others may fit)
                tracing::info!(
                    "💰 Skipping {}: needs {} but pool has {} (will try smaller candidates)",
                    addr,
                    payable,
                    pool_balance_atomic
                );
                continue;
            }

            let amount_zion = atomic_to_zion(payable);
            let tx = self
                .rpc
                .send_transaction(&self.pool_wallet, &addr, amount_zion, Some("pool-payout"))
                .await
                .map_err(|e| anyhow!("sendtransaction failed: {}", e))?;

            let tx_id = tx
                .get("tx_id")
                .and_then(|v| v.as_str())
                .or_else(|| tx.get("txid").and_then(|v| v.as_str()))
                .ok_or_else(|| anyhow!("sendtransaction returned no tx_id"))?
                .to_string();

            self.record_payout_sent(&addr, payable, &tx_id).await?;
            let settled = self
                .pplns
                .settle_pending_amount(&addr, payable, &tx_id)
                .await
                .unwrap_or(0);
            if settled == 0 {
                tracing::warn!("Payout sent but nothing settled: addr={} txid={}", addr, tx_id);
            }

            pool_balance_atomic = pool_balance_atomic.saturating_sub(payable);
            tracing::info!("💸 Payout sent: {} amount={} tx_id={}", addr, amount_zion, tx_id);
        }

        Ok(())
    }

    async fn record_payout_sent(&self, addr: &str, amount_atomic: u64, txid: &str) -> Result<()> {
        let mut conn = self.storage.get_connection_manager().await?;
        let id: u64 = conn.incr("payout:record:id", 1u64).await.unwrap_or(0);
        let key = format!("payout:record:{}", id);
        let now = Utc::now().timestamp();
        let amount_zion = atomic_to_zion(amount_atomic);

        let _: () = conn
            .hset_multiple(
                &key,
                &[
                    ("id", id.to_string()),
                    ("address", addr.to_string()),
                    ("amount_atomic", amount_atomic.to_string()),
                    ("amount", amount_zion.to_string()),
                    ("status", "sent".to_string()),
                    ("tx_id", txid.to_string()),
                    ("created_ts", now.to_string()),
                    ("updated_ts", now.to_string()),
                ],
            )
            .await
            .map_err(|e| anyhow!("Failed to store payout record: {}", e))?;

        let sent_key = "payout:sent";
        conn.zadd::<_, _, _, ()>(sent_key, id, now)
            .await
            .map_err(|e| anyhow!("Failed to index sent payout: {}", e))?;

        let records_key = "payout:records";
        conn.lpush::<_, _, ()>(records_key, id)
            .await
            .map_err(|e| anyhow!("Failed to index payout record: {}", e))?;
        let _: () = redis::cmd("LTRIM")
            .arg(records_key)
            .arg(0)
            .arg(999)
            .query_async(&mut conn)
            .await
            .unwrap_or(());

        Ok(())
    }

    async fn confirm_sent_payouts(&self) -> Result<()> {
        let mut conn = self.storage.get_connection_manager().await?;
        let now = Utc::now().timestamp();

        let mut cmd = redis::cmd("ZRANGEBYSCORE");
        cmd.arg("payout:sent")
            .arg(0)
            .arg("+inf")
            .arg("LIMIT")
            .arg(0)
            .arg(self.payout_batch_limit as i64);
        let ids: Vec<u64> = cmd
            .query_async(&mut conn)
            .await
            .map_err(|e| anyhow!("Failed to list sent payouts: {}", e))?;

        for id in ids {
            let key = format!("payout:record:{}", id);
            let record: std::collections::HashMap<String, String> = conn
                .hgetall(&key)
                .await
                .unwrap_or_default();

            let tx_id = record.get("tx_id").cloned().unwrap_or_default();
            let updated_ts = record
                .get("updated_ts")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            if tx_id.is_empty() {
                continue;
            }

            let tx = self.rpc.get_transaction(&tx_id).await;
            match tx {
                Ok(v) => {
                    let confirmed = v
                        .get("block_height")
                        .and_then(|h| h.as_u64())
                        .is_some();
                    if confirmed {
                        let _: () = conn
                            .hset(&key, "status", "confirmed")
                            .await
                            .unwrap_or(());
                        let _: () = conn
                            .hset(&key, "updated_ts", now)
                            .await
                            .unwrap_or(());
                        let _: () = conn.zrem("payout:sent", id).await.unwrap_or(());
                    }
                }
                Err(e) => {
                    if self.confirm_timeout_secs > 0 && updated_ts > 0 {
                        let age = now.saturating_sub(updated_ts as i64);
                        if age as u64 >= self.confirm_timeout_secs {
                            let _: () = conn
                                .hset(&key, "status", "failed")
                                .await
                                .unwrap_or(());
                            let _: () = conn
                                .hset(&key, "error", e.to_string())
                                .await
                                .unwrap_or(());
                            let _: () = conn
                                .hset(&key, "updated_ts", now)
                                .await
                                .unwrap_or(());
                            let _: () = conn.zrem("payout:sent", id).await.unwrap_or(());
                            metrics::inc_redis_errors();
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
