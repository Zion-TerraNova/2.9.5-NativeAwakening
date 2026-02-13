//! Algorithm Workers - GPU workers for each external algorithm
//!
//! Each worker handles mining for a specific algorithm using native libraries.

pub mod ethash_worker;
pub mod kawpow_worker;
pub mod autolykos_worker;
pub mod kheavyhash_worker;
pub mod blake3_worker;

use super::{ExternalChain, MiningJob, ChainStats};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::RwLock;

pub use ethash_worker::EthashWorker;
pub use kawpow_worker::KawPowWorker;
pub use autolykos_worker::AutolykosWorker;
pub use kheavyhash_worker::KHeavyHashWorker;
pub use blake3_worker::Blake3Worker;

/// Share found by worker
#[derive(Debug, Clone)]
pub struct FoundShare {
    pub chain: ExternalChain,
    pub job_id: String,
    pub nonce: u64,
    pub hash: Vec<u8>,
    pub mix_hash: Option<Vec<u8>>,
    pub extra: HashMap<String, serde_json::Value>,
}

/// Algorithm worker trait
#[async_trait::async_trait]
pub trait AlgorithmWorker: Send + Sync {
    /// Get supported chain
    fn chain(&self) -> ExternalChain;
    
    /// Get algorithm name
    fn algorithm(&self) -> &'static str;
    
    /// Initialize worker
    async fn init(&mut self) -> Result<()>;
    
    /// Start mining on job
    async fn mine(&self, job: &MiningJob, allocation: f32) -> Result<()>;
    
    /// Stop mining
    async fn stop(&self);
    
    /// Get current hashrate
    fn hashrate(&self) -> f64;
    
    /// Get stats
    fn stats(&self) -> ChainStats;
    
    /// Check if running
    fn is_running(&self) -> bool;
}

/// Worker pool managing all algorithm workers
pub struct WorkerPool {
    workers: HashMap<ExternalChain, Arc<RwLock<Box<dyn AlgorithmWorker>>>>,
    share_tx: Option<tokio::sync::mpsc::Sender<FoundShare>>,
}

impl WorkerPool {
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
            share_tx: None,
        }
    }

    /// Initialize with share channel
    pub fn with_share_channel(mut self, tx: tokio::sync::mpsc::Sender<FoundShare>) -> Self {
        self.share_tx = Some(tx);
        self
    }

    /// Add worker for chain
    pub fn add_worker(&mut self, worker: Box<dyn AlgorithmWorker>) {
        let chain = worker.chain();
        self.workers.insert(chain, Arc::new(RwLock::new(worker)));
    }

    /// Initialize default workers
    pub async fn init_default_workers(&mut self) -> Result<()> {
        // Ethash (ETC)
        let mut ethash = Box::new(EthashWorker::new()) as Box<dyn AlgorithmWorker>;
        ethash.init().await?;
        self.add_worker(ethash);

        // KawPow (RVN)
        let mut kawpow = Box::new(KawPowWorker::new(ExternalChain::RVN)) as Box<dyn AlgorithmWorker>;
        kawpow.init().await?;
        self.add_worker(kawpow);

        // Autolykos (ERG)
        let mut autolykos = Box::new(AutolykosWorker::new()) as Box<dyn AlgorithmWorker>;
        autolykos.init().await?;
        self.add_worker(autolykos);

        // kHeavyHash (KAS)
        let mut kheavy = Box::new(KHeavyHashWorker::new()) as Box<dyn AlgorithmWorker>;
        kheavy.init().await?;
        self.add_worker(kheavy);

        // Blake3 (ALPH)
        let mut blake3 = Box::new(Blake3Worker::new()) as Box<dyn AlgorithmWorker>;
        blake3.init().await?;
        self.add_worker(blake3);

        log::info!("ch3_worker_pool_initialized workers={}", self.workers.len());
        Ok(())
    }

    /// Start all workers
    pub async fn start_all(&self) -> Result<()> {
        for (chain, worker) in &self.workers {
            log::info!("ch3_worker_started chain={:?}", chain);
        }
        Ok(())
    }

    /// Stop all workers
    pub async fn stop_all(&self) {
        for (chain, worker) in &self.workers {
            let w = worker.read().await;
            w.stop().await;
            log::info!("ch3_worker_stopped chain={:?}", chain);
        }
    }

    /// Get worker for chain
    pub fn get_worker(&self, chain: ExternalChain) -> Option<Arc<RwLock<Box<dyn AlgorithmWorker>>>> {
        self.workers.get(&chain).cloned()
    }

    /// Aggregate stats from all workers
    pub async fn aggregate_stats(&self) -> super::MultiChainStats {
        let mut stats = super::MultiChainStats::default();

        for (chain, worker) in &self.workers {
            let w = worker.read().await;
            let chain_stats = w.stats();
            
            stats.total_hashrate += chain_stats.hashrate;
            stats.shares_accepted += chain_stats.shares_accepted;
            stats.shares_rejected += chain_stats.shares_rejected;
            stats.per_chain.insert(*chain, chain_stats);
        }

        stats
    }
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new()
    }
}
