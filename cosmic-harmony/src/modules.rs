//! Module pipeline executor

use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::{AlgorithmType, Config, algorithms};

/// Result of pipeline execution
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Final ZION hash
    pub final_hash: [u8; 32],
    
    /// Intermediate hashes for export
    pub intermediate_hashes: HashMap<AlgorithmType, Vec<u8>>,
}

/// Module pipeline executor
pub struct ModulePipeline {
    config: Arc<RwLock<Config>>,
}

impl ModulePipeline {
    pub fn new(config: Arc<RwLock<Config>>) -> Self {
        Self { config }
    }
    
    /// Execute full pipeline
    pub async fn execute(&self, input: &[u8], _nonce: u64) -> anyhow::Result<PipelineResult> {
        let config = self.config.read().await;
        let mut intermediate_hashes = HashMap::new();
        
        // Step 1: Keccak-256
        let step1 = algorithms::keccak256(input)?;
        
        // Check if export is enabled for Keccak
        for slot in &config.pipeline.slots {
            if slot.algorithm == AlgorithmType::Keccak256 && slot.export_enabled {
                intermediate_hashes.insert(AlgorithmType::Keccak256, step1.hash.clone());
            }
        }
        
        // Step 2: SHA3-512
        let step2 = algorithms::sha3_512(&step1.hash)?;
        
        for slot in &config.pipeline.slots {
            if slot.algorithm == AlgorithmType::Sha3_512 && slot.export_enabled {
                intermediate_hashes.insert(AlgorithmType::Sha3_512, step2.hash.clone());
            }
        }
        
        // Step 3: Golden Matrix
        let step3 = algorithms::golden_matrix(&step2.hash)?;
        
        // Step 4: Cosmic Fusion
        let step4 = algorithms::cosmic_fusion(&step3.hash)?;
        
        // Convert to fixed size
        let mut final_hash = [0u8; 32];
        final_hash.copy_from_slice(&step4.hash[..32]);
        
        Ok(PipelineResult {
            final_hash,
            intermediate_hashes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_pipeline() {
        let config = Arc::new(RwLock::new(Config::default()));
        let pipeline = ModulePipeline::new(config);
        
        let result = pipeline.execute(b"test input", 0).await.unwrap();
        assert_eq!(result.final_hash.len(), 32);
    }
}
