//! Profit router with WhatToMine integration

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::{AlgorithmType, Config, WhatToMineClient};

/// Profitability data for an algorithm
#[derive(Debug, Clone)]
pub struct AlgoProfitability {
    pub algorithm: AlgorithmType,
    pub coin: String,
    pub revenue_per_day_usd: f64,
    pub power_cost_usd: f64,
    pub profit_per_day_usd: f64,
}

/// Profit router for dynamic algorithm switching
pub struct ProfitRouter {
    config: Arc<RwLock<Config>>,
    current_algo: Arc<RwLock<AlgorithmType>>,
    profitability: Arc<RwLock<HashMap<AlgorithmType, AlgoProfitability>>>,
    whattomine: WhatToMineClient,
}

impl ProfitRouter {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        Self {
            config,
            current_algo: Arc::new(RwLock::new(AlgorithmType::Autolykos2)),
            profitability: Arc::new(RwLock::new(HashMap::new())),
            whattomine: WhatToMineClient::new(),
        }
    }
    
    /// Get current algorithm
    pub async fn current_algorithm(&self) -> AlgorithmType {
        *self.current_algo.read().await
    }
    
    /// Update profitability data
    pub async fn update_profitability(&self) -> anyhow::Result<()> {
        let data = self.whattomine.fetch_profitability().await?;
        let config = self.config.read().await;
        let switchable = &config.profit_router.switchable_algos;

        let mut prof = self.profitability.write().await;
        prof.clear();

        for algo in switchable {
            if let Some(d) = data.get(algo) {
                prof.insert(
                    *algo,
                    AlgoProfitability {
                        algorithm: *algo,
                        coin: d.coin.clone(),
                        revenue_per_day_usd: d.revenue_usd,
                        power_cost_usd: d.power_cost_usd,
                        profit_per_day_usd: d.profit_usd,
                    },
                );
            }
        }

        Ok(())
    }
    
    /// Check if should switch algorithm
    pub async fn should_switch(&self) -> Option<AlgorithmType> {
        let config = self.config.read().await;
        let current = *self.current_algo.read().await;
        let prof = self.profitability.read().await;
        
        let current_profit = prof.get(&current)
            .map(|p| p.profit_per_day_usd)
            .unwrap_or(0.0);
        
        let mut best_algo = current;
        let mut best_profit = current_profit;
        
        for (algo, data) in prof.iter() {
            if data.profit_per_day_usd > best_profit {
                best_profit = data.profit_per_day_usd;
                best_algo = *algo;
            }
        }
        
        // Check if improvement exceeds threshold
        if best_algo != current {
            let improvement = if current_profit.abs() < f64::EPSILON {
                if best_profit > current_profit {
                    f64::INFINITY
                } else {
                    0.0
                }
            } else {
                (best_profit - current_profit) / current_profit
            };

            if improvement >= config.profit_router.switch_threshold {
                return Some(best_algo);
            }
        }
        
        None
    }
    
    /// Switch to new algorithm
    pub async fn switch_to(&self, algo: AlgorithmType) {
        let mut current = self.current_algo.write().await;
        *current = algo;
    }
    
    /// Get best algorithm for GPU
    pub async fn get_best_gpu_algo(&self) -> Option<AlgorithmType> {
        let prof = self.profitability.read().await;
        
        let gpu_algos = [
            AlgorithmType::Autolykos2,
            AlgorithmType::KawPow,
            AlgorithmType::KHeavyHash,
            AlgorithmType::Blake3,
        ];
        
        gpu_algos.iter()
            .filter_map(|algo| prof.get(algo).map(|p| (*algo, p.profit_per_day_usd)))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
            .map(|(algo, _)| algo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_profit_router() {
        let config = Arc::new(RwLock::new(Config::default()));
        let router = ProfitRouter::new(config);
        
        router.update_profitability().await.unwrap();
        
        let best = router.get_best_gpu_algo().await;
        assert!(best.is_some());
    }
}
