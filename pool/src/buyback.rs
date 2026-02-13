//! BTC/XMR Revenue Tracker & Buyback Engine for CH3 External Mining
//!
//! Monitors earnings from ALL external mining pools:
//!   - 2miners (ETC/RVN/KAS/ERG) → BTC payouts
//!   - MoneroOcean (XMR) → XMR payouts (CPU-only mode)
//!
//! Provides framework for automated ZION buyback when
//! DEX/CEX integration becomes available.
//!
//! Current Phase (TestNet): Monitor + Report only
//! Future Phase (MainNet):  Auto-buy ZION on DEX → burn/distribute
//!
//! Architecture:
//!   BuybackEngine ──poll──→ 2miners Dashboard API (BTC)
//!       │                   MoneroOcean API (XMR)
//!       │
//!       ├── Track: cumulative BTC + XMR earned
//!       ├── Track: payouts received (both coins)
//!       ├── Future: Auto market order on DEX/CEX
//!       └── Dashboard: transparent reporting
//!
//! Integration:
//!   - main.rs spawns BuybackEngine::run()
//!   - API endpoint /api/v1/buyback/status exposes state
//!   - Revenue proxy provides per-coin earnings data

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use serde::{Deserialize, Serialize};
use chrono::Utc;

// ═══════════════════════════════════════════════════════════════
// Configuration
// ═══════════════════════════════════════════════════════════════

/// BTC/XMR Revenue Tracker & Buyback engine configuration
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BuybackConfig {
    /// Enable/disable the buyback monitoring
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// BTC wallet address to monitor (2miners GPU coin payouts)
    #[serde(default)]
    pub btc_wallet: String,
    /// XMR wallet address (MoneroOcean CPU mining payouts)
    #[serde(default = "default_xmr_wallet")]
    pub xmr_wallet: String,
    /// How often to check balance (seconds)
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,
    /// Minimum BTC balance to trigger buyback (when enabled)
    #[serde(default = "default_min_buyback")]
    pub min_buyback_btc: f64,
    /// Enable auto-buyback (false = monitor only)
    #[serde(default)]
    pub auto_buyback_enabled: bool,
    /// Target DEX/CEX for buyback (future)
    #[serde(default = "default_exchange")]
    pub exchange: String,
    /// Percentage of BTC to keep as reserve (0-100)
    #[serde(default = "default_reserve")]
    pub reserve_pct: f64,
    /// External pool dashboard URLs to monitor (BTC payouts)
    #[serde(default)]
    pub pool_dashboards: Vec<PoolDashboard>,
    /// XMR pool dashboards to monitor (CPU-only mode)
    #[serde(default)]
    pub xmr_dashboards: Vec<PoolDashboard>,
}

fn default_true() -> bool { true }
fn default_poll_interval() -> u64 { 600 } // 10 minutes
fn default_min_buyback() -> f64 { 0.001 } // 0.001 BTC minimum
fn default_exchange() -> String { "manual".to_string() }
fn default_reserve() -> f64 { 10.0 } // Keep 10% BTC reserve
fn default_xmr_wallet() -> String {
    crate::config::default_xmr_wallet()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PoolDashboard {
    pub name: String,
    pub coin: String,
    pub url: String,
}

impl Default for BuybackConfig {
    fn default() -> Self {
        let btc = crate::config::default_btc_wallet();
        let xmr = crate::config::default_xmr_wallet();
        Self {
            enabled: true,
            btc_wallet: btc.clone(),
            xmr_wallet: xmr.clone(),
            poll_interval_secs: 600,
            min_buyback_btc: 0.001,
            auto_buyback_enabled: false,
            exchange: "manual".to_string(),
            reserve_pct: 10.0,
            pool_dashboards: vec![
                PoolDashboard {
                    name: "2miners-KAS".to_string(),
                    coin: "KAS".to_string(),
                    url: format!("https://kas.2miners.com/api/accounts/{}", btc),
                },
                PoolDashboard {
                    name: "2miners-ETC".to_string(),
                    coin: "ETC".to_string(),
                    url: format!("https://etc.2miners.com/api/accounts/{}", btc),
                },
                PoolDashboard {
                    name: "2miners-RVN".to_string(),
                    coin: "RVN".to_string(),
                    url: format!("https://rvn.2miners.com/api/accounts/{}", btc),
                },
                PoolDashboard {
                    name: "2miners-ERG".to_string(),
                    coin: "ERG".to_string(),
                    url: format!("https://erg.2miners.com/api/accounts/{}", btc),
                },
            ],
            xmr_dashboards: vec![
                PoolDashboard {
                    name: "MoneroOcean".to_string(),
                    coin: "XMR".to_string(),
                    url: format!("https://api.moneroocean.stream/miner/{}/stats", xmr),
                },
            ],
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// Data Structures
// ═══════════════════════════════════════════════════════════════

/// Balance data from an external mining pool (BTC or XMR)
#[derive(Debug, Clone, Serialize, Default)]
pub struct PoolBalance {
    pub pool_name: String,
    /// Mining coin (ETC, RVN, KAS, ERG, XMR)
    pub coin: String,
    /// Payout coin (BTC for 2miners, XMR for MoneroOcean)
    #[serde(default)]
    pub payout_coin: String,
    /// Current unpaid balance (in payout coin units)
    pub balance: f64,
    /// Total paid out
    pub total_paid: f64,
    /// Number of payouts received
    pub payout_count: u64,
    /// Current hashrate reported by pool
    pub current_hashrate: f64,
    /// Average hashrate (24h)
    pub avg_hashrate_24h: f64,
    /// Number of valid shares
    pub valid_shares: u64,
    /// Number of stale shares
    pub stale_shares: u64,
    /// Last update timestamp
    pub last_updated: i64,
    /// Is the data fresh (updated within last poll interval)?
    pub is_fresh: bool,
    /// Last error (if any)
    pub last_error: Option<String>,
}

/// Buyback transaction record
#[derive(Debug, Clone, Serialize)]
pub struct BuybackRecord {
    pub btc_amount: f64,
    pub zion_amount: f64,
    pub btc_price_usd: f64,
    pub zion_price_usd: f64,
    pub exchange: String,
    pub tx_hash: Option<String>,
    pub status: String, // "pending", "completed", "failed"
    pub timestamp: i64,
}

/// 2miners API response structure
#[derive(Debug, Deserialize)]
struct TwoMinersAccountResponse {
    #[serde(default, alias = "currentHashrate")]
    current_hashrate: f64,
    #[serde(default)]
    hashrate: f64,
    #[serde(default, alias = "paymentsTotal")]
    payments_total: u64,
    #[serde(default)]
    stats: TwoMinersStats,
    #[serde(default, alias = "workersOnline")]
    workers_online: u64,
}

#[derive(Debug, Deserialize, Default)]
struct TwoMinersStats {
    #[serde(default)]
    balance: f64,
    #[serde(default)]
    paid: f64,
    #[serde(default, alias = "blocksFound")]
    blocks_found: u64,
    #[serde(default, alias = "immature")]
    immature: f64,
}

// ═══════════════════════════════════════════════════════════════
// MoneroOcean API Response
// ═══════════════════════════════════════════════════════════════

/// MoneroOcean miner stats response
#[derive(Debug, Deserialize)]
struct MoneroOceanStatsResponse {
    #[serde(default)]
    hash: f64,
    #[serde(default)]
    hash2: f64,
    /// Amount due in piconero (1 XMR = 1e12 piconero)
    #[serde(default, alias = "amtDue")]
    amt_due: u64,
    /// Amount paid in piconero
    #[serde(default, alias = "amtPaid")]
    amt_paid: u64,
    /// Number of payout transactions
    #[serde(default, alias = "txnCount")]
    txn_count: u64,
}

/// Fetch miner stats from MoneroOcean API
async fn fetch_moneroocean_stats(client: &reqwest::Client, url: &str) -> Result<MoneroOceanStatsResponse, String> {
    let response = client.get(url).send().await
        .map_err(|e| format!("MoneroOcean request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("MoneroOcean HTTP {}", response.status()));
    }

    let body = response.text().await
        .map_err(|e| format!("MoneroOcean body read failed: {}", e))?;

    serde_json::from_str(&body)
        .map_err(|e| format!("MoneroOcean parse failed: {} (body: {})", e, &body[..body.len().min(200)]))
}

// ═══════════════════════════════════════════════════════════════
// 2miners API Client
// ═══════════════════════════════════════════════════════════════

/// Fetch account stats from 2miners pool API
async fn fetch_2miners_account(client: &reqwest::Client, url: &str) -> Result<TwoMinersAccountResponse, String> {
    let response = client.get(url).send().await
        .map_err(|e| format!("2miners request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("2miners HTTP {}", response.status()));
    }

    let body = response.text().await
        .map_err(|e| format!("2miners body read failed: {}", e))?;

    serde_json::from_str(&body)
        .map_err(|e| format!("2miners parse failed: {} (body: {})", e, &body[..body.len().min(200)]))
}

// ═══════════════════════════════════════════════════════════════
// Buyback Engine
// ═══════════════════════════════════════════════════════════════

/// BTC/XMR Revenue Tracker & Buyback engine
pub struct BuybackEngine {
    config: BuybackConfig,
    /// Pool balances per tracked dashboard (BTC + XMR)
    pool_balances: RwLock<Vec<PoolBalance>>,
    /// Total estimated BTC earnings across all GPU pools
    total_btc_earned: RwLock<f64>,
    /// Total BTC paid out across all GPU pools
    total_btc_paid: RwLock<f64>,
    /// Total estimated XMR earnings (MoneroOcean)
    total_xmr_earned: RwLock<f64>,
    /// Total XMR paid out (MoneroOcean)
    total_xmr_paid: RwLock<f64>,
    /// Buyback transaction history
    buyback_history: RwLock<Vec<BuybackRecord>>,
    /// Number of successful API fetches
    api_fetches: AtomicU64,
    /// Number of failed API fetches
    api_errors: AtomicU64,
    /// Engine start time
    start_time: i64,
}

impl BuybackEngine {
    pub fn new(config: BuybackConfig) -> Arc<Self> {
        let now = Utc::now().timestamp();
        Arc::new(Self {
            config,
            pool_balances: RwLock::new(Vec::new()),
            total_btc_earned: RwLock::new(0.0),
            total_btc_paid: RwLock::new(0.0),
            total_xmr_earned: RwLock::new(0.0),
            total_xmr_paid: RwLock::new(0.0),
            buyback_history: RwLock::new(Vec::new()),
            api_fetches: AtomicU64::new(0),
            api_errors: AtomicU64::new(0),
            start_time: now,
        })
    }

    /// Get current pool balances
    pub async fn pool_balances(&self) -> Vec<PoolBalance> {
        self.pool_balances.read().await.clone()
    }

    /// Get stats as JSON for API
    pub async fn stats_json(&self) -> serde_json::Value {
        let balances = self.pool_balances.read().await.clone();
        let total_earned = *self.total_btc_earned.read().await;
        let total_paid = *self.total_btc_paid.read().await;
        let total_xmr_earned = *self.total_xmr_earned.read().await;
        let total_xmr_paid = *self.total_xmr_paid.read().await;
        let history = self.buyback_history.read().await.clone();

        let pool_stats: Vec<serde_json::Value> = balances.iter().map(|b| {
            serde_json::json!({
                "pool": b.pool_name,
                "coin": b.coin,
                "payout_coin": b.payout_coin,
                "balance": b.balance,
                "total_paid": b.total_paid,
                "payout_count": b.payout_count,
                "hashrate": b.current_hashrate,
                "avg_hashrate_24h": b.avg_hashrate_24h,
                "valid_shares": b.valid_shares,
                "stale_shares": b.stale_shares,
                "is_fresh": b.is_fresh,
                "last_updated": b.last_updated,
                "last_error": b.last_error,
            })
        }).collect();

        let recent_buybacks: Vec<serde_json::Value> = history.iter()
            .rev()
            .take(10)
            .map(|b| serde_json::json!({
                "btc_amount": b.btc_amount,
                "zion_amount": b.zion_amount,
                "exchange": b.exchange,
                "status": b.status,
                "timestamp": b.timestamp,
            }))
            .collect();

        let uptime = Utc::now().timestamp() - self.start_time;

        serde_json::json!({
            "enabled": self.config.enabled,
            "auto_buyback": self.config.auto_buyback_enabled,
            "btc_wallet": self.config.btc_wallet,
            "xmr_wallet": self.config.xmr_wallet,
            "exchange": self.config.exchange,
            "min_buyback_btc": self.config.min_buyback_btc,
            "reserve_pct": self.config.reserve_pct,
            "total_btc_earned": total_earned,
            "total_btc_paid": total_paid,
            "total_btc_pending": total_earned - total_paid,
            "total_xmr_earned": total_xmr_earned,
            "total_xmr_paid": total_xmr_paid,
            "total_xmr_pending": total_xmr_earned - total_xmr_paid,
            "pools": pool_stats,
            "recent_buybacks": recent_buybacks,
            "api_fetches": self.api_fetches.load(Ordering::Relaxed),
            "api_errors": self.api_errors.load(Ordering::Relaxed),
            "uptime_secs": uptime,
        })
    }

    /// Main run loop — periodically check balances
    pub async fn run(self: Arc<Self>) {
        if !self.config.enabled {
            info!("💰 Buyback Engine DISABLED");
            return;
        }

        let btc_pools = self.config.pool_dashboards.len();
        let xmr_pools = self.config.xmr_dashboards.len();

        if btc_pools == 0 && xmr_pools == 0 {
            info!("💰 Buyback Engine: No pool dashboards configured, running in passive mode");
            return;
        }

        info!(
            "💰 Buyback Engine started (btc_wallet={}, xmr_wallet={}, interval={}s, btc_pools={}, xmr_pools={}, auto_buyback={})",
            &self.config.btc_wallet[..12],
            &self.config.xmr_wallet[..12],
            self.config.poll_interval_secs,
            btc_pools,
            xmr_pools,
            self.config.auto_buyback_enabled,
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("HTTP client");

        let mut interval = tokio::time::interval(
            tokio::time::Duration::from_secs(self.config.poll_interval_secs)
        );

        loop {
            interval.tick().await;

            let mut balances = Vec::new();
            let mut total_earned = 0.0_f64;
            let mut total_paid = 0.0_f64;

            for dashboard in &self.config.pool_dashboards {
                let balance = match fetch_2miners_account(&client, &dashboard.url).await {
                    Ok(account) => {
                        self.api_fetches.fetch_add(1, Ordering::Relaxed);

                        // 2miners returns balance in smallest unit (satoshi-like)
                        // For BTC payout mode, balance is in satoshis (1e8)
                        let bal_btc = account.stats.balance / 1e8;
                        let paid_btc = account.stats.paid / 1e8;

                        info!(
                            "💰 {} ({}): balance={:.8} BTC, paid={:.8} BTC, hashrate={:.0}, workers={}",
                            dashboard.name, dashboard.coin,
                            bal_btc, paid_btc,
                            account.current_hashrate,
                            account.workers_online,
                        );

                        total_earned += bal_btc + paid_btc;
                        total_paid += paid_btc;

                        PoolBalance {
                            pool_name: dashboard.name.clone(),
                            coin: dashboard.coin.clone(),
                            payout_coin: "BTC".to_string(),
                            balance: bal_btc,
                            total_paid: paid_btc,
                            payout_count: account.payments_total,
                            current_hashrate: account.current_hashrate,
                            avg_hashrate_24h: account.hashrate,
                            valid_shares: 0, // Not in account endpoint
                            stale_shares: 0,
                            last_updated: Utc::now().timestamp(),
                            is_fresh: true,
                            last_error: None,
                        }
                    }
                    Err(e) => {
                        self.api_errors.fetch_add(1, Ordering::Relaxed);
                        debug!("💰 {} fetch error: {}", dashboard.name, e);

                        PoolBalance {
                            pool_name: dashboard.name.clone(),
                            coin: dashboard.coin.clone(),
                            payout_coin: "BTC".to_string(),
                            last_error: Some(e),
                            is_fresh: false,
                            last_updated: Utc::now().timestamp(),
                            ..Default::default()
                        }
                    }
                };

                balances.push(balance);
            }

            // ── XMR pools (MoneroOcean) ─────────────────────────────
            let mut total_xmr_earned = 0.0_f64;
            let mut total_xmr_paid = 0.0_f64;

            for dashboard in &self.config.xmr_dashboards {
                let balance = match fetch_moneroocean_stats(&client, &dashboard.url).await {
                    Ok(stats) => {
                        self.api_fetches.fetch_add(1, Ordering::Relaxed);

                        // MoneroOcean amounts are in piconero (1 XMR = 1e12 piconero)
                        let due_xmr = stats.amt_due as f64 / 1e12;
                        let paid_xmr = stats.amt_paid as f64 / 1e12;

                        info!(
                            "⛏️  {} ({}): due={:.12} XMR, paid={:.12} XMR, hashrate={:.0}, txns={}",
                            dashboard.name, dashboard.coin,
                            due_xmr, paid_xmr,
                            stats.hash,
                            stats.txn_count,
                        );

                        total_xmr_earned += due_xmr + paid_xmr;
                        total_xmr_paid += paid_xmr;

                        PoolBalance {
                            pool_name: dashboard.name.clone(),
                            coin: dashboard.coin.clone(),
                            payout_coin: "XMR".to_string(),
                            balance: due_xmr,
                            total_paid: paid_xmr,
                            payout_count: stats.txn_count,
                            current_hashrate: stats.hash,
                            avg_hashrate_24h: stats.hash2,
                            valid_shares: 0,
                            stale_shares: 0,
                            last_updated: Utc::now().timestamp(),
                            is_fresh: true,
                            last_error: None,
                        }
                    }
                    Err(e) => {
                        self.api_errors.fetch_add(1, Ordering::Relaxed);
                        debug!("⛏️  {} fetch error: {}", dashboard.name, e);

                        PoolBalance {
                            pool_name: dashboard.name.clone(),
                            coin: dashboard.coin.clone(),
                            payout_coin: "XMR".to_string(),
                            last_error: Some(e),
                            is_fresh: false,
                            last_updated: Utc::now().timestamp(),
                            ..Default::default()
                        }
                    }
                };

                balances.push(balance);
            }

            // Update stored data
            *self.pool_balances.write().await = balances;
            *self.total_btc_earned.write().await = total_earned;
            *self.total_btc_paid.write().await = total_paid;
            *self.total_xmr_earned.write().await = total_xmr_earned;
            *self.total_xmr_paid.write().await = total_xmr_paid;

            // Summary log
            info!(
                "💰 Buyback Summary: BTC earned={:.8}, paid={:.8}, pending={:.8} | XMR earned={:.12}, paid={:.12}, pending={:.12}",
                total_earned, total_paid, total_earned - total_paid,
                total_xmr_earned, total_xmr_paid, total_xmr_earned - total_xmr_paid
            );

            // Check if buyback should trigger
            if self.config.auto_buyback_enabled {
                let pending = total_earned - total_paid;
                let available = pending * (1.0 - self.config.reserve_pct / 100.0);
                if available >= self.config.min_buyback_btc {
                    info!(
                        "💰 BUYBACK TRIGGER: {:.8} BTC available (min={:.8}), but auto-buyback not yet implemented",
                        available, self.config.min_buyback_btc
                    );
                    // TODO: Phase 5.2 — Execute buyback on DEX/CEX
                    // 1. Get current ZION price
                    // 2. Place market order
                    // 3. Record transaction
                    // 4. Optionally burn ZION or distribute
                }
            }
        }
    }
}
