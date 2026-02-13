//! Algorithm Module Library - CH v3 Core Component
//!
//! Provides a unified registry of all 12+ mining algorithms that can be
//! dynamically loaded, switched, and routed based on profitability.
//!
//! Architecture:
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │           ALGORITHM MODULE LIBRARY                       │
//! │                                                          │
//! │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐            │
//! │  │Keccak  │ │ SHA3   │ │RandomX │ │Autolykos│            │
//! │  │  256   │ │  512   │ │  (CPU) │ │  v2    │            │
//! │  └────────┘ └────────┘ └────────┘ └────────┘            │
//! │                                                          │
//! │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐            │
//! │  │KawPow  │ │Equihash│ │ Blake3 │ │KHeavyH │            │
//! │  │  (GPU) │ │ 144,5  │ │  ALPH  │ │  KAS   │            │
//! │  └────────┘ └────────┘ └────────┘ └────────┘            │
//! │                                                          │
//! │  ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐            │
//! │  │ProgPow │ │ Ethash │ │Yescrypt│ │ Argon2 │            │
//! │  │        │ │  ETC   │ │  (CPU) │ │   d    │            │
//! │  └────────┘ └────────┘ └────────┘ └────────┘            │
//! └─────────────────────────────────────────────────────────┘
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

use crate::{AlgorithmType, algorithms};
use crate::algorithms::HashOutput;

/// Algorithm metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlgorithmInfo {
    /// Algorithm type
    pub algo_type: AlgorithmType,
    
    /// Human-readable name
    pub name: String,
    
    /// Target coins for this algorithm
    pub target_coins: Vec<TargetCoin>,
    
    /// Hardware type (CPU/GPU/ASIC)
    pub hardware: HardwareType,
    
    /// Is this a native CH module (always active)?
    pub is_native: bool,
    
    /// Memory requirement in MB
    pub memory_mb: u32,
    
    /// Typical hashrate for reference hardware
    pub reference_hashrate: String,
}

/// Target coin for an algorithm
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetCoin {
    /// Coin ticker (e.g., "ERG", "RVN")
    pub ticker: String,
    
    /// Full name
    pub name: String,
    
    /// Pool stratum address
    pub pool_stratum: Option<String>,
    
    /// WhatToMine coin ID
    pub whattomine_id: Option<u32>,
}

/// Hardware requirement type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HardwareType {
    /// CPU-optimized (RandomX, Yescrypt, Argon2)
    Cpu,
    
    /// GPU-optimized (Ethash, KawPow, Autolykos2)
    Gpu,
    
    /// ASIC-resistant (mixed)
    AsicResistant,
    
    /// Native hash (Keccak, SHA3)
    Native,
}

/// Algorithm execution result
#[derive(Debug, Clone)]
pub struct AlgorithmResult {
    /// Algorithm that produced this
    pub algorithm: AlgorithmType,
    
    /// Hash output
    pub hash: Vec<u8>,
    
    /// Execution time in microseconds
    pub execution_time_us: u64,
    
    /// Was this exported for merged mining?
    pub exported: bool,
    
    /// Target coin if exported
    pub export_target: Option<String>,
}

/// The Algorithm Module Library
pub struct AlgorithmModuleLibrary {
    /// All registered algorithms
    algorithms: HashMap<AlgorithmType, AlgorithmInfo>,
    
    /// Currently active algorithms in pipeline
    active_pipeline: Arc<RwLock<Vec<AlgorithmType>>>,
    
    /// Export routing table
    export_routes: Arc<RwLock<HashMap<AlgorithmType, String>>>,
}

impl AlgorithmModuleLibrary {
    /// Create new library with all 12 algorithms
    pub fn new() -> Self {
        let mut algorithms = HashMap::new();
        
        // ========================
        // NATIVE MODULES (Always Active)
        // ========================
        
        algorithms.insert(AlgorithmType::Keccak256, AlgorithmInfo {
            algo_type: AlgorithmType::Keccak256,
            name: "Keccak-256".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "ETC".to_string(),
                    name: "Ethereum Classic".to_string(),
                    pool_stratum: Some("etc.2miners.com:1010".to_string()),
                    whattomine_id: Some(162),
                },
                TargetCoin {
                    ticker: "NICEHASH".to_string(),
                    name: "NiceHash Keccak".to_string(),
                    pool_stratum: Some("keccak.eu.nicehash.com:3338".to_string()),
                    whattomine_id: None,
                },
            ],
            hardware: HardwareType::Native,
            is_native: true,
            memory_mb: 64,
            reference_hashrate: "~500 MH/s (GPU)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::Sha3_512, AlgorithmInfo {
            algo_type: AlgorithmType::Sha3_512,
            name: "SHA3-512".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "NXS".to_string(),
                    name: "Nexus".to_string(),
                    pool_stratum: Some("pool.nexus.io:3333".to_string()),
                    whattomine_id: None,
                },
                TargetCoin {
                    ticker: "0xBTC".to_string(),
                    name: "0xBitcoin".to_string(),
                    pool_stratum: Some("mike.rs:8080".to_string()),
                    whattomine_id: None,
                },
            ],
            hardware: HardwareType::Native,
            is_native: true,
            memory_mb: 64,
            reference_hashrate: "~400 MH/s (GPU)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::GoldenMatrix, AlgorithmInfo {
            algo_type: AlgorithmType::GoldenMatrix,
            name: "Golden Matrix (φ)".to_string(),
            target_coins: vec![],  // ZION-specific, no export
            hardware: HardwareType::Native,
            is_native: true,
            memory_mb: 128,
            reference_hashrate: "N/A (transform)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::CosmicFusion, AlgorithmInfo {
            algo_type: AlgorithmType::CosmicFusion,
            name: "Cosmic Fusion".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "ZION".to_string(),
                    name: "ZION TerraNova".to_string(),
                    pool_stratum: Some("pool.zionterranova.com:3333".to_string()),
                    whattomine_id: None,
                },
            ],
            hardware: HardwareType::Native,
            is_native: true,
            memory_mb: 128,
            reference_hashrate: "~10 MH/s (combined)".to_string(),
        });
        
        // ========================
        // GPU ALGORITHMS (Switchable)
        // ========================
        
        algorithms.insert(AlgorithmType::Autolykos2, AlgorithmInfo {
            algo_type: AlgorithmType::Autolykos2,
            name: "Autolykos2".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "ERG".to_string(),
                    name: "Ergo".to_string(),
                    pool_stratum: Some("erg.2miners.com:8888".to_string()),
                    whattomine_id: Some(340),
                },
            ],
            hardware: HardwareType::Gpu,
            is_native: false,
            memory_mb: 4096,
            reference_hashrate: "~300 MH/s (RTX 4090)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::KawPow, AlgorithmInfo {
            algo_type: AlgorithmType::KawPow,
            name: "KawPow".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "RVN".to_string(),
                    name: "Ravencoin".to_string(),
                    pool_stratum: Some("rvn.2miners.com:6060".to_string()),
                    whattomine_id: Some(234),
                },
                TargetCoin {
                    ticker: "CLORE".to_string(),
                    name: "Clore.ai".to_string(),
                    pool_stratum: Some("clore.2miners.com:3030".to_string()),
                    whattomine_id: None,
                },
            ],
            hardware: HardwareType::Gpu,
            is_native: false,
            memory_mb: 4096,
            reference_hashrate: "~60 MH/s (RTX 4090)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::KHeavyHash, AlgorithmInfo {
            algo_type: AlgorithmType::KHeavyHash,
            name: "kHeavyHash".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "KAS".to_string(),
                    name: "Kaspa".to_string(),
                    pool_stratum: Some("kas.2miners.com:2020".to_string()),
                    whattomine_id: Some(352),
                },
            ],
            hardware: HardwareType::Gpu,
            is_native: false,
            memory_mb: 2048,
            reference_hashrate: "~1.5 GH/s (RTX 4090)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::Blake3, AlgorithmInfo {
            algo_type: AlgorithmType::Blake3,
            name: "Blake3".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "ALPH".to_string(),
                    name: "Alephium".to_string(),
                    pool_stratum: Some("alph.2miners.com:2020".to_string()),
                    whattomine_id: Some(347),
                },
            ],
            hardware: HardwareType::Gpu,
            is_native: false,
            memory_mb: 2048,
            reference_hashrate: "~5 GH/s (RTX 4090)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::Ethash, AlgorithmInfo {
            algo_type: AlgorithmType::Ethash,
            name: "Ethash (Etchash)".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "ETC".to_string(),
                    name: "Ethereum Classic".to_string(),
                    pool_stratum: Some("etc.2miners.com:1010".to_string()),
                    whattomine_id: Some(162),
                },
            ],
            hardware: HardwareType::Gpu,
            is_native: false,
            memory_mb: 4096,
            reference_hashrate: "~130 MH/s (RTX 4090)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::Equihash, AlgorithmInfo {
            algo_type: AlgorithmType::Equihash,
            name: "Equihash 144,5".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "ZEC".to_string(),
                    name: "Zcash".to_string(),
                    pool_stratum: Some("zec.2miners.com:1010".to_string()),
                    whattomine_id: Some(166),
                },
                TargetCoin {
                    ticker: "ZEN".to_string(),
                    name: "Horizen".to_string(),
                    pool_stratum: Some("zen.2miners.com:3030".to_string()),
                    whattomine_id: Some(185),
                },
            ],
            hardware: HardwareType::Gpu,
            is_native: false,
            memory_mb: 2048,
            reference_hashrate: "~100 Sol/s (RTX 4090)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::ProgPow, AlgorithmInfo {
            algo_type: AlgorithmType::ProgPow,
            name: "ProgPow".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "VEIL".to_string(),
                    name: "Veil".to_string(),
                    pool_stratum: None,
                    whattomine_id: None,
                },
            ],
            hardware: HardwareType::Gpu,
            is_native: false,
            memory_mb: 4096,
            reference_hashrate: "~50 MH/s (RTX 4090)".to_string(),
        });
        
        // ========================
        // CPU ALGORITHMS
        // ========================
        
        algorithms.insert(AlgorithmType::RandomX, AlgorithmInfo {
            algo_type: AlgorithmType::RandomX,
            name: "RandomX".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "XMR".to_string(),
                    name: "Monero".to_string(),
                    pool_stratum: Some("xmr.2miners.com:2222".to_string()),
                    whattomine_id: Some(101),
                },
            ],
            hardware: HardwareType::Cpu,
            is_native: false,
            memory_mb: 2048,
            reference_hashrate: "~15 kH/s (Ryzen 9)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::Yescrypt, AlgorithmInfo {
            algo_type: AlgorithmType::Yescrypt,
            name: "Yescrypt".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "YTN".to_string(),
                    name: "Yenten".to_string(),
                    pool_stratum: None,
                    whattomine_id: None,
                },
            ],
            hardware: HardwareType::Cpu,
            is_native: false,
            memory_mb: 512,
            reference_hashrate: "~5 kH/s (Ryzen 9)".to_string(),
        });
        
        algorithms.insert(AlgorithmType::Argon2d, AlgorithmInfo {
            algo_type: AlgorithmType::Argon2d,
            name: "Argon2d".to_string(),
            target_coins: vec![
                TargetCoin {
                    ticker: "DYN".to_string(),
                    name: "Dynamic".to_string(),
                    pool_stratum: None,
                    whattomine_id: None,
                },
            ],
            hardware: HardwareType::Cpu,
            is_native: false,
            memory_mb: 1024,
            reference_hashrate: "~10 kH/s (Ryzen 9)".to_string(),
        });
        
        // Default pipeline: Keccak → SHA3 → Golden → Autolykos → Fusion
        let default_pipeline = vec![
            AlgorithmType::Keccak256,
            AlgorithmType::Sha3_512,
            AlgorithmType::GoldenMatrix,
            AlgorithmType::Autolykos2,
            AlgorithmType::CosmicFusion,
        ];
        
        // Default export routes
        let mut export_routes = HashMap::new();
        export_routes.insert(AlgorithmType::Keccak256, "ETC".to_string());
        export_routes.insert(AlgorithmType::Sha3_512, "NXS".to_string());
        export_routes.insert(AlgorithmType::Autolykos2, "ERG".to_string());
        
        Self {
            algorithms,
            active_pipeline: Arc::new(RwLock::new(default_pipeline)),
            export_routes: Arc::new(RwLock::new(export_routes)),
        }
    }
    
    /// Get algorithm info
    pub fn get_algorithm(&self, algo: &AlgorithmType) -> Option<&AlgorithmInfo> {
        self.algorithms.get(algo)
    }
    
    /// List all available algorithms
    pub fn list_all(&self) -> Vec<&AlgorithmInfo> {
        self.algorithms.values().collect()
    }
    
    /// List GPU algorithms only
    pub fn list_gpu_algorithms(&self) -> Vec<&AlgorithmInfo> {
        self.algorithms.values()
            .filter(|a| a.hardware == HardwareType::Gpu)
            .collect()
    }
    
    /// List CPU algorithms only
    pub fn list_cpu_algorithms(&self) -> Vec<&AlgorithmInfo> {
        self.algorithms.values()
            .filter(|a| a.hardware == HardwareType::Cpu)
            .collect()
    }
    
    /// List native algorithms (always active)
    pub fn list_native_algorithms(&self) -> Vec<&AlgorithmInfo> {
        self.algorithms.values()
            .filter(|a| a.is_native)
            .collect()
    }
    
    /// Get current active pipeline
    pub async fn get_active_pipeline(&self) -> Vec<AlgorithmType> {
        self.active_pipeline.read().await.clone()
    }
    
    /// Update active pipeline
    pub async fn set_pipeline(&self, pipeline: Vec<AlgorithmType>) -> anyhow::Result<()> {
        // Validate pipeline
        // 1. Must include all native modules
        let natives = [
            AlgorithmType::Keccak256,
            AlgorithmType::Sha3_512,
            AlgorithmType::GoldenMatrix,
            AlgorithmType::CosmicFusion,
        ];
        
        for native in &natives {
            if !pipeline.contains(native) {
                anyhow::bail!("Pipeline must include native module: {:?}", native);
            }
        }
        
        // 2. CosmicFusion must be last
        if pipeline.last() != Some(&AlgorithmType::CosmicFusion) {
            anyhow::bail!("CosmicFusion must be the last module in pipeline");
        }
        
        let mut p = self.active_pipeline.write().await;
        *p = pipeline;
        Ok(())
    }
    
    /// Set export route for algorithm
    pub async fn set_export_route(&self, algo: AlgorithmType, target: String) {
        let mut routes = self.export_routes.write().await;
        routes.insert(algo, target);
    }
    
    /// Get export route for algorithm
    pub async fn get_export_route(&self, algo: &AlgorithmType) -> Option<String> {
        self.export_routes.read().await.get(algo).cloned()
    }
    
    /// Execute a single algorithm
    pub fn execute_algorithm(&self, algo: &AlgorithmType, input: &[u8]) -> anyhow::Result<HashOutput> {
        match algo {
            // Native
            AlgorithmType::Keccak256 => algorithms::keccak256(input),
            AlgorithmType::Sha3_512 => algorithms::sha3_512(input),
            AlgorithmType::GoldenMatrix => algorithms::golden_matrix(input),
            AlgorithmType::CosmicFusion => algorithms::cosmic_fusion(input),
            
            // GPU
            AlgorithmType::Autolykos2 => algorithms::autolykos2(input),
            // KawPow returns (hash, mix) - we only need hash for simple execution
            AlgorithmType::KawPow => algorithms::kawpow_simple(input),
            AlgorithmType::KHeavyHash => algorithms::kheavyhash(input),
            AlgorithmType::Blake3 => algorithms::blake3_hash(input),
            AlgorithmType::Ethash => algorithms::ethash(input),
            AlgorithmType::Equihash => algorithms::equihash(input),
            AlgorithmType::ProgPow => algorithms::progpow(input),
            
            // CPU
            AlgorithmType::RandomX => algorithms::randomx(input),
            AlgorithmType::Yescrypt => algorithms::yescrypt(input),
            AlgorithmType::Argon2d => algorithms::argon2d(input),
        }
    }
    
    /// Execute full pipeline with exports
    pub async fn execute_pipeline(&self, input: &[u8]) -> anyhow::Result<PipelineExecutionResult> {
        let pipeline = self.active_pipeline.read().await.clone();
        let routes = self.export_routes.read().await.clone();
        
        let mut current_input = input.to_vec();
        let mut results = Vec::new();
        let mut final_hash = [0u8; 32];
        
        for algo in &pipeline {
            let start = std::time::Instant::now();
            let output = self.execute_algorithm(algo, &current_input)?;
            let elapsed = start.elapsed().as_micros() as u64;
            
            let export_target = routes.get(algo).cloned();
            let exported = export_target.is_some();
            
            results.push(AlgorithmResult {
                algorithm: *algo,
                hash: output.hash.clone(),
                execution_time_us: elapsed,
                exported,
                export_target,
            });
            
            // Feed output to next stage
            current_input = output.hash;
        }
        
        // Calculate total time before moving results
        let total_time: u64 = results.iter().map(|r| r.execution_time_us).sum();
        
        // Last result is the final hash
        if let Some(last) = results.last() {
            let len = last.hash.len().min(32);
            final_hash[..len].copy_from_slice(&last.hash[..len]);
        }
        
        Ok(PipelineExecutionResult {
            final_hash,
            algorithm_results: results,
            total_execution_time_us: total_time,
        })
    }
    
    /// Switch dynamic slot to new algorithm
    pub async fn switch_dynamic_slot(&self, new_algo: AlgorithmType) -> anyhow::Result<()> {
        let algo_info = self.get_algorithm(&new_algo)
            .ok_or_else(|| anyhow::anyhow!("Algorithm not found: {:?}", new_algo))?;
        
        // Only non-native algorithms can be switched
        if algo_info.is_native {
            anyhow::bail!("Cannot switch to native algorithm: {:?}", new_algo);
        }
        
        let mut pipeline = self.active_pipeline.write().await;
        
        // Find and replace the dynamic slot (position 3, before Golden Matrix)
        for i in 0..pipeline.len() {
            let algo = &pipeline[i];
            if let Some(info) = self.algorithms.get(algo) {
                if !info.is_native {
                    pipeline[i] = new_algo;
                    break;
                }
            }
        }
        
        Ok(())
    }
}

impl Default for AlgorithmModuleLibrary {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of full pipeline execution
#[derive(Debug, Clone)]
pub struct PipelineExecutionResult {
    /// Final ZION hash (32 bytes)
    pub final_hash: [u8; 32],
    
    /// Results from each algorithm in pipeline
    pub algorithm_results: Vec<AlgorithmResult>,
    
    /// Total execution time
    pub total_execution_time_us: u64,
}

impl PipelineExecutionResult {
    /// Get hashes to export for merged mining
    pub fn get_export_hashes(&self) -> Vec<(&AlgorithmResult, &str)> {
        self.algorithm_results.iter()
            .filter_map(|r| {
                r.export_target.as_ref().map(|t| (r, t.as_str()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_library_creation() {
        let lib = AlgorithmModuleLibrary::new();
        
        // Should have all 12 algorithms
        assert!(lib.list_all().len() >= 12);
        
        // Check native algorithms
        let natives = lib.list_native_algorithms();
        assert_eq!(natives.len(), 4);  // Keccak, SHA3, Golden, Fusion
        
        // Check GPU algorithms
        let gpu = lib.list_gpu_algorithms();
        assert!(gpu.len() >= 6);  // Autolykos, KawPow, KHeavy, Blake3, Ethash, Equihash
        
        // Check CPU algorithms
        let cpu = lib.list_cpu_algorithms();
        assert!(cpu.len() >= 3);  // RandomX, Yescrypt, Argon2d
    }
    
    #[tokio::test]
    async fn test_pipeline_execution() {
        let lib = AlgorithmModuleLibrary::new();
        
        let result = lib.execute_pipeline(b"test input").await.unwrap();
        
        assert_eq!(result.final_hash.len(), 32);
        assert!(!result.algorithm_results.is_empty());
        
        // Check exports
        let exports = result.get_export_hashes();
        assert!(!exports.is_empty());
    }
    
    #[tokio::test]
    async fn test_dynamic_switch() {
        let lib = AlgorithmModuleLibrary::new();
        
        // Switch from Autolykos2 to KawPow
        lib.switch_dynamic_slot(AlgorithmType::KawPow).await.unwrap();
        
        let pipeline = lib.get_active_pipeline().await;
        assert!(pipeline.contains(&AlgorithmType::KawPow));
    }
    
    #[tokio::test]
    async fn test_invalid_pipeline() {
        let lib = AlgorithmModuleLibrary::new();
        
        // Missing CosmicFusion should fail
        let invalid = vec![
            AlgorithmType::Keccak256,
            AlgorithmType::Sha3_512,
        ];
        
        assert!(lib.set_pipeline(invalid).await.is_err());
    }
}
