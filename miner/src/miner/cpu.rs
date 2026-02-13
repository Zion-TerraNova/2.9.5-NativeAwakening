use anyhow::Result;
use hex::FromHex;
use std::sync::{Arc, RwLock};
use tokio::sync::{mpsc, RwLock as AsyncRwLock};

use super::native_algos;
use super::stats::MinerStats;
use super::Algorithm;
use crate::stratum::{Job, StratumClient};

#[derive(Debug, Clone)]
struct PendingShare {
    job_id: String,
    nonce: u32,
    result_hex: String,
}

pub struct CpuMiner {
    algorithm: Algorithm,
    threads: usize,
    stats: Arc<AsyncRwLock<MinerStats>>,
    job_state: Arc<RwLock<Option<Job>>>,
    stratum: Arc<StratumClient>,
    /// Shared flag — set to true when connection is lost, mining loop checks this
    connection_alive: Arc<std::sync::atomic::AtomicBool>,
}

impl CpuMiner {
    pub fn new(
        algorithm: Algorithm,
        threads: usize,
        stats: Arc<AsyncRwLock<MinerStats>>,
        job_state: Arc<RwLock<Option<Job>>>,
        stratum: Arc<StratumClient>,
    ) -> Self {
        Self {
            algorithm,
            threads,
            stats,
            job_state,
            stratum,
            connection_alive: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        }
    }

    /// Returns a clone of the connection_alive flag so the caller can set it to false
    pub fn connection_alive_flag(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.connection_alive)
    }

    pub async fn start(&self) -> Result<()> {
        log::debug!("cpu {} threads — stream-aware mining started", self.threads);

        let (share_tx, mut share_rx) = mpsc::channel::<PendingShare>(1024);

        // Async submit loop (prevents thread explosion)
        {
            let stratum = Arc::clone(&self.stratum);
            let stats = Arc::clone(&self.stats);
            let alive = Arc::clone(&self.connection_alive);
            let job_state_submit = Arc::clone(&self.job_state);
            tokio::spawn(async move {
                log::debug!("Share submit loop started");
                let mut share_count = 0u64;
                let mut consecutive_errors = 0u32;
                let mut stale_dropped = 0u64;
                while let Some(share) = share_rx.recv().await {
                    // Bug fix: drop stale shares from old jobs before submitting.
                    // After a job switch, pending shares in the queue have the old job_id.
                    // Pool rejects them ("Does not meet target difficulty"), which triggers
                    // consecutive_errors and kills the submit loop. Skip them instead.
                    {
                        let current_job_id = job_state_submit
                            .read()
                            .expect("job_state poisoned")
                            .as_ref()
                            .map(|j| j.job_id.clone());
                        if current_job_id.as_deref() != Some(&share.job_id) {
                            stale_dropped += 1;
                            log::debug!(
                                "📤 Dropping stale share #{}: job={} (current={})",
                                stale_dropped,
                                share.job_id,
                                current_job_id.unwrap_or_default()
                            );
                            continue;
                        }
                    }
                    // Check if connection is still alive
                    if !stratum.is_connected() {
                        alive.store(false, std::sync::atomic::Ordering::Relaxed);
                        log::debug!("net connection lost");
                        break;
                    }
                    share_count += 1;
                    log::debug!("Submitting share #{}: job={}, nonce={}", share_count, share.job_id, share.nonce);
                    let accepted = match stratum
                        .submit_share(&share.job_id, share.nonce, &share.result_hex)
                        .await
                    {
                        Ok(v) => {
                            consecutive_errors = 0;
                            log::debug!("Share #{} result: accepted={}", share_count, v);
                            v
                        },
                        Err(e) => {
                            consecutive_errors += 1;
                            log::debug!("net submit error #{}: {}", consecutive_errors, e);
                            if consecutive_errors >= 3 {
                                log::debug!("too many errors — reconnecting");
                                alive.store(false, std::sync::atomic::Ordering::Relaxed);
                                break;
                            }
                            false
                        }
                    };

                    let mut stats_guard = stats.write().await;
                    if accepted {
                        stats_guard.share_accepted();
                        stats_guard.print_accepted();
                    } else {
                        stats_guard.share_rejected();
                        stats_guard.print_rejected("low difficulty share");
                    }
                }
                log::debug!("Share submit loop ended");
            });
        }

        // Mining threads — spawn N blocking tasks for parallelism.
        // Each thread gets a non-overlapping nonce range and its own
        // thread-local RandomX VM.  For CH/single-algo, threads share the
        // job state and submit channel.
        let num_threads = self.threads.max(1);
        let nonce_space_per_thread = (u32::MAX as u64 + 1) / num_threads as u64;

        for thread_idx in 0..num_threads {
            let algorithm = self.algorithm;
            let stats = Arc::clone(&self.stats);
            let job_state = Arc::clone(&self.job_state);
            let alive = Arc::clone(&self.connection_alive);
            let share_tx = share_tx.clone();
            let nonce_offset = (thread_idx as u64 * nonce_space_per_thread) as u32;

            tokio::task::spawn_blocking(move || {
                Self::mining_loop(algorithm, stats, job_state, share_tx, alive, thread_idx, nonce_offset);
            });
        }
        // Drop our copy so the channel closes when all threads exit
        drop(share_tx);

        Ok(())
    }

    fn mining_loop(
        algorithm: Algorithm,
        stats: Arc<AsyncRwLock<MinerStats>>,
        job_state: Arc<RwLock<Option<Job>>>,
        share_tx: mpsc::Sender<PendingShare>,
        connection_alive: Arc<std::sync::atomic::AtomicBool>,
        thread_idx: usize,
        nonce_offset: u32,
    ) {
        let stream_switch_enabled = std::env::var("ZION_ENABLE_STREAM_SWITCH")
            .map(|v| {
                let v = v.trim().to_ascii_lowercase();
                v == "1" || v == "true" || v == "yes"
            })
            .unwrap_or(false);

        let mut nonce_start = nonce_offset;
        // Active algorithm — can change dynamically when pool switches stream
        let mut active_algorithm = algorithm;
        let mut batch_size = Self::batch_size_for_algo(active_algorithm);
        let mut last_job_id: Option<String> = None;
        let mut last_seed_hash: Option<String> = None;
        let mut compute_error_logged = false;
        let mut last_stats_flush = std::time::Instant::now();
        // Per-blob nonce bookmarks: when we switch away from a job, save our
        // nonce position so we can resume where we left off when we return.
        // Without this, stream switches (CH→RandomX→CH) reset nonce to 0 and
        // cause Duplicate Share rejections because the same nonces get re-hashed.
        let mut nonce_bookmarks: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

        loop {
            let job = {
                let guard = job_state.read().expect("job_state poisoned");
                guard.clone()
            };

            let Some(job) = job else {
                std::thread::sleep(std::time::Duration::from_millis(250));
                continue;
            };

            if last_job_id.as_deref() != Some(job.job_id.as_str()) {
                // Save current nonce position for the old job — we may return
                // to it after a stream switch (e.g. CH → RandomX → CH).
                if let Some(old_id) = last_job_id.take() {
                    let bk_key = Self::bookmark_key(&old_id);
                    nonce_bookmarks.insert(bk_key, nonce_start);
                }

                // For RandomX: external pools (MoneroOcean) send a new job_id
                // every ~30s even though seed_hash stays the same.  We must NOT
                // reset nonce to 0 each time or we'll keep re-hashing the same
                // nonces and never cover enough search space to find a share.
                // Only reset nonce when the seed_hash actually changes (new block).
                let same_seed = matches!(active_algorithm, Algorithm::RandomX)
                    && last_seed_hash.as_deref() == job.seed_hash.as_deref().filter(|s| !s.is_empty());
                if !same_seed {
                    // Check if we have a saved bookmark for this job
                    let bk_key = Self::bookmark_key(&job.job_id);
                    nonce_start = nonce_bookmarks
                        .get(bk_key.as_str())
                        .copied()
                        .unwrap_or(0);
                    log::debug!("nonce resume: job={} bk_key={} → start={}", job.job_id, bk_key, nonce_start);
                }
                last_job_id = Some(job.job_id.clone());
                last_seed_hash = job.seed_hash.clone();
                compute_error_logged = false;

                // ═══ Stream Scheduler v2: Dynamic algorithm detection (opt-in) ═══
                // Default for desktop miner: keep the configured algo pinned (CHv3),
                // so hashrate doesn't collapse when pool sends Revenue stream jobs.
                // Set ZION_ENABLE_STREAM_SWITCH=1 to enable dynamic switching.
                if stream_switch_enabled {
                    let job_algo = job.algo.as_deref()
                        .and_then(Algorithm::from_str)
                        .unwrap_or(algorithm);

                    if job_algo != active_algorithm {
                        // CPU-only mode: if pool sends a GPU-only algo, replace with RandomX
                        let effective_algo = if matches!(job_algo,
                            Algorithm::Ethash | Algorithm::KawPow | Algorithm::Autolykos |
                            Algorithm::KHeavyHash | Algorithm::ProgPow
                        ) {
                            // Check if GPU is available — if not, use RandomX instead
                            if std::env::var("ZION_HAS_GPU").map(|v| v == "1" || v.to_lowercase() == "true").unwrap_or(false) {
                                job_algo // GPU available, keep original
                            } else {
                                log::debug!("cpu:switch {} is GPU-only → RandomX", job_algo.name());
                                Algorithm::RandomX
                            }
                        } else {
                            job_algo
                        };

                        log::debug!("cpu:switch {} → {}", active_algorithm.name(), effective_algo.name());
                        active_algorithm = effective_algo;
                        batch_size = Self::batch_size_for_algo(active_algorithm);

                        // Re-initialize algorithm-specific state if needed
                        if active_algorithm == Algorithm::RandomX {
                            // Use seed_hash from pool job (MoneroOcean/CryptoNote sends
                            // the proper seed_hash for RandomX dataset initialization)
                            let seed_key = job.seed_hash.as_deref()
                                .filter(|s| !s.is_empty() && s.len() >= 16)
                                .and_then(|s| hex::decode(s).ok())
                                .unwrap_or_else(|| b"ZION_RANDOMX_TESTNET_2026".to_vec());

                            if let Err(e) = native_algos::init_randomx_with_key(&seed_key) {
                                log::debug!("randomx init failed: {}", e);
                            }
                        }
                    }
                }

                let t = job.target.trim();
                if matches!(active_algorithm, Algorithm::CosmicHarmony) {
                    let tu = parse_cosmic_target_to_u32(t);
                    let endian = job
                        .cosmic_state0_endian
                        .as_deref()
                        .unwrap_or("little");
                    log::debug!(
                        "New mining job: id={}, height={}, algo={}, target='{}' (u32=0x{:08x}) endian={}",
                        job.job_id,
                        job.height,
                        active_algorithm.name(),
                        t,
                        tu,
                        endian
                    );
                } else {
                    log::debug!(
                        "New mining job: id={}, height={}, algo={}, target='{}'",
                        job.job_id,
                        job.height,
                        active_algorithm.name(),
                        t
                    );
                }
            }

            let blob_hex = job.blob.trim_start_matches("0x");
            let blob_bytes = match Vec::from_hex(blob_hex) {
                Ok(b) => b,
                Err(e) => {
                    log::debug!("Failed to parse blob hex: {}", e);
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    continue;
                }
            };

            let nonce_end = nonce_start.wrapping_add(batch_size);
            let target_hex = job.target.clone();

            let start_time = std::time::Instant::now();
            let mut hashes_count: u64 = 0;
            let mut shares_found: u32 = 0;
            let mut hashes_pending_stats: u64 = 0;
            let mut hit_unsupported = false;

            // ═══ RandomX fast path: batch hashing with pipeline mode ═══
            // calculate_hash_set uses RandomX's internal first/next pipeline
            // for ~1.5-2× throughput vs sequential calculate_hash calls.
            if matches!(active_algorithm, Algorithm::RandomX) {
                let t0 = std::time::Instant::now();
                match native_algos::compute_hash_batch_randomx(
                    &blob_bytes,
                    nonce_start as u64,
                    batch_size,
                ) {
                    Ok(results) => {
                        if thread_idx == 0 {
                            let first_ms = t0.elapsed().as_millis();
                            let rate = results.len() as f64 / t0.elapsed().as_secs_f64();
                            log::debug!(
                                "RandomX batch: {} hashes in {}ms ({:.1} H/s) [T{}]",
                                results.len(), first_ms, rate, thread_idx
                            );
                        }
                        for (nonce, hash) in &results {
                            hashes_count += 1;
                            hashes_pending_stats += 1;

                            if hashes_pending_stats >= 64
                                || last_stats_flush.elapsed() >= std::time::Duration::from_secs(1)
                            {
                                let mut stats_guard = stats.blocking_write();
                                stats_guard.add_hashes(hashes_pending_stats);
                                hashes_pending_stats = 0;
                                last_stats_flush = std::time::Instant::now();
                            }

                            if Self::meets_target(active_algorithm, hash, &target_hex, job.cosmic_state0_endian.as_deref()) {
                                shares_found = shares_found.saturating_add(1);
                                let pending = PendingShare {
                                    job_id: job.job_id.clone(),
                                    nonce: *nonce as u32,
                                    result_hex: hex::encode(hash),
                                };
                                let _ = share_tx.try_send(pending);
                            }
                        }
                    }
                    Err(e) => {
                        if !compute_error_logged {
                            log::error!("❌ RandomX batch error [T{}]: {}", thread_idx, e);
                            compute_error_logged = true;
                        }
                        let msg = e.to_string();
                        if msg.contains("not compiled") || msg.contains("not supported") {
                            hit_unsupported = true;
                        }
                    }
                }
            } else {
            // ═══ Standard path: sequential hashing for CH/Yescrypt/etc ═══

            for nonce in nonce_start..nonce_end {
                let native_algo = active_algorithm.to_native();
                let is_first = nonce == nonce_start && thread_idx == 0;
                let t0 = if is_first && matches!(active_algorithm, Algorithm::RandomX) {
                    log::debug!("RandomX first hash: starting...");
                    Some(std::time::Instant::now())
                } else {
                    None
                };

                let hash_vec = match native_algos::compute_hash(
                    native_algo,
                    &blob_bytes,
                    nonce as u64,
                    job.height as u32,
                ) {
                    Ok(h) => {
                        if let Some(t0) = t0 {
                            log::debug!(
                                "RandomX first hash: OK in {:?} (len={})",
                                t0.elapsed(),
                                h.len()
                            );
                        }
                        h
                    }
                    Err(e) => {
                        if let Some(t0) = t0 {
                            log::error!(
                                "🧪 RandomX first hash: ERROR in {:?}: {} (algo={:?})",
                                t0.elapsed(),
                                e,
                                native_algo
                            );
                        } else if !compute_error_logged {
                            // Log once per job to avoid flooding.
                            log::error!("❌ compute_hash error: {} (algo={:?})", e, native_algo);
                            compute_error_logged = true;
                        }

                        // If algo is not compiled/supported, don't spin hot.
                        // Break the batch and sleep a bit; the algorithm selection won't change.
                        let msg = e.to_string();
                        if msg.contains("not compiled") || msg.contains("not supported") {
                            hit_unsupported = true;
                            break;
                        }
                        continue;
                    }
                };

                if hash_vec.len() < 32 {
                    continue;
                }

                let mut hash = [0u8; 32];
                hash.copy_from_slice(&hash_vec[..32]);

                hashes_count += 1;
                hashes_pending_stats += 1;

                // Průběžný flush statistik i během velkého batchu.
                // Bez tohoto se u CH (batch 250k) může UI držet dlouho na 0 H/s,
                // i když miner aktivně počítá.
                if hashes_pending_stats > 0
                    && (hashes_pending_stats >= 4096
                        || last_stats_flush.elapsed() >= std::time::Duration::from_secs(1))
                {
                    let mut stats_guard = stats.blocking_write();
                    stats_guard.add_hashes(hashes_pending_stats);
                    hashes_pending_stats = 0;
                    last_stats_flush = std::time::Instant::now();
                }

                if Self::meets_target(active_algorithm, &hash, &target_hex, job.cosmic_state0_endian.as_deref()) {
                    shares_found = shares_found.saturating_add(1);

                    let pending = PendingShare {
                        job_id: job.job_id.clone(),
                        nonce,
                        result_hex: hex::encode(hash),
                    };

                    // Best-effort: if queue is full, drop (avoid stalling hashing)
                    let _ = share_tx.try_send(pending);
                }
            }

            } // end else (non-RandomX)

            if hit_unsupported {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }

            let elapsed = start_time.elapsed();
            let hash_rate_khs = hashes_count as f64 / elapsed.as_secs_f64() / 1000.0;
            if shares_found > 0 {
                log::debug!("batch {} hashes {:.2} kH/s {} shares", hashes_count, hash_rate_khs, shares_found);
            }

            if hashes_pending_stats > 0 {
                let mut stats_guard = stats.blocking_write();
                stats_guard.add_hashes(hashes_pending_stats);
                last_stats_flush = std::time::Instant::now();
            } else if hashes_count > 0
                && !matches!(active_algorithm, Algorithm::RandomX | Algorithm::Yescrypt | Algorithm::CosmicHarmonyV2)
            {
                let mut stats_guard = stats.blocking_write();
                stats_guard.add_hashes(hashes_count);
                last_stats_flush = std::time::Instant::now();
            }

            nonce_start = nonce_end;

            // Check if connection is still alive (set to false by submit loop or connection monitor)
            if !connection_alive.load(std::sync::atomic::Ordering::Relaxed) {
                log::debug!("Connection lost — mining loop stopping");
                break;
            }
        }
        log::debug!("Mining loop exited");
    }

    /// Public static accessor for target checking (used by stream_aware module)
    pub fn meets_target_static(
        algorithm: Algorithm,
        hash: &[u8; 32],
        target_hex: &str,
        cosmic_state0_endian: Option<&str>,
    ) -> bool {
        Self::meets_target(algorithm, hash, target_hex, cosmic_state0_endian)
    }

    /// Compute a stable bookmark key from a job_id.
    ///
    /// Pool job_ids include a changing timestamp component that makes naive
    /// HashMap lookups miss on every rotation:
    ///   ZION:  "h2288-90000000-1770943066-cosmic_harmony"
    ///   ext-*: "ext-xmr-48772489"
    ///
    /// We strip the timestamp so the key stays constant across rotations
    /// at the same height/prev_hash:
    ///   ZION:  "h2288-90000000-cosmic_harmony"
    ///   ext-*: "ext-xmr" (external jobs share nonce space within a coin)
    fn bookmark_key(job_id: &str) -> String {
        if job_id.starts_with("ext-") {
            // ext-xmr-48772489 → "ext-xmr"
            let parts: Vec<&str> = job_id.splitn(3, '-').collect();
            if parts.len() >= 2 {
                return format!("{}-{}", parts[0], parts[1]);
            }
            return job_id.to_string();
        }

        // ZION jobs: h{height}-{prev8}-{timestamp}-{algo}
        // Strip the timestamp (3rd component) → "h{height}-{prev8}-{algo}"
        let parts: Vec<&str> = job_id.split('-').collect();
        if parts.len() == 4 {
            // h2288-90000000-1770943066-cosmic_harmony → h2288-90000000-cosmic_harmony
            format!("{}-{}-{}", parts[0], parts[1], parts[3])
        } else {
            // Legacy or already stripped format — use as-is
            job_id.to_string()
        }
    }

    /// Get optimal batch size for an algorithm
    fn batch_size_for_algo(algo: Algorithm) -> u32 {
        match algo {
            // RandomX uses calculate_hash_set pipeline mode — bigger
            // batches amortise the first/next pipeline overhead.  Each
            // hash takes ~30 ms (light) or ~3 ms (full), so 256 hashes
            // ≈ 8 s (light) / 0.8 s (full) — comfortably within the
            // ~30 s job cadence from MoneroOcean.
            Algorithm::RandomX => 256,
            Algorithm::Yescrypt => 5_000,
            Algorithm::CosmicHarmonyV2 => 1_000,
            Algorithm::Ethash => 50_000,
            Algorithm::Autolykos => 50_000,
            Algorithm::KawPow => 50_000,
            _ => 250_000,
        }
    }

    fn meets_target(
        algorithm: Algorithm,
        hash: &[u8; 32],
        target_hex: &str,
        cosmic_state0_endian: Option<&str>,
    ) -> bool {
        let target_hex = target_hex.trim_start_matches("0x");
        if target_hex.is_empty() {
            return false;
        }

        match algorithm {
            Algorithm::RandomX => {
                // CryptoNote/XMRig target check (matches XMRig's Job::setTarget):
                // Pool sends compact 4-byte target (little-endian u32), e.g. "b88d0600".
                // XMRig expands to 64-bit: target64 = 0xFFFFFFFFFFFFFFFF / (0xFFFFFFFF / target_u32)
                // Then compares: hash[24..32] as u64 LE < target64
                let tbytes = Vec::from_hex(target_hex).unwrap_or_default();
                let target64 = if tbytes.len() == 4 {
                    let target_u32 = u32::from_le_bytes([
                        tbytes[0], tbytes[1], tbytes[2], tbytes[3],
                    ]);
                    if target_u32 > 0 {
                        0xFFFFFFFFFFFFFFFFu64 / (0xFFFFFFFFu64 / target_u32 as u64)
                    } else {
                        0
                    }
                } else if tbytes.len() >= 8 {
                    let mut b8 = [0u8; 8];
                    b8.copy_from_slice(&tbytes[..8]);
                    u64::from_le_bytes(b8)
                } else {
                    0
                };
                let mut hash_hi = [0u8; 8];
                hash_hi.copy_from_slice(&hash[24..32]);
                let hash_val = u64::from_le_bytes(hash_hi);
                // Log near-misses every ~100 hashes for debugging
                static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let cnt = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if cnt % 100 == 0 || hash_val < target64 {
                    log::debug!("🎯 RandomX target check: hash_val=0x{:016X} target64=0x{:016X} {} (n={})",
                        hash_val, target64, if hash_val < target64 { "✅ HIT" } else { "miss" }, cnt);
                }
                hash_val < target64
            }
            Algorithm::Yescrypt => {
                // Pool-side yescrypt compares first 28 bytes (224-bit) big-endian.
                // Target may be delivered as a hex string shorter than 28 bytes; pad on the left.
                let target_bytes = parse_target_to_fixed_be(target_hex, 28);
                for (a, b) in hash.iter().take(28).zip(target_bytes.iter()) {
                    if a < b {
                        return true;
                    }
                    if a > b {
                        return false;
                    }
                }
                true
            }
            Algorithm::CosmicHarmony => {
                // Match native pool validator logic for Cosmic Harmony v1/v3:
                // - state0 is derived from the first 4 bytes of the hash
                // - endian is configurable (pool currently uses little)
                // - job target is a u32 hex string (8 chars) computed from difficulty
                let target_u32 = parse_cosmic_target_to_u32(target_hex);
                let endian = cosmic_state0_endian.unwrap_or("little").to_lowercase();
                let state0 = if endian == "big" {
                    u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]])
                } else {
                    u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]])
                };
                let result = state0 <= target_u32;
                
                // Debug: log first few comparisons
                static DEBUG_COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let count = DEBUG_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if count < 5 || (result && count < 100) {
                    log::debug!(
                        "🔍 CosmicHarmony target check: state0={} (0x{:08x}), target={} (0x{:08x}, hex='{}'), meets={}",
                        state0, state0, target_u32, target_u32, target_hex, result
                    );
                }
                
                result
            }
            _ => {
                // Generic lexicographic compare against 32-byte target
                let mut target_bytes = vec![0u8; 32];
                if let Ok(tbytes) = Vec::from_hex(target_hex) {
                    let start = 32usize.saturating_sub(tbytes.len());
                    target_bytes[start..].copy_from_slice(&tbytes);
                }

                for (a, b) in hash.iter().zip(target_bytes.iter()) {
                    if a < b {
                        return true;
                    }
                    if a > b {
                        return false;
                    }
                }
                true
            }
        }
    }
}

fn parse_target_to_fixed_be(target_hex: &str, size: usize) -> Vec<u8> {
    let t = target_hex.trim_start_matches("0x").trim();
    let mut out = vec![0u8; size];
    if let Ok(mut tbytes) = Vec::from_hex(t) {
        if tbytes.len() > size {
            tbytes = tbytes.split_off(tbytes.len() - size);
        }
        let start = size.saturating_sub(tbytes.len());
        out[start..].copy_from_slice(&tbytes);
    }
    out
}

fn parse_target_to_u32(target_hex: &str) -> u32 {
    // Pool-side (native) parses cosmic targets as a hex number (big-endian text).
    // - If <= 8 chars: parse full string.
    // - If longer: use the last 8 chars (low32).
    let t = target_hex.trim_start_matches("0x").trim();
    if t.is_empty() {
        return 0;
    }

    if t.len() <= 8 {
        return u32::from_str_radix(t, 16).unwrap_or(0);
    }

    u32::from_str_radix(&t[t.len() - 8..], 16).unwrap_or(0)
}

fn parse_cosmic_target_to_u32(target_hex: &str) -> u32 {
    let t = target_hex.trim_start_matches("0x").trim();
    if t.is_empty() {
        return 0;
    }

    if t.len() <= 8 {
        return u32::from_str_radix(t, 16).unwrap_or(0);
    }

    // Native pool uses a u32 target; if longer, take the low 32-bits.
    u32::from_str_radix(&t[t.len() - 8..], 16).unwrap_or(0)
}
