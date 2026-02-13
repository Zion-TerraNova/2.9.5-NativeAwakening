//! Work Dispatcher - Routes work based on profitability
//!
//! Allocates GPU hashpower across enabled chains based on
//! real-time profitability from WhatToMine/CoinGecko APIs.

use super::{ExternalChain, MultiChainConfig};
use crate::whattomine::WhatToMineClient;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// Allocation strategy
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AllocationStrategy {
    /// All hashpower to most profitable
    MostProfitable,
    /// Split proportionally by profit
    Proportional,
    /// Equal split across all enabled
    Equal,
    /// Manual fixed allocation
    Manual,
}

/// Work allocation for a chain
#[derive(Debug, Clone)]
pub struct ChainAllocation {
    pub chain: ExternalChain,
    pub percentage: f32,
    pub profit_per_day: f64,
    pub last_update: Instant,
}

/// Work dispatcher
pub struct WorkDispatcher {
    config: MultiChainConfig,
    strategy: AllocationStrategy,
    allocations: HashMap<ExternalChain, ChainAllocation>,
    whattomine: WhatToMineClient,
    last_switch: Option<Instant>,
    running: bool,
}

impl WorkDispatcher {
    pub fn new(config: MultiChainConfig) -> Self {
        Self {
            config,
            strategy: AllocationStrategy::MostProfitable,
            allocations: HashMap::new(),
            whattomine: WhatToMineClient::new(),
            last_switch: None,
            running: false,
        }
    }

    /// Set allocation strategy
    pub fn set_strategy(&mut self, strategy: AllocationStrategy) {
        self.strategy = strategy;
        log::info!("ch3_allocation_strategy_changed strategy={:?}", strategy);
    }

    /// Start dispatcher (updates allocations periodically)
    pub async fn start(&self) -> anyhow::Result<()> {
        log::info!("ch3_work_dispatcher_started");
        Ok(())
    }

    /// Update allocations based on profitability
    pub async fn update_allocations(&mut self) -> anyhow::Result<()> {
        // Check cooldown
        if let Some(last) = self.last_switch {
            let cooldown = Duration::from_secs(self.config.switch_cooldown_secs);
            if last.elapsed() < cooldown {
                return Ok(());
            }
        }

        // Fetch profits from WhatToMine
        let mut profits: HashMap<ExternalChain, f64> = HashMap::new();
        
        for chain in &self.config.enabled_chains {
            match self.whattomine.get_coin_profit(chain.algorithm()).await {
                Ok(profit) => {
                    profits.insert(*chain, profit);
                }
                Err(e) => {
                    log::warn!("Failed to fetch profit for {:?}: {}", chain, e);
                }
            }
        }

        if profits.is_empty() {
            return Ok(());
        }

        // Calculate allocations based on strategy
        let new_allocations = match self.strategy {
            AllocationStrategy::MostProfitable => {
                self.calculate_most_profitable(&profits)
            }
            AllocationStrategy::Proportional => {
                self.calculate_proportional(&profits)
            }
            AllocationStrategy::Equal => {
                self.calculate_equal()
            }
            AllocationStrategy::Manual => {
                self.calculate_manual()
            }
        };

        // Check if switch is needed (threshold check)
        let should_switch = self.should_switch(&new_allocations);

        if should_switch {
            self.allocations = new_allocations;
            self.last_switch = Some(Instant::now());
            
            log::info!(
                "ch3_allocation_updated allocations={:?}",
                self.allocations.iter()
                    .map(|(c, a)| format!("{:?}:{:.1}%", c, a.percentage))
                    .collect::<Vec<_>>()
            );
        }

        Ok(())
    }

    /// Most profitable strategy: 100% to best chain
    fn calculate_most_profitable(
        &self,
        profits: &HashMap<ExternalChain, f64>,
    ) -> HashMap<ExternalChain, ChainAllocation> {
        let mut result = HashMap::new();
        
        if let Some((best_chain, &best_profit)) = profits.iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
        {
            for chain in &self.config.enabled_chains {
                let percentage = if chain == best_chain { 100.0 } else { 0.0 };
                result.insert(*chain, ChainAllocation {
                    chain: *chain,
                    percentage,
                    profit_per_day: *profits.get(chain).unwrap_or(&0.0),
                    last_update: Instant::now(),
                });
            }
        }
        
        result
    }

    /// Proportional strategy: split by relative profit
    fn calculate_proportional(
        &self,
        profits: &HashMap<ExternalChain, f64>,
    ) -> HashMap<ExternalChain, ChainAllocation> {
        let mut result = HashMap::new();
        
        let total: f64 = profits.values().filter(|&&p| p > 0.0).sum();
        if total <= 0.0 {
            return self.calculate_equal();
        }

        for chain in &self.config.enabled_chains {
            let profit = *profits.get(chain).unwrap_or(&0.0);
            let percentage = if profit > 0.0 {
                (profit / total * 100.0) as f32
            } else {
                0.0
            };
            
            result.insert(*chain, ChainAllocation {
                chain: *chain,
                percentage,
                profit_per_day: profit,
                last_update: Instant::now(),
            });
        }
        
        result
    }

    /// Equal strategy: split evenly
    fn calculate_equal(&self) -> HashMap<ExternalChain, ChainAllocation> {
        let mut result = HashMap::new();
        let count = self.config.enabled_chains.len() as f32;
        let pct = 100.0 / count;

        for chain in &self.config.enabled_chains {
            result.insert(*chain, ChainAllocation {
                chain: *chain,
                percentage: pct,
                profit_per_day: 0.0,
                last_update: Instant::now(),
            });
        }
        
        result
    }

    /// Manual strategy: use configured allocations
    fn calculate_manual(&self) -> HashMap<ExternalChain, ChainAllocation> {
        let mut result = HashMap::new();

        for chain in &self.config.enabled_chains {
            let percentage = *self.config.allocations.get(chain).unwrap_or(&0.0);
            result.insert(*chain, ChainAllocation {
                chain: *chain,
                percentage,
                profit_per_day: 0.0,
                last_update: Instant::now(),
            });
        }
        
        result
    }

    /// Check if switch threshold is met
    fn should_switch(&self, new_allocs: &HashMap<ExternalChain, ChainAllocation>) -> bool {
        if self.allocations.is_empty() {
            return true;
        }

        // Find current best and new best
        let current_best = self.allocations.values()
            .max_by(|a, b| a.percentage.partial_cmp(&b.percentage).unwrap())
            .map(|a| (a.chain, a.profit_per_day));

        let new_best = new_allocs.values()
            .max_by(|a, b| a.profit_per_day.partial_cmp(&b.profit_per_day).unwrap())
            .map(|a| (a.chain, a.profit_per_day));

        match (current_best, new_best) {
            (Some((curr_chain, curr_profit)), Some((new_chain, new_profit))) => {
                if curr_chain == new_chain {
                    return false; // No change needed
                }
                
                if curr_profit <= 0.0 {
                    return true; // Switch if current has no profit
                }
                
                // Check threshold
                let improvement = (new_profit - curr_profit) / curr_profit * 100.0;
                improvement >= self.config.profit_switch_threshold as f64
            }
            (None, Some(_)) => true,
            _ => false,
        }
    }

    /// Get current allocation for chain
    pub fn get_allocation(&self, chain: ExternalChain) -> f32 {
        self.allocations
            .get(&chain)
            .map(|a| a.percentage)
            .unwrap_or(0.0)
    }

    /// Get all allocations
    pub fn get_all_allocations(&self) -> HashMap<ExternalChain, f32> {
        self.allocations
            .iter()
            .map(|(c, a)| (*c, a.percentage))
            .collect()
    }
}
