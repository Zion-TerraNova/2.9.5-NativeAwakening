//! Profit Switching Engine for CH3 External Mining
//!
//! Periodically fetches profitability data from WhatToMine API,
//! compares coins, and switches the active mining target to the
//! most profitable coin, respecting hysteresis and cooldown.
//!
//! Architecture:
//!   ProfitSwitcher ──poll──→ WhatToMine API
//!       │                       │
//!       │  ← CoinProfitData ────┘
//!       │
//!       ├── Compare: current vs best coin
//!       ├── If best > current + threshold → switch
//!       └── Notify pool via broadcast channel
//!
//! Integration:
//!   - main.rs spawns ProfitSwitcher::run()
//!   - Revenue proxy reads active_coin to route jobs
//!   - Pool external miner filters jobs by active coin
//!   - API endpoint /api/v1/profit/status exposes state

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use tokio::sync::{RwLock, watch};
use tracing::{info, warn, error, debug};
use serde::{Deserialize, Serialize};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════
// GPU Detection — CH3 Rule: No GPU → 25% CPU → XMR (MoneroOcean)
// ═══════════════════════════════════════════════════════════════

/// Detect if the server has a usable GPU for mining.
/// Checks NVIDIA (nvidia-smi), AMD (rocm-smi), and env override.
///
/// CH3 Architecture Rule:
///   - GPU present → ProfitSwitcher picks best GPU coin (ETH/RVN/ERG/KAS)
///   - No GPU → Revenue 25% forced to XMR (RandomX, CPU-only, MoneroOcean)
fn detect_gpu_available() -> bool {
    // Allow manual override via environment variable
    if let Ok(val) = std::env::var("ZION_HAS_GPU") {
        let has = matches!(val.to_lowercase().as_str(), "1" | "true" | "yes");
        info!("🎮 GPU override via ZION_HAS_GPU={} → {}", val, if has { "GPU mode" } else { "CPU-only mode" });
        return has;
    }

    // Check NVIDIA GPU
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout);
            let name = name.trim();
            if !name.is_empty() {
                info!("🎮 NVIDIA GPU detected: {}", name);
                return true;
            }
        }
    }

    // Check AMD GPU
    if let Ok(output) = std::process::Command::new("rocm-smi")
        .arg("--showproductname")
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout);
            if name.contains("GPU") || name.contains("Radeon") || name.contains("Instinct") {
                info!("🎮 AMD GPU detected");
                return true;
            }
        }
    }

    // Check if any /dev/dri render nodes exist (Linux GPU)
    if std::path::Path::new("/dev/dri/renderD128").exists() {
        // Could be integrated graphics — check if it's a real mining GPU
        // For now, require explicit nvidia-smi/rocm-smi detection
        debug!("🎮 /dev/dri found but no nvidia-smi/rocm-smi — treating as CPU-only");
    }

    info!("🎮 No GPU detected → CPU-only mode (Revenue 25% → XMR/MoneroOcean)");
    false
}

// ═══════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════

/// Profit switching configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProfitSwitchConfig {
    /// Enable/disable automatic switching
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// How often to check profitability (seconds)
    #[serde(default = "default_check_interval")]
    pub check_interval_secs: u64,
    /// Minimum profit advantage to trigger a switch (percentage)
    #[serde(default = "default_threshold")]
    pub switch_threshold_pct: f64,
    /// Minimum time between switches (seconds)
    #[serde(default = "default_cooldown")]
    pub min_switch_interval_secs: u64,
    /// Coins to consider (empty = all enabled in config)
    #[serde(default)]
    pub preferred_coins: Vec<String>,
    /// Coins to never switch to
    #[serde(default)]
    pub excluded_coins: Vec<String>,
    /// Default coin when no profitability data available
    #[serde(default = "default_fallback")]
    pub fallback_coin: String,
}

fn default_true() -> bool { true }
fn default_check_interval() -> u64 { 300 } // 5 minutes
fn default_threshold() -> f64 { 10.0 } // 10%
fn default_cooldown() -> u64 { 1800 } // 30 minutes
fn default_fallback() -> String { "XMR".to_string() }

impl Default for ProfitSwitchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_secs: 300,
            switch_threshold_pct: 10.0,
            min_switch_interval_secs: 1800,
            preferred_coins: vec![
                "XMR".to_string(),
                "ETC".to_string(),
                "RVN".to_string(),
            ],
            excluded_coins: vec![],
            fallback_coin: "XMR".to_string(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Profitability Data
// ═══════════════════════════════════════════════════════════════

/// Profitability data for a single coin
#[derive(Debug, Clone, Serialize)]
pub struct CoinProfitData {
    pub coin: String,
    pub algorithm: String,
    pub price_usd: f64,
    pub btc_revenue_24h: f64,
    pub usd_revenue_24h: f64,
    pub difficulty: f64,
    pub block_reward: f64,
    pub nethash: f64,
    pub profit_score: f64,  // Normalized score (higher = more profitable)
    pub timestamp: i64,
}

/// Switch event record
#[derive(Debug, Clone, Serialize)]
pub struct SwitchEvent {
    pub from_coin: String,
    pub to_coin: String,
    pub reason: String,
    pub profit_advantage_pct: f64,
    pub timestamp: i64,
}

// ═══════════════════════════════════════════════════════════════
// WhatToMine API Client
// ═══════════════════════════════════════════════════════════════

/// WhatToMine API response structures
#[derive(Debug, Deserialize)]
struct WtmGpuResponse {
    coins: HashMap<String, WtmCoinData>,
}

#[derive(Debug, Deserialize)]
struct WtmCoinData {
    #[serde(default)]
    tag: String,
    #[serde(default)]
    algorithm: String,
    #[serde(default)]
    block_reward: f64,
    #[serde(default)]
    difficulty: f64,
    #[serde(default)]
    nethash: f64,
    #[serde(default, alias = "exchange_rate")]
    exchange_rate: f64,
    #[serde(default, alias = "btc_revenue")]
    btc_revenue: String,
    #[serde(default, alias = "estimated_rewards")]
    estimated_rewards: String,
    #[serde(default)]
    profitability: f64,
    #[serde(default)]
    profitability24: f64,
}

/// Fetch profitability data from WhatToMine GPU + ASIC APIs
/// GPU API (coins.json): ETC, RVN, ERG, FLUX, etc.
/// ASIC API (asic.json): KAS, ALPH, XMR, LTC, etc.
async fn fetch_whattomine(coins: &[String]) -> Result<Vec<CoinProfitData>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    // Fetch both APIs in parallel
    let (gpu_result, asic_result) = tokio::join!(
        fetch_wtm_endpoint(&client, "https://whattomine.com/coins.json", coins),
        fetch_wtm_endpoint(&client, "https://whattomine.com/asic.json", coins),
    );

    let mut results = Vec::new();

    match gpu_result {
        Ok(mut data) => {
            info!("💹 WhatToMine GPU: {} coins matched", data.len());
            results.append(&mut data);
        }
        Err(e) => warn!("💹 WhatToMine GPU API error: {}", e),
    }

    match asic_result {
        Ok(mut data) => {
            info!("💹 WhatToMine ASIC: {} coins matched", data.len());
            // Merge: if a coin exists in both APIs, keep the one with higher score
            for asic_coin in data.drain(..) {
                if let Some(existing) = results.iter_mut().find(|r: &&mut CoinProfitData| r.coin == asic_coin.coin) {
                    if asic_coin.profit_score > existing.profit_score {
                        *existing = asic_coin;
                    }
                } else {
                    results.push(asic_coin);
                }
            }
        }
        Err(e) => warn!("💹 WhatToMine ASIC API error: {}", e),
    }

    if results.is_empty() {
        return Err("No data from either WhatToMine API".to_string());
    }

    // Sort by profitability (descending)
    results.sort_by(|a, b| b.profit_score.partial_cmp(&a.profit_score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(results)
}

/// Fetch a single WhatToMine endpoint and parse results
async fn fetch_wtm_endpoint(
    client: &reqwest::Client,
    url: &str,
    coins: &[String],
) -> Result<Vec<CoinProfitData>, String> {
    let response = client.get(url).send().await
        .map_err(|e| format!("Request to {} failed: {}", url, e))?;

    if !response.status().is_success() {
        return Err(format!("{} HTTP {}", url, response.status()));
    }

    let body = response.text().await
        .map_err(|e| format!("Body read from {} failed: {}", url, e))?;

    let wtm: WtmGpuResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Parse {} failed: {}", url, e))?;

    let now = Utc::now().timestamp();
    let mut results = Vec::new();

    // Map WhatToMine coin names to our tags
    let coin_map: HashMap<&str, &str> = [
        ("Ethereum Classic", "ETC"),
        ("EthereumPoW", "ETHW"),
        ("Ravencoin", "RVN"),
        ("Kaspa", "KAS"),
        ("Ergo", "ERG"),
        ("Alephium", "ALPH"),
        ("Flux", "FLUX"),
        ("Nexa", "NEXA"),
        ("Neoxa", "NEOXA"),
        ("Clore.ai", "CLORE"),
        ("Monero", "XMR"),
        ("Litecoin", "LTC"),
    ].iter().cloned().collect();

    for (name, data) in &wtm.coins {
        // Skip Nicehash entries
        if name.starts_with("Nicehash") {
            continue;
        }

        let tag = coin_map.get(name.as_str())
            .map(|t| t.to_string())
            .unwrap_or_else(|| data.tag.clone());

        // Filter: only coins we care about
        if !coins.is_empty() && !coins.iter().any(|c| c.eq_ignore_ascii_case(&tag)) {
            continue;
        }

        let btc_rev: f64 = data.btc_revenue.parse().unwrap_or(0.0);
        let usd_rev = btc_rev * data.exchange_rate;

        results.push(CoinProfitData {
            coin: tag.to_uppercase(),
            algorithm: data.algorithm.clone(),
            price_usd: data.exchange_rate,
            btc_revenue_24h: btc_rev,
            usd_revenue_24h: usd_rev,
            difficulty: data.difficulty,
            block_reward: data.block_reward,
            nethash: data.nethash,
            profit_score: data.profitability24,
            timestamp: now,
        });
    }

    Ok(results)
}

/// Fallback: manual profitability estimation when WhatToMine is unavailable
fn estimate_profitability_fallback(coins: &[String]) -> Vec<CoinProfitData> {
    let now = Utc::now().timestamp();

    // Static estimates based on typical February 2026 values
    // Updated periodically when WhatToMine data is available
    let estimates: Vec<(&str, &str, f64)> = vec![
        ("XMR",  "RandomX",    90.0),   // CPU-minable, most profitable for our servers
        ("ETC",  "Ethash",     60.0),
        ("RVN",  "KawPow",     40.0),
        ("ERG",  "Autolykos",  35.0),
    ];

    estimates.iter()
        .filter(|(tag, _, _)| coins.is_empty() || coins.iter().any(|c| c.eq_ignore_ascii_case(tag)))
        .map(|(tag, algo, score)| CoinProfitData {
            coin: tag.to_string(),
            algorithm: algo.to_string(),
            price_usd: 0.0,
            btc_revenue_24h: 0.0,
            usd_revenue_24h: 0.0,
            difficulty: 0.0,
            block_reward: 0.0,
            nethash: 0.0,
            profit_score: *score,
            timestamp: now,
        })
        .collect()
}

// ═══════════════════════════════════════════════════════════════
// Profit Switcher
// ═══════════════════════════════════════════════════════════════

/// The main profit switching engine
pub struct ProfitSwitcher {
    config: ProfitSwitchConfig,
    /// Currently active mining coin
    active_coin: RwLock<String>,
    /// Watch channel to notify consumers of coin switches
    coin_tx: watch::Sender<String>,
    coin_rx: watch::Receiver<String>,
    /// Latest profitability data
    profit_data: RwLock<Vec<CoinProfitData>>,
    /// History of switch events
    switch_history: RwLock<Vec<SwitchEvent>>,
    /// Timestamp of last switch
    last_switch_time: AtomicU64,
    /// Total number of switches
    total_switches: AtomicU64,
    /// Number of successful API fetches
    api_fetches: AtomicU64,
    /// Number of failed API fetches
    api_errors: AtomicU64,
    /// CPU-only mode: no GPU detected → always mine XMR via RandomX
    /// CH3 Architecture: 25% Revenue → MoneroOcean (auto-algo CPU)
    cpu_only_mode: AtomicBool,
}

impl ProfitSwitcher {
    pub fn new(config: ProfitSwitchConfig) -> Arc<Self> {
        let gpu_available = detect_gpu_available();
        let cpu_only = !gpu_available;

        // CH3 Rule: No GPU → force XMR as initial coin (RandomX = CPU-only)
        let initial_coin = if cpu_only {
            info!("💹 CPU-only mode: Revenue 25% locked to XMR (RandomX → MoneroOcean)");
            "XMR".to_string()
        } else {
            config.fallback_coin.clone()
        };

        let (coin_tx, coin_rx) = watch::channel(initial_coin.clone());

        Arc::new(Self {
            config,
            active_coin: RwLock::new(initial_coin),
            coin_tx,
            coin_rx,
            profit_data: RwLock::new(Vec::new()),
            switch_history: RwLock::new(Vec::new()),
            last_switch_time: AtomicU64::new(0),
            total_switches: AtomicU64::new(0),
            api_fetches: AtomicU64::new(0),
            api_errors: AtomicU64::new(0),
            cpu_only_mode: AtomicBool::new(cpu_only),
        })
    }

    /// Check if running in CPU-only mode (no GPU → XMR locked)
    pub fn is_cpu_only(&self) -> bool {
        self.cpu_only_mode.load(Ordering::Relaxed)
    }

    /// Subscribe to coin switch notifications
    pub fn subscribe(&self) -> watch::Receiver<String> {
        self.coin_rx.clone()
    }

    /// Get the currently active mining coin
    pub async fn active_coin(&self) -> String {
        self.active_coin.read().await.clone()
    }

    /// Get current profitability data
    pub async fn profit_data(&self) -> Vec<CoinProfitData> {
        self.profit_data.read().await.clone()
    }

    /// Get switch history
    pub async fn switch_history(&self) -> Vec<SwitchEvent> {
        self.switch_history.read().await.clone()
    }

    /// Get stats as JSON for API
    pub async fn stats_json(&self) -> serde_json::Value {
        let active = self.active_coin.read().await.clone();
        let profit = self.profit_data.read().await.clone();
        let history = self.switch_history.read().await.clone();

        // Build profitability table
        let profit_table: Vec<serde_json::Value> = profit.iter().map(|p| {
            serde_json::json!({
                "coin": p.coin,
                "algorithm": p.algorithm,
                "profit_score": p.profit_score,
                "price_usd": p.price_usd,
                "btc_revenue_24h": p.btc_revenue_24h,
                "usd_revenue_24h": p.usd_revenue_24h,
                "is_active": p.coin.eq_ignore_ascii_case(&active),
            })
        }).collect();

        // Last 10 switch events
        let recent_switches: Vec<serde_json::Value> = history.iter()
            .rev()
            .take(10)
            .map(|s| serde_json::json!({
                "from": s.from_coin,
                "to": s.to_coin,
                "reason": s.reason,
                "advantage_pct": s.profit_advantage_pct,
                "timestamp": s.timestamp,
            }))
            .collect();

        serde_json::json!({
            "enabled": self.config.enabled,
            "active_coin": active,
            "cpu_only_mode": self.cpu_only_mode.load(Ordering::Relaxed),
            "gpu_detected": !self.cpu_only_mode.load(Ordering::Relaxed),
            "check_interval_secs": self.config.check_interval_secs,
            "switch_threshold_pct": self.config.switch_threshold_pct,
            "min_switch_interval_secs": self.config.min_switch_interval_secs,
            "total_switches": self.total_switches.load(Ordering::Relaxed),
            "api_fetches": self.api_fetches.load(Ordering::Relaxed),
            "api_errors": self.api_errors.load(Ordering::Relaxed),
            "profitability": profit_table,
            "recent_switches": recent_switches,
        })
    }

    /// Main run loop — periodically check profitability and switch if needed
    pub async fn run(self: Arc<Self>) {
        if !self.config.enabled {
            info!("💹 Profit Switcher DISABLED — mining all enabled coins");
            return;
        }

        // ── CH3 CPU-Only Mode ──
        // No GPU detected → lock Revenue 25% to XMR (RandomX) permanently.
        // This saves server CPU by skipping WhatToMine API polling entirely.
        // The miner uses its native RandomX implementation (via zion_core),
        // pool's RevenueProxy connects to MoneroOcean and forwards XMR jobs.
        if self.cpu_only_mode.load(Ordering::Relaxed) {
            info!("╔════════════════════════════════════════════════════════╗");
            info!("║  💹 CPU-ONLY MODE — Revenue 25% → XMR (MoneroOcean)   ║");
            info!("║  No GPU detected. Profit switching DISABLED.           ║");
            info!("║  RandomX mining via native CPU (no xmrig needed).     ║");
            info!("║  Set ZION_HAS_GPU=1 to override.                      ║");
            info!("╚════════════════════════════════════════════════════════╝");

            // Force XMR and keep it locked
            *self.active_coin.write().await = "XMR".to_string();
            let _ = self.coin_tx.send("XMR".to_string());

            // Stay alive but don't poll WhatToMine — just sleep forever
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(3600)).await;
            }
        }

        info!(
            "💹 Profit Switcher started (interval={}s, threshold={}%, cooldown={}s, fallback={})",
            self.config.check_interval_secs,
            self.config.switch_threshold_pct,
            self.config.min_switch_interval_secs,
            self.config.fallback_coin,
        );

        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(self.config.check_interval_secs)
        );

        // First tick is immediate
        interval.tick().await;

        loop {
            // Fetch profitability data
            let coins = if self.config.preferred_coins.is_empty() {
                vec!["KAS".to_string(), "ETC".to_string(), "RVN".to_string(), "ERG".to_string(), "ALPH".to_string()]
            } else {
                self.config.preferred_coins.clone()
            };

            let profit_result = fetch_whattomine(&coins).await;

            let profit_data = match profit_result {
                Ok(data) if !data.is_empty() => {
                    self.api_fetches.fetch_add(1, Ordering::Relaxed);
                    info!(
                        "💹 WhatToMine: {} coins fetched, top={} (score={:.1})",
                        data.len(),
                        data[0].coin,
                        data[0].profit_score,
                    );
                    data
                }
                Ok(_) => {
                    warn!("💹 WhatToMine returned no matching coins, using fallback");
                    self.api_errors.fetch_add(1, Ordering::Relaxed);
                    estimate_profitability_fallback(&coins)
                }
                Err(e) => {
                    warn!("💹 WhatToMine error: {}, using fallback estimates", e);
                    self.api_errors.fetch_add(1, Ordering::Relaxed);
                    estimate_profitability_fallback(&coins)
                }
            };

            // Store latest data
            *self.profit_data.write().await = profit_data.clone();

            // Evaluate switch
            self.evaluate_switch(&profit_data).await;

            // Wait for next interval
            interval.tick().await;
        }
    }

    /// Evaluate whether to switch coins based on profitability data
    async fn evaluate_switch(&self, profit_data: &[CoinProfitData]) {
        if profit_data.is_empty() {
            return;
        }

        let current_coin = self.active_coin.read().await.clone();

        // Find the most profitable coin (excluding excluded list)
        let best = profit_data.iter()
            .filter(|p| !self.config.excluded_coins.iter().any(|e| e.eq_ignore_ascii_case(&p.coin)))
            .max_by(|a, b| a.profit_score.partial_cmp(&b.profit_score).unwrap_or(std::cmp::Ordering::Equal));

        let best = match best {
            Some(b) => b,
            None => return,
        };

        // Find current coin's profitability
        let current_profit = profit_data.iter()
            .find(|p| p.coin.eq_ignore_ascii_case(&current_coin))
            .map(|p| p.profit_score)
            .unwrap_or(0.0);

        // Calculate advantage percentage
        let advantage_pct = if current_profit > 0.0 {
            ((best.profit_score - current_profit) / current_profit) * 100.0
        } else {
            100.0 // If current has 0 profit, any positive coin is infinitely better
        };

        // Already mining the best coin?
        if best.coin.eq_ignore_ascii_case(&current_coin) {
            debug!(
                "💹 Already mining best coin: {} (score={:.1})",
                current_coin, current_profit
            );
            return;
        }

        // Check threshold
        if advantage_pct < self.config.switch_threshold_pct {
            debug!(
                "💹 {} is better ({:.1} vs {:.1}, +{:.1}%) but below threshold ({}%)",
                best.coin, best.profit_score, current_profit,
                advantage_pct, self.config.switch_threshold_pct
            );
            return;
        }

        // Check cooldown
        let now = Utc::now().timestamp() as u64;
        let last_switch = self.last_switch_time.load(Ordering::Relaxed);
        if last_switch > 0 && (now - last_switch) < self.config.min_switch_interval_secs {
            let remaining = self.config.min_switch_interval_secs - (now - last_switch);
            info!(
                "💹 Want to switch {} → {} (+{:.1}%) but cooldown active ({}s remaining)",
                current_coin, best.coin, advantage_pct, remaining
            );
            return;
        }

        // === SWITCH! ===
        info!(
            "🔄 PROFIT SWITCH: {} → {} (advantage: +{:.1}%, score: {:.1} vs {:.1})",
            current_coin, best.coin, advantage_pct,
            best.profit_score, current_profit
        );

        // Update active coin
        *self.active_coin.write().await = best.coin.clone();
        let _ = self.coin_tx.send(best.coin.clone());
        self.last_switch_time.store(now, Ordering::Relaxed);
        self.total_switches.fetch_add(1, Ordering::Relaxed);

        // Record event
        let event = SwitchEvent {
            from_coin: current_coin,
            to_coin: best.coin.clone(),
            reason: format!(
                "Profitability advantage: +{:.1}% (score {:.1} vs {:.1})",
                advantage_pct, best.profit_score, current_profit
            ),
            profit_advantage_pct: advantage_pct,
            timestamp: now as i64,
        };

        self.switch_history.write().await.push(event);

        // Keep only last 100 events
        let mut history = self.switch_history.write().await;
        if history.len() > 100 {
            let drain_count = history.len() - 100;
            history.drain(..drain_count);
        }
    }

    /// Force switch to a specific coin (manual override via API)
    pub async fn force_switch(&self, coin: &str) -> Result<(), String> {
        let current = self.active_coin.read().await.clone();

        if current.eq_ignore_ascii_case(coin) {
            return Err(format!("Already mining {}", coin));
        }

        info!("🔄 MANUAL SWITCH: {} → {} (forced by API)", current, coin);

        let now = Utc::now().timestamp() as u64;
        *self.active_coin.write().await = coin.to_uppercase();
        let _ = self.coin_tx.send(coin.to_uppercase());
        self.last_switch_time.store(now, Ordering::Relaxed);
        self.total_switches.fetch_add(1, Ordering::Relaxed);

        let event = SwitchEvent {
            from_coin: current,
            to_coin: coin.to_uppercase(),
            reason: "Manual switch via API".to_string(),
            profit_advantage_pct: 0.0,
            timestamp: now as i64,
        };
        self.switch_history.write().await.push(event);

        Ok(())
    }
}
