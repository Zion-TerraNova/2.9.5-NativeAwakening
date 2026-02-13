//! Stream-Aware Mining — Integration with Pool StreamScheduler v2
//!
//! When the pool runs StreamScheduler v2 (hybrid per-miner / time-split),
//! each miner is assigned to either a ZION group or Revenue group.
//! The pool pushes jobs with different `algo` fields depending on group.
//!
//! This module handles:
//! - Detecting algorithm changes in incoming jobs
//! - Dynamically switching the active mining algorithm
//! - Managing CH v3 as the primary ZION hashing engine
//! - Supporting external coin algorithms (ethash, autolykos, randomx, etc.)
//!
//! ## Protocol
//!
//! Pool → Miner flow:
//! 1. Miner connects, sends login with algo=cosmic_harmony_v3
//! 2. Pool assigns miner to ZION or Revenue group
//! 3. Pool sends job with `algo` field matching the assigned stream
//!    - ZION group: algo="cosmic_harmony_v3"  
//!    - Revenue group: algo="autolykos" / "ethash" / "randomx" etc.
//! 4. If ProfitSwitcher changes coin, Revenue miners get new job with new algo
//! 5. Miner detects algo change and switches hashing engine

use anyhow::Result;
use log::info;
use std::sync::{RwLock, atomic::{AtomicBool, Ordering}};

use super::Algorithm;
use super::native_algos;
use crate::stratum::Job;

/// Stream assignment from pool (mirrors pool's MinerGroup)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamGroup {
    /// Mining ZION (Cosmic Harmony v3)
    Zion,
    /// Mining external coin (determined by pool ProfitSwitcher)
    Revenue,
    /// Unknown / not assigned yet
    Unknown,
}

/// Tracks the current mining stream and handles algorithm transitions
pub struct StreamState {
    /// Current algorithm being mined
    current_algo: RwLock<Algorithm>,
    
    /// Current stream group assignment
    current_group: RwLock<StreamGroup>,
    
    /// Whether an algorithm switch is pending (signals mining threads to reload)
    algo_switch_pending: AtomicBool,
    
    /// The default/requested algorithm (what miner was started with)
    default_algo: Algorithm,
    
    /// Current external coin name (e.g., "ERG", "ETC", "XMR")
    current_coin: RwLock<String>,
    
    /// Number of algorithm switches performed
    switch_count: RwLock<u64>,
    
    /// CPU-only mode: GPU algos are automatically replaced with RandomX
    cpu_only_mode: bool,
}

impl StreamState {
    pub fn new(default_algo: Algorithm) -> Self {
        Self {
            current_algo: RwLock::new(default_algo),
            current_group: RwLock::new(StreamGroup::Unknown),
            algo_switch_pending: AtomicBool::new(false),
            default_algo,
            current_coin: RwLock::new("ZION".to_string()),
            switch_count: RwLock::new(0),
            cpu_only_mode: false,
        }
    }
    
    /// Create StreamState with CPU-only mode enabled.
    /// In this mode, GPU-only algorithms (ethash, kawpow, autolykos) are
    /// automatically replaced with RandomX (XMR) for CPU mining.
    pub fn new_cpu_only(default_algo: Algorithm) -> Self {
        Self {
            current_algo: RwLock::new(default_algo),
            current_group: RwLock::new(StreamGroup::Unknown),
            algo_switch_pending: AtomicBool::new(false),
            default_algo,
            current_coin: RwLock::new("ZION".to_string()),
            switch_count: RwLock::new(0),
            cpu_only_mode: true,
        }
    }
    
    /// Check if an algorithm requires GPU (can't run efficiently on CPU)
    fn is_gpu_only_algo(algo: Algorithm) -> bool {
        matches!(algo, 
            Algorithm::Ethash | Algorithm::KawPow | Algorithm::Autolykos |
            Algorithm::KHeavyHash | Algorithm::ProgPow
        )
    }
    
    /// Process a new job from the pool and detect if algorithm change is needed.
    /// Returns true if the mining algorithm changed.
    pub fn process_job(&self, job: &Job) -> bool {
        let job_algo_str = job.algo.as_deref().unwrap_or("");
        
        // Detect stream group from job_id prefix
        let is_external = job.job_id.starts_with("ext-");
        let new_group = if is_external {
            StreamGroup::Revenue
        } else {
            StreamGroup::Zion
        };
        
        // Parse the algorithm from job
        let mut new_algo = if let Some(parsed) = Algorithm::from_str(job_algo_str) {
            parsed
        } else if is_external {
            // Try to detect from job_id: ext-erg-xxxx → Autolykos
            self.detect_algo_from_job_id(&job.job_id)
                .unwrap_or(self.default_algo)
        } else {
            self.default_algo
        };
        
        // ═══ CPU-Only Mode: Replace GPU-only algorithms with RandomX ═══
        // When no GPU is detected, Revenue stream jobs that require GPU
        // (ethash, kawpow, autolykos) are automatically redirected to
        // RandomX (XMR/MoneroOcean) which the CPU can mine natively.
        if self.cpu_only_mode && is_external && Self::is_gpu_only_algo(new_algo) {
            info!(
                "🖥️ CPU-only: Replacing GPU algo {} → RandomX (XMR) for Revenue stream",
                new_algo.name()
            );
            new_algo = Algorithm::RandomX;
        }
        
        // Check if algorithm changed
        let current = *self.current_algo.read().unwrap();
        let algo_changed = current != new_algo;
        
        if algo_changed {
            let coin = if is_external {
                self.extract_coin_from_job_id(&job.job_id)
                    .unwrap_or_else(|| job_algo_str.to_uppercase())
            } else {
                "ZION".to_string()
            };
            
            info!(
                "🔄 Stream switch: {:?} → {:?} (coin: {} → {}, group: {:?})",
                current, new_algo,
                self.current_coin.read().unwrap(),
                coin,
                new_group
            );
            
            *self.current_algo.write().unwrap() = new_algo;
            *self.current_group.write().unwrap() = new_group;
            *self.current_coin.write().unwrap() = coin;
            *self.switch_count.write().unwrap() += 1;
            self.algo_switch_pending.store(true, Ordering::Release);
        } else {
            // Update group even if algo didn't change
            *self.current_group.write().unwrap() = new_group;
        }
        
        algo_changed
    }
    
    /// Check and clear the pending algorithm switch flag.
    /// Mining threads should call this periodically.
    pub fn take_pending_switch(&self) -> Option<Algorithm> {
        if self.algo_switch_pending.swap(false, Ordering::AcqRel) {
            Some(*self.current_algo.read().unwrap())
        } else {
            None
        }
    }
    
    /// Get the current mining algorithm
    pub fn current_algorithm(&self) -> Algorithm {
        *self.current_algo.read().unwrap()
    }
    
    /// Get the current stream group
    pub fn current_group(&self) -> StreamGroup {
        self.current_group.read().unwrap().clone()
    }
    
    /// Get the current coin being mined
    pub fn current_coin(&self) -> String {
        self.current_coin.read().unwrap().clone()
    }
    
    /// Get switch count
    pub fn switch_count(&self) -> u64 {
        *self.switch_count.read().unwrap()
    }
    
    /// Detect algorithm from external job_id (e.g., "ext-erg-12345" → Autolykos)
    fn detect_algo_from_job_id(&self, job_id: &str) -> Option<Algorithm> {
        let parts: Vec<&str> = job_id.splitn(3, '-').collect();
        if parts.len() >= 2 {
            match parts[1].to_lowercase().as_str() {
                "erg" => Some(Algorithm::Autolykos),
                "etc" => Some(Algorithm::Ethash),
                "rvn" => Some(Algorithm::KawPow),
                "xmr" => Some(Algorithm::RandomX),
                "kas" => Some(Algorithm::KHeavyHash),
                "alph" => Some(Algorithm::Blake3),
                "zec" => Some(Algorithm::Equihash),
                _ => None,
            }
        } else {
            None
        }
    }
    
    /// Extract coin name from job_id
    fn extract_coin_from_job_id(&self, job_id: &str) -> Option<String> {
        let parts: Vec<&str> = job_id.splitn(3, '-').collect();
        if parts.len() >= 2 {
            Some(parts[1].to_uppercase())
        } else {
            None
        }
    }
}

/// Compute hash using the appropriate algorithm for the current stream.
/// This is the unified entry point that respects stream scheduler assignments.
pub fn compute_stream_hash(
    algo: Algorithm,
    header: &[u8],
    nonce: u64,
    height: u32,
) -> Result<Vec<u8>> {
    let native_algo = algo.to_native();
    native_algos::compute_hash(native_algo, header, nonce, height)
}

/// Check if hash meets target for the given algorithm.
/// Different algorithms use different target comparison logic.
pub fn meets_stream_target(
    algo: Algorithm,
    hash: &[u8; 32],
    target_hex: &str,
    cosmic_state0_endian: Option<&str>,
) -> bool {
    use super::cpu::CpuMiner;
    CpuMiner::meets_target_static(algo, hash, target_hex, cosmic_state0_endian)
}
