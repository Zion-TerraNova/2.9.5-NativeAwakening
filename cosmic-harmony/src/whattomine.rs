//! WhatToMine API Integration for CH v3 Profit Router
//!
//! Fetches real-time profitability data from WhatToMine and CoinGecko
//! to enable dynamic algorithm switching based on actual market conditions.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::AlgorithmType;

/// WhatToMine coin data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatToMineCoin {
    pub id: u32,
    pub tag: String,
    pub algorithm: String,
    pub block_time: f64,
    pub block_reward: f64,
    pub difficulty: f64,
    pub nethash: f64,
    pub exchange_rate: f64,
    pub exchange_rate_vol: f64,
    pub btc_revenue: String,
    pub revenue: String,
    pub cost: String,
    pub profit: String,
    pub estimated_rewards: String,
    pub profit24: Option<String>,
}

/// WhatToMine API response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WhatToMineResponse {
    coins: HashMap<String, WhatToMineCoin>,
}

/// CoinGecko price data
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoinGeckoPrice {
    usd: f64,
    usd_24h_change: Option<f64>,
}

/// Profitability calculation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitabilityData {
    /// Algorithm type
    pub algorithm: AlgorithmType,
    
    /// Target coin ticker
    pub coin: String,
    
    /// Current price in USD
    pub price_usd: f64,
    
    /// Network difficulty
    pub difficulty: f64,
    
    /// Network hashrate
    pub nethash: f64,
    
    /// Estimated coins per day (at 1 MH/s reference)
    pub coins_per_day: f64,
    
    /// Revenue per day in USD
    pub revenue_usd: f64,
    
    /// Estimated power cost (at 0.10 USD/kWh, 300W)
    pub power_cost_usd: f64,
    
    /// Net profit per day
    pub profit_usd: f64,
    
    /// Last updated timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// WhatToMine API client
pub struct WhatToMineClient {
    /// HTTP client
    client: reqwest::Client,
    
    /// Cached profitability data
    cache: Arc<RwLock<HashMap<AlgorithmType, ProfitabilityData>>>,
    
    /// Cache TTL in seconds
    cache_ttl: u64,
    
    /// Last update time
    last_update: Arc<RwLock<Option<chrono::DateTime<chrono::Utc>>>>,
    
    /// Power cost per kWh
    power_cost_kwh: f64,
    
    /// Reference power draw in watts
    power_draw_watts: f64,
}

impl WhatToMineClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("ZION-CH3/1.0")
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_ttl: 300,  // 5 minutes
            last_update: Arc::new(RwLock::new(None)),
            power_cost_kwh: 0.10,  // USD/kWh
            power_draw_watts: 300.0,  // Watts
        }
    }
    
    /// Set power cost parameters
    pub fn with_power_cost(mut self, cost_kwh: f64, power_watts: f64) -> Self {
        self.power_cost_kwh = cost_kwh;
        self.power_draw_watts = power_watts;
        self
    }
    
    /// Get profit for a specific algorithm (used by multichain work dispatcher)
    /// Returns USD/day profit for the algorithm
    pub async fn get_coin_profit(&self, algorithm: &str) -> anyhow::Result<f64> {
        let data = self.fetch_profitability().await?;
        
        // Map algorithm name to AlgorithmType
        let algo_type = match algorithm.to_lowercase().as_str() {
            "ethash" | "etchash" => Some(crate::AlgorithmType::Ethash),
            "kawpow" => Some(crate::AlgorithmType::KawPow),
            "autolykos" | "autolykos2" => Some(crate::AlgorithmType::Autolykos2),
            "kheavyhash" => Some(crate::AlgorithmType::KHeavyHash),
            "blake3" => Some(crate::AlgorithmType::Blake3),
            "equihash" => Some(crate::AlgorithmType::Equihash),
            "randomx" => Some(crate::AlgorithmType::RandomX),
            _ => None,
        };
        
        if let Some(algo) = algo_type {
            if let Some(profit_data) = data.get(&algo) {
                return Ok(profit_data.profit_usd);
            }
        }
        
        Ok(0.0)
    }
    
    /// Check if cache is valid
    async fn is_cache_valid(&self) -> bool {
        if let Some(last) = *self.last_update.read().await {
            let now = chrono::Utc::now();
            let diff = now.signed_duration_since(last);
            return diff.num_seconds() < self.cache_ttl as i64;
        }
        false
    }
    
    /// Fetch profitability from WhatToMine
    pub async fn fetch_profitability(&self) -> anyhow::Result<HashMap<AlgorithmType, ProfitabilityData>> {
        // Check cache first
        if self.is_cache_valid().await {
            return Ok(self.cache.read().await.clone());
        }
        
        // Fetch from WhatToMine GPU endpoint
        let gpu_data = self.fetch_whattomine_gpu().await;
        
        // Fetch from CoinGecko as fallback/supplement
        let prices = self.fetch_coingecko_prices().await;
        
        // Merge and calculate
        let mut result = HashMap::new();
        
        // Process WhatToMine data if available
        if let Ok(coins) = gpu_data {
            result.extend(self.process_whattomine_coins(&coins, &prices.unwrap_or_default()));
        } else {
            // Fallback: use CoinGecko prices with estimated network data
            result.extend(self.create_fallback_data(&prices.unwrap_or_default()));
        }
        
        // Update cache
        {
            let mut cache = self.cache.write().await;
            *cache = result.clone();
        }
        {
            let mut last = self.last_update.write().await;
            *last = Some(chrono::Utc::now());
        }
        
        Ok(result)
    }
    
    /// Fetch WhatToMine GPU coins
    async fn fetch_whattomine_gpu(&self) -> anyhow::Result<HashMap<String, WhatToMineCoin>> {
        // WhatToMine API endpoint
        // Using coins.json with GPU parameters for various algorithms
        let url = "https://whattomine.com/coins.json?\
            eth=true&factor%5Beth_hr%5D=100.0&\
            etc_hr=130.0&etc=true&\
            kawpow_hr=60.0&kawpow=true&\
            autolykos_hr=300.0&autolykos=true&\
            kheavyhash_hr=1500.0&kheavyhash=true&\
            blake3_hr=5000.0&blake3=true&\
            equihash_hr=100.0&equihash=true";
        
        let resp = self.client
            .get(url)
            .send()
            .await?;
        
        if !resp.status().is_success() {
            anyhow::bail!("WhatToMine API error: {}", resp.status());
        }
        
        let data: WhatToMineResponse = resp.json().await?;
        Ok(data.coins)
    }
    
    /// Fetch prices from CoinGecko
    async fn fetch_coingecko_prices(&self) -> anyhow::Result<HashMap<String, f64>> {
        let coins = "ethereum-classic,ergo,ravencoin,kaspa,alephium,zcash,monero";
        let url = format!(
            "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
            coins
        );
        
        let resp = self.client
            .get(&url)
            .send()
            .await?;
        
        if !resp.status().is_success() {
            anyhow::bail!("CoinGecko API error: {}", resp.status());
        }
        
        let data: HashMap<String, CoinGeckoPrice> = resp.json().await?;
        
        // Map to ticker
        let mut prices = HashMap::new();
        let mapping = [
            ("ethereum-classic", "ETC"),
            ("ergo", "ERG"),
            ("ravencoin", "RVN"),
            ("kaspa", "KAS"),
            ("alephium", "ALPH"),
            ("zcash", "ZEC"),
            ("monero", "XMR"),
        ];
        
        for (gecko_id, ticker) in mapping {
            if let Some(price_data) = data.get(gecko_id) {
                prices.insert(ticker.to_string(), price_data.usd);
            }
        }
        
        Ok(prices)
    }
    
    /// Process WhatToMine coins into ProfitabilityData
    fn process_whattomine_coins(
        &self,
        coins: &HashMap<String, WhatToMineCoin>,
        prices: &HashMap<String, f64>,
    ) -> HashMap<AlgorithmType, ProfitabilityData> {
        let mut result = HashMap::new();
        let now = chrono::Utc::now();
        
        // Daily power cost
        let daily_power_cost = (self.power_draw_watts / 1000.0) * 24.0 * self.power_cost_kwh;
        
        // Map WhatToMine algorithms to our types
        let algo_mapping: HashMap<&str, (AlgorithmType, &str)> = [
            ("Etchash", (AlgorithmType::Ethash, "ETC")),
            ("Autolykos", (AlgorithmType::Autolykos2, "ERG")),
            ("KawPow", (AlgorithmType::KawPow, "RVN")),
            ("kHeavyHash", (AlgorithmType::KHeavyHash, "KAS")),
            ("Blake3", (AlgorithmType::Blake3, "ALPH")),
            ("Equihash", (AlgorithmType::Equihash, "ZEC")),
        ].into_iter().collect();
        
        for (_name, coin) in coins {
            if let Some((algo_type, ticker)) = algo_mapping.get(coin.algorithm.as_str()) {
                let price = prices.get(*ticker)
                    .copied()
                    .unwrap_or(coin.exchange_rate);
                
                let revenue: f64 = coin.revenue.replace("$", "").parse().unwrap_or(0.0);
                let profit = revenue - daily_power_cost;
                
                result.insert(*algo_type, ProfitabilityData {
                    algorithm: *algo_type,
                    coin: ticker.to_string(),
                    price_usd: price,
                    difficulty: coin.difficulty,
                    nethash: coin.nethash,
                    coins_per_day: coin.estimated_rewards.parse().unwrap_or(0.0),
                    revenue_usd: revenue,
                    power_cost_usd: daily_power_cost,
                    profit_usd: profit,
                    updated_at: now,
                });
            }
        }
        
        result
    }
    
    /// Create fallback data from CoinGecko prices
    fn create_fallback_data(
        &self,
        prices: &HashMap<String, f64>,
    ) -> HashMap<AlgorithmType, ProfitabilityData> {
        let mut result = HashMap::new();
        let now = chrono::Utc::now();
        let daily_power_cost = (self.power_draw_watts / 1000.0) * 24.0 * self.power_cost_kwh;
        
        // Estimated daily coin earnings (very rough estimates)
        let estimates: HashMap<&str, (AlgorithmType, f64)> = [
            ("ETC", (AlgorithmType::Ethash, 0.05)),        // ~0.05 ETC/day at 130 MH/s
            ("ERG", (AlgorithmType::Autolykos2, 0.3)),     // ~0.3 ERG/day at 300 MH/s
            ("RVN", (AlgorithmType::KawPow, 30.0)),        // ~30 RVN/day at 60 MH/s
            ("KAS", (AlgorithmType::KHeavyHash, 100.0)),   // ~100 KAS/day at 1.5 GH/s
            ("ALPH", (AlgorithmType::Blake3, 0.5)),        // ~0.5 ALPH/day at 5 GH/s
            ("ZEC", (AlgorithmType::Equihash, 0.01)),      // ~0.01 ZEC/day at 100 Sol/s
            ("XMR", (AlgorithmType::RandomX, 0.003)),      // ~0.003 XMR/day at 15 kH/s
        ].into_iter().collect();
        
        for (ticker, (algo_type, coins_per_day)) in &estimates {
            if let Some(&price) = prices.get(*ticker) {
                let revenue = coins_per_day * price;
                let profit = revenue - daily_power_cost;
                
                result.insert(*algo_type, ProfitabilityData {
                    algorithm: *algo_type,
                    coin: ticker.to_string(),
                    price_usd: price,
                    difficulty: 0.0,  // Unknown
                    nethash: 0.0,     // Unknown
                    coins_per_day: *coins_per_day,
                    revenue_usd: revenue,
                    power_cost_usd: daily_power_cost,
                    profit_usd: profit,
                    updated_at: now,
                });
            }
        }
        
        result
    }
    
    /// Get best algorithm for GPU mining
    pub async fn get_best_gpu_algorithm(&self) -> anyhow::Result<Option<(AlgorithmType, ProfitabilityData)>> {
        let data = self.fetch_profitability().await?;
        
        let gpu_algos = [
            AlgorithmType::Ethash,
            AlgorithmType::Autolykos2,
            AlgorithmType::KawPow,
            AlgorithmType::KHeavyHash,
            AlgorithmType::Blake3,
            AlgorithmType::Equihash,
        ];
        
        let best = gpu_algos.iter()
            .filter_map(|a| data.get(a).map(|d| (*a, d.clone())))
            .max_by(|a, b| a.1.profit_usd.partial_cmp(&b.1.profit_usd).unwrap());
        
        Ok(best)
    }
    
    /// Get best algorithm for CPU mining
    pub async fn get_best_cpu_algorithm(&self) -> anyhow::Result<Option<(AlgorithmType, ProfitabilityData)>> {
        let data = self.fetch_profitability().await?;
        
        // Currently only RandomX is properly tracked
        if let Some(xmr) = data.get(&AlgorithmType::RandomX) {
            return Ok(Some((AlgorithmType::RandomX, xmr.clone())));
        }
        
        Ok(None)
    }
    
    /// Get all profitability sorted by profit
    pub async fn get_sorted_profitability(&self) -> anyhow::Result<Vec<ProfitabilityData>> {
        let data = self.fetch_profitability().await?;
        let mut sorted: Vec<_> = data.into_values().collect();
        sorted.sort_by(|a, b| b.profit_usd.partial_cmp(&a.profit_usd).unwrap());
        Ok(sorted)
    }
}

impl Default for WhatToMineClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_coingecko_fallback() {
        let client = WhatToMineClient::new();
        
        // CoinGecko should work
        let prices = client.fetch_coingecko_prices().await;
        // May fail due to rate limiting, so just check it doesn't panic
        if let Ok(prices) = prices {
            println!("Fetched {} prices", prices.len());
            for (ticker, price) in &prices {
                println!("  {}: ${:.2}", ticker, price);
            }
        }
    }
    
    #[tokio::test]
    async fn test_fallback_data() {
        let client = WhatToMineClient::new();
        
        // Create with mock prices
        let mut prices = HashMap::new();
        prices.insert("ETC".to_string(), 25.0);
        prices.insert("ERG".to_string(), 1.5);
        prices.insert("RVN".to_string(), 0.02);
        prices.insert("KAS".to_string(), 0.15);
        
        let data = client.create_fallback_data(&prices);
        
        assert!(!data.is_empty());
        
        // ETC should have positive revenue
        if let Some(etc) = data.get(&AlgorithmType::Ethash) {
            assert!(etc.revenue_usd > 0.0);
            println!("ETC: ${:.2}/day revenue, ${:.2}/day profit", 
                etc.revenue_usd, etc.profit_usd);
        }
    }
}
