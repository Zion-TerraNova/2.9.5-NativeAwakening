use serde::Deserialize;
use crate::profit_switcher::ProfitSwitchConfig;
use crate::buyback::BuybackConfig;

#[derive(Deserialize, Clone, Debug)]
pub struct RevenueSettings {
    #[serde(default = "default_revenue_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub streams: StreamsConfig,
    #[serde(default)]
    pub profit_switching: ProfitSwitchConfig,
    #[serde(default)]
    pub buyback: BuybackConfig,
}

impl Default for RevenueSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            streams: StreamsConfig::default(),
            profit_switching: ProfitSwitchConfig::default(),
            buyback: BuybackConfig::default(),
        }
    }
}

fn default_revenue_enabled() -> bool { true }

#[derive(Deserialize, Clone, Debug, Default)]
pub struct StreamsConfig {
    #[serde(default)]
    pub zion: StreamConfig,
    #[serde(default)]
    pub etc: StreamEtcConfig,
    #[serde(default)]
    pub nxs: StreamNxsConfig,
    #[serde(default)]
    pub dynamic_gpu: StreamDynamicGpuConfig,
    #[serde(default)]
    pub ncl: StreamNclConfig,
}

#[derive(Deserialize, Clone, Debug)]
pub struct StreamConfig {
    pub enabled: bool,
    pub target_share: f64,
}

impl Default for StreamConfig {
    fn default() -> Self {
        // CH v3 spec: ZION = 50% of total compute
        Self { enabled: true, target_share: 0.50 }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct StreamEtcConfig {
    pub enabled: bool,
    pub pool: ExternalPoolConfig,
    pub target_share: f64,
    #[serde(default)]
    pub proxy_listen: Option<String>,
}

/// Default BTC wallet for all external pool payouts (2miners BTC payout)
/// P1-22: Read from ZION_BTC_WALLET env var; hardcoded fallback for backward compat
const FALLBACK_BTC_WALLET: &str = "YOUR_BTC_WALLET_ADDRESS";
pub fn default_btc_wallet() -> String {
    std::env::var("ZION_BTC_WALLET").unwrap_or_else(|_| FALLBACK_BTC_WALLET.to_string())
}

/// Default XMR wallet for MoneroOcean mining payouts
/// P1-23: Read from ZION_XMR_WALLET env var; hardcoded fallback for backward compat
const FALLBACK_XMR_WALLET: &str = "YOUR_XMR_WALLET_ADDRESS";
pub fn default_xmr_wallet() -> String {
    std::env::var("ZION_XMR_WALLET").unwrap_or_else(|_| FALLBACK_XMR_WALLET.to_string())
}

impl Default for StreamEtcConfig {
    fn default() -> Self {
        // CH v3 spec: ETC = FREE byproduct (Keccak intermediate from CosmicHarmony)
        // Part of the 25% external pool allocation, but costs 0% extra compute
        Self { 
            enabled: true, 
            pool: ExternalPoolConfig {
                stratum: "stratum+tcp://etc.2miners.com:1010".to_string(),
                wallet: default_btc_wallet(),
                worker: "zion_merged".to_string(),
            },
            target_share: 0.05,
            proxy_listen: None,
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct ExternalPoolConfig {
    pub stratum: String,
    pub wallet: String,
    #[serde(default)]
    pub worker: String,
}

#[derive(Deserialize, Clone, Debug)]
pub struct StreamNxsConfig {
    pub enabled: bool,
    pub pool: ExternalPoolConfig,
    pub target_share: f64,
    #[serde(default)]
    pub proxy_listen: Option<String>,
}

impl Default for StreamNxsConfig {
    fn default() -> Self {
        Self { 
            enabled: false, 
            pool: ExternalPoolConfig {
                stratum: "stratum+tcp://pool.nexus.io:9549".to_string(),
                wallet: default_btc_wallet(),
                worker: "zion_merged".to_string(),
            },
            target_share: 0.0,
            proxy_listen: None,
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct StreamDynamicGpuConfig {
    pub enabled: bool,
    pub mode: String,
    pub target_share: f64,
    #[serde(default)]
    pub pools: std::collections::HashMap<String, StreamDynamicPoolEntry>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct StreamDynamicPoolEntry {
    #[serde(default)]
    pub coin: String,
    pub stratum: String,
    pub wallet: String,
    #[serde(default)]
    pub worker: String,
    pub enabled: bool,
    /// Mining algorithm: "ethash", "kawpow", "kheavyhash", "autolykos", "blake3"
    #[serde(default)]
    pub algorithm: Option<String>,
    /// Stratum protocol: "ethstratum" (default), "stratum" (standard v1), "kaspa"
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub proxy_listen: Option<String>,
}

impl Default for StreamDynamicGpuConfig {
    fn default() -> Self {
        // CH v3 spec: Dynamic GPU = 20% compute (part of 25% external allocation)
        // Profit-switched between ERG/RVN/KAS/ALPH via external pools
        Self { 
            enabled: true, 
            mode: "auto".to_string(),
            target_share: 0.20,
            pools: std::collections::HashMap::new(),
        }
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct StreamNclConfig {
    pub enabled: bool,
    pub npu_allocation: f64,
    pub target_share: f64,
}

impl Default for StreamNclConfig {
    fn default() -> Self {
        // CH v3 spec: NCL AI = 25% of total compute
        // Neural Compute Layer for embeddings, inference, code analysis
        Self { 
            enabled: true, 
            npu_allocation: 0.30,
            target_share: 0.25 
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct Config {
    pub listen: String,
    pub metrics_listen: String,
    pub redis_url: String,
    pub notify_secs: u64,
    pub core_rpc_url: String,
    #[serde(default)]
    pub pool_wallet: String,
    /// Humanitarian tithe destination wallet (10% of block reward)
    #[serde(default)]
    pub humanitarian_wallet: String,
    pub api_listen: String,
    #[serde(default)]
    pub pplns_size: usize,
    #[serde(default)]
    pub pplns_window_shares: usize,
    #[serde(default)]
    pub min_payout: f64,
    #[serde(default)]
    pub max_payout_per_tx: f64,
    #[serde(default)]
    pub payout_interval_seconds: u64,
    #[serde(default)]
    pub payout_batch_limit: usize,
    #[serde(default)]
    pub payout_confirm_timeout_seconds: u64,
    #[serde(default)]
    pub pool_fee_percent: f64,
    /// Humanitarian tithe percentage (default 10%)
    #[serde(default = "default_tithe_percent")]
    pub humanitarian_tithe_percent: f64,
    #[serde(default)]
    pub revenue: RevenueSettings,
}

fn default_tithe_percent() -> f64 { 10.0 }

impl Config {
    pub fn load() -> Self {
        let mut cfg = Self { 
            listen: "0.0.0.0:3333".to_string(), 
            metrics_listen: "0.0.0.0:9100".to_string(), 
            redis_url: "redis://127.0.0.1/".to_string(), 
            notify_secs: 10, 
            core_rpc_url: "http://127.0.0.1:8444/jsonrpc".to_string(), 
            pool_wallet: "ZION_TEST_WALLET".to_string(), 
            humanitarian_wallet: String::new(),
            api_listen: "0.0.0.0:8080".to_string(), 
            pplns_size: 5000, 
            pplns_window_shares: 0, 
            min_payout: 0.1, 
            max_payout_per_tx: 0.0, 
            payout_interval_seconds: 30, 
            payout_batch_limit: 50, 
            payout_confirm_timeout_seconds: 3600, 
            pool_fee_percent: 1.0,
            humanitarian_tithe_percent: 10.0,
            revenue: RevenueSettings::default(),
        };
        if let Ok(l) = std::env::var("ZION_POOL_LISTEN") {
            cfg.listen = l;
        } else if let Ok(l) = std::env::var("ZION_LISTEN") {
            // legacy
            cfg.listen = l;
        }
        if let Ok(m) = std::env::var("ZION_POOL_METRICS") { cfg.metrics_listen = m; }
        if let Ok(r) = std::env::var("ZION_REDIS_URL") {
            cfg.redis_url = r;
        } else if let Ok(r) = std::env::var("REDIS_URL") {
            // legacy
            cfg.redis_url = r;
        }
        if let Ok(n) = std::env::var("ZION_NOTIFY_SECS") { cfg.notify_secs = n.parse().unwrap_or(10); }
        if let Ok(c) = std::env::var("ZION_CORE_RPC") {
            cfg.core_rpc_url = c;
        } else if let Ok(c) = std::env::var("ZION_RPC_URL") {
            // Docker/legacy alias
            cfg.core_rpc_url = c;
        }
        if let Ok(w) = std::env::var("ZION_POOL_WALLET") {
            cfg.pool_wallet = w;
        } else if let Ok(w) = std::env::var("ZION_POOL_ADDRESS") {
            // Docker/legacy alias
            cfg.pool_wallet = w;
        }
        if let Ok(w) = std::env::var("ZION_HUMANITARIAN_WALLET") { cfg.humanitarian_wallet = w; }
        if let Ok(p) = std::env::var("ZION_HUMANITARIAN_TITHE_PERCENT") {
            cfg.humanitarian_tithe_percent = p.parse().unwrap_or(10.0);
        }
        if let Ok(a) = std::env::var("ZION_POOL_API") {
            cfg.api_listen = a;
        } else if let Ok(a) = std::env::var("ZION_API_LISTEN") {
            // legacy
            cfg.api_listen = a;
        }
        if let Ok(p) = std::env::var("ZION_PPLNS_SIZE") { cfg.pplns_size = p.parse().unwrap_or(1024); }
        if let Ok(p) = std::env::var("ZION_PPLNS_WINDOW_SHARES") { cfg.pplns_window_shares = p.parse().unwrap_or(0); }
        if let Ok(p) = std::env::var("ZION_MIN_PAYOUT") {
            cfg.min_payout = p.parse().unwrap_or(0.1);
        } else if let Ok(p) = std::env::var("POOL_MIN_PAYOUT") {
            // legacy
            cfg.min_payout = p.parse().unwrap_or(0.1);
        }
        if let Ok(p) = std::env::var("ZION_MAX_PAYOUT_PER_TX") { cfg.max_payout_per_tx = p.parse().unwrap_or(0.0); }
        if let Ok(p) = std::env::var("ZION_PAYOUT_INTERVAL") { cfg.payout_interval_seconds = p.parse().unwrap_or(30); }
        if let Ok(p) = std::env::var("ZION_PAYOUT_BATCH_LIMIT") { cfg.payout_batch_limit = p.parse().unwrap_or(50); }
        if let Ok(p) = std::env::var("ZION_PAYOUT_CONFIRM_TIMEOUT") { cfg.payout_confirm_timeout_seconds = p.parse().unwrap_or(3600); }
        if let Ok(p) = std::env::var("ZION_POOL_FEE") {
            cfg.pool_fee_percent = p.parse().unwrap_or(1.0);
        } else if let Ok(p) = std::env::var("POOL_FEE") {
            // legacy
            cfg.pool_fee_percent = p.parse().unwrap_or(1.0);
        }
        
        // Load main pool config
        if let Ok(txt) = std::fs::read_to_string("pool_config.json") {
            if let Ok(file_cfg) = serde_json::from_str::<Config>(&txt) {
                let fallback_wallet = cfg.pool_wallet.clone();
                cfg = file_cfg;
                if cfg.pool_wallet.is_empty() {
                    cfg.pool_wallet = fallback_wallet;
                }
            }
        }

        // Load CH v3 Revenue Settings (if available)
        // Checks: env var ZION_REVENUE_CONFIG, local directory, /config/ mount, and up two levels
        let mut revenue_paths: Vec<String> = Vec::new();
        if let Ok(p) = std::env::var("ZION_REVENUE_CONFIG") {
            revenue_paths.push(p);
        }
        revenue_paths.extend([
            "ch3_revenue_settings.json".to_string(),
            "/config/ch3_revenue_settings.json".to_string(),
            "/app/config/ch3_revenue_settings.json".to_string(),
            "../../config/ch3_revenue_settings.json".to_string(),
        ]);
        for path in &revenue_paths {
            if let Ok(txt) = std::fs::read_to_string(path) {
                #[derive(Deserialize, Debug)]
                struct RevenueFile {
                    streams: StreamsConfig,
                }
                
                if let Ok(rev_file) = serde_json::from_str::<RevenueFile>(&txt) {
                    println!("✅ Loaded Revenue Settings from {}", path);
                    cfg.revenue.streams = rev_file.streams;
                    break;
                } else {
                    let err = serde_json::from_str::<RevenueFile>(&txt).unwrap_err();
                    eprintln!("⚠️ Failed to parse revenue settings from {}: {}", path, err);
                }
            }
        }

        if cfg.pplns_window_shares == 0 {
            cfg.pplns_window_shares = cfg.pplns_size;
        }
        if cfg.pplns_window_shares == 0 {
            cfg.pplns_window_shares = 5000;
        }
        if cfg.min_payout <= 0.0 {
            cfg.min_payout = 0.1;
        }
        if cfg.payout_interval_seconds == 0 {
            cfg.payout_interval_seconds = 30;
        }
        if cfg.payout_batch_limit == 0 {
            cfg.payout_batch_limit = 50;
        }
        if cfg.payout_confirm_timeout_seconds == 0 {
            cfg.payout_confirm_timeout_seconds = 3600;
        }
        if cfg.pool_fee_percent <= 0.0 {
            cfg.pool_fee_percent = 1.0;
        }
        if cfg.humanitarian_tithe_percent <= 0.0 {
            cfg.humanitarian_tithe_percent = 10.0;
        }
        // Warn if humanitarian wallet is not configured
        if cfg.humanitarian_wallet.is_empty() {
            eprintln!("⚠️  ZION_HUMANITARIAN_WALLET not set — 10% tithe will accumulate in pool wallet");
        } else {
            println!("✅ Humanitarian tithe wallet: {} ({}%)", cfg.humanitarian_wallet, cfg.humanitarian_tithe_percent);
        }

        // AUDIT-FIX P0-14: Reject startup if pool wallet is a test placeholder on mainnet.
        // This prevents accidental mainnet launches where block rewards go to a dead address.
        let is_mainnet = std::env::var("ZION_NETWORK")
            .unwrap_or_default()
            .to_lowercase() == "mainnet";
        if is_mainnet && (cfg.pool_wallet == "ZION_TEST_WALLET" || cfg.pool_wallet.is_empty()) {
            panic!(
                "🚨 FATAL: ZION_POOL_WALLET must be set to a real wallet address on mainnet! \
                 Current value: '{}'. Set ZION_POOL_WALLET env var and restart.",
                cfg.pool_wallet
            );
        }
        if cfg.pool_wallet == "ZION_TEST_WALLET" {
            eprintln!("⚠️  Pool wallet is 'ZION_TEST_WALLET' — acceptable for testnet/devnet only");
        }

        cfg
    }
}
