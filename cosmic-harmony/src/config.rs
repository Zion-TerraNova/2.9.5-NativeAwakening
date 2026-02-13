//! Configuration for Cosmic Harmony v3

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::AlgorithmType;
use crate::multichain::ExternalChain;

/// Main configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Algorithm version
    pub version: u32,
    
    /// Module pipeline configuration
    pub pipeline: PipelineConfig,
    
    /// Profit router settings
    pub profit_router: ProfitRouterConfig,
    
    /// Revenue collection settings
    pub revenue: RevenueConfig,
    
    /// Pool connections
    pub pools: HashMap<String, PoolConfig>,
    
    /// ZION wallet for fee collection
    pub zion_fee_wallet: String,
    
    /// Multi-chain GPU mining configuration
    #[serde(default)]
    pub multichain: MultiChainMiningConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 3,
            pipeline: PipelineConfig::default(),
            profit_router: ProfitRouterConfig::default(),
            revenue: RevenueConfig::default(),
            pools: HashMap::new(),
            zion_fee_wallet: "ZION_TREASURY_WALLET".to_string(),
            multichain: MultiChainMiningConfig::default(),
        }
    }
}

/// Pipeline module configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Active module slots
    pub slots: Vec<ModuleSlot>,
    
    /// Minimum ZION allocation (0.50 = 50% in 50/25/25 model)
    /// Keccak & SHA3 are FREE byproducts of ZION pipeline
    pub min_zion_allocation: f64,
    
    /// Enable merged mining (export Keccak/SHA3 intermediate hashes — FREE)
    pub merged_mining_enabled: bool,
    
    /// Enable dynamic profit switching
    pub profit_switching_enabled: bool,

    /// Optional per-algorithm difficulty targets for merged mining exports.
    ///
    /// Value is hex-encoded big-endian target bytes. If shorter than 32 bytes,
    /// it is treated as a MSB prefix and the remainder is filled with 0xFF.
    #[serde(default)]
    pub merged_mining_targets: HashMap<AlgorithmType, String>,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            slots: vec![
                // Native modules (always active)
                ModuleSlot {
                    position: 0,
                    algorithm: AlgorithmType::Keccak256,
                    enabled: true,
                    export_enabled: true,
                    switchable: false,
                },
                ModuleSlot {
                    position: 1,
                    algorithm: AlgorithmType::Sha3_512,
                    enabled: true,
                    export_enabled: true,
                    switchable: false,
                },
                ModuleSlot {
                    position: 2,
                    algorithm: AlgorithmType::GoldenMatrix,
                    enabled: true,
                    export_enabled: false,  // ZION-specific, no export
                    switchable: false,
                },
                // Switchable slot for profit optimization
                ModuleSlot {
                    position: 3,
                    algorithm: AlgorithmType::Autolykos2,  // Default
                    enabled: true,
                    export_enabled: true,
                    switchable: true,  // Can be switched based on profit
                },
                // Final fusion (always last)
                ModuleSlot {
                    position: 99,
                    algorithm: AlgorithmType::CosmicFusion,
                    enabled: true,
                    export_enabled: false,
                    switchable: false,
                },
            ],
            min_zion_allocation: 0.50,
            merged_mining_enabled: true,
            profit_switching_enabled: true,
            merged_mining_targets: HashMap::new(),
        }
    }
}

/// Module slot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleSlot {
    /// Position in pipeline (0 = first, 99 = fusion)
    pub position: u8,
    
    /// Algorithm type
    pub algorithm: AlgorithmType,
    
    /// Is this slot enabled?
    pub enabled: bool,
    
    /// Export hash for merged mining?
    pub export_enabled: bool,
    
    /// Can be dynamically switched?
    pub switchable: bool,
}

/// Profit router configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfitRouterConfig {
    /// WhatToMine API endpoint
    pub whattomine_api: String,
    
    /// Price feed endpoints
    pub price_feeds: Vec<String>,
    
    /// Check interval in seconds
    pub check_interval_secs: u64,
    
    /// Minimum improvement to trigger switch (0.1 = 10%)
    pub switch_threshold: f64,
    
    /// Algorithms to consider for switching
    pub switchable_algos: Vec<AlgorithmType>,
}

impl Default for ProfitRouterConfig {
    fn default() -> Self {
        Self {
            whattomine_api: "https://whattomine.com/api".to_string(),
            price_feeds: vec![
                "https://api.coingecko.com/api/v3".to_string(),
                "https://api.binance.com/api/v3".to_string(),
            ],
            check_interval_secs: 300,  // 5 minutes
            switch_threshold: 0.10,     // 10%
            switchable_algos: vec![
                AlgorithmType::Autolykos2,
                AlgorithmType::KawPow,
                AlgorithmType::KHeavyHash,
                AlgorithmType::Blake3,
                AlgorithmType::RandomX,
            ],
        }
    }
}

/// Revenue collection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueConfig {
    /// Fee on merged mining (0.05 = 5%)
    pub merged_mining_fee: f64,
    
    /// Fee on profit switching (0.02 = 2%)
    pub profit_switch_fee: f64,
    
    /// Fee on NCL AI tasks (0.10 = 10%)
    pub ncl_fee: f64,
    
    /// Auto-convert to ZION?
    pub auto_convert_to_zion: bool,
    
    /// Payout threshold (in USD)
    pub payout_threshold_usd: f64,
}

impl Default for RevenueConfig {
    fn default() -> Self {
        Self {
            merged_mining_fee: 0.05,   // 5%
            profit_switch_fee: 0.02,   // 2%
            ncl_fee: 0.10,             // 10%
            auto_convert_to_zion: true,
            payout_threshold_usd: 10.0,
        }
    }
}

/// Pool connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Pool URL (stratum://host:port)
    pub url: String,
    
    /// Wallet address for this coin
    pub wallet: String,
    
    /// Worker name
    pub worker: String,
    
    /// Pool password
    pub password: String,
    
    /// Algorithm this pool accepts
    pub algorithm: AlgorithmType,
    
    /// Is this enabled?
    pub enabled: bool,
}

/// Multi-chain GPU mining configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiChainMiningConfig {
    /// Enable multi-chain mining
    pub enabled: bool,
    
    /// Enabled external chains
    pub enabled_chains: Vec<ExternalChain>,
    
    /// External pool configurations per chain
    pub external_pools: HashMap<ExternalChain, ExternalPoolConfig>,
    
    /// Profit switch threshold (percentage, e.g., 5.0 = 5%)
    pub profit_switch_threshold: f32,
    
    /// Cooldown between chain switches (seconds)
    pub switch_cooldown_secs: u64,
    
    /// ZION allocation percentage (50% in 50/25/25 model)
    pub zion_allocation: f32,
    
    /// Multi-Algo allocation percentage (25% in 50/25/25 model)
    pub multi_algo_allocation: f32,
    
    /// NCL AI allocation percentage (25% in 50/25/25 model)
    pub ncl_allocation: f32,
    
    /// Enable automatic profit-based allocation
    pub auto_profit_routing: bool,
}

impl Default for MultiChainMiningConfig {
    fn default() -> Self {
        Self {
            enabled: false,  // Disabled by default
            enabled_chains: vec![],
            external_pools: HashMap::new(),
            profit_switch_threshold: 5.0,
            switch_cooldown_secs: 300,
            zion_allocation: 0.50,         // 50% to ZION (Keccak/SHA3 = FREE bonus)
            multi_algo_allocation: 0.25,   // 25% to Multi-Algo (ERG/RVN/KAS/ALPH)
            ncl_allocation: 0.25,          // 25% to NCL AI inference
            auto_profit_routing: true,
        }
    }
}

/// External pool configuration for a specific chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalPoolConfig {
    /// Chain this pool mines
    pub chain: ExternalChain,
    
    /// Pool hostname
    pub host: String,
    
    /// Pool port
    pub port: u16,
    
    /// Wallet address for this chain
    pub wallet: String,
    
    /// Worker name
    pub worker: String,
    
    /// Is this pool enabled?
    pub enabled: bool,
}
