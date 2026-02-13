//! Revenue collection and fee tracking

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::{AlgorithmType, Config, ExportHash, MERGED_MINING_FEE, PROFIT_SWITCH_FEE, NCL_FEE};

/// Revenue breakdown
#[derive(Debug, Clone, Default)]
pub struct RevenueStats {
    /// Total earnings (before fees)
    pub total_earnings_usd: f64,
    
    /// ZION fees collected
    pub zion_fees_usd: f64,
    
    /// Miner payout (after fees)
    pub miner_payout_usd: f64,
    
    /// Breakdown by source
    pub by_source: HashMap<String, f64>,
}

/// Revenue collector
pub struct RevenueCollector {
    #[allow(dead_code)]
    config: Arc<RwLock<Config>>,
    stats: Arc<RwLock<RevenueStats>>,
    pending_fees: Arc<RwLock<f64>>,
}

impl RevenueCollector {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        Self {
            config,
            stats: Arc::new(RwLock::new(RevenueStats::default())),
            pending_fees: Arc::new(RwLock::new(0.0)),
        }
    }
    
    /// Track export for fee calculation
    pub async fn track_export(&self, export: &ExportHash, value_usd: f64) {
        if !export.meets_difficulty {
            return;
        }

        let fee_rate = if export.algorithm.is_native() {
            MERGED_MINING_FEE
        } else {
            PROFIT_SWITCH_FEE
        };
        
        let fee = value_usd * fee_rate;
        let miner_share = value_usd - fee;
        
        let mut stats = self.stats.write().await;
        stats.total_earnings_usd += value_usd;
        stats.zion_fees_usd += fee;
        stats.miner_payout_usd += miner_share;
        
        let source = format!("{:?}", export.algorithm);
        *stats.by_source.entry(source).or_insert(0.0) += value_usd;
        
        let mut pending = self.pending_fees.write().await;
        *pending += fee;
    }
    
    /// Track NCL AI task revenue
    pub async fn track_ncl_task(&self, value_usd: f64) {
        let fee = value_usd * NCL_FEE;
        let miner_share = value_usd - fee;
        
        let mut stats = self.stats.write().await;
        stats.total_earnings_usd += value_usd;
        stats.zion_fees_usd += fee;
        stats.miner_payout_usd += miner_share;
        
        *stats.by_source.entry("NCL".to_string()).or_insert(0.0) += value_usd;
        
        let mut pending = self.pending_fees.write().await;
        *pending += fee;
    }
    
    /// Get current stats
    pub async fn get_stats(&self) -> RevenueStats {
        self.stats.read().await.clone()
    }
    
    /// Get pending fees to be paid to ZION treasury
    pub async fn get_pending_fees(&self) -> f64 {
        *self.pending_fees.read().await
    }
    
    /// Process payout (mark fees as paid)
    pub async fn process_payout(&self) -> anyhow::Result<f64> {
        let mut pending = self.pending_fees.write().await;
        let amount = *pending;
        *pending = 0.0;
        Ok(amount)
    }
    
    /// Calculate fee for a given algorithm and value
    pub fn calculate_fee(algorithm: AlgorithmType, value_usd: f64) -> f64 {
        let fee_rate = if algorithm.is_native() {
            MERGED_MINING_FEE
        } else {
            PROFIT_SWITCH_FEE
        };
        value_usd * fee_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_revenue_tracking() {
        let config = Arc::new(RwLock::new(Config::default()));
        let collector = RevenueCollector::new(config);
        
        let export = ExportHash {
            algorithm: AlgorithmType::Keccak256,
            hash: vec![0u8; 32],
            target_coin: "ETC".to_string(),
            meets_difficulty: true,
        };
        
        collector.track_export(&export, 10.0).await;
        
        let stats = collector.get_stats().await;
        assert_eq!(stats.total_earnings_usd, 10.0);
        assert!((stats.zion_fees_usd - 0.5).abs() < 0.001); // 5% of 10
    }

    #[tokio::test]
    async fn test_revenue_ignores_non_qualifying_export() {
        let config = Arc::new(RwLock::new(Config::default()));
        let collector = RevenueCollector::new(config);

        let export = ExportHash {
            algorithm: AlgorithmType::Keccak256,
            hash: vec![0u8; 32],
            target_coin: "ETC".to_string(),
            meets_difficulty: false,
        };

        collector.track_export(&export, 10.0).await;

        let stats = collector.get_stats().await;
        assert_eq!(stats.total_earnings_usd, 0.0);
        assert_eq!(stats.zion_fees_usd, 0.0);
        assert_eq!(stats.miner_payout_usd, 0.0);
    }
    
    #[test]
    fn test_fee_calculation() {
        // Merged mining: 5%
        let fee = RevenueCollector::calculate_fee(AlgorithmType::Keccak256, 100.0);
        assert!((fee - 5.0).abs() < 0.001);
        
        // Profit switch: 2%
        let fee = RevenueCollector::calculate_fee(AlgorithmType::Autolykos2, 100.0);
        assert!((fee - 2.0).abs() < 0.001);
    }
}
