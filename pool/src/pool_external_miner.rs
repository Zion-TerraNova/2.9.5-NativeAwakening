//! Pool-Side External Mining Worker
//!
//! Manages xmrig subprocess for CPU mining via MoneroOcean (auto algo-switching).
//! MoneroOcean automatically selects the most profitable CPU algorithm
//! and pays out in XMR. For GPU algorithms (ethash, kawpow) — only proxies jobs.
//!
//! ⚠️ CH3 UPDATE (v2.9.5): In CPU-only mode (no GPU detected), this module is
//! SKIPPED entirely. The 25% Revenue stream is instead handled by the ZION miner
//! itself using native RandomX (via zion_core::algorithms::randomx).
//! Pool's RevenueProxy connects to MoneroOcean, forwards XMR jobs to miners,
//! and miners solve them natively — no xmrig subprocess needed.
//!
//! This module is only activated when ZION_HAS_GPU=1 (GPU servers).
//!
//! Architecture (CPU-only mode):
//!   RevenueProxy → MoneroOcean stratum → XMR jobs
//!       │
//!       └── StreamScheduler → Revenue phase → Miner (native RandomX)
//!
//! Architecture (GPU mode — when xmrig IS needed):
//!   RevenueProxy ──broadcast──→ PoolExternalMiner
//!       │                              │
//!       │  jobs for all coins          ├── MoneroOcean → spawn xmrig (auto-algo)
//!       │                              ├── ETC/Ethash  → skip (GPU only)
//!       └──────────────────────────────└── RVN/KawPow  → skip (GPU only)
//!
//! xmrig connects to MoneroOcean (gulf.moneroocean.stream:10001) which
//! auto-switches between CPU algorithms for maximum profitability.
//! We manage its lifecycle (start/stop/restart) and parse its stdout for stats.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::Mutex;
use tokio::process::{Child, Command};
use tokio::io::AsyncBufReadExt;
use tracing::{info, warn, error, debug};

use crate::revenue_proxy::RevenueProxyManager;

/// Configuration for the pool-side external miner
#[derive(Debug, Clone)]
pub struct ExternalMinerConfig {
    /// Number of CPU threads for xmrig (0 = auto-detect)
    pub threads: usize,
    /// Which coins to mine (empty = all CPU-minable)
    pub coins: Vec<String>,
    /// Worker name prefix for submitted shares
    pub worker_prefix: String,
    /// Path to xmrig binary
    pub xmrig_path: String,
}

impl Default for ExternalMinerConfig {
    fn default() -> Self {
        Self {
            threads: 2,
            coins: vec![],
            worker_prefix: "zion-pool".to_string(),
            xmrig_path: "/usr/local/bin/xmrig".to_string(),
        }
    }
}

/// Stats for the pool-side miner (parsed from xmrig output)
#[derive(Debug, Default)]
pub struct MinerStats {
    pub hashes_computed: AtomicU64,
    pub shares_found: AtomicU64,
    pub shares_accepted: AtomicU64,
    pub shares_rejected: AtomicU64,
    pub jobs_processed: AtomicU64,
    pub current_hashrate: AtomicU64,
}

/// Active mining target info
#[derive(Debug, Clone)]
pub struct MiningTarget {
    pub coin: String,
    pub algorithm: String,
    pub pool_url: String,
    pub wallet: String,
    pub worker: String,
}

/// The pool-side external mining worker
pub struct PoolExternalMiner {
    config: ExternalMinerConfig,
    #[allow(dead_code)]
    revenue_proxy: Arc<RevenueProxyManager>,
    stats: Arc<MinerStats>,
    running: Arc<AtomicBool>,
    xmrig_process: Arc<Mutex<Option<Child>>>,
    current_target: Arc<Mutex<Option<MiningTarget>>>,
}

impl PoolExternalMiner {
    pub fn new(
        config: ExternalMinerConfig,
        revenue_proxy: Arc<RevenueProxyManager>,
    ) -> Self {
        Self {
            config,
            revenue_proxy,
            stats: Arc::new(MinerStats::default()),
            running: Arc::new(AtomicBool::new(false)),
            xmrig_process: Arc::new(Mutex::new(None)),
            current_target: Arc::new(Mutex::new(None)),
        }
    }

    /// Get mining stats
    pub fn stats(&self) -> &Arc<MinerStats> {
        &self.stats
    }

    /// Get stats as JSON
    pub fn stats_json(&self) -> serde_json::Value {
        serde_json::json!({
            "running": self.running.load(Ordering::Relaxed),
            "threads": self.config.threads,
            "hashes_computed": self.stats.hashes_computed.load(Ordering::Relaxed),
            "shares_found": self.stats.shares_found.load(Ordering::Relaxed),
            "shares_accepted": self.stats.shares_accepted.load(Ordering::Relaxed),
            "shares_rejected": self.stats.shares_rejected.load(Ordering::Relaxed),
            "jobs_processed": self.stats.jobs_processed.load(Ordering::Relaxed),
            "current_hashrate": self.stats.current_hashrate.load(Ordering::Relaxed),
            "coins": self.config.coins,
            "worker_prefix": self.config.worker_prefix,
            "xmrig_path": self.config.xmrig_path,
            "mining_active": self.xmrig_process.try_lock().map(|p| p.is_some()).unwrap_or(false),
        })
    }

    /// Start the mining worker — spawns xmrig for XMR mining on 2miners
    pub async fn start(self: Arc<Self>) {
        if self.running.swap(true, Ordering::SeqCst) {
            warn!("Pool external miner already running");
            return;
        }

        info!(
            "⛏️ Starting Pool External Miner (xmrig mode, threads={}, coins={:?})",
            self.config.threads,
            self.config.coins
        );

        // Check if xmrig is available
        let xmrig_available = self.check_xmrig().await;
        if !xmrig_available {
            warn!("⚠️ xmrig not found at '{}' — attempting to install", self.config.xmrig_path);
            self.install_xmrig().await;
        }

        // Start mining on MoneroOcean (auto algo-switching, pays XMR)
        let target = MiningTarget {
            coin: "XMR".to_string(),
            algorithm: "auto".to_string(),
            pool_url: "gulf.moneroocean.stream:10001".to_string(),
            wallet: crate::config::default_xmr_wallet(),
            worker: self.config.worker_prefix.clone(),
        };

        if let Err(e) = self.start_xmrig(&target).await {
            error!("❌ Failed to start xmrig: {}", e);
        }

        // Spawn xmrig health monitor
        let miner = Arc::clone(&self);
        tokio::spawn(async move {
            miner.xmrig_monitor().await;
        });

        info!("✅ Pool External Miner started (xmrig subprocess)");
    }

    /// Check if xmrig binary exists
    async fn check_xmrig(&self) -> bool {
        for path in &[
            self.config.xmrig_path.as_str(),
            "/usr/local/bin/xmrig",
            "/usr/bin/xmrig",
            "/opt/xmrig/xmrig",
            "./xmrig",
        ] {
            if tokio::fs::metadata(path).await.is_ok() {
                info!("✅ Found xmrig at {}", path);
                return true;
            }
        }
        false
    }

    /// Install xmrig from GitHub releases (auto-detects architecture)
    async fn install_xmrig(&self) {
        info!("📦 Installing xmrig v6.22.2...");
        // Detect architecture (aarch64 vs x86_64)
        let script = r#"
ARCH=$(uname -m)
cd /tmp
if [ "$ARCH" = "aarch64" ] || [ "$ARCH" = "arm64" ]; then
    echo "Detected ARM64 — building xmrig from source..."
    apt-get update -qq && apt-get install -y -qq cmake gcc g++ libhwloc-dev libuv1-dev libssl-dev >/dev/null 2>&1
    git clone --depth 1 --branch v6.22.2 https://github.com/xmrig/xmrig.git /tmp/xmrig-src 2>/dev/null
    cd /tmp/xmrig-src && mkdir build && cd build
    cmake .. -DWITH_HWLOC=OFF -DWITH_OPENCL=OFF -DWITH_CUDA=OFF -DCMAKE_BUILD_TYPE=Release >/dev/null 2>&1
    make -j$(nproc) >/dev/null 2>&1
    cp xmrig /usr/local/bin/xmrig
    chmod +x /usr/local/bin/xmrig
    rm -rf /tmp/xmrig-src
    echo 'OK'
else
    echo "Detected x86_64"
    wget -q https://github.com/xmrig/xmrig/releases/download/v6.22.2/xmrig-6.22.2-linux-static-x64.tar.gz -O xmrig.tar.gz
    tar xzf xmrig.tar.gz
    cp xmrig-6.22.2/xmrig /usr/local/bin/xmrig
    chmod +x /usr/local/bin/xmrig
    rm -rf xmrig.tar.gz xmrig-6.22.2
    echo 'OK'
fi
"#;
        match Command::new("bash").arg("-c").arg(script).output().await {
            Ok(output) => {
                if output.status.success() {
                    info!("✅ xmrig installed successfully");
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    error!("❌ xmrig install failed: {}", stderr.chars().take(200).collect::<String>());
                }
            }
            Err(e) => error!("❌ Failed to run xmrig installer: {}", e),
        }
    }

    /// Start xmrig subprocess for a specific mining target
    pub async fn start_xmrig(&self, target: &MiningTarget) -> Result<(), String> {
        // Stop any existing xmrig first
        self.stop_xmrig().await;

        let xmrig_path = self.find_xmrig().await
            .ok_or_else(|| "xmrig binary not found".to_string())?;

        let threads_str = self.config.threads.to_string();

        info!(
            "🚀 Starting xmrig: coin={} algo={} pool={} wallet={} worker={} threads={}",
            target.coin, target.algorithm, target.pool_url, target.wallet, target.worker, threads_str
        );

        // MoneroOcean auto-switches algos via mining.set_algo extension
        // Do NOT specify --algo or --coin — let the pool control algorithm selection
        // MoneroOcean format: --user XMR_WALLET --pass worker_name
        let mut child = Command::new(&xmrig_path)
            .arg("--url").arg(&target.pool_url)
            .arg("--user").arg(&target.wallet)
            .arg("--pass").arg(&target.worker)
            .arg("--threads").arg(&threads_str)
            .arg("--no-color")
            .arg("--print-time").arg("10")
            .arg("--donate-level").arg("0")
            .arg("--keepalive")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to spawn xmrig: {}", e))?;

        let pid = child.id().unwrap_or(0);
        info!("✅ xmrig started (PID={}, coin={}, algo={})", pid, target.coin, target.algorithm);

        // Take stdout for parsing before storing child
        let stdout = child.stdout.take();

        // Store process and target
        {
            let mut proc = self.xmrig_process.lock().await;
            *proc = Some(child);
        }
        {
            let mut tgt = self.current_target.lock().await;
            *tgt = Some(target.clone());
        }

        // Spawn stdout parser
        if let Some(stdout) = stdout {
            let stats = self.stats.clone();
            tokio::spawn(async move {
                parse_xmrig_output(stdout, stats).await;
            });
        }

        Ok(())
    }

    /// Find xmrig binary path
    async fn find_xmrig(&self) -> Option<String> {
        for path in &[
            self.config.xmrig_path.as_str(),
            "/usr/local/bin/xmrig",
            "/usr/bin/xmrig",
            "/opt/xmrig/xmrig",
        ] {
            if tokio::fs::metadata(path).await.is_ok() {
                return Some(path.to_string());
            }
        }
        None
    }

    /// Stop xmrig subprocess
    pub async fn stop_xmrig(&self) {
        let mut proc = self.xmrig_process.lock().await;
        if let Some(ref mut child) = *proc {
            let pid = child.id().unwrap_or(0);
            info!("🛑 Stopping xmrig (PID={})", pid);
            let _ = child.kill().await;
            let _ = child.wait().await;
            info!("✅ xmrig stopped");
        }
        *proc = None;
    }

    /// Switch to a different coin (stop current xmrig, start new one)
    pub async fn switch_coin(&self, target: &MiningTarget) -> Result<(), String> {
        info!("🔄 Switching mining to {} ({})", target.coin, target.algorithm);
        self.start_xmrig(target).await
    }

    /// Monitor xmrig health — restart if it crashes
    async fn xmrig_monitor(self: Arc<Self>) {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;

            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            let is_running = {
                let mut proc = self.xmrig_process.lock().await;
                if let Some(ref mut child) = *proc {
                    match child.try_wait() {
                        Ok(None) => true,
                        Ok(Some(status)) => {
                            warn!("⚠️ xmrig exited with status: {}", status);
                            false
                        }
                        Err(e) => {
                            error!("❌ Failed to check xmrig status: {}", e);
                            false
                        }
                    }
                } else {
                    false
                }
            };

            if !is_running && self.running.load(Ordering::Relaxed) {
                info!("🔄 xmrig not running, attempting restart...");
                let target = {
                    let tgt = self.current_target.lock().await;
                    tgt.clone()
                };
                if let Some(target) = target {
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    if let Err(e) = self.start_xmrig(&target).await {
                        error!("❌ Failed to restart xmrig: {}", e);
                    }
                } else {
                    let default_target = MiningTarget {
                        coin: "XMR".to_string(),
                        algorithm: "auto".to_string(),
                        pool_url: "gulf.moneroocean.stream:10001".to_string(),
                        wallet: crate::config::default_xmr_wallet(),
                        worker: self.config.worker_prefix.clone(),
                    };
                    tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                    if let Err(e) = self.start_xmrig(&default_target).await {
                        error!("❌ Failed to restart xmrig with defaults: {}", e);
                    }
                }
            }
        }
    }

    /// Stop the miner
    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        info!("🛑 Pool External Miner stopped");
    }
}

// ═══════════════════════════════════════════════════════════════
// xmrig Output Parsing
// ═══════════════════════════════════════════════════════════════

/// Parse xmrig stdout stream and update stats
async fn parse_xmrig_output(
    stdout: tokio::process::ChildStdout,
    stats: Arc<MinerStats>,
) {
    let reader = tokio::io::BufReader::new(stdout);
    let mut lines = reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        // xmrig output examples:
        //   speed 10s/60s/15m 1234.5 1200.0 1150.0 H/s max 1300.0 H/s
        //   accepted (1/0) diff 100000 (112 ms)
        //   new job from xmr.2miners.com:2222 diff 100000 algo rx/0
        //   * DATASET://rx/0 (2336 MB) ... dataset ready (12345 ms)

        if line.contains("speed") && line.contains("H/s") {
            if let Some(hr) = parse_hashrate(&line) {
                stats.current_hashrate.store(hr as u64, Ordering::Relaxed);
                let (val, unit) = if hr > 1_000_000.0 {
                    (hr / 1_000_000.0, "MH/s")
                } else if hr > 1_000.0 {
                    (hr / 1_000.0, "kH/s")
                } else {
                    (hr, "H/s")
                };
                info!(
                    "📊 xmrig: {:.1} {} | accepted={} rejected={}",
                    val, unit,
                    stats.shares_accepted.load(Ordering::Relaxed),
                    stats.shares_rejected.load(Ordering::Relaxed),
                );
            }
        } else if line.contains("accepted") {
            stats.shares_found.fetch_add(1, Ordering::Relaxed);
            if let Some((acc, rej)) = parse_accepted_count(&line) {
                stats.shares_accepted.store(acc, Ordering::Relaxed);
                stats.shares_rejected.store(rej, Ordering::Relaxed);
            }
            info!("�� {}", line.trim());
        } else if line.contains("rejected") {
            stats.shares_rejected.fetch_add(1, Ordering::Relaxed);
            warn!("❌ {}", line.trim());
        } else if line.contains("new job") {
            stats.jobs_processed.fetch_add(1, Ordering::Relaxed);
            debug!("📦 {}", line.trim());
        } else if line.contains("dataset ready") || line.contains("READY") {
            info!("✅ RandomX dataset ready — mining active!");
        } else if line.contains("ERROR") || line.contains("error") {
            error!("⚠️ xmrig: {}", line.trim());
        }
    }

    info!("📡 xmrig output stream ended");
}

/// Parse hashrate from xmrig speed line
fn parse_hashrate(line: &str) -> Option<f64> {
    // "speed 10s/60s/15m 1234.5 1200.0 1150.0 H/s"
    if let Some(pos) = line.find("10s/60s/15m") {
        let after = line[pos + 11..].trim();
        if let Some(end) = after.find(|c: char| !c.is_ascii_digit() && c != '.') {
            return after[..end].parse::<f64>().ok();
        }
        return after.parse::<f64>().ok();
    }
    // Fallback: number before "H/s"
    if let Some(hs_pos) = line.find("H/s") {
        let before = line[..hs_pos].trim();
        if let Some(last_space) = before.rfind(' ') {
            return before[last_space + 1..].parse::<f64>().ok();
        }
    }
    None
}

/// Parse accepted/rejected count from "accepted (N/M)" format
fn parse_accepted_count(line: &str) -> Option<(u64, u64)> {
    if let Some(start) = line.find('(') {
        if let Some(end) = line[start..].find(')') {
            let inner = &line[start + 1..start + end];
            let parts: Vec<&str> = inner.split('/').collect();
            if parts.len() == 2 {
                if let (Ok(a), Ok(r)) = (parts[0].parse(), parts[1].parse()) {
                    return Some((a, r));
                }
            }
        }
    }
    None
}
