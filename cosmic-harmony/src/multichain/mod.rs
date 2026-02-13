//! Multi-Chain GPU Mining for Cosmic Harmony v3
//!
//! Enables mining on external chains (ETC, RVN, ERG, KAS, etc.)
//! while simultaneously contributing to ZION block rewards.
//!
//! Architecture:
//! ```text
//! External Pools --> Job Receiver --> Algorithm Workers --> Multi-Submitter
//!                                             |
//!                                   Work Dispatcher (profit routing)
//!                                             |
//!                                   ZION Cosmic Fusion
//! ```

pub mod job_receiver;
pub mod work_dispatcher;
pub mod multi_submitter;
pub mod algorithm_workers;

pub use job_receiver::{ExternalJobReceiver, MiningJob, PoolConnection};
pub use work_dispatcher::{WorkDispatcher, AllocationStrategy};
pub use multi_submitter::{MultiChainSubmitter, SubmitResult};
pub use algorithm_workers::{AlgorithmWorker, WorkerPool};

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Serialize, Deserialize};

/// Supported external chains for multi-chain mining
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExternalChain {
    /// Ethereum Classic (Ethash)
    ETC,
    /// Ravencoin (KawPow)
    RVN,
    /// Ergo (Autolykos v2)
    ERG,
    /// Kaspa (kHeavyHash)
    KAS,
    /// Alephium (Blake3)
    ALPH,
    /// Zcash (Equihash)
    ZEC,
    /// Veil (ProgPow)
    VEIL,
    /// Dynamic (Argon2d)
    DYN,
    /// Clore.ai (KawPow)
    CLORE,
}

impl ExternalChain {
    pub fn algorithm(&self) -> &'static str {
        match self {
            Self::ETC => "ethash",
            Self::RVN | Self::CLORE => "kawpow",
            Self::ERG => "autolykos_v2",
            Self::KAS => "kheavyhash",
            Self::ALPH => "blake3",
            Self::ZEC => "equihash",
            Self::VEIL => "progpow",
            Self::DYN => "argon2d",
        }
    }

    pub fn default_pool(&self) -> (&'static str, u16) {
        match self {
            Self::ETC => ("etc.2miners.com", 1010),
            Self::RVN => ("rvn.2miners.com", 6060),
            Self::ERG => ("erg.2miners.com", 8888),
            Self::KAS => ("pool.woolypooly.com", 3112),
            Self::ALPH => ("pool.woolypooly.com", 3106),
            Self::ZEC => ("zec.2miners.com", 1010),
            Self::VEIL => ("veil.suprnova.cc", 7220),
            Self::DYN => ("pool.dynamic.org", 3333),
            Self::CLORE => ("clore.herominers.com", 1170),
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "etc" | "ethereum_classic" => Some(Self::ETC),
            "rvn" | "ravencoin" => Some(Self::RVN),
            "erg" | "ergo" => Some(Self::ERG),
            "kas" | "kaspa" => Some(Self::KAS),
            "alph" | "alephium" => Some(Self::ALPH),
            "zec" | "zcash" => Some(Self::ZEC),
            "veil" => Some(Self::VEIL),
            "dyn" | "dynamic" => Some(Self::DYN),
            "clore" => Some(Self::CLORE),
            _ => None,
        }
    }
}

/// Multi-chain mining configuration
#[derive(Debug, Clone)]
pub struct MultiChainConfig {
    /// Enabled chains
    pub enabled_chains: Vec<ExternalChain>,
    /// Wallet addresses per chain
    pub wallets: HashMap<ExternalChain, String>,
    /// Pool overrides (chain -> (host, port))
    pub pool_overrides: HashMap<ExternalChain, (String, u16)>,
    /// Allocation percentages (chain -> % of hashpower)
    pub allocations: HashMap<ExternalChain, f32>,
    /// Minimum profit threshold for switching (%)
    pub profit_switch_threshold: f32,
    /// Cooldown between switches (seconds)
    pub switch_cooldown_secs: u64,
}

impl Default for MultiChainConfig {
    fn default() -> Self {
        Self {
            enabled_chains: vec![ExternalChain::ETC, ExternalChain::ERG, ExternalChain::RVN],
            wallets: HashMap::new(),
            pool_overrides: HashMap::new(),
            allocations: HashMap::new(),
            profit_switch_threshold: 10.0,  // 10% minimum improvement
            switch_cooldown_secs: 300,       // 5 minutes
        }
    }
}

/// Main multi-chain mining engine
pub struct MultiChainEngine {
    config: MultiChainConfig,
    job_receiver: Arc<RwLock<ExternalJobReceiver>>,
    dispatcher: Arc<RwLock<WorkDispatcher>>,
    submitter: Arc<MultiChainSubmitter>,
    workers: Arc<RwLock<WorkerPool>>,
    running: std::sync::atomic::AtomicBool,
}

impl MultiChainEngine {
    /// Create new multi-chain engine
    pub fn new(config: MultiChainConfig) -> Self {
        Self {
            config: config.clone(),
            job_receiver: Arc::new(RwLock::new(ExternalJobReceiver::new())),
            dispatcher: Arc::new(RwLock::new(WorkDispatcher::new(config.clone()))),
            submitter: Arc::new(MultiChainSubmitter::new()),
            workers: Arc::new(RwLock::new(WorkerPool::new())),
            running: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Start multi-chain mining
    pub async fn start(&self) -> anyhow::Result<()> {
        use std::sync::atomic::Ordering;
        
        if self.running.swap(true, Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Already running"));
        }

        log::info!("🚀 Starting multi-chain mining engine");

        // Connect to all enabled pools
        for chain in &self.config.enabled_chains {
            let (host, port) = self.config.pool_overrides
                .get(chain)
                .map(|(h, p)| (h.as_str(), *p))
                .unwrap_or_else(|| chain.default_pool());
            
            let wallet = self.config.wallets
                .get(chain)
                .cloned()
                .unwrap_or_else(|| "YOUR_WALLET".to_string());

            let mut receiver = self.job_receiver.write().await;
            if let Err(e) = receiver.connect_pool(*chain, host, port, &wallet).await {
                log::error!("Failed to connect to {:?} pool: {}", chain, e);
            } else {
                log::info!("✅ Connected to {:?} pool: {}:{}", chain, host, port);
            }
        }

        // Start workers
        let workers = self.workers.read().await;
        workers.start_all().await?;

        // Start dispatcher
        let dispatcher = self.dispatcher.read().await;
        dispatcher.start().await?;

        log::info!("✅ Multi-chain mining active on {} chains", self.config.enabled_chains.len());
        
        Ok(())
    }

    /// Stop multi-chain mining
    pub async fn stop(&self) {
        use std::sync::atomic::Ordering;
        
        self.running.store(false, Ordering::SeqCst);
        
        // Stop workers
        let workers = self.workers.read().await;
        workers.stop_all().await;

        // Disconnect pools
        let mut receiver = self.job_receiver.write().await;
        receiver.disconnect_all().await;

        log::info!("🛑 Multi-chain mining stopped");
    }

    /// Get current stats
    pub async fn stats(&self) -> MultiChainStats {
        let workers = self.workers.read().await;
        workers.aggregate_stats().await
    }
}

/// Aggregated multi-chain stats
#[derive(Debug, Clone, Default)]
pub struct MultiChainStats {
    pub total_hashrate: f64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub blocks_found: u64,
    pub per_chain: HashMap<ExternalChain, ChainStats>,
}

#[derive(Debug, Clone, Default)]
pub struct ChainStats {
    pub chain: Option<ExternalChain>,
    pub hashrate: f64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub last_share_time: Option<u64>,
}
