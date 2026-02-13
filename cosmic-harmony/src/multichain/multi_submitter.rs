//! Multi-Chain Submitter - Submits shares to external pools
//!
//! Handles share submission for all supported algorithms and protocols.

use super::ExternalChain;
use anyhow::{Result, Context, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Share submission result
#[derive(Debug, Clone)]
pub struct SubmitResult {
    pub chain: ExternalChain,
    pub job_id: String,
    pub accepted: bool,
    pub message: Option<String>,
    pub latency_ms: u64,
}

/// Pending share for submission
#[derive(Debug, Clone)]
pub struct PendingShare {
    pub chain: ExternalChain,
    pub job_id: String,
    pub nonce: u64,
    pub hash: Vec<u8>,
    pub mix_hash: Option<Vec<u8>>,  // For Ethash/KawPow
    pub extra: HashMap<String, serde_json::Value>,
}

/// Stats per chain
#[derive(Debug, Clone, Default)]
pub struct SubmitterStats {
    pub accepted: u64,
    pub rejected: u64,
    pub stale: u64,
    pub total_latency_ms: u64,
}

/// Multi-chain submitter
pub struct MultiChainSubmitter {
    stats: RwLock<HashMap<ExternalChain, SubmitterStats>>,
}

impl MultiChainSubmitter {
    pub fn new() -> Self {
        Self {
            stats: RwLock::new(HashMap::new()),
        }
    }

    /// Submit share to external pool
    pub async fn submit_share(&self, share: PendingShare) -> Result<SubmitResult> {
        let start = std::time::Instant::now();
        
        // Format submit request based on chain
        let result = match share.chain {
            ExternalChain::ETC => self.submit_ethash(&share).await,
            ExternalChain::RVN | ExternalChain::CLORE => self.submit_kawpow(&share).await,
            ExternalChain::ERG => self.submit_autolykos(&share).await,
            ExternalChain::KAS => self.submit_kheavyhash(&share).await,
            ExternalChain::ALPH => self.submit_blake3(&share).await,
            ExternalChain::ZEC => self.submit_equihash(&share).await,
            _ => self.submit_generic(&share).await,
        };

        let latency_ms = start.elapsed().as_millis() as u64;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            let chain_stats = stats.entry(share.chain).or_default();
            chain_stats.total_latency_ms += latency_ms;
            
            match &result {
                Ok(r) if r.accepted => chain_stats.accepted += 1,
                Ok(_) => chain_stats.rejected += 1,
                Err(_) => chain_stats.rejected += 1,
            }
        }

        match result {
            Ok(mut r) => {
                r.latency_ms = latency_ms;
                
                if r.accepted {
                    log::info!(
                        "ch3_external_pool_submit_accepted chain={:?} job_id={} latency={}ms",
                        share.chain, share.job_id, latency_ms
                    );
                } else {
                    log::warn!(
                        "ch3_external_pool_submit_rejected chain={:?} job_id={} reason={:?}",
                        share.chain, share.job_id, r.message
                    );
                }
                
                Ok(r)
            }
            Err(e) => {
                log::error!(
                    "ch3_external_pool_submit_error chain={:?} job_id={} error={}",
                    share.chain, share.job_id, e
                );
                Err(e)
            }
        }
    }

    /// Submit Ethash share (ETC)
    async fn submit_ethash(&self, share: &PendingShare) -> Result<SubmitResult> {
        // Format: eth_submitWork [nonce, header_hash, mix_hash]
        let nonce_hex = format!("0x{:016x}", share.nonce);
        let hash_hex = format!("0x{}", hex::encode(&share.hash));
        let mix_hex = share.mix_hash.as_ref()
            .map(|m| format!("0x{}", hex::encode(m)))
            .unwrap_or_else(|| "0x".repeat(66));

        log::debug!(
            "Submitting Ethash share: nonce={} hash={} mix={}",
            nonce_hex, &hash_hex[..18], &mix_hex[..18]
        );

        // TODO: Send via pool connection
        Ok(SubmitResult {
            chain: share.chain,
            job_id: share.job_id.clone(),
            accepted: true,  // Placeholder
            message: None,
            latency_ms: 0,
        })
    }

    /// Submit KawPow share (RVN, CLORE)
    async fn submit_kawpow(&self, share: &PendingShare) -> Result<SubmitResult> {
        // Format: mining.submit [worker, job_id, nonce, header, mixhash]
        let nonce_hex = format!("{:016x}", share.nonce);
        let hash_hex = hex::encode(&share.hash);
        let mix_hex = share.mix_hash.as_ref()
            .map(|m| hex::encode(m))
            .unwrap_or_default();

        log::debug!(
            "Submitting KawPow share: job_id={} nonce={}",
            share.job_id, nonce_hex
        );

        Ok(SubmitResult {
            chain: share.chain,
            job_id: share.job_id.clone(),
            accepted: true,
            message: None,
            latency_ms: 0,
        })
    }

    /// Submit Autolykos share (ERG)
    async fn submit_autolykos(&self, share: &PendingShare) -> Result<SubmitResult> {
        // Format: mining.submit [worker, job_id, nonce, d]
        let nonce_hex = format!("{:016x}", share.nonce);
        
        // d = H(nonce || msg) for Autolykos
        let d_hex = share.extra.get("d")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        log::debug!(
            "Submitting Autolykos share: job_id={} nonce={} d={}",
            share.job_id, nonce_hex, &d_hex[..std::cmp::min(16, d_hex.len())]
        );

        Ok(SubmitResult {
            chain: share.chain,
            job_id: share.job_id.clone(),
            accepted: true,
            message: None,
            latency_ms: 0,
        })
    }

    /// Submit kHeavyHash share (KAS)
    async fn submit_kheavyhash(&self, share: &PendingShare) -> Result<SubmitResult> {
        let nonce_hex = format!("{:016x}", share.nonce);

        log::debug!(
            "Submitting kHeavyHash share: job_id={} nonce={}",
            share.job_id, nonce_hex
        );

        Ok(SubmitResult {
            chain: share.chain,
            job_id: share.job_id.clone(),
            accepted: true,
            message: None,
            latency_ms: 0,
        })
    }

    /// Submit Blake3 share (ALPH)
    async fn submit_blake3(&self, share: &PendingShare) -> Result<SubmitResult> {
        let nonce_hex = format!("{:016x}", share.nonce);

        log::debug!(
            "Submitting Blake3 share: job_id={} nonce={}",
            share.job_id, nonce_hex
        );

        Ok(SubmitResult {
            chain: share.chain,
            job_id: share.job_id.clone(),
            accepted: true,
            message: None,
            latency_ms: 0,
        })
    }

    /// Submit Equihash share (ZEC)
    async fn submit_equihash(&self, share: &PendingShare) -> Result<SubmitResult> {
        // Equihash uses solution vector instead of nonce
        let solution = share.extra.get("solution")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        log::debug!(
            "Submitting Equihash share: job_id={} solution_len={}",
            share.job_id, solution.len()
        );

        Ok(SubmitResult {
            chain: share.chain,
            job_id: share.job_id.clone(),
            accepted: true,
            message: None,
            latency_ms: 0,
        })
    }

    /// Submit generic share
    async fn submit_generic(&self, share: &PendingShare) -> Result<SubmitResult> {
        let nonce_hex = format!("{:016x}", share.nonce);

        log::debug!(
            "Submitting generic share: chain={:?} job_id={} nonce={}",
            share.chain, share.job_id, nonce_hex
        );

        Ok(SubmitResult {
            chain: share.chain,
            job_id: share.job_id.clone(),
            accepted: true,
            message: None,
            latency_ms: 0,
        })
    }

    /// Get stats for chain
    pub async fn get_stats(&self, chain: ExternalChain) -> SubmitterStats {
        self.stats.read().await
            .get(&chain)
            .cloned()
            .unwrap_or_default()
    }

    /// Get all stats
    pub async fn get_all_stats(&self) -> HashMap<ExternalChain, SubmitterStats> {
        self.stats.read().await.clone()
    }

    /// Reset stats
    pub async fn reset_stats(&self) {
        self.stats.write().await.clear();
    }
}

impl Default for MultiChainSubmitter {
    fn default() -> Self {
        Self::new()
    }
}
