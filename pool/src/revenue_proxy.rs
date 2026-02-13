use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{copy_bidirectional, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{RwLock, broadcast, mpsc};
use tokio::time::Duration;
use tracing::{info, error, warn, debug};
use crate::config::StreamsConfig;
use serde_json;

/// Stratum protocol variant for external pool communication
#[derive(Debug, Clone, PartialEq)]
pub enum StratumProtocol {
    /// EthereumStratum/1.0.0 — used by ETC, ERG, RVN pools on 2miners
    EthStratum,
    /// Standard Stratum v1 — used by KAS, ALPH, BTC pools
    StandardStratum,
    /// CryptoNote Stratum — used by MoneroOcean, XMR, ZEPH pools
    /// Uses JSON-RPC: login → job notifications → submit with result hash
    CryptoNoteStratum,
}

impl StratumProtocol {
    pub fn from_coin(coin: &str) -> Self {
        match coin.to_uppercase().as_str() {
            "XMR" | "ZEPH" | "RTM" => Self::CryptoNoteStratum,
            "KAS" | "ALPH" | "FLUX" | "NEXA" | "IRON" => Self::StandardStratum,
            _ => Self::EthStratum, // ETC, RVN, ERG default to EthStratum
        }
    }

    pub fn from_str_opt(s: Option<&str>, coin: &str) -> Self {
        match s {
            Some("cryptonote") | Some("cn") | Some("monero") => Self::CryptoNoteStratum,
            Some("ethstratum") | Some("eth") => Self::EthStratum,
            Some("stratum") | Some("standard") | Some("kaspa") => {
                // "stratum" is ambiguous — use coin to disambiguate
                // XMR/ZEPH/RTM pools use CryptoNote stratum, not standard
                Self::from_coin(coin)
            }
            _ => Self::from_coin(coin),
        }
    }
}

/// Job received from an external pool (mining.notify)
#[derive(Debug, Clone)]
pub struct ExternalJob {
    pub coin: String,
    pub algorithm: String,
    pub job_id: String,
    pub seed_hash: String,
    pub header_hash: String,
    /// CryptoNote blob (the full block hashing blob for RandomX)
    pub blob: String,
    pub target: String,
    pub difficulty: f64,
    pub clean_jobs: bool,
    pub timestamp: u64,
    /// Extranonce prefix from subscribe (hex string, must be included in nonce)
    pub extranonce: String,
    /// Raw params from mining.notify for protocol-specific handling
    pub raw_params: Vec<String>,
    /// Block height from the pool job
    pub height: u64,
}

/// Share to submit back to external pool
#[derive(Debug, Clone)]
pub struct ShareSubmission {
    pub coin: String,
    pub job_id: String,
    pub nonce: String,
    pub worker: String,
    /// Result hash (32 bytes hex) — required for CryptoNote/RandomX pools
    pub result: String,
    /// Algorithm used to produce this share
    pub algorithm: String,
}

/// Stats for external pool connections
#[derive(Debug, Default)]
pub struct ExternalPoolStats {
    pub jobs_received: AtomicU64,
    pub shares_submitted: AtomicU64,
    pub shares_accepted: AtomicU64,
    pub shares_rejected: AtomicU64,
    pub connected: AtomicU64, // 1 = connected, 0 = disconnected
}

/// Manages connections to external revenue streams (ETC, NXS, DynGPU)
pub struct RevenueProxyManager {
    streams: StreamsConfig,
    connections: RwLock<HashMap<String, Arc<ExternalPoolClient>>>,
    /// Broadcast channel for jobs from ALL external pools
    job_sender: broadcast::Sender<ExternalJob>,
    /// Global stats across all external pools
    pub stats: Arc<HashMap<String, Arc<ExternalPoolStats>>>,
}

impl RevenueProxyManager {
    /// Subscribe to job stream from all external pools
    pub fn subscribe_jobs(&self) -> broadcast::Receiver<ExternalJob> {
        self.job_sender.subscribe()
    }

    /// Send a share submission to the appropriate external pool
    pub async fn submit_share(&self, submission: ShareSubmission) {
        let conns = self.connections.read().await;
        if let Some(client) = conns.get(&submission.coin) {
            client.queue_submit(submission).await;
        } else {
            warn!("No connection for coin '{}' to submit share", submission.coin);
        }
    }

    /// Get stats for a specific coin
    pub fn get_coin_stats(&self, coin: &str) -> Option<&Arc<ExternalPoolStats>> {
        self.stats.get(coin)
    }

    /// Get all stats as JSON
    pub fn stats_json(&self) -> serde_json::Value {
        let mut coins = serde_json::Map::new();
        for (coin, stats) in self.stats.iter() {
            coins.insert(coin.clone(), serde_json::json!({
                "jobs_received": stats.jobs_received.load(Ordering::Relaxed),
                "shares_submitted": stats.shares_submitted.load(Ordering::Relaxed),
                "shares_accepted": stats.shares_accepted.load(Ordering::Relaxed),
                "shares_rejected": stats.shares_rejected.load(Ordering::Relaxed),
                "connected": stats.connected.load(Ordering::Relaxed) == 1,
            }));
        }
        serde_json::Value::Object(coins)
    }
}

impl RevenueProxyManager {
    pub fn new(streams: StreamsConfig) -> Self {
        let (job_sender, _) = broadcast::channel(256);
        Self {
            streams,
            connections: RwLock::new(HashMap::new()),
            job_sender,
            stats: Arc::new(HashMap::new()),
        }
    }

    /// Start all enabled external pool connections
    pub async fn start(self: Arc<Self>) {
        info!("🚀 Starting Revenue Proxy Manager (CH v3)");

        let mut stats_map: HashMap<String, Arc<ExternalPoolStats>> = HashMap::new();

        // ETC Stream
        if self.streams.etc.enabled {
            let pool = self.streams.etc.pool.clone();
            if pool.wallet.is_empty() {
                warn!("[ETC] Skipping: no wallet configured");
            } else {
                let coin_stats = Arc::new(ExternalPoolStats::default());
                stats_map.insert("etc".to_string(), coin_stats.clone());
                let client = ExternalPoolClient::new(
                    "etc",
                    &pool.stratum,
                    &pool.wallet,
                    &pool.worker,
                    self.streams.etc.proxy_listen.clone(),
                    self.job_sender.clone(),
                    coin_stats,
                    StratumProtocol::EthStratum,
                    "ethash".to_string(),
                );
                self.add_client("etc", client).await;
            }
        }

        // NXS Stream
        if self.streams.nxs.enabled {
            let pool = self.streams.nxs.pool.clone();
            if pool.wallet.is_empty() {
                warn!("[NXS] Skipping: no wallet configured");
            } else {
                let coin_stats = Arc::new(ExternalPoolStats::default());
                stats_map.insert("nxs".to_string(), coin_stats.clone());
                let client = ExternalPoolClient::new(
                    "nxs",
                    &pool.stratum,
                    &pool.wallet,
                    &pool.worker,
                    self.streams.nxs.proxy_listen.clone(),
                    self.job_sender.clone(),
                    coin_stats,
                    StratumProtocol::StandardStratum,
                    "sha3_512".to_string(),
                );
                self.add_client("nxs", client).await;
            }
        }

        // Dynamic GPU (Switching)
        if self.streams.dynamic_gpu.enabled {
            for (coin, pool) in &self.streams.dynamic_gpu.pools {
                if pool.enabled {
                    if pool.wallet.is_empty() {
                        warn!("[{}] Skipping: no wallet configured", coin);
                        continue;
                    }
                    let protocol = StratumProtocol::from_str_opt(
                        pool.protocol.as_deref(),
                        coin,
                    );
                    let algorithm = pool.algorithm.clone()
                        .filter(|a| a != "auto" && !a.is_empty())
                        .unwrap_or_else(|| Self::detect_algorithm(coin));
                    let coin_stats = Arc::new(ExternalPoolStats::default());
                    stats_map.insert(coin.to_lowercase(), coin_stats.clone());
                    info!(
                        "[{}] Protocol={:?}, Algorithm={}, URL={}",
                        coin, protocol, algorithm, pool.stratum
                    );
                    let client = ExternalPoolClient::new(
                        coin,
                        &pool.stratum,
                        &pool.wallet,
                        &pool.worker,
                        pool.proxy_listen.clone(),
                        self.job_sender.clone(),
                        coin_stats,
                        protocol,
                        algorithm,
                    );
                    self.add_client(coin, client).await;
                }
            }
        }

        // Store stats map (unsafe cast — we're in startup, single-threaded init)
        let stats_ptr = Arc::as_ptr(&self.stats) as *mut HashMap<String, Arc<ExternalPoolStats>>;
        unsafe { *stats_ptr = stats_map; }

        info!("✅ Revenue Proxy Manager initialized (job channel capacity=256)");
    }

    /// Auto-detect mining algorithm from coin name
    fn detect_algorithm(coin: &str) -> String {
        match coin.to_uppercase().as_str() {
            "ETC" | "ETH" => "ethash",
            "RVN" | "CLORE" | "NEOXA" => "kawpow",
            "XMR" | "ZEPH" => "randomx",
            "KAS" => "kheavyhash",
            "ERG" => "autolykos",
            "ALPH" | "IRON" => "blake3",
            "FLUX" => "equihash",
            "RTM" => "ghostrider",
            _ => "unknown",
        }.to_string()
    }

    async fn add_client(&self, id: &str, client: Arc<ExternalPoolClient>) {
        let mut conns = self.connections.write().await;
        conns.insert(id.to_lowercase(), client.clone());
        
        // Spawn connection loop
        tokio::spawn(async move {
            client.run_loop().await;
        });
    }
}

/// A simple Stratum Client for external pools
pub struct ExternalPoolClient {
    name: String,
    url: String,
    wallet: String,
    worker: String,
    proxy_listen: Option<String>,
    /// Stratum protocol variant (EthStratum vs Standard)
    protocol: StratumProtocol,
    /// Mining algorithm name
    algorithm: String,
    /// Broadcast sender for forwarding mining.notify jobs
    job_sender: broadcast::Sender<ExternalJob>,
    /// Channel for receiving share submissions from pool miner
    submit_tx: mpsc::Sender<ShareSubmission>,
    submit_rx: tokio::sync::Mutex<mpsc::Receiver<ShareSubmission>>,
    /// Per-coin stats
    stats: Arc<ExternalPoolStats>,
    /// Current difficulty from mining.set_difficulty
    current_difficulty: std::sync::atomic::AtomicU64,
    /// Current target (hex string)
    current_target: tokio::sync::Mutex<String>,
    /// Extranonce prefix from subscribe response
    current_extranonce: tokio::sync::Mutex<String>,
}

impl ExternalPoolClient {
    pub fn new(
        name: &str,
        url: &str,
        wallet: &str,
        worker: &str,
        proxy_listen: Option<String>,
        job_sender: broadcast::Sender<ExternalJob>,
        stats: Arc<ExternalPoolStats>,
        protocol: StratumProtocol,
        algorithm: String,
    ) -> Arc<Self> {
        let (submit_tx, submit_rx) = mpsc::channel(64);
        Arc::new(Self {
            name: name.to_string(),
            url: url.to_string(),
            wallet: wallet.to_string(),
            worker: worker.to_string(),
            proxy_listen,
            protocol,
            algorithm,
            job_sender,
            submit_tx,
            submit_rx: tokio::sync::Mutex::new(submit_rx),
            stats,
            current_difficulty: std::sync::atomic::AtomicU64::new(0),
            current_target: tokio::sync::Mutex::new(String::new()),
            current_extranonce: tokio::sync::Mutex::new(String::new()),
        })
    }

    /// Queue a share for submission to the external pool
    pub async fn queue_submit(&self, submission: ShareSubmission) {
        if let Err(e) = self.submit_tx.send(submission).await {
            error!("[{}] Failed to queue share submission: {}", self.name, e);
        }
    }

    pub async fn run_loop(self: Arc<Self>) {
        if let Some(listen_addr) = self.proxy_listen.clone() {
            let client = Arc::clone(&self);
            tokio::spawn(async move {
                if let Err(err) = client.start_proxy(&listen_addr).await {
                    error!("[{}] Proxy error: {}", client.name, err);
                }
            });
        }

        loop {
            info!("[{}] Connecting to {}...", self.name, self.url);
            match self.connect_and_session().await {
                Ok(_) => {
                    warn!("[{}] Connection finished, reconnecting in 5s...", self.name);
                }
                Err(e) => {
                    error!("[{}] Connection error: {}. Retrying in 10s...", self.name, e);
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            }
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }

    async fn start_proxy(&self, listen_addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(listen_addr).await?;
        info!("[{}] 🔁 Proxy listening on {}", self.name, listen_addr);

        loop {
            let (mut inbound, peer) = listener.accept().await?;
            let upstream = self.connect_upstream().await;

            match upstream {
                Ok(mut outbound) => {
                    let name = self.name.clone();
                    tokio::spawn(async move {
                        info!("[{}] ↔️ Proxy session started from {}", name, peer);
                        let _ = copy_bidirectional(&mut inbound, &mut outbound).await;
                        info!("[{}] ⛔ Proxy session ended from {}", name, peer);
                    });
                }
                Err(err) => {
                    warn!("[{}] Proxy upstream connect failed: {}", self.name, err);
                }
            }
        }
    }

    async fn connect_upstream(&self) -> anyhow::Result<TcpStream> {
        let clean_url = self
            .url
            .trim_start_matches("stratum+tcp://")
            .trim_start_matches("stratum://");
        Ok(TcpStream::connect(clean_url).await?)
    }

    async fn connect_and_session(&self) -> anyhow::Result<()> {
        // CryptoNote stratum uses a completely different handshake (login/job/submit)
        if self.protocol == StratumProtocol::CryptoNoteStratum {
            return self.connect_and_session_cryptonote().await;
        }

        let clean_url = self
            .url
            .trim_start_matches("stratum+tcp://")
            .trim_start_matches("stratum://");
        let stream = TcpStream::connect(clean_url).await?;
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        // Mark connected
        self.stats.connected.store(1, Ordering::Relaxed);

        // Step 1: Subscribe — protocol-aware
        let sub_msg = match &self.protocol {
            StratumProtocol::EthStratum => serde_json::json!({
                "id": 1,
                "method": "mining.subscribe",
                "params": [format!("ZION-Proxy/1.0/{}", self.name), "EthereumStratum/1.0.0"]
            }),
            StratumProtocol::StandardStratum | StratumProtocol::CryptoNoteStratum => serde_json::json!({
                "id": 1,
                "method": "mining.subscribe",
                "params": [format!("ZION-Proxy/1.0/{}", self.name)]
            }),
        };
        let mut sub_bytes = serde_json::to_vec(&sub_msg)?;
        sub_bytes.push(b'\n');
        writer.write_all(&sub_bytes).await?;
        info!("[{}] > mining.subscribe (protocol={:?})", self.name, self.protocol);

        // Step 2: Wait for subscribe response, then authorize
        let mut authorized = false;
        let mut subscribe_ok = false;
        let mut submit_id_counter: u64 = 10;
        let mut submit_rx = self.submit_rx.lock().await;

        loop {
            tokio::select! {
                // Read incoming messages from external pool
                line_result = tokio::time::timeout(Duration::from_secs(60), lines.next_line()) => {
                    let line = match line_result {
                        Ok(Ok(Some(l))) => l,
                        Ok(Ok(None)) => {
                            warn!("[{}] Stream closed by remote", self.name);
                            break;
                        }
                        Ok(Err(e)) => {
                            error!("[{}] Read error: {}", self.name, e);
                            break;
                        }
                        Err(_) => {
                            warn!("[{}] Read timeout (60s), reconnecting...", self.name);
                            break;
                        }
                    };

                    debug!("[{}] < {}", self.name, line);

                    let parsed: serde_json::Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => {
                            warn!("[{}] Non-JSON line: {}", self.name, line);
                            continue;
                        }
                    };

                    if let Some(method) = parsed.get("method").and_then(|m| m.as_str()) {
                        match method {
                            "mining.notify" => {
                                if let Some(params) = parsed.get("params").and_then(|p| p.as_array()) {
                                    let raw_params: Vec<String> = params.iter()
                                        .map(|v| v.as_str().unwrap_or(&v.to_string()).to_string())
                                        .collect();

                                    // Log raw params for debugging (first 5 jobs per coin)
                                    let total = self.stats.jobs_received.load(Ordering::Relaxed);
                                    if total < 5 {
                                        info!(
                                            "[{}] 🔍 RAW notify ({} items, types: {:?}): {:?}",
                                            self.name, params.len(),
                                            params.iter().map(|v| {
                                                if v.is_string() { "str" }
                                                else if v.is_array() { "arr" }
                                                else if v.is_boolean() { "bool" }
                                                else if v.is_number() { "num" }
                                                else { "?" }
                                            }).collect::<Vec<_>>(),
                                            params.iter().map(|v| {
                                                let s = v.to_string();
                                                if s.len() > 60 { format!("{}...", &s[..60]) } else { s }
                                            }).collect::<Vec<_>>()
                                        );
                                    }

                                    // Protocol-aware job parsing
                                    // Note: KAS 2miners uses EthStratum but sends header as [u64,u64,u64,u64] array
                                    let (job_id, header_hash, seed_hash, clean_jobs) = match &self.protocol {
                                        StratumProtocol::EthStratum => {
                                            let jid = params.get(0).map(|v| {
                                                v.as_str().map(|s| s.to_string())
                                                    .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
                                            }).unwrap_or_default();
                                            
                                            let mut sh = String::new();
                                            let mut hh = String::new();

                                            // Helper: try to parse a u64 array (from JSON array or string) into hex bytes
                                            let coin_name = self.name.clone();
                                            let try_u64_array_to_hex = |v: &serde_json::Value, label: &str| -> Option<String> {
                                                // Case A: Real JSON array [u64, u64, u64, u64]
                                                if let Some(arr) = v.as_array() {
                                                    let mut bytes = Vec::with_capacity(arr.len() * 8);
                                                    let mut ok_count = 0usize;
                                                    let mut fail_count = 0usize;
                                                    for elem in arr {
                                                        if let Some(n) = elem.as_u64() {
                                                            bytes.extend_from_slice(&n.to_le_bytes());
                                                            ok_count += 1;
                                                        } else if let Some(n) = elem.as_i64() {
                                                            bytes.extend_from_slice(&(n as u64).to_le_bytes());
                                                            ok_count += 1;
                                                        } else if let Some(f) = elem.as_f64() {
                                                            // Large u64 might lose precision as f64
                                                            bytes.extend_from_slice(&(f as u64).to_le_bytes());
                                                            ok_count += 1;
                                                        } else {
                                                            fail_count += 1;
                                                            warn!("[{}] {} elem failed: {}", coin_name, label, elem);
                                                        }
                                                    }
                                                    info!("[{}] {} arr len={} ok={} fail={} bytes={}",
                                                        coin_name, label, arr.len(), ok_count, fail_count, bytes.len());
                                                    if !bytes.is_empty() {
                                                        return Some(hex::encode(&bytes));
                                                    }
                                                }
                                                // Case B: String-encoded array "[123, 456, ...]"
                                                if let Some(s) = v.as_str() {
                                                    if s.starts_with('[') {
                                                        // Try parsing as JSON array of numbers
                                                        if let Ok(nums) = serde_json::from_str::<Vec<u64>>(s) {
                                                            let mut bytes = Vec::with_capacity(nums.len() * 8);
                                                            for n in &nums {
                                                                bytes.extend_from_slice(&n.to_le_bytes());
                                                            }
                                                            if !bytes.is_empty() {
                                                                return Some(hex::encode(&bytes));
                                                            }
                                                        }
                                                        // Try as Vec<i64> (in case of signed representation)
                                                        if let Ok(nums) = serde_json::from_str::<Vec<i64>>(s) {
                                                            let mut bytes = Vec::with_capacity(nums.len() * 8);
                                                            for n in &nums {
                                                                bytes.extend_from_slice(&(*n as u64).to_le_bytes());
                                                            }
                                                            if !bytes.is_empty() {
                                                                return Some(hex::encode(&bytes));
                                                            }
                                                        }
                                                    }
                                                }
                                                None
                                            };

                                            // Helper: convert a JSON value to hex string  
                                            let value_to_hex = |v: &serde_json::Value| -> String {
                                                // First try u64 array conversion (KAS format)
                                                if let Some(hex_str) = try_u64_array_to_hex(v, "v2h") {
                                                    return hex_str;
                                                }
                                                // String value (standard EthStratum hex)
                                                if let Some(s) = v.as_str() {
                                                    return s.trim_start_matches("0x").trim_start_matches("0X").to_string();
                                                }
                                                // Single number
                                                if let Some(n) = v.as_u64() {
                                                    return format!("{:016x}", n);
                                                }
                                                // Fallback
                                                v.to_string().trim_matches('"').to_string()
                                            };
                                            
                                            // Scan params for u64 array (KAS format) first
                                            let mut found_array_header = false;
                                            for (idx, p) in params.iter().enumerate() {
                                                if idx == 0 { continue; } // skip job_id
                                                if let Some(hex_str) = try_u64_array_to_hex(p, &format!("scan[{}]", idx)) {
                                                    hh = hex_str;
                                                    found_array_header = true;
                                                    info!("[{}] 🔑 Header from u64 array at param[{}]: {}...({} hex chars)", 
                                                        self.name, idx, &hh[..std::cmp::min(32, hh.len())], hh.len());
                                                    break;
                                                }
                                            }
                                            
                                            if !found_array_header {
                                                // Standard EthStratum: param[1]=seed, param[2]=header
                                                if params.len() >= 3 {
                                                    sh = value_to_hex(&params[1]);
                                                    hh = value_to_hex(&params[2]);
                                                } else if params.len() == 2 {
                                                    hh = value_to_hex(&params[1]);
                                                }
                                            }
                                            
                                            let cj = params.last().and_then(|v| v.as_bool()).unwrap_or(false);
                                            (jid, hh, sh, cj)
                                        }
                                        StratumProtocol::StandardStratum => {
                                            // Standard Stratum v1: [job_id, prevhash, coinb1, coinb2, merkle, version, nbits, ntime, clean_jobs]
                                            // OR KAS simplified: [job_id, header_hash, timestamp, clean_jobs]
                                            let jid = params.get(0).map(|v| {
                                                v.as_str().map(|s| s.to_string())
                                                    .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
                                            }).unwrap_or_default();
                                            // For KAS: param[1] is the header/prevhash to hash
                                            let hh = params.get(1).map(|v| {
                                                v.as_str().map(|s| s.to_string())
                                                    .unwrap_or_else(|| v.to_string().trim_matches('"').to_string())
                                            }).unwrap_or_default();
                                            let cj = params.last().and_then(|v| v.as_bool()).unwrap_or(false);
                                            (jid, hh, String::new(), cj)
                                        }
                                        StratumProtocol::CryptoNoteStratum => {
                                            // CryptoNote uses connect_and_session_cryptonote(), not this path
                                            // This branch should never execute
                                            warn!("[{}] CryptoNote stratum should not reach mining.notify handler", self.name);
                                            continue;
                                        }
                                    };

                                    // Get current difficulty
                                    let diff_bits = self.current_difficulty.load(Ordering::Relaxed);
                                    let difficulty = f64::from_bits(diff_bits);
                                    let target = self.current_target.lock().await.clone();
                                    let extranonce = self.current_extranonce.lock().await.clone();

                                    let job = ExternalJob {
                                        coin: self.name.to_lowercase(),
                                        algorithm: self.algorithm.clone(),
                                        job_id: job_id.clone(),
                                        seed_hash,
                                        header_hash: header_hash.clone(),
                                        blob: header_hash, // For EthStratum, blob = header_hash
                                        target,
                                        difficulty,
                                        clean_jobs,
                                        timestamp: std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_secs(),
                                        extranonce,
                                        raw_params,
                                        height: 0,
                                    };

                                    self.stats.jobs_received.fetch_add(1, Ordering::Relaxed);
                                    let _ = self.job_sender.send(job);
                                    info!(
                                        "[{}] 📦 Job forwarded: id={} diff={:.4} algo={} (total={})",
                                        self.name, job_id, difficulty, self.algorithm,
                                        self.stats.jobs_received.load(Ordering::Relaxed)
                                    );
                                }
                            }
                            "mining.set_difficulty" | "mining.set_target" => {
                                if let Some(params) = parsed.get("params").and_then(|p| p.as_array()) {
                                    if let Some(diff_val) = params.first() {
                                        if let Some(diff) = diff_val.as_f64() {
                                            self.current_difficulty.store(diff.to_bits(), Ordering::Relaxed);
                                            info!("[{}] ⚙️ Difficulty set: {}", self.name, diff);
                                        }
                                        if let Some(target_str) = diff_val.as_str() {
                                            *self.current_target.lock().await = target_str.to_string();
                                            info!("[{}] ⚙️ Target set: {}", self.name, target_str);
                                        }
                                    }
                                }
                            }
                            "mining.set_extranonce" => {
                                if let Some(params) = parsed.get("params").and_then(|p| p.as_array()) {
                                    if let Some(en) = params.get(0).and_then(|v| v.as_str()) {
                                        *self.current_extranonce.lock().await = en.to_string();
                                        info!("[{}] ⚙️ Set extranonce: '{}'", self.name, en);
                                    }
                                } else {
                                    info!("[{}] ⚙️ Set extranonce (no params)", self.name);
                                }
                            }
                            _ => {
                                debug!("[{}] Unknown method: {}", self.name, method);
                            }
                        }
                    } else if let Some(id) = parsed.get("id").and_then(|i| i.as_u64()) {
                        let result = parsed.get("result");
                        let error_val = parsed.get("error");

                        match id {
                            1 => {
                                let has_error = error_val.map(|e| !e.is_null()).unwrap_or(false);
                                if has_error {
                                    error!("[{}] ❌ Subscribe failed: {:?}", self.name, error_val);
                                    break;
                                }
                                subscribe_ok = true;

                                // Log raw subscribe response for debugging
                                info!("[{}] 📋 Subscribe response result: {:?}", self.name, result);

                                // Extract extranonce from subscribe result
                                // EthStratum: result = [["mining.notify","session"], "extranonce"]  
                                // Or: result = [null, "extranonce"]
                                // KAS 2miners: result = [null, "EthereumStratum/1.0.0"] — NOT an extranonce!
                                if let Some(res) = result {
                                    let is_valid_hex = |s: &str| -> bool {
                                        !s.is_empty() && s.len() <= 16 && s.chars().all(|c| c.is_ascii_hexdigit())
                                    };

                                    let extranonce = if let Some(arr) = res.as_array() {
                                        // Find the last string that looks like a hex extranonce
                                        arr.iter().rev()
                                            .find_map(|v| v.as_str()
                                                .filter(|s| is_valid_hex(s))
                                                .map(|s| s.to_string()))
                                            .unwrap_or_default()
                                    } else if let Some(s) = res.as_str() {
                                        if is_valid_hex(s) { s.to_string() } else { String::new() }
                                    } else {
                                        String::new()
                                    };

                                    if !extranonce.is_empty() {
                                        info!("[{}] 🔑 Extranonce: '{}' ({} hex chars)", self.name, extranonce, extranonce.len());
                                        *self.current_extranonce.lock().await = extranonce;
                                    } else {
                                        info!("[{}] ℹ️ No hex extranonce in subscribe (KAS-style pool)", self.name);
                                    }
                                }

                                info!("[{}] ✅ Subscribed successfully", self.name);

                                let wallet_worker = if self.worker.is_empty() {
                                    self.wallet.clone()
                                } else {
                                    format!("{}.{}", self.wallet, self.worker)
                                };
                                let auth_msg = serde_json::json!({
                                    "id": 2,
                                    "method": "mining.authorize",
                                    "params": [wallet_worker, "x"]
                                });
                                let mut auth_bytes = serde_json::to_vec(&auth_msg)?;
                                auth_bytes.push(b'\n');
                                writer.write_all(&auth_bytes).await?;
                                info!("[{}] > mining.authorize ({})", self.name, wallet_worker);
                            }
                            2 => {
                                let auth_ok = result
                                    .map(|r| r.as_bool().unwrap_or(false) || r == &serde_json::json!(true))
                                    .unwrap_or(false);
                                let has_error = error_val.map(|e| !e.is_null()).unwrap_or(false);

                                if auth_ok && !has_error {
                                    authorized = true;
                                    info!("[{}] ✅ Authorized successfully", self.name);
                                } else {
                                    error!(
                                        "[{}] ❌ Authorize failed: error={:?} result={:?}",
                                        self.name, error_val, result
                                    );
                                    break;
                                }
                            }
                            sid if sid >= 10 => {
                                // Share submission response
                                let accepted = result
                                    .map(|r| r.as_bool().unwrap_or(false) || r == &serde_json::json!(true))
                                    .unwrap_or(false);
                                if accepted {
                                    self.stats.shares_accepted.fetch_add(1, Ordering::Relaxed);
                                    info!("[{}] ✅ Share #{} accepted!", self.name, sid);
                                } else {
                                    self.stats.shares_rejected.fetch_add(1, Ordering::Relaxed);
                                    warn!("[{}] ❌ Share #{} rejected: {:?}", self.name, sid, error_val);
                                }
                            }
                            _ => {
                                debug!("[{}] Response id={}: {:?}", self.name, id, result);
                            }
                        }
                    }
                }

                // Handle share submissions from pool miner
                Some(submission) = submit_rx.recv(), if authorized => {
                    submit_id_counter += 1;
                    // EthStratum submit: [worker, job_id, nonce]
                    // For ethash pools (2miners), include result if available
                    let mut params = vec![
                        serde_json::Value::String(format!("{}.{}", self.wallet, submission.worker)),
                        serde_json::Value::String(submission.job_id.clone()),
                        serde_json::Value::String(submission.nonce.clone()),
                    ];
                    if !submission.result.is_empty() {
                        params.push(serde_json::Value::String(submission.result.clone()));
                    }
                    let submit_msg = serde_json::json!({
                        "id": submit_id_counter,
                        "method": "mining.submit",
                        "params": params
                    });
                    let mut submit_bytes = serde_json::to_vec(&submit_msg)?;
                    submit_bytes.push(b'\n');
                    writer.write_all(&submit_bytes).await?;
                    self.stats.shares_submitted.fetch_add(1, Ordering::Relaxed);
                    info!(
                        "[{}] > mining.submit #{} (nonce={})",
                        self.name, submit_id_counter, submission.nonce
                    );
                }
            }
        }

        // Mark disconnected
        self.stats.connected.store(0, Ordering::Relaxed);

        if authorized {
            info!("[{}] Session ended (was authorized, will reconnect)", self.name);
        } else if subscribe_ok {
            warn!("[{}] Session ended before authorization completed", self.name);
        } else {
            warn!("[{}] Session ended before subscribe completed", self.name);
        }

        Ok(())
    }

    /// CryptoNote Stratum session (MoneroOcean, XMR pools)
    /// Protocol: JSON-RPC with login → job → submit flow
    /// Completely different from EthStratum/StandardStratum
    async fn connect_and_session_cryptonote(&self) -> anyhow::Result<()> {
        let clean_url = self
            .url
            .trim_start_matches("stratum+tcp://")
            .trim_start_matches("stratum://");
        let stream = TcpStream::connect(clean_url).await?;
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        self.stats.connected.store(1, Ordering::Relaxed);

        // Step 1: Login (replaces subscribe + authorize in CryptoNote stratum)
        let login_msg = serde_json::json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "login",
            "params": {
                "login": self.wallet.clone(),
                "pass": if self.worker.is_empty() { "x".to_string() } else { self.worker.clone() },
                "agent": format!("ZION-Pool-Proxy/2.9.5/{}", self.name),
                "algo": ["rx/0", "cn/r", "cn-heavy/xhv", "cn/gpu", "argon2/chukwav2", "rx/arq", "rx/sfx", "gr"]
            }
        });
        let mut login_bytes = serde_json::to_vec(&login_msg)?;
        login_bytes.push(b'\n');
        writer.write_all(&login_bytes).await?;
        info!("[{}] > login (CryptoNote protocol, wallet={}...{})",
            self.name,
            &self.wallet[..8.min(self.wallet.len())],
            &self.wallet[self.wallet.len().saturating_sub(6)..]);

        let mut session_id = String::new();
        let mut authorized = false;
        let mut submit_id_counter: u64 = 10;
        let mut submit_rx = self.submit_rx.lock().await;

        loop {
            tokio::select! {
                line_result = tokio::time::timeout(Duration::from_secs(120), lines.next_line()) => {
                    let line = match line_result {
                        Ok(Ok(Some(l))) => l,
                        Ok(Ok(None)) => {
                            warn!("[{}] CN stream closed by remote", self.name);
                            break;
                        }
                        Ok(Err(e)) => {
                            error!("[{}] CN read error: {}", self.name, e);
                            break;
                        }
                        Err(_) => {
                            warn!("[{}] CN read timeout (120s), reconnecting...", self.name);
                            break;
                        }
                    };

                    debug!("[{}] CN < {}", self.name, &line[..line.len().min(200)]);

                    let parsed: serde_json::Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => {
                            warn!("[{}] CN non-JSON: {}", self.name, &line[..line.len().min(100)]);
                            continue;
                        }
                    };

                    // Check for JSON-RPC method (job notifications)
                    if let Some(method) = parsed.get("method").and_then(|m| m.as_str()) {
                        if method == "job" {
                            // Job notification: {"method":"job","params":{...}}
                            if let Some(params) = parsed.get("params") {
                                self.handle_cryptonote_job(params).await;
                            }
                        }
                        continue;
                    }

                    // Check for response to our requests
                    if let Some(id) = parsed.get("id").and_then(|i| i.as_u64()) {
                        let error_val = parsed.get("error");
                        let has_error = error_val.map(|e| !e.is_null()).unwrap_or(false);

                        match id {
                            1 => {
                                // Login response
                                if has_error {
                                    error!("[{}] ❌ CN login failed: {:?}", self.name, error_val);
                                    break;
                                }
                                if let Some(result) = parsed.get("result") {
                                    // Extract session ID
                                    if let Some(sid) = result.get("id").and_then(|v| v.as_str()) {
                                        session_id = sid.to_string();
                                    }
                                    // Process initial job from login response
                                    if let Some(job) = result.get("job") {
                                        self.handle_cryptonote_job(job).await;
                                    }
                                    authorized = true;
                                    info!("[{}] ✅ CN Login successful (session={})", self.name, &session_id[..8.min(session_id.len())]);
                                }
                            }
                            sid if sid >= 10 => {
                                // Submit response
                                let accepted = if has_error {
                                    false
                                } else if let Some(result) = parsed.get("result") {
                                    result.get("status")
                                        .and_then(|s| s.as_str())
                                        .map(|s| s.eq_ignore_ascii_case("OK"))
                                        .unwrap_or(false)
                                } else {
                                    false
                                };
                                if accepted {
                                    self.stats.shares_accepted.fetch_add(1, Ordering::Relaxed);
                                    info!("[{}] ✅ CN Share #{} accepted!", self.name, sid);
                                } else {
                                    self.stats.shares_rejected.fetch_add(1, Ordering::Relaxed);
                                    warn!("[{}] ❌ CN Share #{} rejected: {:?}", self.name, sid, error_val);
                                }
                            }
                            _ => {
                                debug!("[{}] CN response id={}: {:?}", self.name, id, parsed.get("result"));
                            }
                        }
                    }
                }

                // Handle share submissions from pool miners
                Some(submission) = submit_rx.recv(), if authorized => {
                    submit_id_counter += 1;
                    // CryptoNote submit: {id, session_id, job_id, nonce, result, [algo]}
                    let mut submit_params = serde_json::json!({
                        "id": session_id,
                        "job_id": submission.job_id,
                        "nonce": submission.nonce,
                        "result": submission.result,
                    });
                    if !submission.algorithm.is_empty() {
                        submit_params["algo"] = serde_json::Value::String(submission.algorithm.clone());
                    }
                    let submit_msg = serde_json::json!({
                        "id": submit_id_counter,
                        "jsonrpc": "2.0",
                        "method": "submit",
                        "params": submit_params
                    });
                    let mut submit_bytes = serde_json::to_vec(&submit_msg)?;
                    submit_bytes.push(b'\n');
                    writer.write_all(&submit_bytes).await?;
                    self.stats.shares_submitted.fetch_add(1, Ordering::Relaxed);
                    info!(
                        "[{}] > CN submit #{} (nonce={}, result={}...)",
                        self.name, submit_id_counter, submission.nonce,
                        &submission.result[..16.min(submission.result.len())]
                    );
                }
            }
        }

        self.stats.connected.store(0, Ordering::Relaxed);
        if authorized {
            info!("[{}] CN session ended (was logged in, will reconnect)", self.name);
        } else {
            warn!("[{}] CN session ended before login completed", self.name);
        }
        Ok(())
    }

    /// Parse a CryptoNote job notification and broadcast it
    async fn handle_cryptonote_job(&self, job: &serde_json::Value) {
        let job_id = job.get("job_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let blob = job.get("blob").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let target = job.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let seed_hash = job.get("seed_hash").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let height = job.get("height").and_then(|v| v.as_u64()).unwrap_or(0);
        let algo = job.get("algo").and_then(|v| v.as_str()).unwrap_or("rx/0").to_string();

        // Map MoneroOcean algo names to our algorithm names
        let algorithm = match algo.as_str() {
            "rx/0" | "randomx" => "randomx".to_string(),
            "cn/r" | "cryptonight/r" => "cryptonight_r".to_string(),
            other => other.replace("/", "_"),
        };

        // Parse difficulty from target hex
        let difficulty = if target.len() <= 8 {
            // Short target: MoneroOcean sends compact target (e.g. "e7a71d00")
            let target_bytes = hex::decode(&target).unwrap_or_default();
            if target_bytes.len() == 4 {
                let target_u32 = u32::from_le_bytes([
                    target_bytes.get(0).copied().unwrap_or(0),
                    target_bytes.get(1).copied().unwrap_or(0),
                    target_bytes.get(2).copied().unwrap_or(0),
                    target_bytes.get(3).copied().unwrap_or(0),
                ]);
                if target_u32 > 0 { 0xFFFFFFFF_u64 as f64 / target_u32 as f64 } else { 1.0 }
            } else {
                1.0
            }
        } else {
            1.0
        };

        let ext_job = ExternalJob {
            coin: self.name.to_lowercase(),
            algorithm: algorithm.clone(),
            job_id: job_id.clone(),
            seed_hash,
            header_hash: blob.clone(),
            blob,
            target: target.clone(),
            difficulty,
            clean_jobs: true,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            extranonce: String::new(),
            raw_params: Vec::new(),
            height,
        };

        self.stats.jobs_received.fetch_add(1, Ordering::Relaxed);
        let _ = self.job_sender.send(ext_job);
        info!(
            "[{}] 📦 CN Job: id={} algo={} height={} diff={:.0} target={} (total={})",
            self.name, job_id, algorithm, height, difficulty, target,
            self.stats.jobs_received.load(Ordering::Relaxed)
        );
    }
}
