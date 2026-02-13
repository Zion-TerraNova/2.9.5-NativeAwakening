//! Main Cosmic Harmony v3 Engine

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, debug};

use crate::{
    Config, AlgorithmType, MiningResult, ExportHash, RevenueBreakdown,
    algorithms_opt,
};
use crate::multichain::{MultiChainEngine, MultiChainConfig, ExternalChain};
use crate::pool_manager::{MiningJob, PoolManager, Share};

fn decode_hex_loose(s: &str) -> anyhow::Result<Vec<u8>> {
    let cleaned = s.trim().trim_start_matches("0x");
    if cleaned.is_empty() {
        return Ok(Vec::new());
    }
    Ok(hex::decode(cleaned)?)
}

fn build_job_input_bytes(job: &MiningJob, nonce: u64) -> anyhow::Result<Vec<u8>> {
    // E2E: we need a deterministic mapping from (job, nonce) -> bytes hashed.
    // Without an algorithm-specific nonce offset spec in config, we do the minimal safe thing:
    // append nonce (little-endian u64) to the decoded blob bytes.
    let mut blob = decode_hex_loose(&job.blob)?;
    blob.extend_from_slice(&nonce.to_le_bytes());
    Ok(blob)
}

fn hash_bytes_for_algorithm(result: &MiningResult, algorithm: AlgorithmType) -> Option<Vec<u8>> {
    if algorithm == AlgorithmType::CosmicFusion || algorithm == AlgorithmType::GoldenMatrix {
        return Some(result.zion_hash.to_vec());
    }

    result
        .exports
        .iter()
        .find(|e| e.algorithm == algorithm)
        .map(|e| e.hash.clone())
}

fn parse_target_hex_prefix(target_hex: &str) -> anyhow::Result<[u8; 32]> {
    let cleaned = target_hex.trim();
    if cleaned.is_empty() {
        anyhow::bail!("empty target hex");
    }
    if cleaned.len() % 2 != 0 {
        anyhow::bail!("target hex must have even length");
    }

    let bytes = hex::decode(cleaned)?;
    if bytes.len() > 32 {
        anyhow::bail!("target too long: {} bytes", bytes.len());
    }

    // Treat shorter targets as a big-endian MSB prefix; fill remainder with 0xFF.
    let mut target = [0xFFu8; 32];
    target[..bytes.len()].copy_from_slice(&bytes);
    Ok(target)
}

#[inline]
fn hash_meets_target_be(hash32_be: &[u8; 32], target_be: &[u8; 32]) -> bool {
    // Big-endian comparison: hash <= target
    for i in 0..32 {
        if hash32_be[i] < target_be[i] {
            return true;
        }
        if hash32_be[i] > target_be[i] {
            return false;
        }
    }
    true
}

#[inline]
fn first_32_bytes(hash: &[u8]) -> Option<[u8; 32]> {
    if hash.len() < 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&hash[..32]);
    Some(out)
}

/// Main Cosmic Harmony v3 Engine
pub struct CosmicHarmonyV3 {
    config: Arc<RwLock<Config>>,
    running: Arc<RwLock<bool>>,
    pool_manager: Arc<PoolManager>,
    multichain_engine: Arc<RwLock<Option<MultiChainEngine>>>,
}

impl CosmicHarmonyV3 {
    /// Create new engine instance
    pub async fn new(config: Config) -> anyhow::Result<Self> {
        info!("Initializing Cosmic Harmony v3 Engine");

        let pool_manager = Arc::new(PoolManager::new());

        // Best-effort pool wiring from config (don't fail engine init on network issues).
        for (pool_id, pool_cfg) in &config.pools {
            if pool_cfg.enabled {
                pool_manager.add_pool(pool_id, pool_cfg.clone()).await;
            }
        }
        if !config.pools.is_empty() {
            if let Err(e) = pool_manager.connect_all().await {
                warn!("Pool connect_all failed during init: {}", e);
            }
        }
        
        // Initialize multichain engine if enabled
        let multichain_engine = if config.multichain.enabled {
            info!("Initializing Multi-Chain GPU Mining Engine");
            let mc_config = MultiChainConfig {
                enabled_chains: config.multichain.enabled_chains.clone(),
                wallets: std::collections::HashMap::new(),
                pool_overrides: std::collections::HashMap::new(),
                allocations: std::collections::HashMap::new(),
                profit_switch_threshold: config.multichain.profit_switch_threshold,
                switch_cooldown_secs: config.multichain.switch_cooldown_secs,
            };
            Some(MultiChainEngine::new(mc_config))
        } else {
            None
        };
        
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            running: Arc::new(RwLock::new(false)),
            pool_manager,
            multichain_engine: Arc::new(RwLock::new(multichain_engine)),
        })
    }

    pub fn pool_manager(&self) -> Arc<PoolManager> {
        Arc::clone(&self.pool_manager)
    }

    pub async fn get_pool_job(&self, pool_id: &str) -> anyhow::Result<Option<MiningJob>> {
        self.pool_manager.get_job(pool_id).await
    }

    pub async fn get_job_for_algorithm(
        &self,
        algorithm: AlgorithmType,
    ) -> anyhow::Result<Option<(String, MiningJob)>> {
        let pool_id = {
            let cfg = self.config.read().await;
            cfg.pools
                .iter()
                .find(|(_, p)| p.enabled && p.algorithm == algorithm)
                .map(|(id, _)| id.clone())
        };

        let Some(pool_id) = pool_id else {
            return Ok(None);
        };

        let job = self.pool_manager.get_job(&pool_id).await?;
        Ok(job.map(|j| (pool_id, j)))
    }

    pub async fn submit_share(&self, pool_id: &str, share: Share) -> anyhow::Result<bool> {
        self.pool_manager.submit_share(pool_id, share).await
    }

    pub async fn mine_job(&self, job: &MiningJob, nonce: u64) -> anyhow::Result<MiningResult> {
        let input = build_job_input_bytes(job, nonce)?;
        self.mine(&input, nonce).await
    }

    pub async fn mine_and_submit_from_pool(
        &self,
        pool_id: &str,
        nonce: u64,
        difficulty: f64,
    ) -> anyhow::Result<(bool, MiningResult)> {
        let pool_algorithm = {
            let cfg = self.config.read().await;
            cfg.pools
                .get(pool_id)
                .ok_or_else(|| anyhow::anyhow!("Pool {} not configured", pool_id))?
                .algorithm
        };

        let job = self
            .pool_manager
            .get_job(pool_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("No job available for pool {}", pool_id))?;

        let result = self.mine_job(&job, nonce).await?;
        let hash_bytes = hash_bytes_for_algorithm(&result, pool_algorithm)
            .ok_or_else(|| anyhow::anyhow!("No hash available for pool algorithm {:?}", pool_algorithm))?;

        let share = Share {
            job_id: job.job_id.clone(),
            nonce: format!("{:08x}", nonce as u32),
            hash: hex::encode(hash_bytes),
            difficulty,
        };

        let accepted = self.pool_manager.submit_share(pool_id, share).await?;
        Ok((accepted, result))
    }
    
    /// Start the engine (including multichain if enabled)
    pub async fn start(&self) -> anyhow::Result<()> {
        info!("Starting Cosmic Harmony v3");
        
        // Start multichain engine if enabled
        if let Some(ref mc_engine) = *self.multichain_engine.read().await {
            info!("Starting Multi-Chain GPU Mining");
            
            // Connect to external pools
            let config = self.config.read().await;
            for (chain, pool_cfg) in &config.multichain.external_pools {
                if pool_cfg.enabled {
                    debug!("Connecting to {:?} pool: {}:{}", chain, pool_cfg.host, pool_cfg.port);
                    // Pool connection would happen here via mc_engine
                }
            }
            
            mc_engine.start().await?;
        }
        
        let mut running = self.running.write().await;
        *running = true;
        Ok(())
    }
    
    /// Stop the engine
    pub async fn stop(&self) -> anyhow::Result<()> {
        info!("Stopping Cosmic Harmony v3");
        
        // Stop multichain engine if running
        if let Some(ref mc_engine) = *self.multichain_engine.read().await {
            info!("Stopping Multi-Chain GPU Mining");
            mc_engine.stop().await;
        }
        
        let mut running = self.running.write().await;
        *running = false;
        Ok(())
    }
    
    /// Check if multichain mining is active
    pub async fn is_multichain_active(&self) -> bool {
        self.multichain_engine.read().await.is_some()
    }
    
    /// Get multichain stats
    pub async fn get_multichain_stats(&self) -> Option<crate::multichain::MultiChainStats> {
        if let Some(ref mc_engine) = *self.multichain_engine.read().await {
            Some(mc_engine.stats().await)
        } else {
            None
        }
    }
    
    /// Enable/disable a specific external chain
    pub async fn set_chain_enabled(&self, chain: ExternalChain, enabled: bool) -> anyhow::Result<()> {
        let mut config = self.config.write().await;
        
        if enabled {
            if !config.multichain.enabled_chains.contains(&chain) {
                config.multichain.enabled_chains.push(chain);
            }
        } else {
            config.multichain.enabled_chains.retain(|c| *c != chain);
        }
        
        info!("Chain {:?} enabled={}", chain, enabled);
        Ok(())
    }
    
    /// Mine a single hash with all modules
    pub async fn mine(&self, block_header: &[u8], nonce: u64) -> anyhow::Result<MiningResult> {
        let config = self.config.read().await.clone();

        // Step 1: Keccak-256 (deterministic, zero-alloc)
        let step1 = algorithms_opt::keccak256_opt(block_header);
        
        // Step 2: SHA3-512 (deterministic, zero-alloc)
        let step2 = algorithms_opt::sha3_512_opt(&step1.data);
        
        // Step 3: Golden Matrix — fixed-point for cross-platform determinism
        let step3_opt = algorithms_opt::golden_matrix_opt(&step2.data);
        
        // Step 4: Cosmic Fusion — deterministic XOR mask
        let step4_opt = algorithms_opt::cosmic_fusion_opt(&step3_opt.data);
        
        // Convert to fixed-size array
        let mut zion_hash = [0u8; 32];
        zion_hash.copy_from_slice(&step4_opt.data);
        
        // Collect exports
        let keccak_meets = config
            .pipeline
            .merged_mining_targets
            .get(&AlgorithmType::Keccak256)
            .and_then(|t| parse_target_hex_prefix(t).ok())
            .and_then(|target| first_32_bytes(&step1.data).map(|h| (h, target)))
            .map(|(hash, target)| hash_meets_target_be(&hash, &target))
            .unwrap_or(true);

        let sha3_meets = config
            .pipeline
            .merged_mining_targets
            .get(&AlgorithmType::Sha3_512)
            .and_then(|t| parse_target_hex_prefix(t).ok())
            .and_then(|target| first_32_bytes(&step2.data).map(|h| (h, target)))
            .map(|(hash, target)| hash_meets_target_be(&hash, &target))
            .unwrap_or(true);

        let exports = vec![
            ExportHash {
                algorithm: AlgorithmType::Keccak256,
                hash: step1.data.to_vec(),
                target_coin: "ETC".to_string(),
                meets_difficulty: keccak_meets,
            },
            ExportHash {
                algorithm: AlgorithmType::Sha3_512,
                hash: step2.data.to_vec(),
                target_coin: "NXS".to_string(),
                meets_difficulty: sha3_meets,
            },
        ];
        
        Ok(MiningResult {
            zion_hash,
            nonce,
            exports,
            revenue: RevenueBreakdown::default(),
        })
    }
    
    /// Get current config
    pub async fn get_config(&self) -> Config {
        self.config.read().await.clone()
    }
    
    /// Check if engine is running
    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoolConfig;
    use tokio::net::TcpListener;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    
    #[tokio::test]
    async fn test_engine_creation() {
        let config = Config::default();
        let engine = CosmicHarmonyV3::new(config).await.unwrap();
        assert!(!engine.is_running().await);
    }

    #[tokio::test]
    async fn test_engine_pool_wiring_can_fetch_job() {
        // Local mock pool server (xmrig-style)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            loop {
                line.clear();
                let bytes = reader.read_line(&mut line).await.unwrap();
                if bytes == 0 {
                    break;
                }

                let v: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let id = v.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let method = v.get("method").and_then(|v| v.as_str()).unwrap_or("");

                if method == "login" {
                    let resp = serde_json::json!({
                        "id": id,
                        "result": {
                            "id": "session-1",
                            "job": {
                                "job_id": "job-1",
                                "blob": "00",
                                "target": "ff",
                                "height": 1,
                                "seed_hash": null
                            }
                        },
                        "error": null
                    });
                    let _ = write_half.write_all(serde_json::to_string(&resp).unwrap().as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                    let _ = write_half.flush().await;
                } else if method == "getjob" {
                    let resp = serde_json::json!({
                        "id": id,
                        "result": {
                            "job_id": "job-1",
                            "blob": "00",
                            "target": "ff",
                            "height": 1,
                            "seed_hash": null
                        },
                        "error": null
                    });
                    let _ = write_half.write_all(serde_json::to_string(&resp).unwrap().as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                    let _ = write_half.flush().await;
                }
            }
        });

        let mut config = Config::default();
        config.pools.insert(
            "zion-main".to_string(),
            PoolConfig {
                url: format!("stratum+tcp://{}", addr),
                wallet: "ZION_WALLET".to_string(),
                worker: "test".to_string(),
                password: "x".to_string(),
                algorithm: AlgorithmType::CosmicFusion,
                enabled: true,
            },
        );

        let engine = CosmicHarmonyV3::new(config).await.unwrap();
        let job = engine.get_job_for_algorithm(AlgorithmType::CosmicFusion).await.unwrap();
        assert!(job.is_some());
    }

    #[tokio::test]
    async fn test_engine_e2e_mine_and_submit() {
        // Local mock pool server (xmrig-style).
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let (seen_submit_tx, seen_submit_rx) = tokio::sync::oneshot::channel::<serde_json::Value>();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            let mut seen_submit_tx = Some(seen_submit_tx);

            loop {
                line.clear();
                let bytes = reader.read_line(&mut line).await.unwrap();
                if bytes == 0 {
                    break;
                }

                let v: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let id = v.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let method = v.get("method").and_then(|v| v.as_str()).unwrap_or("");

                if method == "login" {
                    let resp = serde_json::json!({
                        "id": id,
                        "result": {
                            "id": "session-1",
                            "job": {
                                "job_id": "job-1",
                                "blob": "00",
                                "target": "ff",
                                "height": 1,
                                "seed_hash": null
                            }
                        },
                        "error": null
                    });
                    let _ = write_half.write_all(serde_json::to_string(&resp).unwrap().as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                    let _ = write_half.flush().await;
                } else if method == "submit" {
                    if let Some(tx) = seen_submit_tx.take() {
                        let _ = tx.send(v.clone());
                    }
                    let resp = serde_json::json!({
                        "id": id,
                        "result": true,
                        "error": null
                    });
                    let _ = write_half.write_all(serde_json::to_string(&resp).unwrap().as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                    let _ = write_half.flush().await;
                }
            }
        });

        let mut config = Config::default();
        config.pools.insert(
            "zion-main".to_string(),
            PoolConfig {
                url: format!("stratum+tcp://{}", addr),
                wallet: "ZION_WALLET".to_string(),
                worker: "test".to_string(),
                password: "x".to_string(),
                // For E2E, submit the final ZION hash.
                algorithm: AlgorithmType::CosmicFusion,
                enabled: true,
            },
        );

        let engine = CosmicHarmonyV3::new(config).await.unwrap();
        let (accepted, result) = engine
            .mine_and_submit_from_pool("zion-main", 42, 1.0)
            .await
            .unwrap();
        assert!(accepted);
        assert_eq!(result.nonce, 42);

        let submit_msg = tokio::time::timeout(std::time::Duration::from_secs(2), seen_submit_rx)
            .await
            .unwrap()
            .unwrap();
        let params = submit_msg.get("params").unwrap();
        assert_eq!(submit_msg.get("method").and_then(|v| v.as_str()), Some("submit"));
        assert_eq!(params.get("job_id").and_then(|v| v.as_str()), Some("job-1"));
        assert_eq!(params.get("nonce").and_then(|v| v.as_str()), Some("0000002a"));
        assert!(params.get("result").and_then(|v| v.as_str()).unwrap_or("").len() > 0);
    }
    
    #[tokio::test]
    async fn test_mining() {
        let config = Config::default();
        let engine = CosmicHarmonyV3::new(config).await.unwrap();
        
        let result = engine.mine(b"test block header", 12345).await.unwrap();
        assert_eq!(result.zion_hash.len(), 32);
        assert_eq!(result.nonce, 12345);
        assert_eq!(result.exports.len(), 2);
    }

    #[tokio::test]
    async fn test_export_difficulty_check_hard_target() {
        let mut config = Config::default();
        config
            .pipeline
            .merged_mining_targets
            .insert(AlgorithmType::Keccak256, "00".repeat(32));
        config
            .pipeline
            .merged_mining_targets
            .insert(AlgorithmType::Sha3_512, "00".repeat(32));

        let engine = CosmicHarmonyV3::new(config).await.unwrap();
        let result = engine.mine(b"test block header", 12345).await.unwrap();

        let keccak = result
            .exports
            .iter()
            .find(|e| e.algorithm == AlgorithmType::Keccak256)
            .unwrap();
        assert!(!keccak.meets_difficulty);

        let sha3 = result
            .exports
            .iter()
            .find(|e| e.algorithm == AlgorithmType::Sha3_512)
            .unwrap();
        assert!(!sha3.meets_difficulty);
    }

    #[tokio::test]
    async fn test_export_difficulty_check_easy_target() {
        let mut config = Config::default();
        config
            .pipeline
            .merged_mining_targets
            .insert(AlgorithmType::Keccak256, "ff".repeat(32));
        config
            .pipeline
            .merged_mining_targets
            .insert(AlgorithmType::Sha3_512, "ff".repeat(32));

        let engine = CosmicHarmonyV3::new(config).await.unwrap();
        let result = engine.mine(b"test block header", 12345).await.unwrap();

        let keccak = result
            .exports
            .iter()
            .find(|e| e.algorithm == AlgorithmType::Keccak256)
            .unwrap();
        assert!(keccak.meets_difficulty);

        let sha3 = result
            .exports
            .iter()
            .find(|e| e.algorithm == AlgorithmType::Sha3_512)
            .unwrap();
        assert!(sha3.meets_difficulty);
    }
}
