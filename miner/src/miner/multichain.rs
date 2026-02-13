//! Multi-Chain Native Miner
//!
//! Unified mining interface supporting all 12 algorithms via native libraries.
//! Enables profit-switching based on WhatToMine data.

use anyhow::{anyhow, Result};
use log::{debug, info, warn};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::native_algos::{self, NativeAlgorithm};
use crate::stratum::StratumClient;

/// Miner statistics for multi-chain mining
#[derive(Debug, Clone, Default)]
pub struct MultiChainStats {
    pub shares_found: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub total_hashes: u64,
}

/// Supported chains with their algorithms and pool URLs
#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub coin: String,
    pub algorithm: NativeAlgorithm,
    pub pool_url: String,
    pub wallet: String,
    pub enabled: bool,
}

impl ChainConfig {
    pub fn zion(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "ZION".into(),
            algorithm: NativeAlgorithm::CosmicHarmony,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
    
    pub fn monero(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "XMR".into(),
            algorithm: NativeAlgorithm::RandomX,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
    
    pub fn ravencoin(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "RVN".into(),
            algorithm: NativeAlgorithm::KawPow,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
    
    pub fn ergo(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "ERG".into(),
            algorithm: NativeAlgorithm::Autolykos,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
    
    pub fn kaspa(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "KAS".into(),
            algorithm: NativeAlgorithm::KHeavyHash,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
    
    pub fn etc(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "ETC".into(),
            algorithm: NativeAlgorithm::Ethash,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
    
    pub fn zcash(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "ZEC".into(),
            algorithm: NativeAlgorithm::Equihash,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
    
    pub fn alephium(wallet: &str, pool: &str) -> Self {
        Self {
            coin: "ALPH".into(),
            algorithm: NativeAlgorithm::Blake3,
            pool_url: pool.into(),
            wallet: wallet.into(),
            enabled: true,
        }
    }
}

/// Multi-chain miner with profit switching
pub struct MultiChainMiner {
    chains: Vec<ChainConfig>,
    active_chain: Option<usize>,
    worker_name: String,
    cpu_threads: usize,
    gpu_enabled: bool,
    stats: Arc<RwLock<MultiChainStats>>,
    running: Arc<RwLock<bool>>,
}

impl MultiChainMiner {
    pub fn new(worker_name: &str, cpu_threads: usize, gpu_enabled: bool) -> Self {
        Self {
            chains: Vec::new(),
            active_chain: None,
            worker_name: worker_name.into(),
            cpu_threads,
            gpu_enabled,
            stats: Arc::new(RwLock::new(MultiChainStats::default())),
            running: Arc::new(RwLock::new(false)),
        }
    }
    
    pub fn add_chain(&mut self, config: ChainConfig) {
        self.chains.push(config);
    }
    
    pub fn chains(&self) -> &[ChainConfig] {
        &self.chains
    }
    
    /// Select most profitable chain (placeholder - use whattomine.rs for real data)
    pub async fn select_best_chain(&mut self) -> Result<&ChainConfig> {
        // For now, just select first enabled chain
        // TODO: Integrate with whattomine profit router
        for (i, chain) in self.chains.iter().enumerate() {
            if chain.enabled {
                self.active_chain = Some(i);
                return Ok(&self.chains[i]);
            }
        }
        Err(anyhow!("No enabled chains"))
    }
    
    /// Start mining on selected chain
    pub async fn start(&self) -> Result<()> {
        let chain_idx = self.active_chain.ok_or_else(|| anyhow!("No chain selected"))?;
        let chain = &self.chains[chain_idx];
        
        info!("⛏️  Starting {} mining ({:?})", chain.coin, chain.algorithm);
        info!("   Pool: {}", chain.pool_url);
        info!("   Worker: {}", self.worker_name);
        info!("   CPU threads: {}", self.cpu_threads);
        info!("   GPU enabled: {}", self.gpu_enabled);
        
        *self.running.write().await = true;
        
        // Connect to pool
        let stratum = StratumClient::new(
            &chain.pool_url,
            &chain.wallet,
            &self.worker_name,
            chain.algorithm.coin(),
            None,
        )?;
        
        stratum.connect().await?;
        
        // Request initial job
        let _ = stratum.request_job().await;
        
        // Start mining loop
        self.mine_loop(chain, &stratum).await
    }
    
    async fn mine_loop(&self, chain: &ChainConfig, stratum: &StratumClient) -> Result<()> {
        let mut job_rx = stratum.subscribe_jobs().await;
        
        info!("🔄 Waiting for jobs from pool...");
        
        while *self.running.read().await {
            // Wait for new job
            if job_rx.changed().await.is_err() {
                warn!("Job channel closed");
                break;
            }
            
            let job = job_rx.borrow().clone();
            if let Some(job) = job {
                debug!("Got job: {}", job.job_id);
                
                // Parse job data
                let blob = hex::decode(&job.blob).unwrap_or_default();
                let target_str = &job.target;
                
                // Mine!
                self.mine_job(chain, stratum, &blob, target_str, &job.job_id, job.height).await?;
            }
        }
        
        Ok(())
    }
    
    async fn mine_job(
        &self,
        chain: &ChainConfig,
        stratum: &StratumClient,
        blob: &[u8],
        target_str: &str,
        job_id: &str,
        height: u64,
    ) -> Result<()> {
        let threads = self.cpu_threads;
        let algo = chain.algorithm;
        
        // Parse target from hex string
        let target_bytes = hex::decode(target_str).unwrap_or_else(|_| vec![0xff; 32]);
        let target_u64 = if target_bytes.len() >= 8 {
            u64::from_le_bytes(target_bytes[0..8].try_into().unwrap_or([0xff; 8]))
        } else {
            u64::MAX
        };
        
        // Parallel nonce search
        let results: Vec<Option<(u64, Vec<u8>)>> = (0..threads)
            .map(|thread_id| {
                let start_nonce = thread_id as u64 * (u64::MAX / threads as u64);
                let end_nonce = start_nonce + (u64::MAX / threads as u64);
                
                for nonce in (start_nonce..end_nonce).step_by(1000) {
                    // Compute hash using native algorithm
                    if let Ok(hash) = native_algos::compute_hash(algo, blob, nonce, height as u32) {
                        // Check if meets target (simplified)
                        let hash_u64 = u64::from_le_bytes(hash[0..8].try_into().unwrap_or([0; 8]));
                        if hash_u64 < target_u64 {
                            return Some((nonce, hash));
                        }
                    }
                    
                    // Check every 1000 iterations if we should stop
                    // In real impl, use atomic flag
                }
                None
            })
            .collect();
        
        // Submit any found solutions
        for result in results {
            if let Some((nonce, hash)) = result {
                info!("💎 Found share! Nonce: {}", nonce);
                let nonce_u32 = (nonce & 0xFFFFFFFF) as u32;
                stratum.submit_share(job_id, nonce_u32, &hex::encode(&hash)).await?;
                
                // Update stats
                let mut stats = self.stats.write().await;
                stats.shares_found += 1;
            }
        }
        
        Ok(())
    }
    
    pub async fn stop(&self) {
        *self.running.write().await = false;
        info!("⏹️  Miner stopped");
    }
    
    pub async fn get_stats(&self) -> MultiChainStats {
        self.stats.read().await.clone()
    }
    
    /// Run benchmark on all available native algorithms
    pub async fn benchmark_all(&self, iterations: i32) -> Vec<(NativeAlgorithm, f64)> {
        let mut results = Vec::new();
        
        for algo in native_algos::available_algorithms() {
            info!("📊 Benchmarking {:?}...", algo);
            match native_algos::benchmark(algo, iterations) {
                Ok(hashrate) => {
                    info!("   {:?}: {:.2} H/s", algo, hashrate);
                    results.push((algo, hashrate));
                }
                Err(e) => {
                    warn!("   {:?}: Failed - {}", algo, e);
                }
            }
        }
        
        results
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_chain_configs() {
        let zion = ChainConfig::zion("ZION_test_wallet", "pool.zion.io:3333");
        assert_eq!(zion.coin, "ZION");
        assert_eq!(zion.algorithm, NativeAlgorithm::CosmicHarmony);
        
        let rvn = ChainConfig::ravencoin("rvn_wallet", "pool.rvn.io:3333");
        assert_eq!(rvn.algorithm, NativeAlgorithm::KawPow);
    }
    
    #[tokio::test]
    async fn test_miner_creation() {
        let mut miner = MultiChainMiner::new("test-worker", 4, false);
        
        miner.add_chain(ChainConfig::zion("wallet", "localhost:3333"));
        miner.add_chain(ChainConfig::ergo("wallet", "localhost:3334"));
        
        assert_eq!(miner.chains().len(), 2);
        
        let best = miner.select_best_chain().await.unwrap();
        assert_eq!(best.coin, "ZION");
    }
}
