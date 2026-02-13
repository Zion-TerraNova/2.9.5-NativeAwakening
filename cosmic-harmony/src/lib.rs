//! # Cosmic Harmony v3 - Multi-Algorithm Mining Engine
//! 
//! Modulární mining algoritmus s profit routingem a ZION revenue.
//! 
//! ## Architecture
//! 
//! ```text
//! Input → [Module Pipeline] → [Profit Router] → [Outputs]
//!                                    ↓
//!                              ZION Revenue (fee)
//! ```
//! 
//! ## Revenue Model (50/25/25)
//! 
//! Compute allocation:
//! - 50% → ZION mining (Keccak→SHA3→Matrix→Fusion)
//!   └── FREE byproducts: Keccak→ETC/NiceHash, SHA3→Nexus/0xBTC
//! - 25% → Multi-Algo profit-switch (ERG/RVN/KAS/ALPH)
//! - 25% → NCL AI inference tasks
//!
//! ZION projekt získává revenue z:
//! - 5% fee z merged mining outputs (Keccak/SHA3 FREE byproducts)
//! - 2% fee z multi-algo profit-switched mining
//! - 10% fee z NCL AI tasks
//! 
//! ## Usage
//! 
//! ```rust,ignore
//! use zion_cosmic_harmony_v3::{CosmicHarmonyV3, Config};
//! 
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = Config::default();
//!     let engine = CosmicHarmonyV3::new(config).await?;
//!     
//!     // Mine with automatic profit routing
//!     let result = engine.mine(b"block header", 12345).await?;
//!     Ok(())
//! }
//! ```

// Note: SIMD optimizations use target_feature for stable Rust compatibility

pub mod algorithms;
pub mod algorithms_opt;  // Optimized versions (no nightly required)
pub mod algorithm_library;  // CH v3 Algorithm Module Library (12+ algorithms)
pub mod config;
pub mod engine;
pub mod ffi;  // C-compatible FFI for Python/Node.js
pub mod gpu;  // GPU mining (OpenCL/Metal)
pub mod modules;
pub mod multichain;  // Multi-chain GPU mining for external pools (ETC, RVN, ERG, KAS)
pub mod native_ffi;  // FFI to native C libraries (RandomX, Yescrypt, CH v2)
pub mod ncl_integration;  // NCL AI Bonus - 5th revenue stream
pub mod pool_manager;
pub mod profit_router;
pub mod revenue;
pub mod whattomine;  // WhatToMine/CoinGecko API integration

pub use config::Config;
pub use config::{MultiChainMiningConfig, ExternalPoolConfig};
pub use engine::CosmicHarmonyV3;
pub use revenue::RevenueCollector;
pub use algorithms_opt::{Hash32, Hash64, cosmic_harmony_v3};
pub use algorithm_library::{AlgorithmModuleLibrary, AlgorithmInfo, PipelineExecutionResult};
pub use whattomine::{WhatToMineClient, ProfitabilityData};
pub use multichain::{ExternalChain, MultiChainEngine, MultiChainConfig};

#[cfg(feature = "gpu")]
pub use gpu::{GpuMiner, GpuConfig, GpuDevice, GpuBackend};

// Re-export fee constants at crate root for convenience
pub use fees::{
    MERGED_MINING_FEE, PROFIT_SWITCH_FEE, NCL_FEE, MIN_ZION_ALLOCATION,
    ZION_ALLOCATION, MULTI_ALGO_ALLOCATION, NCL_ALLOCATION,
};


/// ZION fee percentages (50/25/25 model)
pub mod fees {
    /// Fee on merged mining outputs (Keccak, SHA3 — FREE byproducts)
    pub const MERGED_MINING_FEE: f64 = 0.05;  // 5%
    
    /// Fee on multi-algo profit-switched mining (25% compute)
    pub const PROFIT_SWITCH_FEE: f64 = 0.02;  // 2%
    
    /// Fee on NCL AI task revenue (25% compute)
    pub const NCL_FEE: f64 = 0.10;  // 10%
    
    /// ZION compute allocation (always 50%+)
    pub const ZION_ALLOCATION: f64 = 0.50;  // 50%
    
    /// Multi-Algo compute allocation
    pub const MULTI_ALGO_ALLOCATION: f64 = 0.25;  // 25%
    
    /// NCL AI compute allocation
    pub const NCL_ALLOCATION: f64 = 0.25;  // 25%
    
    /// Minimum ZION allocation (never goes below)
    pub const MIN_ZION_ALLOCATION: f64 = 0.50;  // 50%
}

use serde::{Serialize, Deserialize};

/// Algorithm types supported by CH v3
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AlgorithmType {
    // Native (always active)
    Keccak256,
    Sha3_512,
    GoldenMatrix,
    CosmicFusion,
    
    // GPU algorithms
    Autolykos2,
    KawPow,
    KHeavyHash,
    Blake3,
    Ethash,
    Equihash,
    ProgPow,
    
    // CPU algorithms
    RandomX,
    Yescrypt,
    Argon2d,
}

impl AlgorithmType {
    /// Get target coin for this algorithm
    pub fn target_coin(&self) -> &'static str {
        match self {
            Self::Keccak256 => "ETC",
            Self::Sha3_512 => "NXS",
            Self::Autolykos2 => "ERG",
            Self::KawPow => "RVN",
            Self::KHeavyHash => "KAS",
            Self::Blake3 => "ALPH",
            Self::Ethash => "ETC",
            Self::Equihash => "ZEC",
            Self::ProgPow => "SERO",
            Self::RandomX => "XMR",
            Self::Yescrypt => "YTN",
            Self::Argon2d => "DYNAMIC",
            Self::GoldenMatrix => "ZION",
            Self::CosmicFusion => "ZION",
        }
    }
    
    /// Is this a native ZION module?
    pub fn is_native(&self) -> bool {
        matches!(self, 
            Self::Keccak256 | 
            Self::Sha3_512 | 
            Self::GoldenMatrix | 
            Self::CosmicFusion
        )
    }
    
    /// Requires GPU?
    pub fn requires_gpu(&self) -> bool {
        matches!(self,
            Self::Autolykos2 |
            Self::KawPow |
            Self::KHeavyHash |
            Self::Blake3 |
            Self::Ethash |
            Self::Equihash |
            Self::ProgPow
        )
    }
}

/// Result of a mining operation
#[derive(Debug, Clone)]
pub struct MiningResult {
    /// ZION hash (always produced)
    pub zion_hash: [u8; 32],
    
    /// Nonce used
    pub nonce: u64,
    
    /// Exportable hashes for other networks
    pub exports: Vec<ExportHash>,
    
    /// Revenue breakdown
    pub revenue: RevenueBreakdown,
}

/// Exportable hash for another network
#[derive(Debug, Clone)]
pub struct ExportHash {
    pub algorithm: AlgorithmType,
    pub hash: Vec<u8>,
    pub target_coin: String,
    pub meets_difficulty: bool,
}

/// Revenue breakdown per mining operation
#[derive(Debug, Clone, Default)]
pub struct RevenueBreakdown {
    /// Miner's share (in USD equivalent)
    pub miner_share: f64,
    
    /// ZION project fee (in USD equivalent)
    pub zion_fee: f64,
    
    /// Per-algorithm breakdown
    pub by_algorithm: std::collections::HashMap<AlgorithmType, f64>,
}
