use tokio_postgres::{Client, NoTls, Error};
use std::time::Duration;
use tokio::time::sleep;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::future::Future;

use crate::blockchain::ZionRPCClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPayout {
    pub id: i64,
    pub miner_address: String,
    pub amount: f64,
    pub shares_count: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedPayout {
    pub id: i64,
    pub miner_address: String,
    pub amount: f64,
    pub tx_hash: String,
    pub paid_at: DateTime<Utc>,
}

pub struct PayoutScheduler {
    client: Client,
    min_payout_amount: f64,
    payout_interval: Duration,
}

impl PayoutScheduler {
    /// Create new payout scheduler
    pub async fn new(
        db_url: &str,
        min_payout_amount: f64,
        payout_interval: Duration,
    ) -> Result<Self, Error> {
        let (client, connection) = tokio_postgres::connect(db_url, NoTls).await?;
        
        // Spawn connection handler
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("PostgreSQL connection error: {}", e);
            }
        });
        
        Ok(Self {
            client,
            min_payout_amount,
            payout_interval,
        })
    }
    
    /// Initialize database schema
    pub async fn init_schema(&self) -> Result<(), Error> {
        self.client.batch_execute(r#"
            CREATE TABLE IF NOT EXISTS pending_payouts (
                id BIGSERIAL PRIMARY KEY,
                miner_address VARCHAR(256) NOT NULL,
                amount DOUBLE PRECISION NOT NULL,
                shares_count BIGINT NOT NULL,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                UNIQUE(miner_address)
            );
            
            CREATE INDEX IF NOT EXISTS idx_pending_payouts_amount 
            ON pending_payouts(amount) WHERE amount >= 0.1;
            
            CREATE TABLE IF NOT EXISTS completed_payouts (
                id BIGSERIAL PRIMARY KEY,
                miner_address VARCHAR(256) NOT NULL,
                amount DOUBLE PRECISION NOT NULL,
                tx_hash VARCHAR(128) NOT NULL,
                paid_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            );
            
            CREATE INDEX IF NOT EXISTS idx_completed_payouts_miner 
            ON completed_payouts(miner_address);
            
            CREATE INDEX IF NOT EXISTS idx_completed_payouts_date 
            ON completed_payouts(paid_at DESC);
        "#).await?;
        
        Ok(())
    }
    
    /// Add or update pending payout
    pub async fn add_pending_payout(
        &self,
        miner_address: &str,
        amount: f64,
        shares_count: i64,
    ) -> Result<(), Error> {
        self.client.execute(
            r#"
            INSERT INTO pending_payouts 
                (miner_address, amount, shares_count)
            VALUES ($1, $2, $3)
            ON CONFLICT (miner_address) DO UPDATE 
            SET 
                amount = pending_payouts.amount + EXCLUDED.amount,
                shares_count = pending_payouts.shares_count + EXCLUDED.shares_count
            "#,
            &[&miner_address, &amount, &shares_count],
        ).await?;
        
        Ok(())
    }
    
    /// Get all payouts ready to be paid
    pub async fn get_payouts_ready(&self) -> Result<Vec<PendingPayout>, Error> {
        let rows = self.client.query(
            r#"
            SELECT id, miner_address, amount, shares_count, created_at
            FROM pending_payouts
            WHERE amount >= $1
            ORDER BY amount DESC
            "#,
            &[&self.min_payout_amount],
        ).await?;
        
        let payouts = rows.iter().map(|row| PendingPayout {
            id: row.get(0),
            miner_address: row.get(1),
            amount: row.get(2),
            shares_count: row.get(3),
            created_at: row.get(4),
        }).collect();
        
        Ok(payouts)
    }
    
    /// Mark payout as completed
    pub async fn mark_payout_completed(
        &self,
        payout_id: i64,
        tx_hash: &str,
    ) -> Result<(), Error> {
        // Move to completed_payouts
        self.client.execute(
            r#"
            INSERT INTO completed_payouts (miner_address, amount, tx_hash)
            SELECT miner_address, amount, $2
            FROM pending_payouts
            WHERE id = $1
            "#,
            &[&payout_id, &tx_hash],
        ).await?;
        
        // Remove from pending
        self.client.execute(
            "DELETE FROM pending_payouts WHERE id = $1",
            &[&payout_id],
        ).await?;
        
        Ok(())
    }
    
    /// Get payout history for miner
    pub async fn get_miner_history(
        &self,
        miner_address: &str,
        limit: i64,
    ) -> Result<Vec<CompletedPayout>, Error> {
        let rows = self.client.query(
            r#"
            SELECT id, miner_address, amount, tx_hash, paid_at
            FROM completed_payouts
            WHERE miner_address = $1
            ORDER BY paid_at DESC
            LIMIT $2
            "#,
            &[&miner_address, &limit],
        ).await?;
        
        let payouts = rows.iter().map(|row| CompletedPayout {
            id: row.get(0),
            miner_address: row.get(1),
            amount: row.get(2),
            tx_hash: row.get(3),
            paid_at: row.get(4),
        }).collect();
        
        Ok(payouts)
    }
    
    /// Get total paid to miner
    pub async fn get_miner_total_paid(&self, miner_address: &str) -> Result<f64, Error> {
        let row = self.client.query_one(
            r#"
            SELECT COALESCE(SUM(amount), 0.0)
            FROM completed_payouts
            WHERE miner_address = $1
            "#,
            &[&miner_address],
        ).await?;
        
        Ok(row.get(0))
    }
    
    /// Start automatic payout processing loop
    pub async fn start_auto_payout_loop<F>(
        &self,
        mut process_payout: F,
    ) -> Result<(), Error>
    where
        F: FnMut(&PendingPayout) -> Result<String, Box<dyn std::error::Error + Send + Sync>>,
    {
        loop {
            sleep(self.payout_interval).await;
            
            tracing::info!("🔄 Running automatic payout cycle...");
            
            let payouts = match self.get_payouts_ready().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!("Failed to get payouts: {}", e);
                    continue;
                }
            };
            
            tracing::info!("📤 Found {} payouts ready to process", payouts.len());
            
            for payout in payouts {
                tracing::info!(
                    "💰 Processing payout: {} -> {} ZION",
                    payout.miner_address,
                    payout.amount,
                );
                
                match process_payout(&payout) {
                    Ok(tx_hash) => {
                        match self.mark_payout_completed(payout.id, &tx_hash).await {
                            Ok(_) => {
                                tracing::info!("✅ Payout completed: tx {}", tx_hash);
                            }
                            Err(e) => {
                                tracing::error!("❌ Failed to mark payout completed: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("❌ Failed to process payout: {}", e);
                    }
                }
                
                // Small delay between payouts
                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    /// Async variant of `start_auto_payout_loop`.
    pub async fn start_auto_payout_loop_async<F, Fut>(
        &self,
        mut process_payout: F,
    ) -> Result<(), Error>
    where
        F: FnMut(PendingPayout) -> Fut,
        Fut: Future<Output = Result<String, Box<dyn std::error::Error + Send + Sync>>> + Send,
    {
        loop {
            sleep(self.payout_interval).await;

            tracing::info!("🔄 Running automatic payout cycle...");

            let payouts = match self.get_payouts_ready().await {
                Ok(p) => p,
                Err(e) => {
                    tracing::error!("Failed to get payouts: {}", e);
                    continue;
                }
            };

            tracing::info!("📤 Found {} payouts ready to process", payouts.len());

            for payout in payouts {
                tracing::info!(
                    "💰 Processing payout: {} -> {} ZION",
                    payout.miner_address,
                    payout.amount,
                );

                match process_payout(payout.clone()).await {
                    Ok(tx_hash) => match self.mark_payout_completed(payout.id, &tx_hash).await {
                        Ok(_) => tracing::info!("✅ Payout completed: tx {}", tx_hash),
                        Err(e) => tracing::error!("❌ Failed to mark payout completed: {}", e),
                    },
                    Err(e) => tracing::error!("❌ Failed to process payout: {}", e),
                }

                sleep(Duration::from_millis(500)).await;
            }
        }
    }

    /// Start automatic payout loop that actually sends transactions via `ZionRPCClient`.
    ///
    /// This is intentionally separate from the Redis-based `PayoutManager` pipeline.
    /// Enable it only when you explicitly want PostgreSQL-driven payouts.
    pub async fn start_rpc_payout_loop(
        &self,
        rpc_client: Arc<ZionRPCClient>,
        pool_wallet: String,
    ) -> anyhow::Result<()> {
        if pool_wallet.is_empty() {
            anyhow::bail!("PayoutScheduler: pool wallet is empty");
        }

        self.start_auto_payout_loop_async(move |payout| {
            let rpc_client = rpc_client.clone();
            let pool_wallet = pool_wallet.clone();
            async move {
                let tx = rpc_client
                    .send_transaction(
                        &pool_wallet,
                        &payout.miner_address,
                        payout.amount,
                        Some("pool-payout"),
                    )
                    .await?;
                let tx_id = tx
                    .get("tx_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| tx.get("txid").and_then(|v| v.as_str()))
                    .ok_or_else(|| anyhow::anyhow!("sendtransaction returned no tx_id"))?;
                Ok::<String, Box<dyn std::error::Error + Send + Sync>>(tx_id.to_string())
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))
    }
    
    /// Simple monitoring loop that logs payouts but does not send transactions.
    ///
    /// For real payouts, prefer `PayoutManager` (Redis-based) or call
    /// `start_rpc_payout_loop` explicitly.
    pub async fn run(&self) {
        println!("PayoutScheduler: Starting monitoring loop...");
        loop {
            sleep(self.payout_interval).await;
            
            match self.get_payouts_ready().await {
                Ok(payouts) if !payouts.is_empty() => {
                    println!("PayoutScheduler: {} payouts ready (min: {} ZION)", 
                        payouts.len(), self.min_payout_amount);
                    for p in &payouts {
                        println!("  - {} → {} ZION ({} shares)",
                            p.miner_address, p.amount, p.shares_count);
                    }
                }
                Ok(_) => {
                    // No payouts ready - silent
                }
                Err(e) => {
                    eprintln!("PayoutScheduler error: {}", e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    #[ignore] // Requires PostgreSQL
    async fn test_payout_scheduler() {
        let scheduler = PayoutScheduler::new(
            "postgresql://zion:zion@localhost/zion_pool",
            0.1,
            Duration::from_secs(60),
        ).await.unwrap();
        
        scheduler.init_schema().await.unwrap();
        
        // Add pending payout
        scheduler.add_pending_payout(
            "ZION_TEST_ADDRESS",
            1.5,
            100,
        ).await.unwrap();
        
        // Get payouts ready
        let payouts = scheduler.get_payouts_ready().await.unwrap();
        assert_eq!(payouts.len(), 1);
        assert_eq!(payouts[0].miner_address, "ZION_TEST_ADDRESS");
        assert_eq!(payouts[0].amount, 1.5);
    }
}
