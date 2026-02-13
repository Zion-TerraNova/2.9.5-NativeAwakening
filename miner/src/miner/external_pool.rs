//! External Pool Mining Module
//!
//! Enables mining on external pools (2miners, Ethermine, etc.)
//! as part of ZION's CH3 multi-revenue architecture.
//!
//! Revenue model:
//! - 50% → ZION native mining (Cosmic Harmony)
//! - ~25% → External GPU mining (ETC/RVN/ERG/KAS)
//! - ~25% → NCL AI bonus
//!
//! The miner connects to external pools via EthStratum and submits
//! real shares computed with the correct algorithm.
//!
//! GPU Mining: Uses Metal (Apple Silicon) for Ethash with full DAG.

use anyhow::{anyhow, Result};
use log::{info, warn, error, debug};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::RwLock;

use crate::stratum::ethstratum::{EthStratumClient, EthStratumJob, ExternalCoin, ExternalPoolStats};

/// Configuration for external pool mining
#[derive(Debug, Clone)]
pub struct ExternalPoolConfig {
    /// Coin to mine (ETC, RVN, ERG, etc.)
    pub coin: ExternalCoin,
    /// Pool URL (host:port)
    pub pool_url: String,
    /// Wallet address (usually BTC for payout)
    pub wallet: String,
    /// Worker name
    pub worker: String,
    /// Number of CPU threads for external mining (0 = auto)
    pub cpu_threads: usize,
    /// Enable GPU for external mining
    pub gpu_enabled: bool,
    /// Percentage of hashpower allocated to external mining (0-100)
    pub hashpower_percent: u8,
}

impl ExternalPoolConfig {
    /// Create config for ETC mining on 2miners
    pub fn etc_2miners(wallet: &str, worker: &str) -> Self {
        Self {
            coin: ExternalCoin::ETC,
            pool_url: "etc.2miners.com:1010".to_string(),
            wallet: wallet.to_string(),
            worker: worker.to_string(),
            cpu_threads: 0,
            gpu_enabled: true,
            hashpower_percent: 25,
        }
    }

    /// Create config for RVN mining on 2miners
    pub fn rvn_2miners(wallet: &str, worker: &str) -> Self {
        Self {
            coin: ExternalCoin::RVN,
            pool_url: "rvn.2miners.com:6060".to_string(),
            wallet: wallet.to_string(),
            worker: worker.to_string(),
            cpu_threads: 1,
            gpu_enabled: true,
            hashpower_percent: 25,
        }
    }

    /// Create config for ERG mining on 2miners
    pub fn erg_2miners(wallet: &str, worker: &str) -> Self {
        Self {
            coin: ExternalCoin::ERG,
            pool_url: "erg.2miners.com:8888".to_string(),
            wallet: wallet.to_string(),
            worker: worker.to_string(),
            cpu_threads: 1,
            gpu_enabled: true,
            hashpower_percent: 25,
        }
    }
}

/// External pool miner — runs alongside ZION native mining
/// For ETC: Uses Metal GPU with full Ethash DAG (~2.4 GB)
pub struct ExternalMiner {
    config: ExternalPoolConfig,
    client: Arc<EthStratumClient>,
    running: Arc<AtomicBool>,
    stats: Arc<RwLock<ExternalMinerStats>>,
}

#[derive(Debug, Clone, Default)]
pub struct ExternalMinerStats {
    pub hashrate: f64,
    pub total_hashes: u64,
    pub shares_found: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub uptime_secs: u64,
}

impl ExternalMiner {
    pub fn new(config: ExternalPoolConfig) -> Self {
        let client = Arc::new(EthStratumClient::new(
            &config.pool_url,
            &config.wallet,
            &config.worker,
            config.coin,
        ));

        Self {
            config,
            client,
            running: Arc::new(AtomicBool::new(false)),
            stats: Arc::new(RwLock::new(ExternalMinerStats::default())),
        }
    }

    /// Start external pool mining
    /// For ETC: Initializes Metal GPU + Ethash DAG, then runs GPU mining loop
    pub async fn start(&self) -> Result<()> {
        info!("🚀 [{}] Starting external pool mining", self.config.coin.name());
        info!("   Pool: {}", self.config.pool_url);
        info!("   Wallet: {}", self.config.wallet);
        info!("   Worker: {}", self.config.worker);
        info!("   Algorithm: {}", self.config.coin.algorithm());
        info!("   GPU enabled: {}", self.config.gpu_enabled);
        info!("   Hashpower: {}%", self.config.hashpower_percent);

        // Connect to external pool
        self.client.connect_with_retry(5).await?;

        self.running.store(true, Ordering::SeqCst);

        // For ETC with GPU: use Metal Ethash miner
        #[cfg(all(feature = "metal", target_os = "macos"))]
        if self.config.gpu_enabled && self.config.coin == ExternalCoin::ETC {
            return self.start_gpu_ethash_mining().await;
        }

        // For ERG with GPU: use Metal Autolykos2 miner
        #[cfg(all(feature = "metal", target_os = "macos"))]
        if self.config.gpu_enabled && self.config.coin == ExternalCoin::ERG {
            return self.start_gpu_autolykos_mining().await;
        }

        // Fallback: CPU mining for non-ETC or non-GPU
        self.start_cpu_mining().await
    }

    /// GPU Ethash mining loop (Metal, Apple Silicon)
    #[cfg(all(feature = "metal", target_os = "macos"))]
    async fn start_gpu_ethash_mining(&self) -> Result<()> {
        use zion_cosmic_harmony_v3::gpu::{EthashMetalMiner, EthashEpoch};
        
        info!("🍎 [ETC] Initializing Metal GPU Ethash miner...");
        
        let client = Arc::clone(&self.client);
        let running = Arc::clone(&self.running);
        let stats = Arc::clone(&self.stats);
        let coin_name = self.config.coin.name().to_string();
        
        // Subscribe to jobs
        let mut job_rx = self.client.subscribe_jobs().await;
        
        // Wait for first job to get seed_hash for DAG epoch
        info!("[ETC] ⏳ Waiting for initial job to determine DAG epoch...");
        
        let initial_job = loop {
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                job_rx.changed()
            ).await {
                Ok(Ok(())) => {
                    if let Some(job) = job_rx.borrow().clone() {
                        break job;
                    }
                }
                Ok(Err(_)) => return Err(anyhow!("Job channel closed")),
                Err(_) => return Err(anyhow!("Timeout waiting for initial ETC job")),
            }
        };
        
        info!("[ETC] 📋 Initial job: id={}, seed={}", initial_job.job_id, &initial_job.seed_hash[..16]);
        
        // Determine epoch from seed hash
        let epoch = EthashEpoch::from_seed_hash(&initial_job.seed_hash);
        info!("[ETC] 📊 Epoch {} — DAG size: {:.2} GB ({} items)",
            epoch.number,
            epoch.dataset_size as f64 / 1_073_741_824.0,
            epoch.dataset_items,
        );
        
        // GPU mining runs in a blocking thread (Metal API is synchronous)
        let batch_size = 65_536u64; // 64K nonces per batch (Ethash is heavy per-nonce)
        
        tokio::task::spawn_blocking(move || {
            // Initialize Metal Ethash miner
            let mut miner = match EthashMetalMiner::new(batch_size as usize) {
                Ok(m) => m,
                Err(e) => {
                    log::error!("🍎 [ETC] Failed to init Metal Ethash miner: {}", e);
                    return;
                }
            };
            
            // Generate and upload DAG
            log::info!("🔧 [ETC] Generating Ethash DAG — this takes 30-60s...");
            match miner.load_dag_for_epoch(&epoch) {
                Ok(_) => {
                    log::info!("✅ [ETC] DAG loaded — starting GPU mining!");
                }
                Err(e) => {
                    log::error!("🍎 [ETC] DAG loading failed: {}. GPU memory may be insufficient.", e);
                    log::warn!("🍎 [ETC] ETC epoch {} requires {:.2} GB DAG — your GPU has {:.2} GB.",
                        epoch.number,
                        epoch.dataset_size as f64 / 1_073_741_824.0,
                        5461.0 / 1024.0, // Approximate, actual checked in load_dag
                    );
                    log::info!("🍎 [ETC] Consider: M1 Pro/Max (16+ GB), or mine coins without DAG (ALPH, KAS).");
                    return;
                }
            }
            
            let mut nonce_start = 0u64;
            let mut last_job_id: Option<String> = None;
            let mut current_header_hash = [0u8; 32];
            let mut current_target = [0xFFu8; 32]; // Easy target initially
            let mut gpu_total_hashes: u64 = 0;
            let mut gpu_shares_found: u64 = 0;
            let gpu_start_time = std::time::Instant::now();
            let mut batch_count: u64 = 0;
            let mut current_epoch = epoch.number;
            let mut current_job_id = String::new();
            
            // Set initial job
            {
                if let Ok(hh) = hex::decode(&initial_job.header_hash) {
                    if hh.len() >= 32 {
                        current_header_hash.copy_from_slice(&hh[..32]);
                    }
                }
                // Compute target from difficulty
                current_target = difficulty_to_target(initial_job.difficulty);
                current_job_id = initial_job.job_id.clone();
                last_job_id = Some(initial_job.job_id.clone());
            }
            
            loop {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                
                // Check for new job (non-blocking)
                if let Ok(job) = job_rx.has_changed() {
                    if job {
                        if let Some(new_job) = job_rx.borrow_and_update().clone() {
                            if last_job_id.as_deref() != Some(new_job.job_id.as_str()) {
                                // New job!
                                if let Ok(hh) = hex::decode(&new_job.header_hash) {
                                    if hh.len() >= 32 {
                                        current_header_hash.copy_from_slice(&hh[..32]);
                                    }
                                }
                                current_target = difficulty_to_target(new_job.difficulty);
                                current_job_id = new_job.job_id.clone();
                                last_job_id = Some(new_job.job_id.clone());
                                nonce_start = 0; // Reset nonce for new job
                                
                                // Check if epoch changed
                                let new_epoch = EthashEpoch::from_seed_hash(&new_job.seed_hash);
                                if new_epoch.number != current_epoch {
                                    log::info!("[ETC] 🔄 Epoch changed {} → {} — regenerating DAG...",
                                        current_epoch, new_epoch.number);
                                    if let Err(e) = miner.load_dag_for_epoch(&new_epoch) {
                                        log::error!("[ETC] DAG regen failed: {}", e);
                                        break;
                                    }
                                    current_epoch = new_epoch.number;
                                }
                                
                                log::info!("[ETC] 📋 New job: {} (diff: {:.4})", 
                                    new_job.job_id, new_job.difficulty);
                            }
                        }
                    }
                }
                
                // Mine batch
                let batch_start = std::time::Instant::now();
                match miner.mine(&current_header_hash, &current_target, nonce_start) {
                    Ok(Some((nonce, mix_digest, result_hash))) => {
                        gpu_shares_found += 1;
                        let mix_hex = hex::encode(mix_digest);
                        let result_hex = hex::encode(result_hash);
                        let job_id = current_job_id.clone();
                        
                        log::info!(
                            "🍎💎 [ETC] GPU SHARE FOUND! nonce={} mix={}...{}",
                            nonce, &mix_hex[..8], &mix_hex[56..]
                        );
                        
                        // Submit share async
                        let submit_client = Arc::clone(&client);
                        let submit_stats = Arc::clone(&stats);
                        let total = gpu_shares_found;
                        tokio::runtime::Handle::current().spawn(async move {
                            match submit_client.submit_share(
                                &job_id, nonce, &result_hex, &mix_hex
                            ).await {
                                Ok(accepted) => {
                                    if accepted {
                                        log::info!("🍎✅ [ETC] Share ACCEPTED (total: {})", total);
                                    } else {
                                        log::warn!("🍎❌ [ETC] Share REJECTED");
                                    }
                                    let mut s = submit_stats.write().await;
                                    if accepted {
                                        s.shares_accepted += 1;
                                    } else {
                                        s.shares_rejected += 1;
                                    }
                                    s.shares_found = total;
                                }
                                Err(e) => {
                                    log::warn!("[ETC] Submit error: {}", e);
                                }
                            }
                        });
                    }
                    Ok(None) => {
                        // No solution in this batch — normal
                    }
                    Err(e) => {
                        log::error!("🍎 [ETC] Mine error: {}", e);
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                    }
                }
                
                gpu_total_hashes += batch_size;
                batch_count += 1;
                nonce_start = nonce_start.wrapping_add(batch_size);
                
                // Report hashrate every 20 batches
                if batch_count % 20 == 0 {
                    let elapsed = gpu_start_time.elapsed().as_secs_f64();
                    let gpu_hashrate = gpu_total_hashes as f64 / elapsed;
                    let batch_elapsed = batch_start.elapsed().as_secs_f64();
                    let batch_rate = batch_size as f64 / batch_elapsed;
                    
                    log::info!(
                        "🍎 [ETC] GPU: {:.2} kH/s (batch {:.2} kH/s) | {} shares | nonce {}",
                        gpu_hashrate / 1_000.0,
                        batch_rate / 1_000.0,
                        gpu_shares_found,
                        nonce_start
                    );
                    
                    // Update stats
                    if let Ok(mut s) = stats.try_write() {
                        s.hashrate = gpu_hashrate;
                        s.total_hashes = gpu_total_hashes;
                    }
                }
            }
        });
        
        Ok(())
    }

    /// GPU Autolykos2 mining loop (Metal, Apple Silicon) — ERG on 2miners
    #[cfg(all(feature = "metal", target_os = "macos"))]
    async fn start_gpu_autolykos_mining(&self) -> Result<()> {
        use zion_cosmic_harmony_v3::gpu::{autolykos2_hash_cpu, AutolykosMetalMiner, AutolykosTableInfo};
        
        info!("🍎 [ERG] Initializing Metal GPU Autolykos2 miner...");
        
        let client = Arc::clone(&self.client);
        let running = Arc::clone(&self.running);
        let stats = Arc::clone(&self.stats);
        
        // Subscribe to jobs
        let mut job_rx = self.client.subscribe_jobs().await;
        
        // Wait for first job to get block height for table generation
        info!("[ERG] ⏳ Waiting for initial job...");
        
        let initial_job = loop {
            match tokio::time::timeout(
                tokio::time::Duration::from_secs(30),
                job_rx.changed()
            ).await {
                Ok(Ok(())) => {
                    if let Some(job) = job_rx.borrow().clone() {
                        break job;
                    }
                }
                Ok(Err(_)) => return Err(anyhow!("Job channel closed")),
                Err(_) => return Err(anyhow!("Timeout waiting for initial ERG job")),
            }
        };
        
        info!("[ERG] 📋 Initial job: id={}, header={}", 
            initial_job.job_id, &initial_job.header_hash[..16]);
        
        // ERG 2miners sends height directly in mining.notify params[1]
        // It's stored in job.height (parsed from notify in ethstratum.rs)
        let initial_height = if initial_job.height > 0 {
            initial_job.height
        } else {
            Self::extract_erg_height(&initial_job.seed_hash).unwrap_or(1_200_000)
        };
        let table_info = AutolykosTableInfo::from_height(initial_height);
        
        info!("[ERG] 📊 Block height ~{} — N={} (2^{:.1}) — TABLELESS mode",
            initial_height, table_info.n, (table_info.n as f64).log2());
        
        // GPU mining runs in blocking thread
        // Tableless Autolykos2: 36 Blake2b256 per nonce (each ~8200 bytes) = very compute heavy
        // 65K batch balances throughput vs GPU timeout risk on Apple M1
        let batch_size = 65_536u64;
        
        // Get extranonce from pool — nonce must start with this prefix!
        // ERG Stratum: pool sends extranonce (e.g. "152f"), miner searches nonces 
        // where upper bytes match extranonce. Submit = extranonce + miner_part.
        let mut extranonce_str = client.get_extranonce().await;
        if extranonce_str.is_empty() {
            // Extranonce can arrive slightly after subscribe; wait briefly.
            for _ in 0..50 {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                extranonce_str = client.get_extranonce().await;
                if !extranonce_str.is_empty() {
                    break;
                }
            }
        }
        if extranonce_str.is_empty() {
            log::warn!("[ERG] ⚠️ Extranonce is empty; shares will likely be rejected by pool");
        }
        let extranonce_base: u64 = if !extranonce_str.is_empty() {
            let en_val = u64::from_str_radix(&extranonce_str, 16).unwrap_or(0);
            let shift_bits = (16 - extranonce_str.len()) * 4; // hex chars to bits
            en_val << shift_bits
        } else {
            0
        };
        log::info!("[ERG] 🔑 Extranonce: '{}' → nonce base: 0x{:016x}", extranonce_str, extranonce_base);
        let _extranonce_for_submit = extranonce_str.clone();
        
        tokio::task::spawn_blocking(move || {
            fn be32_leq(a: &[u8; 32], b: &[u8; 32]) -> bool {
                for i in 0..32 {
                    if a[i] < b[i] {
                        return true;
                    }
                    if a[i] > b[i] {
                        return false;
                    }
                }
                true
            }

            // Initialize Metal Autolykos2 miner
            let mut miner = match AutolykosMetalMiner::new(batch_size as usize) {
                Ok(m) => m,
                Err(e) => {
                    log::error!("🍎 [ERG] Failed to init Metal Autolykos2 miner: {}", e);
                    return;
                }
            };
            
            // Prepare for height (TABLELESS — instant, no table generation!)
            match miner.prepare_for_height(initial_height) {
                Ok(_) => {
                    log::info!("✅ [ERG] TABLELESS mode ready — N={}, R values computed on-the-fly!", table_info.n);
                }
                Err(e) => {
                    log::error!("🍎 [ERG] Height preparation failed: {}", e);
                    return;
                }
            }
            
            // Nonce MUST start with extranonce prefix from pool!
            // E.g. extranonce "152f" → nonces are 0x152f000000000000 + offset
            let mut nonce_start = extranonce_base;
            let mut last_job_id: Option<String> = None;
            let mut current_header_hash = [0u8; 32];
            let mut current_share_target = [0xFFu8; 32];
            let mut current_block_target = [0xFFu8; 32];
            let mut current_height = initial_height as u32;
            let mut gpu_total_hashes: u64 = 0;
            let mut gpu_shares_found: u64 = 0;
            let gpu_start_time = std::time::Instant::now();
            let mut batch_count: u64 = 0;
            let mut current_job_id = String::new();
            {
                if let Ok(hh) = hex::decode(&initial_job.header_hash) {
                    if hh.len() >= 32 {
                        current_header_hash.copy_from_slice(&hh[..32]);
                    }
                }
                // ERG: mine SHARES using pool difficulty (VarDiff), not network block target.
                current_share_target = difficulty_to_target(initial_job.difficulty);

                // Keep block target separately (for block-candidate detection/logging only).
                if !initial_job.b_target.is_empty() {
                    current_block_target = decimal_bigint_to_be32(&initial_job.b_target);
                    log::info!(
                        "[ERG] 🎯 Targets: share(diff={:.4})={:02x}{:02x}{:02x}{:02x}...{:02x}{:02x}{:02x}{:02x} | block(b)={}...{} → {:02x}{:02x}{:02x}{:02x}...{:02x}{:02x}{:02x}{:02x}",
                        initial_job.difficulty,
                        current_share_target[0], current_share_target[1], current_share_target[2], current_share_target[3],
                        current_share_target[28], current_share_target[29], current_share_target[30], current_share_target[31],
                        &initial_job.b_target[..initial_job.b_target.len().min(20)],
                        &initial_job.b_target[initial_job.b_target.len().saturating_sub(6)..],
                        current_block_target[0], current_block_target[1], current_block_target[2], current_block_target[3],
                        current_block_target[28], current_block_target[29], current_block_target[30], current_block_target[31]
                    );
                } else {
                    current_block_target = current_share_target;
                    log::info!(
                        "[ERG] 🎯 Targets: share(diff={:.4})={:02x}{:02x}{:02x}{:02x}...{:02x}{:02x}{:02x}{:02x} (no b_target)",
                        initial_job.difficulty,
                        current_share_target[0], current_share_target[1], current_share_target[2], current_share_target[3],
                        current_share_target[28], current_share_target[29], current_share_target[30], current_share_target[31]
                    );
                }
                current_job_id = initial_job.job_id.clone();
                last_job_id = Some(initial_job.job_id.clone());
            }
            
            loop {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                
                // Check for new job (non-blocking)
                if let Ok(job) = job_rx.has_changed() {
                    if job {
                        if let Some(new_job) = job_rx.borrow_and_update().clone() {
                            if last_job_id.as_deref() != Some(new_job.job_id.as_str()) {
                                // New job
                                if let Ok(hh) = hex::decode(&new_job.header_hash) {
                                    if hh.len() >= 32 {
                                        current_header_hash.copy_from_slice(&hh[..32]);
                                    }
                                }
                                current_share_target = difficulty_to_target(new_job.difficulty);
                                current_block_target = if !new_job.b_target.is_empty() {
                                    decimal_bigint_to_be32(&new_job.b_target)
                                } else {
                                    current_share_target
                                };
                                current_job_id = new_job.job_id.clone();
                                last_job_id = Some(new_job.job_id.clone());
                                nonce_start = extranonce_base; // Reset with extranonce prefix!
                                
                                // Check if height changed (requires new table)
                                let new_height = if new_job.height > 0 {
                                    new_job.height
                                } else {
                                    Self::extract_erg_height(&new_job.seed_hash)
                                        .unwrap_or(current_height as u64)
                                };
                                let new_info = AutolykosTableInfo::from_height(new_height);
                                let old_info = AutolykosTableInfo::from_height(current_height as u64);
                                
                                if new_info.n != old_info.n {
                                    log::info!("[ERG] 🔄 Table size changed (N: {} → {}) — regenerating...",
                                        old_info.n, new_info.n);
                                    if let Err(e) = miner.prepare_for_height(new_height) {
                                        log::error!("[ERG] Height update failed: {}", e);
                                        break;
                                    }
                                }
                                current_height = new_height as u32;
                                
                                log::info!("[ERG] 📋 New job: {} (diff: {:.4}, h: {})", 
                                    new_job.job_id, new_job.difficulty, current_height);
                            }
                        }
                    }
                }
                
                // Mine batch
                let batch_start = std::time::Instant::now();
                match miner.mine(&current_header_hash, &current_share_target, current_height, nonce_start) {
                    Ok(Some((nonce, result_hash))) => {
                        gpu_shares_found += 1;
                        let result_hex = hex::encode(result_hash);
                        let job_id = current_job_id.clone();
                        let nonce_hex = format!("{:016x}", nonce);
                        let share_target_hex = hex::encode(current_share_target);
                        let block_target_hex = hex::encode(current_block_target);
                        
                        log::info!(
                            "🍎💎 [ERG] GPU SHARE FOUND! nonce=0x{} hash={} share_target={}...{} block_target={}...{}",
                            nonce_hex,
                            &result_hex[..16],
                            &share_target_hex[..16],
                            &share_target_hex[56..],
                            &block_target_hex[..16],
                            &block_target_hex[56..]
                        );

                        // Validate against CPU reference for this exact nonce.
                        // If this fails, pool rejection is expected (we're hashing something else than pool verifies).
                        let current_n = AutolykosTableInfo::from_height(current_height as u64).n;
                        let cpu_hash = autolykos2_hash_cpu(&current_header_hash, nonce, current_height, current_n);
                        if cpu_hash != result_hash {
                            log::warn!(
                                "[ERG] ❌ GPU/CPU mismatch for nonce=0x{} (cpu={}..., gpu={}...) — skipping submit",
                                nonce_hex,
                                hex::encode(cpu_hash)[..8].to_string(),
                                &result_hex[..8]
                            );
                            continue;
                        }

                        let meets_share = be32_leq(&cpu_hash, &current_share_target);
                        let meets_block = be32_leq(&cpu_hash, &current_block_target);
                        if !meets_share {
                            log::warn!(
                                "[ERG] ⚠️ GPU returned nonce but CPU hash does not meet share target; skipping submit (nonce=0x{})",
                                nonce_hex
                            );
                            continue;
                        }
                        if meets_block {
                            log::info!("[ERG] 🧱 Block-candidate meets network target (nonce=0x{})", nonce_hex);
                        }
                        
                        // Submit share async
                        let submit_client = Arc::clone(&client);
                        let submit_stats = Arc::clone(&stats);
                        let total = gpu_shares_found;
                        tokio::runtime::Handle::current().spawn(async move {
                            match submit_client.submit_share(
                                &job_id, nonce, &result_hex, &nonce_hex
                            ).await {
                                Ok(accepted) => {
                                    if accepted {
                                        log::info!("🍎✅ [ERG] Share ACCEPTED (total: {})", total);
                                    } else {
                                        log::warn!("🍎❌ [ERG] Share REJECTED");
                                    }
                                    let mut s = submit_stats.write().await;
                                    if accepted {
                                        s.shares_accepted += 1;
                                    } else {
                                        s.shares_rejected += 1;
                                    }
                                    s.shares_found = total;
                                }
                                Err(e) => {
                                    log::warn!("[ERG] Submit error: {}", e);
                                }
                            }
                        });
                    }
                    Ok(None) => {
                        // No solution in this batch — normal
                    }
                    Err(e) => {
                        log::error!("🍎 [ERG] Mine error: {}", e);
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                    }
                }
                
                gpu_total_hashes += batch_size;
                batch_count += 1;
                nonce_start = nonce_start.wrapping_add(batch_size);
                
                // Report hashrate every 5 batches (Autolykos2 tableless — batches are slow)
                if batch_count % 5 == 0 {
                    let elapsed = gpu_start_time.elapsed().as_secs_f64();
                    let gpu_hashrate = gpu_total_hashes as f64 / elapsed;
                    let batch_elapsed = batch_start.elapsed().as_secs_f64();
                    let batch_rate = batch_size as f64 / batch_elapsed;
                    
                    log::info!(
                        "🍎 [ERG] GPU: {:.2} kH/s (batch {:.2} kH/s) | {} shares | nonce {} | h:{}",
                        gpu_hashrate / 1_000.0,
                        batch_rate / 1_000.0,
                        gpu_shares_found,
                        nonce_start,
                        current_height,
                    );
                    
                    // Update stats
                    if let Ok(mut s) = stats.try_write() {
                        s.hashrate = gpu_hashrate;
                        s.total_hashes = gpu_total_hashes;
                    }
                }
            }
        });
        
        Ok(())
    }
    
    /// Extract ERG block height from seed_hash
    /// ERG stratum typically encodes height info in the seed_hash field
    fn extract_erg_height(seed_hash: &str) -> Option<u64> {
        // ERG 2miners encodes the height in seed_hash as hex
        // Try to parse first 8 hex chars as height
        if seed_hash.len() >= 8 {
            if let Ok(h) = u64::from_str_radix(&seed_hash[..8], 16) {
                if h > 0 && h < 100_000_000 {
                    return Some(h);
                }
            }
        }
        // If we can't determine height, caller uses default
        None
    }

    /// CPU mining fallback for non-ETC or non-GPU
    async fn start_cpu_mining(&self) -> Result<()> {
        let mut job_rx = self.client.subscribe_jobs().await;

        info!("[{}] ⏳ Waiting for jobs from external pool...", self.config.coin.name());

        while self.running.load(Ordering::SeqCst) {
            tokio::select! {
                result = job_rx.changed() => {
                    if result.is_err() {
                        warn!("[{}] Job channel closed", self.config.coin.name());
                        break;
                    }

                    let job = job_rx.borrow().clone();
                    if let Some(job) = job {
                        self.mine_job_cpu(&job).await;
                    }
                }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {
                    if !self.client.is_connected() {
                        warn!("[{}] Disconnected, reconnecting...", self.config.coin.name());
                        if let Err(e) = self.client.connect_with_retry(3).await {
                            error!("[{}] Reconnection failed: {}", self.config.coin.name(), e);
                            break;
                        }
                        job_rx = self.client.subscribe_jobs().await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Mine a single job from the external pool (CPU fallback)
    async fn mine_job_cpu(&self, job: &EthStratumJob) {
        let coin = self.config.coin;

        debug!("[{}] Mining job {} (diff: {:.4})", coin.name(), job.job_id, job.difficulty);

        let target_u64 = if job.difficulty > 0.0 {
            (u64::MAX as f64 / job.difficulty) as u64
        } else {
            u64::MAX
        };

        let header_bytes = match hex::decode(&job.header_hash) {
            Ok(b) => b,
            Err(e) => {
                warn!("[{}] Invalid header hash: {}", coin.name(), e);
                return;
            }
        };

        let batch_size: u64 = match coin {
            ExternalCoin::ETC => 100_000,
            ExternalCoin::RVN => 50_000,
            ExternalCoin::ERG => 100_000,
            _ => 500_000,
        };

        let start_time = std::time::Instant::now();
        let mut hashes: u64 = 0;

        for nonce in 0..batch_size {
            let hash_result = Self::generic_hash(&header_bytes, nonce);
            hashes += 1;

            if let Some(hash) = hash_result {
                let hash_val = u64::from_le_bytes(hash[..8].try_into().unwrap_or([0xFF; 8]));
                if hash_val < target_u64 {
                    info!("💎 [{}] Share found! nonce={}", coin.name(), nonce);
                    
                    let _ = self.client.submit_share(
                        &job.job_id,
                        nonce,
                        &job.header_hash,
                        &hex::encode(&hash),
                    ).await;

                    let mut stats = self.stats.write().await;
                    stats.shares_found += 1;
                }
            }
        }

        let elapsed = start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            let mut stats = self.stats.write().await;
            stats.total_hashes += hashes;
            stats.hashrate = hashes as f64 / elapsed;
        }
    }

    /// Generic hash for fallback CPU mining (blake3)
    fn generic_hash(header: &[u8], nonce: u64) -> Option<[u8; 32]> {
        let mut input = Vec::with_capacity(header.len() + 8);
        input.extend_from_slice(header);
        input.extend_from_slice(&nonce.to_le_bytes());
        
        let hash = blake3::hash(&input);
        let mut result = [0u8; 32];
        result.copy_from_slice(hash.as_bytes());
        Some(result)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        info!("[{}] External miner stopped", self.config.coin.name());
    }

    pub async fn get_stats(&self) -> ExternalMinerStats {
        self.stats.read().await.clone()
    }

    pub async fn get_pool_stats(&self) -> ExternalPoolStats {
        self.client.get_stats().await
    }
}

/// Convert pool difficulty to a 256-bit target (BIG-ENDIAN).
///
/// Important: 2miners ERG uses an EthStratum-like `mining.set_difficulty`, where
/// difficulty is defined relative to a *diff1 target* (same convention as Ethash stratum):
///
///   target = DIFF1_TARGET / difficulty
///
/// If we use other bases (e.g. secp256k1 `q`), the pool will reject shares as
/// `Low difficulty share`.
fn difficulty_to_target(difficulty: f64) -> [u8; 32] {
    // EthStratum "diff1" target (big-endian):
    // 0x00000000FFFF0000000000000000000000000000000000000000000000000000
    // This is a standard constant in Ethash-stratum implementations.
    const DIFF1_TARGET_BE: [u8; 32] = [
        0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];

    fn div_be_bytes_by_u64(input_be: &[u8], divisor: u64) -> Vec<u8> {
        let mut out = vec![0u8; input_be.len()];
        let mut remainder: u128 = 0;
        let divisor = divisor as u128;

        for (i, &byte) in input_be.iter().enumerate() {
            remainder = (remainder << 8) | (byte as u128);
            out[i] = (remainder / divisor) as u8;
            remainder %= divisor;
        }
        out
    }

    fn mul_be_bytes_by_u64(input_be: &[u8], multiplier: u64) -> Vec<u8> {
        if multiplier == 0 {
            return vec![0u8; input_be.len()];
        }

        let mut out = vec![0u8; input_be.len() + 8];
        let out_len = out.len();
        let mut carry: u128 = 0;
        let m = multiplier as u128;

        for (in_index, &byte) in input_be.iter().rev().enumerate() {
            let prod = (byte as u128) * m + carry;
            let out_pos = out_len - 1 - in_index;
            out[out_pos] = (prod & 0xFF) as u8;
            carry = prod >> 8;
        }
        let mut out_index = out_len - 1 - input_be.len();
        while carry > 0 {
            out[out_index] = (carry & 0xFF) as u8;
            carry >>= 8;
            if out_index == 0 {
                break;
            }
            out_index -= 1;
        }

        // Trim leading zeros (keep at least 1 byte)
        let first_non_zero = out.iter().position(|&b| b != 0).unwrap_or(out.len() - 1);
        out[first_non_zero..].to_vec()
    }

    let base = DIFF1_TARGET_BE;

    // If pool doesn't send difficulty yet, default to diff=1 target (safe).
    if !difficulty.is_finite() || difficulty <= 0.0 {
        return base;
    }

    // Pool difficulty should be >= 1; if not, clamp to base.
    if difficulty < 1.0 {
        return base;
    }

    // Prefer exact integer division when possible.
    let diff_rounded = difficulty.round();
    if (difficulty - diff_rounded).abs() < 1e-9 && diff_rounded <= (u64::MAX as f64) {
        let divisor = diff_rounded as u64;
        if divisor <= 1 {
            return base;
        }
        let q = div_be_bytes_by_u64(&base, divisor);
        let mut result = [0u8; 32];
        if q.len() >= 32 {
            result.copy_from_slice(&q[q.len() - 32..]);
        } else {
            result[32 - q.len()..].copy_from_slice(&q);
        }
        return result;
    }

    // Non-integer difficulty: compute base / difficulty using fixed-point scaling.
    // target = base * scale / round(difficulty * scale)
    const SCALE: u64 = 1_000_000;
    let denom = (difficulty * (SCALE as f64)).round() as u64;
    if denom == 0 {
        return base;
    }

    let num = mul_be_bytes_by_u64(&base, SCALE);
    let q = div_be_bytes_by_u64(&num, denom);
    let mut result = [0u8; 32];
    if q.len() >= 32 {
        result.copy_from_slice(&q[q.len() - 32..]);
    } else {
        result[32 - q.len()..].copy_from_slice(&q);
    }
    result
}

/// Convert a decimal BigInt string (e.g., "6634674375215649981044791689095340972727658017446627184440307089471")
/// to a 32-byte big-endian byte array.
/// Used for ERG pool target 'b' value from mining.notify.
fn decimal_bigint_to_be32(decimal_str: &str) -> [u8; 32] {
    let mut result = [0u8; 32];
    
    if decimal_str.is_empty() || decimal_str == "0" {
        return result;
    }
    
    // Convert decimal string to bytes using repeated division by 256
    // We work with the number as a vector of decimal digits
    let mut digits: Vec<u8> = decimal_str.bytes()
        .filter(|b| b.is_ascii_digit())
        .map(|b| b - b'0')
        .collect();
    
    if digits.is_empty() {
        return [0xFF; 32]; // fallback easy target
    }
    
    // Extract bytes from LSB to MSB by dividing by 256
    let mut byte_index = 31i32; // Start from LSB in BE layout
    
    while !digits.is_empty() && byte_index >= 0 {
        // Divide digits by 256, get remainder as next byte
        let mut remainder: u32 = 0;
        let mut new_digits: Vec<u8> = Vec::new();
        
        for &d in &digits {
            remainder = remainder * 10 + d as u32;
            let quotient = remainder / 256;
            remainder %= 256;
            if !new_digits.is_empty() || quotient > 0 {
                new_digits.push(quotient as u8);
            }
        }
        
        result[byte_index as usize] = remainder as u8;
        byte_index -= 1;
        digits = new_digits;
    }
    
    result
}

/// Multi-external pool manager — runs multiple external miners simultaneously
pub struct ExternalPoolManager {
    miners: Vec<Arc<ExternalMiner>>,
    running: Arc<AtomicBool>,
}

impl ExternalPoolManager {
    pub fn new() -> Self {
        Self {
            miners: Vec::new(),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn add_pool(&mut self, config: ExternalPoolConfig) {
        self.miners.push(Arc::new(ExternalMiner::new(config)));
    }

    /// Start all external miners
    pub async fn start_all(&self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        
        info!("🌐 Starting {} external pool miner(s)...", self.miners.len());

        let mut handles = Vec::new();
        for miner in &self.miners {
            let miner = Arc::clone(miner);
            handles.push(tokio::spawn(async move {
                if let Err(e) = miner.start().await {
                    error!("External miner error: {}", e);
                }
            }));
        }

        // Wait for all miners
        for handle in handles {
            let _ = handle.await;
        }

        Ok(())
    }

    pub fn stop_all(&self) {
        self.running.store(false, Ordering::SeqCst);
        for miner in &self.miners {
            miner.stop();
        }
    }

    pub async fn print_stats(&self) {
        for miner in &self.miners {
            let stats = miner.get_stats().await;
            let pool_stats = miner.get_pool_stats().await;
            info!("📊 [{}] H/s: {:.2}, shares: {}/{}/{}, jobs: {}",
                miner.config.coin.name(),
                stats.hashrate,
                pool_stats.shares_accepted,
                pool_stats.shares_rejected,
                pool_stats.shares_submitted,
                pool_stats.jobs_received,
            );
        }
    }
}
