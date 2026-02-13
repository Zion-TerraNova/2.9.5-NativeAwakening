//! Blake3 Worker - GPU mining for Alephium (ALPH)
//!
//! Implements Blake3 algorithm with Alephium-specific modifications.

use super::{AlgorithmWorker, FoundShare};
use crate::multichain::{ExternalChain, MiningJob, ChainStats};
use anyhow::Result;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Blake3 GPU worker
pub struct Blake3Worker {
    running: AtomicBool,
    hashrate: AtomicU64,
    shares_found: AtomicU64,
    shares_accepted: AtomicU64,
    shares_rejected: AtomicU64,
}

impl Blake3Worker {
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            hashrate: AtomicU64::new(0),
            shares_found: AtomicU64::new(0),
            shares_accepted: AtomicU64::new(0),
            shares_rejected: AtomicU64::new(0),
        }
    }

    /// Blake3 hash with Alephium parameters
    fn alephium_hash(&self, header: &[u8], nonce: u64) -> [u8; 32] {
        // Alephium uses Blake3 with specific serialization
        let mut input = Vec::with_capacity(header.len() + 8);
        input.extend_from_slice(header);
        input.extend_from_slice(&nonce.to_be_bytes()); // Alephium uses big-endian
        
        blake3::hash(&input).into()
    }

    /// Check if hash meets target (Alephium uses leading zeros count)
    fn meets_target(&self, hash: &[u8; 32], target: &[u8]) -> bool {
        // Alephium target is expressed as leading zeros + difficulty
        if target.len() < 32 {
            return false;
        }
        
        // Compare big-endian (Alephium convention)
        for i in 0..32 {
            if hash[i] < target[i] {
                return true;
            }
            if hash[i] > target[i] {
                return false;
            }
        }
        true
    }

    /// Calculate leading zeros for difficulty display
    fn leading_zeros(&self, hash: &[u8; 32]) -> u32 {
        let mut zeros = 0;
        for byte in hash {
            if *byte == 0 {
                zeros += 8;
            } else {
                zeros += byte.leading_zeros();
                break;
            }
        }
        zeros
    }
}

impl Default for Blake3Worker {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl AlgorithmWorker for Blake3Worker {
    fn chain(&self) -> ExternalChain {
        ExternalChain::ALPH
    }

    fn algorithm(&self) -> &'static str {
        "blake3"
    }

    async fn init(&mut self) -> Result<()> {
        log::info!("ch3_blake3_worker_init");
        
        // Blake3 is simple - no DAG, no lookup tables
        // Just need to set up GPU kernels
        
        Ok(())
    }

    async fn mine(&self, job: &MiningJob, allocation: f32) -> Result<()> {
        if allocation <= 0.0 {
            return Ok(());
        }

        self.running.store(true, Ordering::Relaxed);

        log::debug!(
            "ch3_blake3_mining job_id={} allocation={:.1}%",
            job.job_id, allocation
        );

        // GPU mining loop:
        // 1. Serialize header + nonce
        // 2. Blake3 hash
        // 3. Compare to target
        
        // Blake3 is very fast - can achieve high hashrates
        // GPU parallelism is key
        
        Ok(())
    }

    async fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        log::info!("ch3_blake3_worker_stopped");
    }

    fn hashrate(&self) -> f64 {
        self.hashrate.load(Ordering::Relaxed) as f64 / 1_000_000_000.0 // GH/s
    }

    fn stats(&self) -> ChainStats {
        ChainStats {
            chain: Some(ExternalChain::ALPH),
            hashrate: self.hashrate(),
            shares_accepted: self.shares_accepted.load(Ordering::Relaxed),
            shares_rejected: self.shares_rejected.load(Ordering::Relaxed),
            last_share_time: None,
        }
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}
