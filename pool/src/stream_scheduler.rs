/// Stream Scheduler v3 — 50/25/25 Model (CH v3 Architecture)
///
/// Canonical compute allocation (from cosmic-harmony config):
///
///   50% → ZION mining (Keccak→SHA3→Matrix→Fusion)
///         └── BONUS: Keccak & SHA3 intermediates submitted FREE to ETC/Nexus
///   25% → Revenue stream:
///         ├── GPU mode: Multi-Algo profit-switch (ERG/RVN/KAS/ALPH)
///         └── CPU mode: XMR/RandomX on MoneroOcean (auto-detected)
///   25% → NCL AI inference tasks (Neural Compute Layer)
///
/// GPU Detection (automatic):
///   - GPU found → ProfitSwitcher selects best GPU coin from WhatToMine
///   - No GPU → Revenue 25% locked to XMR (RandomX, CPU-native)
///   - Override: ZION_HAS_GPU=1 env var
///
/// Revenue streams: 5 total, but only 3 cost compute!
///   Stream 1: ZION (50% compute)
///   Stream 2: ETC/Keccak (FREE byproduct of ZION mining)
///   Stream 3: NXS/SHA3 (FREE byproduct of ZION mining)
///   Stream 4: Multi-Algo or XMR (25% compute)
///   Stream 5: NCL AI (25% compute → embeddings, inference, etc.)
///
/// With enough miners (≥4), each miner is ASSIGNED to a group:
///   - ZION group (50%): always gets CosmicHarmony jobs
///   - Revenue group (25%): mines best-profit external coin
///   - NCL group (25%): performs AI inference tasks
///
/// With few miners (<4), falls back to time-splitting:
///   - 50% time ZION, 25% time revenue, 25% time NCL
///
/// This is the core L1 revenue architecture of ZION TerraNova.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tokio::sync::{broadcast, watch, RwLock, Notify};
use tracing::{info, warn, debug};

use crate::config::StreamsConfig;
use crate::revenue_proxy::{ExternalJob, RevenueProxyManager, ShareSubmission};

/// Identifies which stream a job belongs to
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StreamId {
    /// Native ZION blockchain mining
    Zion,
    /// ETC fixed stream
    Etc,
    /// Dynamic GPU coin (ERG, RVN, XMR, KAS, etc.)
    DynamicGpu(String),
}

impl std::fmt::Display for StreamId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamId::Zion => write!(f, "ZION"),
            StreamId::Etc => write!(f, "ETC"),
            StreamId::DynamicGpu(coin) => write!(f, "GPU:{}", coin.to_uppercase()),
        }
    }
}

/// Which group a miner is assigned to (50/25/25 model)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MinerGroup {
    /// Always mines ZION (50% compute)
    Zion,
    /// Mines the current best-profit external coin (25% compute)
    Revenue,
    /// Performs NCL AI inference tasks (25% compute)
    Ncl,
}

/// A unified job that can be sent to a miner, regardless of stream
#[derive(Debug, Clone)]
pub struct ScheduledJob {
    /// Which stream this job belongs to
    pub stream_id: StreamId,
    /// Unique job identifier (prefixed with stream tag)
    pub job_id: String,
    /// For ZION: the block blob. For external: header_hash or equivalent.
    pub blob: String,
    /// Share target hex string
    pub target: String,
    /// Numeric difficulty
    pub difficulty: f64,
    /// Block height (ZION) or 0 (external)
    pub height: u64,
    /// Mining algorithm name
    pub algorithm: String,
    /// External coin name (empty for ZION)
    pub coin: String,
    /// Whether to clean previous jobs
    pub clean_jobs: bool,
    /// Extranonce from external pool subscription
    pub extranonce: String,
    /// Raw mining.notify params for protocol-specific forwarding
    pub raw_notify_params: Vec<String>,
    /// Seed hash (for ethash-based algorithms)
    pub seed_hash: String,
    /// Timestamp when this job was created
    pub created_at: Instant,
}

/// Result of share routing
#[derive(Debug)]
pub enum ShareRoute {
    /// Share is for ZION blockchain — process with ShareProcessor
    Zion,
    /// Share was forwarded to external pool
    External(String),
}

/// Scheduling mode — auto-selected based on miner count
#[derive(Debug, Clone, PartialEq)]
pub enum SchedulerMode {
    /// < 3 miners: alternate between ZION and best-profit coin
    TimeSplit,
    /// ≥ 3 miners: assign each miner to a fixed group
    PerMiner,
}

/// The main stream scheduler — 50/25/25 model
pub struct StreamScheduler {
    /// ZION share of total compute (0.50 in 50/25/25 model)
    zion_share: f64,

    /// Multi-Algo revenue share (0.25 in 50/25/25 model)
    revenue_share: f64,

    /// NCL AI share (0.25 in 50/25/25 model)
    ncl_share: f64,

    /// Latest ZION job from BlockTemplateManager
    zion_job: RwLock<Option<ScheduledJob>>,

    /// Latest external jobs per coin (key = coin lowercase)
    external_jobs: RwLock<HashMap<String, ScheduledJob>>,

    /// Currently best-profit coin (from ProfitSwitcher)
    best_coin: RwLock<String>,

    /// Per-miner group assignment (key = session_id)
    miner_groups: RwLock<HashMap<String, MinerGroup>>,

    /// Current scheduler mode
    mode: RwLock<SchedulerMode>,

    /// For time-split mode: is ZION active right now?
    timesplit_on_zion: RwLock<bool>,

    /// When last time-split switch happened
    timesplit_last_switch: RwLock<Instant>,

    /// Cumulative time on ZION (for time-split tracking)
    zion_time_secs: RwLock<f64>,

    /// Cumulative time on revenue (for time-split tracking)
    revenue_time_secs: RwLock<f64>,

    /// Cumulative time on NCL AI (for time-split tracking)
    ncl_time_secs: RwLock<f64>,

    /// Current time-split phase: 0=ZION, 1=Revenue, 2=NCL
    timesplit_phase: RwLock<u8>,

    /// Reference to revenue proxy for share forwarding
    revenue_proxy: Option<Arc<RevenueProxyManager>>,

    /// Map from job_id to coin for share routing
    job_coin_map: RwLock<HashMap<String, String>>,

    /// Notification channel for mode changes
    mode_notify: Notify,

    /// Total miners (tracked for mode switching)
    total_miners: RwLock<usize>,

    /// CPU-only mode: no GPU → Revenue locked to XMR (RandomX)
    cpu_only: AtomicBool,
}

impl StreamScheduler {
    /// Create a new StreamScheduler using the 50/25/25 model
    ///
    /// Compute allocation:
    ///   50% → ZION (CosmicHarmony pipeline) — Keccak/SHA3 exports are FREE
    ///   25% → Multi-Algo revenue (ERG/RVN/KAS/ALPH via external pools)
    ///   25% → NCL AI inference tasks
    pub fn new(
        streams_config: &StreamsConfig,
        revenue_proxy: Option<Arc<RevenueProxyManager>>,
    ) -> Self {
        // Detect CPU-only mode (same logic as ProfitSwitcher)
        let cpu_only = if let Ok(val) = std::env::var("ZION_HAS_GPU") {
            !matches!(val.as_str(), "1" | "true" | "yes")
        } else {
            true // Default: CPU-only (no GPU detected in container)
        };

        // Check what streams are enabled
        let has_external = streams_config.etc.enabled
            || streams_config.dynamic_gpu.enabled;
        let has_ncl = streams_config.ncl.enabled;

        // 50/25/25 model — use target_share from config, with sane defaults
        let zion_share = streams_config.zion.target_share.clamp(0.30, 1.0);
        let revenue_share = if has_external {
            streams_config.dynamic_gpu.target_share
                .max(streams_config.etc.target_share)
                .clamp(0.0, 0.40)
        } else {
            0.0
        };
        let ncl_share = if has_ncl {
            streams_config.ncl.target_share.clamp(0.0, 0.40)
        } else {
            0.0
        };

        // Normalize so they sum to 1.0
        let total = zion_share + revenue_share + ncl_share;
        let (zion_share, revenue_share, ncl_share) = if total > 0.01 {
            (zion_share / total, revenue_share / total, ncl_share / total)
        } else {
            (1.0, 0.0, 0.0)
        };

        // CH3 Rule: CPU-only → default to XMR (RandomX). GPU → default to ERG.
        let default_coin = if cpu_only { "XMR" } else { "ERG" };

        info!("╔════════════════════════════════════════════════════════╗");
        info!("║     CH v3 STREAM SCHEDULER — 50/25/25 MODEL           ║");
        info!("╠════════════════════════════════════════════════════════╣");
        info!("║  ZION (CosmicHarmony):  {:>5.1}% compute                ║", zion_share * 100.0);
        info!("║  Multi-Algo Revenue:    {:>5.1}% compute                ║", revenue_share * 100.0);
        info!("║  NCL AI Inference:      {:>5.1}% compute                ║", ncl_share * 100.0);
        info!("║  ETC/Keccak Export:     FREE (ZION byproduct)         ║");
        info!("║  NXS/SHA3 Export:       FREE (ZION byproduct)         ║");
        info!("║  Revenue Coin Default:  {} ({})          ║", default_coin,
            if cpu_only { "CPU-only" } else { "GPU mode" });
        info!("╚════════════════════════════════════════════════════════╝");

        if has_external || has_ncl {
            info!("📊 Mode: auto (TimeSplit <4 miners, PerMiner ≥4 miners)");
        } else {
            info!("📊 Mode: ZION-only (no external/NCL streams enabled)");
        }

        Self {
            zion_share,
            revenue_share,
            ncl_share,
            zion_job: RwLock::new(None),
            external_jobs: RwLock::new(HashMap::new()),
            best_coin: RwLock::new(default_coin.to_string()),
            miner_groups: RwLock::new(HashMap::new()),
            mode: RwLock::new(SchedulerMode::TimeSplit),
            timesplit_on_zion: RwLock::new(true),
            timesplit_last_switch: RwLock::new(Instant::now()),
            zion_time_secs: RwLock::new(0.0),
            revenue_time_secs: RwLock::new(0.0),
            ncl_time_secs: RwLock::new(0.0),
            timesplit_phase: RwLock::new(0),
            revenue_proxy,
            job_coin_map: RwLock::new(HashMap::new()),
            mode_notify: Notify::new(),
            total_miners: RwLock::new(0),
            cpu_only: AtomicBool::new(cpu_only),
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Job Updates
    // ═══════════════════════════════════════════════════════════

    /// Update the ZION job (called from BlockTemplateManager on_template_change)
    pub async fn update_zion_job(&self, job: ScheduledJob) {
        debug!("📦 StreamScheduler: New ZION job height={}", job.height);
        self.register_job("ZION", &job.job_id).await;
        *self.zion_job.write().await = Some(job);
    }

    /// Update an external pool job (called from revenue_proxy broadcast listener)
    pub async fn update_external_job(&self, external: ExternalJob) {
        let coin = external.coin.to_lowercase();

        let stream_id = match coin.as_str() {
            "etc" => StreamId::Etc,
            c => StreamId::DynamicGpu(c.to_string()),
        };

        let job_id = format!("ext-{}-{}", coin, external.job_id);

        let job = ScheduledJob {
            stream_id,
            job_id: job_id.clone(),
            blob: if external.blob.is_empty() { external.header_hash.clone() } else { external.blob.clone() },
            target: external.target.clone(),
            difficulty: external.difficulty,
            height: external.height,
            algorithm: external.algorithm.clone(),
            coin: external.coin.clone(),
            clean_jobs: external.clean_jobs,
            extranonce: external.extranonce.clone(),
            raw_notify_params: external.raw_params.clone(),
            seed_hash: external.seed_hash.clone(),
            created_at: Instant::now(),
        };

        debug!("📦 StreamScheduler: New {} job id={}", coin.to_uppercase(), job_id);
        self.register_job(&coin, &job_id).await;
        self.external_jobs.write().await.insert(coin, job);
    }

    // ═══════════════════════════════════════════════════════════
    // ProfitSwitcher Integration
    // ═══════════════════════════════════════════════════════════

    /// Called when ProfitSwitcher changes the best coin.
    /// Only the Revenue group needs to switch — ZION group is unaffected.
    pub async fn set_best_coin(&self, coin: &str) -> Option<ScheduledJob> {
        let old = self.best_coin.read().await.clone();
        let new_coin = coin.to_lowercase();

        if old.to_lowercase() == new_coin {
            return None;
        }

        info!("💹 StreamScheduler: Best coin changed {} → {} — Revenue miners will switch",
            old.to_uppercase(), new_coin.to_uppercase());

        *self.best_coin.write().await = new_coin.clone();

        // Return the new job for broadcasting to Revenue miners
        self.get_revenue_job().await
    }

    // ═══════════════════════════════════════════════════════════
    // Miner Assignment (Per-Miner mode)
    // ═══════════════════════════════════════════════════════════

    /// Register a new miner and assign it to a group (50/25/25 model).
    /// Returns the group and the job to send immediately.
    pub async fn register_miner(&self, session_id: &str) -> (MinerGroup, Option<ScheduledJob>) {
        let mut groups = self.miner_groups.write().await;

        let zion_count = groups.values().filter(|g| **g == MinerGroup::Zion).count();
        let revenue_count = groups.values().filter(|g| **g == MinerGroup::Revenue).count();
        let ncl_count = groups.values().filter(|g| **g == MinerGroup::Ncl).count();
        let total = zion_count + revenue_count + ncl_count + 1; // +1 for this new miner

        // Determine group: maintain 50/25/25 ratio
        let target_zion = (total as f64 * self.zion_share).ceil() as usize;
        let target_revenue = (total as f64 * self.revenue_share).ceil() as usize;
        // NCL gets remainder

        let group = if zion_count < target_zion {
            MinerGroup::Zion
        } else if revenue_count < target_revenue && self.revenue_share > 0.0 {
            MinerGroup::Revenue
        } else if self.ncl_share > 0.0 {
            MinerGroup::Ncl
        } else if self.revenue_share > 0.0 {
            MinerGroup::Revenue
        } else {
            MinerGroup::Zion
        };

        groups.insert(session_id.to_string(), group.clone());
        *self.total_miners.write().await = total;

        // Update mode — need ≥4 miners for per-miner mode with 3 groups
        let new_mode = if total >= 4 {
            SchedulerMode::PerMiner
        } else {
            SchedulerMode::TimeSplit
        };
        let old_mode = self.mode.read().await.clone();
        if new_mode != old_mode {
            info!("📊 StreamScheduler: Mode changed to {:?} ({} miners)", new_mode, total);
            *self.mode.write().await = new_mode;
        }

        let sid_short = &session_id[..8.min(session_id.len())];
        let new_zion = zion_count + if group == MinerGroup::Zion { 1 } else { 0 };
        let new_rev = revenue_count + if group == MinerGroup::Revenue { 1 } else { 0 };
        let new_ncl = ncl_count + if group == MinerGroup::Ncl { 1 } else { 0 };
        info!("📊 Miner {} → {:?} group (ZION:{}, Revenue:{}, NCL:{}, total:{})",
            sid_short, group, new_zion, new_rev, new_ncl, total);

        // Get the right job for this miner
        let job = match &group {
            MinerGroup::Zion => self.zion_job.read().await.clone(),
            MinerGroup::Revenue => self.get_revenue_job().await,
            MinerGroup::Ncl => self.zion_job.read().await.clone(), // NCL miners get ZION jobs when not doing AI work
        };

        (group, job)
    }

    /// Unregister a miner (disconnected)
    pub async fn unregister_miner(&self, session_id: &str) {
        let mut groups = self.miner_groups.write().await;
        groups.remove(session_id);
        let total = groups.len();
        *self.total_miners.write().await = total;

        let new_mode = if total >= 4 {
            SchedulerMode::PerMiner
        } else {
            SchedulerMode::TimeSplit
        };
        *self.mode.write().await = new_mode;
    }

    /// Get which group a miner belongs to
    pub async fn get_miner_group(&self, session_id: &str) -> MinerGroup {
        self.miner_groups.read().await
            .get(session_id)
            .cloned()
            .unwrap_or(MinerGroup::Zion)
    }

    /// Rebalance miner assignments to maintain 50/25/25 target ratio.
    /// Returns list of (session_id, new_group, job_to_send) for miners that changed.
    pub async fn rebalance(&self) -> Vec<(String, MinerGroup, Option<ScheduledJob>)> {
        let mut groups = self.miner_groups.write().await;
        let total = groups.len();
        if total == 0 {
            return Vec::new();
        }

        let target_zion = (total as f64 * self.zion_share).ceil() as usize;
        let target_revenue = (total as f64 * self.revenue_share).ceil() as usize;
        // NCL gets the remainder
        let zion_count = groups.values().filter(|g| **g == MinerGroup::Zion).count();
        let revenue_count = groups.values().filter(|g| **g == MinerGroup::Revenue).count();

        let mut changes = Vec::new();

        // Rebalance ZION group
        if zion_count > target_zion {
            let to_move = zion_count - target_zion;
            let mut moved = 0;
            for (sid, group) in groups.iter_mut() {
                if moved >= to_move { break; }
                if *group == MinerGroup::Zion {
                    // Move excess ZION miners to whichever group needs them most
                    if revenue_count < target_revenue && self.revenue_share > 0.0 {
                        *group = MinerGroup::Revenue;
                        changes.push((sid.clone(), MinerGroup::Revenue, None));
                    } else if self.ncl_share > 0.0 {
                        *group = MinerGroup::Ncl;
                        changes.push((sid.clone(), MinerGroup::Ncl, None));
                    }
                    moved += 1;
                }
            }
        } else if zion_count < target_zion {
            let to_move = target_zion - zion_count;
            let mut moved = 0;
            for (sid, group) in groups.iter_mut() {
                if moved >= to_move { break; }
                if *group != MinerGroup::Zion {
                    *group = MinerGroup::Zion;
                    changes.push((sid.clone(), MinerGroup::Zion, None));
                    moved += 1;
                }
            }
        }

        drop(groups);

        // Fill in jobs for changed miners
        let zion_job = self.zion_job.read().await.clone();
        let revenue_job = self.get_revenue_job().await;
        for change in changes.iter_mut() {
            change.2 = match change.1 {
                MinerGroup::Zion => zion_job.clone(),
                MinerGroup::Revenue => revenue_job.clone(),
                MinerGroup::Ncl => zion_job.clone(), // NCL gets ZION jobs when not doing AI
            };
        }

        if !changes.is_empty() {
            info!("📊 StreamScheduler: Rebalanced {} miners", changes.len());
        }

        changes
    }

    // ═══════════════════════════════════════════════════════════
    // Time-Split Mode (< 4 miners) — 50/25/25 round-robin
    // ═══════════════════════════════════════════════════════════

    /// Check if it's time to switch in time-split mode (3-phase: ZION→Revenue→NCL).
    /// Returns Some(ScheduledJob) if a new job should be broadcast to ALL miners.
    pub async fn maybe_switch(&self) -> Option<ScheduledJob> {
        let mode = self.mode.read().await.clone();
        if mode != SchedulerMode::TimeSplit {
            return None;
        }

        let now = Instant::now();
        let elapsed = {
            let last = self.timesplit_last_switch.read().await;
            now.duration_since(*last).as_secs_f64()
        };

        // Minimum 10s stint per phase
        if elapsed < 10.0 {
            return None;
        }

        let phase = *self.timesplit_phase.read().await;

        // Accumulate time for current phase
        match phase {
            0 => *self.zion_time_secs.write().await += elapsed,
            1 => *self.revenue_time_secs.write().await += elapsed,
            2 => *self.ncl_time_secs.write().await += elapsed,
            _ => *self.zion_time_secs.write().await += elapsed,
        }
        *self.timesplit_last_switch.write().await = now;

        let z_time = *self.zion_time_secs.read().await;
        let r_time = *self.revenue_time_secs.read().await;
        let n_time = *self.ncl_time_secs.read().await;
        let total = z_time + r_time + n_time;

        if total < 1.0 {
            return None;
        }

        // Calculate actual shares vs targets
        let actual_zion = z_time / total;
        let actual_revenue = r_time / total;
        let actual_ncl = n_time / total;

        // Find which group is most starved (biggest deficit)
        let zion_deficit = self.zion_share - actual_zion;
        let revenue_deficit = self.revenue_share - actual_revenue;
        let ncl_deficit = self.ncl_share - actual_ncl;

        // Only switch if some phase has > 2% deficit
        let max_deficit = zion_deficit.max(revenue_deficit).max(ncl_deficit);
        if max_deficit < 0.02 {
            return None;
        }

        // Switch to the most starved phase
        let next_phase = if zion_deficit >= revenue_deficit && zion_deficit >= ncl_deficit {
            0 // ZION needs more time
        } else if revenue_deficit >= ncl_deficit && self.revenue_share > 0.0 {
            1 // Revenue needs more time
        } else if self.ncl_share > 0.0 {
            2 // NCL needs more time
        } else {
            0 // Fallback to ZION
        };

        if next_phase == phase {
            return None;
        }

        *self.timesplit_phase.write().await = next_phase;
        *self.timesplit_on_zion.write().await = next_phase == 0;

        let phase_name = match next_phase {
            0 => "ZION",
            1 => {
                let best = self.best_coin.read().await.clone();
                // Use a leaked str to avoid lifetime issues in the log
                let name = format!("Revenue:{}", best.to_uppercase());
                info!("🔄 TimeSplit: → {} (Z:{:.0}% R:{:.0}% N:{:.0}%)",
                    name, actual_zion * 100.0, actual_revenue * 100.0, actual_ncl * 100.0);
                return self.get_revenue_job().await;
            }
            _ => "NCL",
        };

        info!("🔄 TimeSplit: → {} (Z:{:.0}% R:{:.0}% N:{:.0}%)",
            phase_name, actual_zion * 100.0, actual_revenue * 100.0, actual_ncl * 100.0);

        // NCL miners get ZION jobs when not doing AI work (AI is handled separately)
        self.zion_job.read().await.clone()
    }

    // ═══════════════════════════════════════════════════════════
    // Job Retrieval
    // ═══════════════════════════════════════════════════════════

    /// Get the current best-profit revenue job
    pub async fn get_revenue_job(&self) -> Option<ScheduledJob> {
        let best = self.best_coin.read().await.clone().to_lowercase();
        let jobs = self.external_jobs.read().await;

        // Try best-profit coin first
        if let Some(job) = jobs.get(&best) {
            return Some(job.clone());
        }

        // CPU-only mode: ONLY accept XMR (RandomX) — never fallback to GPU coins
        if self.cpu_only.load(Ordering::Relaxed) {
            if let Some(job) = jobs.get("xmr") {
                info!(
                    "📦 Revenue: best_coin={} unavailable, using XMR (CPU-only mode)",
                    best.to_uppercase()
                );
                return Some(job.clone());
            }
            // No XMR job available yet — return None (don't send GPU coin to CPU miner!)
            debug!("📦 Revenue: No XMR job available yet (CPU-only mode, waiting...)");
            return None;
        }

        // GPU mode fallback: use ANY available external job if best coin has no job yet
        // This handles the case where ProfitSwitcher says "RVN" but we only have
        // an ETC connection active — mine ETC instead of mining nothing
        if let Some((fallback_coin, job)) = jobs.iter().next() {
            info!(
                "📦 Revenue fallback: best_coin={} unavailable, using {} job instead",
                best.to_uppercase(), fallback_coin.to_uppercase()
            );
            return Some(job.clone());
        }

        None
    }

    /// Get current job for a specific miner (respects 50/25/25 model)
    pub async fn get_job_for_miner(&self, session_id: &str) -> Option<ScheduledJob> {
        // If ZION-only mode (100% share), always return ZION job
        if self.zion_share >= 0.99 {
            return self.zion_job.read().await.clone();
        }

        let mode = self.mode.read().await.clone();

        match mode {
            SchedulerMode::TimeSplit => {
                let phase = *self.timesplit_phase.read().await;
                match phase {
                    1 if self.revenue_share > 0.0 => self.get_revenue_job().await,
                    // Phase 0 (ZION) and Phase 2 (NCL) both get ZION jobs
                    // NCL AI work is handled by a separate NCL subsystem, not mining jobs
                    _ => self.zion_job.read().await.clone(),
                }
            }
            SchedulerMode::PerMiner => {
                let group = self.get_miner_group(session_id).await;
                match group {
                    MinerGroup::Zion => self.zion_job.read().await.clone(),
                    MinerGroup::Revenue => self.get_revenue_job().await,
                    MinerGroup::Ncl => self.zion_job.read().await.clone(), // NCL gets ZION when not doing AI
                }
            }
        }
    }

    /// Get the current job (backward compat — for time-split broadcasts)
    pub async fn current_job(&self) -> Option<ScheduledJob> {
        // If ZION-only mode, always return ZION job
        if self.zion_share >= 0.99 {
            return self.zion_job.read().await.clone();
        }

        let phase = *self.timesplit_phase.read().await;
        match phase {
            1 if self.revenue_share > 0.0 => self.get_revenue_job().await,
            _ => self.zion_job.read().await.clone(),
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Share Routing
    // ═══════════════════════════════════════════════════════════

    /// Route a share submission to the correct pool
    pub async fn route_share(&self, job_id: &str, nonce: &str, worker: &str, result: &str) -> ShareRoute {
        let map = self.job_coin_map.read().await;
        if let Some(coin) = map.get(job_id) {
            let coin = coin.clone();
            drop(map);

            if coin.eq_ignore_ascii_case("ZION") {
                return ShareRoute::Zion;
            }

            return self.forward_to_external(&coin, job_id, nonce, worker, result).await;
        }
        drop(map);

        // Fallback: detect from job_id prefix
        if job_id.starts_with("ext-") {
            let parts: Vec<&str> = job_id.splitn(3, '-').collect();
            if parts.len() >= 2 {
                let coin = parts[1].to_string();
                return self.forward_to_external(&coin, job_id, nonce, worker, result).await;
            }
        }

        ShareRoute::Zion
    }

    /// Forward share to external pool
    async fn forward_to_external(&self, coin: &str, job_id: &str, nonce: &str, worker: &str, result: &str) -> ShareRoute {
        if let Some(proxy) = &self.revenue_proxy {
            let prefix = format!("ext-{}-", coin.to_lowercase());
            let original_job_id = job_id
                .strip_prefix(&prefix)
                .unwrap_or(job_id)
                .to_string();

            proxy.submit_share(ShareSubmission {
                coin: coin.to_lowercase(),
                job_id: original_job_id,
                nonce: nonce.to_string(),
                worker: worker.to_string(),
                result: result.to_string(),
                algorithm: String::new(),  // proxy will use whatever the current job algo is
            }).await;

            ShareRoute::External(coin.to_lowercase())
        } else {
            ShareRoute::Zion
        }
    }

    /// Check if a job_id belongs to an external stream
    pub fn is_external_job(job_id: &str) -> bool {
        job_id.starts_with("ext-")
    }

    // ═══════════════════════════════════════════════════════════
    // Background Tasks
    // ═══════════════════════════════════════════════════════════

    /// Start listening to external job broadcast from RevenueProxyManager
    pub async fn listen_external_jobs(self: Arc<Self>, mut rx: broadcast::Receiver<ExternalJob>) {
        info!("👂 StreamScheduler: Listening for external pool jobs");

        loop {
            match rx.recv().await {
                Ok(job) => {
                    self.update_external_job(job).await;
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    warn!("StreamScheduler: Missed {} external jobs (lagged)", n);
                }
                Err(broadcast::error::RecvError::Closed) => {
                    warn!("StreamScheduler: External job channel closed");
                    break;
                }
            }
        }
    }

    /// Listen for ProfitSwitcher coin changes
    pub async fn listen_profit_changes(self: Arc<Self>, mut coin_rx: watch::Receiver<String>) {
        info!("👂 StreamScheduler: Listening for ProfitSwitcher changes");

        loop {
            if coin_rx.changed().await.is_err() {
                warn!("StreamScheduler: ProfitSwitcher channel closed");
                break;
            }

            let new_coin = coin_rx.borrow().clone();
            if let Some(_new_job) = self.set_best_coin(&new_coin).await {
                info!("📢 StreamScheduler: Revenue miners switching to {}", new_coin.to_uppercase());
                self.mode_notify.notify_waiters();
            }
        }
    }

    // ═══════════════════════════════════════════════════════════
    // Stats & API
    // ═══════════════════════════════════════════════════════════

    /// Get scheduling statistics as JSON (50/25/25 model)
    pub async fn stats_json(&self) -> serde_json::Value {
        let mode = self.mode.read().await.clone();
        let best = self.best_coin.read().await.clone();
        let groups = self.miner_groups.read().await;
        let z_time = *self.zion_time_secs.read().await;
        let r_time = *self.revenue_time_secs.read().await;
        let n_time = *self.ncl_time_secs.read().await;
        let total_time = z_time + r_time + n_time;
        let phase = *self.timesplit_phase.read().await;
        let total_miners = *self.total_miners.read().await;

        let zion_miners = groups.values().filter(|g| **g == MinerGroup::Zion).count();
        let revenue_miners = groups.values().filter(|g| **g == MinerGroup::Revenue).count();
        let ncl_miners = groups.values().filter(|g| **g == MinerGroup::Ncl).count();

        let has_zion_job = self.zion_job.read().await.is_some();
        let has_revenue_job = self.get_revenue_job().await.is_some();

        let actual_zion_pct = if total_time > 0.0 { z_time / total_time * 100.0 } else { 0.0 };
        let actual_revenue_pct = if total_time > 0.0 { r_time / total_time * 100.0 } else { 0.0 };
        let actual_ncl_pct = if total_time > 0.0 { n_time / total_time * 100.0 } else { 0.0 };

        let ext_jobs = self.external_jobs.read().await;
        let available_coins: Vec<String> = ext_jobs.keys()
            .map(|k| k.to_uppercase())
            .collect();

        let phase_name = match phase {
            0 => "ZION",
            1 => "Revenue",
            2 => "NCL",
            _ => "ZION",
        };

        serde_json::json!({
            "version": "v3-50/25/25",
            "model": "50% ZION + 25% Multi-Algo + 25% NCL AI",
            "mode": format!("{:?}", mode),
            "target_allocation": {
                "zion": format!("{:.0}%", self.zion_share * 100.0),
                "revenue": format!("{:.0}%", self.revenue_share * 100.0),
                "ncl": format!("{:.0}%", self.ncl_share * 100.0),
            },
            "best_profit_coin": best.to_uppercase(),
            "available_coins": available_coins,
            "miners": {
                "total": total_miners,
                "zion_group": zion_miners,
                "revenue_group": revenue_miners,
                "ncl_group": ncl_miners,
            },
            "jobs": {
                "zion_available": has_zion_job,
                "revenue_available": has_revenue_job,
            },
            "time_split": {
                "current_phase": phase_name,
                "zion_actual": format!("{:.1}%", actual_zion_pct),
                "revenue_actual": format!("{:.1}%", actual_revenue_pct),
                "ncl_actual": format!("{:.1}%", actual_ncl_pct),
                "zion_secs": format!("{:.0}", z_time),
                "revenue_secs": format!("{:.0}", r_time),
                "ncl_secs": format!("{:.0}", n_time),
            },
            "free_byproducts": {
                "etc_keccak": "Keccak intermediate → ETC pool (FREE, no extra compute)",
                "nxs_sha3": "SHA3 intermediate → Nexus pool (FREE, no extra compute)",
            },
        })
    }

    // ═══════════════════════════════════════════════════════════
    // Helpers
    // ═══════════════════════════════════════════════════════════

    async fn register_job(&self, coin: &str, job_id: &str) {
        let mut map = self.job_coin_map.write().await;
        map.insert(job_id.to_string(), coin.to_lowercase());
        if map.len() > 1000 {
            let keys: Vec<String> = map.keys().take(500).cloned().collect();
            for k in keys {
                map.remove(&k);
            }
        }
    }

    /// Get all session IDs in Revenue group (for targeted broadcasting)
    pub async fn get_revenue_miners(&self) -> Vec<String> {
        self.miner_groups.read().await
            .iter()
            .filter(|(_, g)| **g == MinerGroup::Revenue)
            .map(|(sid, _)| sid.clone())
            .collect()
    }

    /// Get all session IDs in ZION group
    pub async fn get_zion_miners(&self) -> Vec<String> {
        self.miner_groups.read().await
            .iter()
            .filter(|(_, g)| **g == MinerGroup::Zion)
            .map(|(sid, _)| sid.clone())
            .collect()
    }
}