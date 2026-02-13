//! EthStratum V1 Protocol Client
//!
//! Implementation of Ethereum Stratum protocol for mining on external pools
//! like 2miners, Ethermine, etc. Supports ETC (Etchash) and other Ethash-based coins.
//!
//! Protocol flow:
//! 1. mining.subscribe → extranonce
//! 2. mining.authorize → wallet verification
//! 3. mining.notify → new jobs (seed_hash, header_hash, target)
//! 4. mining.submit → share submission (nonce, header_hash, mix_digest)
//! 5. mining.set_difficulty → dynamic difficulty

use anyhow::{anyhow, Result};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, watch, oneshot};
use tokio::time::{timeout, Duration};

/// External pool coin type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalCoin {
    ETC,    // Ethereum Classic (Etchash/Ethash)
    RVN,    // Ravencoin (KawPow)
    ERG,    // Ergo (Autolykos2)
    KAS,    // Kaspa (kHeavyHash)
    ALPH,   // Alephium (Blake3)
    FLUX,   // Flux (ZelHash/Equihash)
}

impl ExternalCoin {
    pub fn name(&self) -> &'static str {
        match self {
            Self::ETC => "ETC",
            Self::RVN => "RVN",
            Self::ERG => "ERG",
            Self::KAS => "KAS",
            Self::ALPH => "ALPH",
            Self::FLUX => "FLUX",
        }
    }

    pub fn algorithm(&self) -> &'static str {
        match self {
            Self::ETC => "etchash",
            Self::RVN => "kawpow",
            Self::ERG => "autolykos2",
            Self::KAS => "kheavyhash",
            Self::ALPH => "blake3",
            Self::FLUX => "equihash",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "etc" | "ethereum_classic" | "etchash" | "ethash" => Some(Self::ETC),
            "rvn" | "ravencoin" | "kawpow" => Some(Self::RVN),
            "erg" | "ergo" | "autolykos" | "autolykos2" => Some(Self::ERG),
            "kas" | "kaspa" | "kheavyhash" | "heavyhash" => Some(Self::KAS),
            "alph" | "alephium" | "blake3" => Some(Self::ALPH),
            "flux" | "zelcash" | "equihash" => Some(Self::FLUX),
            _ => None,
        }
    }

    /// Default pool URLs (2miners)
    pub fn default_pool_url(&self) -> &'static str {
        match self {
            Self::ETC => "etc.2miners.com:1010",
            Self::RVN => "rvn.2miners.com:6060",
            Self::ERG => "erg.2miners.com:8888",
            Self::KAS => "kas.2miners.com:1111",
            Self::ALPH => "alph.2miners.com:1199",
            Self::FLUX => "flux.2miners.com:9090",
        }
    }
}

/// EthStratum job from mining.notify
#[derive(Debug, Clone)]
pub struct EthStratumJob {
    pub job_id: String,
    pub seed_hash: String,
    pub header_hash: String,
    pub target: String,          // compact difficulty target
    pub difficulty: f64,
    pub height: u64,
    pub clean_jobs: bool,
    pub b_target: String,        // ERG: 'b' value (pool target as decimal BigInt string)
}

/// EthStratum client for external pool connections
pub struct EthStratumClient {
    pool_url: String,
    wallet: String,
    worker: String,
    coin: ExternalCoin,
    extranonce: Arc<Mutex<String>>,
    authorized: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    difficulty: Arc<Mutex<f64>>,
    writer: Arc<Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>>,
    job_tx: watch::Sender<Option<EthStratumJob>>,
    job_rx: Arc<Mutex<watch::Receiver<Option<EthStratumJob>>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
    next_id: Arc<AtomicU64>,
    stats: Arc<Mutex<ExternalPoolStats>>,
}

/// Stats for external pool mining
#[derive(Debug, Clone, Default)]
pub struct ExternalPoolStats {
    pub shares_submitted: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,
    pub jobs_received: u64,
    pub last_share_time: Option<std::time::Instant>,
    pub connected_since: Option<std::time::Instant>,
}

impl EthStratumClient {
    pub fn new(
        pool_url: &str,
        wallet: &str,
        worker: &str,
        coin: ExternalCoin,
    ) -> Self {
        let url = pool_url
            .strip_prefix("stratum+tcp://")
            .or_else(|| pool_url.strip_prefix("tcp://"))
            .unwrap_or(pool_url)
            .trim()
            .to_string();

        let (job_tx, job_rx) = watch::channel(None);

        Self {
            pool_url: url,
            wallet: wallet.to_string(),
            worker: worker.to_string(),
            coin,
            extranonce: Arc::new(Mutex::new(String::new())),
            authorized: Arc::new(AtomicBool::new(false)),
            connected: Arc::new(AtomicBool::new(false)),
            difficulty: Arc::new(Mutex::new(1.0)),
            writer: Arc::new(Mutex::new(None)),
            job_tx,
            job_rx: Arc::new(Mutex::new(job_rx)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            stats: Arc::new(Mutex::new(ExternalPoolStats::default())),
        }
    }

    /// Connect to external pool with full EthStratum handshake
    pub async fn connect(&self) -> Result<()> {
        debug!("[{}] Connecting to external pool: {}", self.coin.name(), self.pool_url);

        let stream = timeout(
            Duration::from_secs(30),
            TcpStream::connect(&self.pool_url),
        )
        .await
        .map_err(|_| anyhow!("Connection timeout"))??;

        debug!("[{}] TCP connected to {}", self.coin.name(), self.pool_url);

        let (read_half, write_half) = stream.into_split();
        *self.writer.lock().await = Some(write_half);
        self.connected.store(true, Ordering::SeqCst);

        // Spawn reader loop
        let extranonce = self.extranonce.clone();
        let authorized = self.authorized.clone();
        let connected = self.connected.clone();
        let difficulty = self.difficulty.clone();
        let job_tx = self.job_tx.clone();
        let pending = self.pending.clone();
        let stats = self.stats.clone();
        let coin = self.coin;

        tokio::spawn(async move {
            if let Err(e) = Self::read_loop(
                read_half, extranonce, authorized, connected,
                difficulty, job_tx, pending, stats, coin,
            ).await {
                debug!("[{}] Read loop ended: {}", coin.name(), e);
            }
        });

        // Step 1: mining.subscribe
        self.subscribe().await?;

        // Step 2: mining.authorize
        self.authorize().await?;

        // Mark connected time
        {
            let mut stats = self.stats.lock().await;
            stats.connected_since = Some(std::time::Instant::now());
        }

        debug!("[{}] Fully connected and authorized on {}", self.coin.name(), self.pool_url);
        Ok(())
    }

    async fn subscribe(&self) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // EthStratum V1 subscribe
        let req = serde_json::json!({
            "id": id,
            "method": "mining.subscribe",
            "params": ["zion-universal-miner/2.9.5", "EthereumStratum/1.0.0"]
        });

        let resp = self.send_and_wait(id, &req).await?;
        debug!("[{}] Subscribe response: {:?}", self.coin.name(), resp);

        // Parse extranonce from subscribe result
        // Format: {"result": [["mining.notify", "xxx", "EthereumStratum/1.0.0"], "extranonce"], ...}
        if let Some(result) = resp.as_array() {
            if result.len() >= 2 {
                if let Some(en) = result[1].as_str() {
                    *self.extranonce.lock().await = en.to_string();
                    debug!("[{}] Extranonce: {}", self.coin.name(), en);
                }
            }
        } else if let Some(result) = resp.as_object() {
            // Some pools return object format
            if let Some(en) = result.get("extranonce").and_then(|v| v.as_str()) {
                *self.extranonce.lock().await = en.to_string();
            }
        }

        debug!("[{}] Subscribed", self.coin.name());
        Ok(())
    }

    async fn authorize(&self) -> Result<()> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        // Format: wallet.worker
        let username = if self.worker.is_empty() {
            self.wallet.clone()
        } else {
            format!("{}.{}", self.wallet, self.worker)
        };

        let req = serde_json::json!({
            "id": id,
            "method": "mining.authorize",
            "params": [username, "x"]
        });

        let resp = self.send_and_wait(id, &req).await?;

        if resp.as_bool() == Some(true) {
            self.authorized.store(true, Ordering::SeqCst);
            debug!("[{}] Authorized as {}", self.coin.name(), username);
            Ok(())
        } else {
            Err(anyhow!("[{}] Authorization rejected: {:?}", self.coin.name(), resp))
        }
    }

    /// Submit a share to the external pool
    /// For EthStratum: params = [worker, job_id, nonce_hex]
    pub async fn submit_share(
        &self,
        job_id: &str,
        nonce: u64,
        _header_hash: &str,
        _mix_digest: &str,
    ) -> Result<bool> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        let worker = if self.worker.is_empty() {
            self.wallet.clone()
        } else {
            format!("{}.{}", self.wallet, self.worker)
        };

        // EthStratum nonce format: pool expects miner_nonce part only (without extranonce prefix)
        // GPU mines with full nonce = extranonce_base | miner_offset
        // Submit: extract just the miner_part by formatting full nonce and taking last N chars
        let extranonce = self.extranonce.lock().await.clone();
        let en_len = extranonce.len();
        let full_nonce_hex = format!("{:016x}", nonce);
        // Take the last miner_nonce_len chars (strip extranonce prefix)
        let nonce_hex = &full_nonce_hex[en_len..];
        
        debug!("[{}] Submit: job={}, full_nonce=0x{}, extranonce={}, submit_nonce={}", 
            self.coin.name(), job_id, full_nonce_hex, extranonce, nonce_hex);

        let req = serde_json::json!({
            "id": id,
            "method": "mining.submit",
            "params": [worker, job_id, nonce_hex]
        });

        {
            let mut stats = self.stats.lock().await;
            stats.shares_submitted += 1;
            stats.last_share_time = Some(std::time::Instant::now());
        }

        match self.send_and_wait(id, &req).await {
            Ok(resp) => {
                let accepted = resp.as_bool().unwrap_or(false);
                {
                    let mut stats = self.stats.lock().await;
                    if accepted {
                        stats.shares_accepted += 1;
                        debug!("[{}] Share ACCEPTED (total: {})", self.coin.name(), stats.shares_accepted);
                    } else {
                        stats.shares_rejected += 1;
                        debug!("[{}] Share rejected: {:?}", self.coin.name(), resp);
                    }
                }
                Ok(accepted)
            }
            Err(e) => {
                debug!("[{}] Submit error: {}", self.coin.name(), e);
                Err(e)
            }
        }
    }

    pub async fn subscribe_jobs(&self) -> watch::Receiver<Option<EthStratumJob>> {
        self.job_rx.lock().await.clone()
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    pub fn is_authorized(&self) -> bool {
        self.authorized.load(Ordering::SeqCst)
    }

    pub async fn get_difficulty(&self) -> f64 {
        *self.difficulty.lock().await
    }

    pub async fn get_extranonce(&self) -> String {
        self.extranonce.lock().await.clone()
    }

    pub async fn get_stats(&self) -> ExternalPoolStats {
        self.stats.lock().await.clone()
    }

    async fn send_and_wait(&self, id: u64, request: &Value) -> Result<Value> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let json = serde_json::to_string(request)?;
        debug!("[EthStratum] → {}", json);

        {
            let mut writer = self.writer.lock().await;
            if let Some(stream) = writer.as_mut() {
                stream.write_all(json.as_bytes()).await?;
                stream.write_all(b"\n").await?;
                stream.flush().await?;
            } else {
                return Err(anyhow!("Not connected"));
            }
        }

        match timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_)) => Err(anyhow!("Response channel closed")),
            Err(_) => Err(anyhow!("Response timeout")),
        }
    }

    async fn read_loop(
        read_half: tokio::net::tcp::OwnedReadHalf,
        extranonce: Arc<Mutex<String>>,
        authorized: Arc<AtomicBool>,
        connected: Arc<AtomicBool>,
        difficulty: Arc<Mutex<f64>>,
        job_tx: watch::Sender<Option<EthStratumJob>>,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
        stats: Arc<Mutex<ExternalPoolStats>>,
        coin: ExternalCoin,
    ) -> Result<()> {
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        loop {
            line.clear();
            let bytes = reader.read_line(&mut line).await?;
            if bytes == 0 {
                connected.store(false, Ordering::SeqCst);
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // For ERG debugging, log raw messages at INFO level
            if coin == ExternalCoin::ERG {
                debug!("[{}] ← RAW: {}", coin.name(), &trimmed[..trimmed.len().min(300)]);
            } else {
                debug!("[{}] ← {}", coin.name(), trimmed);
            }

            let parsed: Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(e) => {
                    debug!("[{}] Invalid JSON: {}", coin.name(), e);
                    continue;
                }
            };

            // Handle notifications (no id, has method)
            if let Some(method) = parsed.get("method").and_then(|v| v.as_str()) {
                match method {
                    "mining.notify" => {
                        if let Some(params) = parsed.get("params").and_then(|v| v.as_array()) {
                            if params.len() >= 3 {
                                let diff = *difficulty.lock().await;
                                
                                // ERG 2miners uses different notify format:
                                // [job_id, height(number), header_hash, "", "", nBits, b_target, "", clean_jobs]
                                // Standard EthStratum:
                                // [job_id, seed_hash, header_hash, clean_jobs]
                                let job = if coin == ExternalCoin::ERG && params.len() >= 7 {
                                    let job_id = params[0].as_str().unwrap_or("").to_string();
                                    let height = params[1].as_u64().unwrap_or(0);
                                    let header_hash = params[2].as_str().unwrap_or("").to_string();
                                    // params[3..5] are empty strings
                                    // params[5] = nBits (compact difficulty encoding)  
                                    // params[6] = b (pool target as decimal BigInt string)
                                    let b_target = params[6].as_str()
                                        .or_else(|| params[6].as_u64().map(|_| ""))
                                        .unwrap_or("").to_string();
                                    // Handle b_target that could be a number
                                    let b_target_str = if b_target.is_empty() {
                                        // Try as number
                                        params[6].to_string().trim_matches('"').to_string()
                                    } else {
                                        b_target
                                    };
                                    let clean_jobs = params.get(8)
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(true);
                                    
                                    debug!("[ERG] New job: {} (height: {}, b_target: {}...)", 
                                        job_id, height,
                                        if b_target_str.len() > 20 { &b_target_str[..20] } else { &b_target_str });
                                    
                                    EthStratumJob {
                                        job_id,
                                        seed_hash: format!("{:016x}", height), // encode height as seed_hash
                                        header_hash,
                                        target: String::new(),
                                        difficulty: diff,
                                        height,
                                        clean_jobs,
                                        b_target: b_target_str,
                                    }
                                } else {
                                    // Standard EthStratum (ETC, etc.)
                                    let job_id = params[0].as_str().unwrap_or("").to_string();
                                    let seed_hash = params[1].as_str().unwrap_or("").to_string();
                                    let header_hash = params[2].as_str().unwrap_or("").to_string();
                                    let clean_jobs = params.get(3)
                                        .and_then(|v| v.as_bool())
                                        .unwrap_or(true);

                                    EthStratumJob {
                                        job_id: job_id.clone(),
                                        seed_hash,
                                        header_hash,
                                        target: String::new(),
                                        difficulty: diff,
                                        height: 0,
                                        clean_jobs,
                                        b_target: String::new(),
                                    }
                                };
                                
                                let job_id = job.job_id.clone();

                                {
                                    let mut s = stats.lock().await;
                                    s.jobs_received += 1;
                                }

                                let _ = job_tx.send(Some(job));
                                if coin != ExternalCoin::ERG {
                                    debug!("[{}] New job: {} (diff: {:.4})", coin.name(), job_id, diff);
                                }
                            }
                        }
                    }
                    "mining.set_difficulty" => {
                        if let Some(params) = parsed.get("params").and_then(|v| v.as_array()) {
                            if let Some(diff) = params.get(0).and_then(|v| v.as_f64()) {
                                *difficulty.lock().await = diff;
                                debug!("[{}] Difficulty set to {:.6}", coin.name(), diff);
                            }
                        }
                    }
                    "mining.set_extranonce" => {
                        if let Some(params) = parsed.get("params").and_then(|v| v.as_array()) {
                            if let Some(en) = params.get(0).and_then(|v| v.as_str()) {
                                *extranonce.lock().await = en.to_string();
                                debug!("[{}] Extranonce updated: {}", coin.name(), en);
                            }
                        }
                    }
                    _ => {
                        debug!("[{}] Unknown method: {}", coin.name(), method);
                    }
                }
                continue;
            }

            // Handle responses (has id)
            if let Some(id) = parsed.get("id").and_then(|v| v.as_u64()) {
                let result = parsed.get("result").cloned().unwrap_or(Value::Null);
                let error = parsed.get("error");

                if let Some(err) = error {
                    if !err.is_null() {
                        debug!("[{}] Error for id={}: {:?}", coin.name(), id, err);
                    }
                }

                if let Some(tx) = pending.lock().await.remove(&id) {
                    let _ = tx.send(result);
                }
            }
        }

        connected.store(false, Ordering::SeqCst);
        debug!("[{}] Connection to {} closed", coin.name(), "external pool");
        Ok(())
    }

    /// Start reconnection loop
    pub async fn connect_with_retry(&self, max_retries: u32) -> Result<()> {
        let mut attempt = 0;
        loop {
            match self.connect().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    attempt += 1;
                    if attempt >= max_retries {
                        return Err(anyhow!("Failed to connect after {} attempts: {}", max_retries, e));
                    }
                    debug!("[{}] Connection attempt {}/{} failed: {}. Retrying in 5s...",
                        self.coin.name(), attempt, max_retries, e);
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    }
}
