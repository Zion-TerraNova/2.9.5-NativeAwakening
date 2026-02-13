//! Pool connection management for multi-chain mining

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, tcp::{OwnedReadHalf, OwnedWriteHalf}};
use tokio::sync::{Mutex, RwLock, oneshot, watch};
use serde::{Deserialize, Serialize};

use crate::config::PoolConfig;

/// Pool connection state
#[derive(Debug, Clone, PartialEq)]
pub enum PoolState {
    Disconnected,
    Connecting,
    Connected,
    Mining,
    Error(String),
}

/// Active pool connection
#[derive(Debug, Clone)]
pub struct PoolConnection {
    pub pool_id: String,
    pub config: PoolConfig,
    pub state: PoolState,
    pub accepted_shares: u64,
    pub rejected_shares: u64,
    pub current_difficulty: f64,
    pub hashrate: f64,
    pub last_share_time: Option<std::time::Instant>,
}

impl PoolConnection {
    pub fn new(pool_id: &str, config: PoolConfig) -> Self {
        Self {
            pool_id: pool_id.to_string(),
            config,
            state: PoolState::Disconnected,
            accepted_shares: 0,
            rejected_shares: 0,
            current_difficulty: 1.0,
            hashrate: 0.0,
            last_share_time: None,
        }
    }
    
    pub fn accept_rate(&self) -> f64 {
        let total = self.accepted_shares + self.rejected_shares;
        if total == 0 {
            return 100.0;
        }
        (self.accepted_shares as f64 / total as f64) * 100.0
    }
}

/// Share to submit to pool
#[derive(Debug, Clone, Serialize)]
pub struct Share {
    pub job_id: String,
    pub nonce: String,
    pub hash: String,
    pub difficulty: f64,
}

/// Job from pool
#[derive(Debug, Clone, Deserialize)]
pub struct MiningJob {
    pub job_id: String,
    pub blob: String,
    pub target: String,
    pub height: u64,
    pub seed_hash: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClientProtocol {
    Xmrig,
    Stratum,
}

#[derive(Clone)]
struct PoolRuntime {
    writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
    session_id: Arc<Mutex<Option<String>>>,
    protocol: Arc<Mutex<ClientProtocol>>,
    job_tx: watch::Sender<Option<MiningJob>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
    next_id: Arc<AtomicU64>,
}

impl PoolRuntime {
    fn new() -> (Self, watch::Receiver<Option<MiningJob>>) {
        let (job_tx, job_rx) = watch::channel(None);
        (
            Self {
                writer: Arc::new(Mutex::new(None)),
                session_id: Arc::new(Mutex::new(None)),
                protocol: Arc::new(Mutex::new(ClientProtocol::Xmrig)),
                job_tx,
                pending: Arc::new(Mutex::new(HashMap::new())),
                next_id: Arc::new(AtomicU64::new(1)),
            },
            job_rx,
        )
    }

    fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn send_request(&self, req: &serde_json::Value) -> anyhow::Result<serde_json::Value> {
        let id = req
            .get("id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("request missing numeric id"))?;

        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);

        let json = serde_json::to_string(req)?;
        let mut writer = self.writer.lock().await;
        if let Some(stream) = writer.as_mut() {
            stream.write_all(json.as_bytes()).await?;
            stream.write_all(b"\n").await?;
            stream.flush().await?;
        } else {
            return Err(anyhow::anyhow!("Not connected"));
        }

        match tokio::time::timeout(std::time::Duration::from_secs(10), rx).await {
            Ok(Ok(resp)) => {
                if let Some(err) = resp.get("error") {
                    if !err.is_null() {
                        return Err(anyhow::anyhow!("Stratum error: {}", err));
                    }
                }
                Ok(resp)
            }
            Ok(Err(_)) => Err(anyhow::anyhow!("Request cancelled")),
            Err(_) => Err(anyhow::anyhow!("Request timeout")),
        }
    }
}

fn normalize_pool_host(url: &str) -> anyhow::Result<String> {
    let url = url
        .strip_prefix("stratum+tcp://")
        .or_else(|| url.strip_prefix("stratum://"))
        .unwrap_or(url);

    if url.trim().is_empty() {
        anyhow::bail!("Empty pool url")
    }

    Ok(url.to_string())
}

fn build_login_request(id: u64, cfg: &PoolConfig) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "method": "login",
        "params": {
            "login": cfg.wallet,
            "pass": cfg.password,
            "rigid": cfg.worker,
            "agent": "zion-chv3/3.0"
        }
    })
}

fn build_subscribe_request(id: u64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "method": "mining.subscribe",
        "params": []
    })
}

fn build_authorize_request(id: u64, cfg: &PoolConfig) -> serde_json::Value {
    let username = if cfg.worker.is_empty() {
        cfg.wallet.clone()
    } else {
        format!("{}.{}", cfg.wallet, cfg.worker)
    };

    serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "method": "mining.authorize",
        "params": [username, cfg.password]
    })
}

fn build_getjob_request(id: u64) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "method": "getjob",
        "params": {}
    })
}

fn build_submit_xmrig_request(id: u64, session_id: &str, share: &Share) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "method": "submit",
        "params": {
            "id": session_id,
            "job_id": share.job_id,
            "nonce": share.nonce,
            "result": share.hash
        }
    })
}

fn build_submit_stratum_request(id: u64, cfg: &PoolConfig, share: &Share) -> serde_json::Value {
    let username = if cfg.worker.is_empty() {
        cfg.wallet.clone()
    } else {
        format!("{}.{}", cfg.wallet, cfg.worker)
    };

    serde_json::json!({
        "id": id,
        "jsonrpc": "2.0",
        "method": "mining.submit",
        "params": [username, share.job_id, share.nonce, share.hash]
    })
}

fn parse_job_from_notify(params: &serde_json::Value) -> Option<MiningJob> {
    let arr = params.as_array()?;
    if arr.len() < 4 {
        return None;
    }

    let job_id = arr.get(0)?.as_str()?.to_string();
    let blob = arr.get(1)?.as_str()?.to_string();
    let target = arr.get(2)?.as_str()?.to_string();
    let height = arr.get(3).and_then(|v| v.as_u64()).unwrap_or(0);
    let seed_hash = arr.get(5).and_then(|v| v.as_str()).map(|s| s.to_string());

    Some(MiningJob {
        job_id,
        blob,
        target,
        height,
        seed_hash,
    })
}

fn parse_job_from_xmrig_result(result: &serde_json::Value) -> Option<(Option<String>, Option<MiningJob>)> {
    let obj = result.as_object()?;

    let session_id = obj.get("id").and_then(|id_val| {
        if let Some(id_str) = id_val.as_str() {
            Some(id_str.to_string())
        } else if let Some(id_num) = id_val.as_u64() {
            Some(id_num.to_string())
        } else {
            None
        }
    });

    if let Some(job_val) = obj.get("job") {
        if let Ok(job) = serde_json::from_value::<MiningJob>(job_val.clone()) {
            return Some((session_id, Some(job)));
        }
    }

    if obj.get("job_id").is_some() && obj.get("blob").is_some() {
        let job = MiningJob {
            job_id: obj.get("job_id").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            blob: obj.get("blob").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            target: obj.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            height: obj.get("height").and_then(|v| v.as_u64()).unwrap_or(0),
            seed_hash: obj
                .get("seed_hash")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        };
        return Some((session_id, Some(job)));
    }

    Some((session_id, None))
}

async fn read_loop(
    read_half: OwnedReadHalf,
    session_id: Arc<Mutex<Option<String>>>,
    job_tx: watch::Sender<Option<MiningJob>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<serde_json::Value>>>>,
) -> anyhow::Result<()> {
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();

    loop {
        line.clear();
        let bytes = reader.read_line(&mut line).await?;
        if bytes == 0 {
            break;
        }

        let parsed: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(method) = parsed.get("method").and_then(|v| v.as_str()) {
            if method == "mining.notify" {
                if let Some(params) = parsed.get("params") {
                    if let Some(job) = parse_job_from_notify(params) {
                        let _ = job_tx.send(Some(job));
                    }
                }
            }
        }

        if let Some(result) = parsed.get("result") {
            if let Some((sid, job_opt)) = parse_job_from_xmrig_result(result) {
                if let Some(sid) = sid {
                    *session_id.lock().await = Some(sid);
                }
                if let Some(job) = job_opt {
                    let _ = job_tx.send(Some(job));
                }
            }
        }

        if let Some(id) = parsed.get("id").and_then(|v| v.as_u64()) {
            if let Some(tx) = pending.lock().await.remove(&id) {
                let _ = tx.send(parsed);
            }
        }
    }

    Ok(())
}

/// Pool manager for all connections
pub struct PoolManager {
    /// Active pool connections
    connections: Arc<RwLock<HashMap<String, PoolConnection>>>,

    runtimes: Arc<RwLock<HashMap<String, PoolRuntime>>>,
    
    /// Revenue tracking per pool
    revenue: Arc<RwLock<HashMap<String, f64>>>,
}

impl PoolManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            runtimes: Arc::new(RwLock::new(HashMap::new())),
            revenue: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Add pool from config
    pub async fn add_pool(&self, pool_id: &str, config: PoolConfig) {
        let connection = PoolConnection::new(pool_id, config);
        let mut connections = self.connections.write().await;
        connections.insert(pool_id.to_string(), connection);
    }
    
    /// Connect to a pool
    pub async fn connect(&self, pool_id: &str) -> anyhow::Result<()> {
        let cfg = {
            let mut connections = self.connections.write().await;
            let conn = connections
                .get_mut(pool_id)
                .ok_or_else(|| anyhow::anyhow!("Pool {} not configured", pool_id))?;
            conn.state = PoolState::Connecting;
            conn.config.clone()
        };

        let host = normalize_pool_host(&cfg.url)?;
        let stream = TcpStream::connect(&host).await?;
        let (read_half, write_half) = stream.into_split();

        let (runtime, mut job_rx) = PoolRuntime::new();
        *runtime.writer.lock().await = Some(write_half);

        // Spawn read loop
        let session_id = runtime.session_id.clone();
        let job_tx = runtime.job_tx.clone();
        let pending = runtime.pending.clone();
        tokio::spawn(async move {
            if let Err(e) = read_loop(read_half, session_id, job_tx, pending).await {
                log::warn!("pool read loop ended: {}", e);
            }
        });

        // Try XMRig login; fallback to Stratum subscribe/authorize.
        let login_id = runtime.next_request_id();
        let login_req = build_login_request(login_id, &cfg);
        let login_ok = runtime.send_request(&login_req).await.is_ok();
        if login_ok {
            *runtime.protocol.lock().await = ClientProtocol::Xmrig;
        } else {
            let sub_id = runtime.next_request_id();
            let sub_req = build_subscribe_request(sub_id);
            let _ = runtime.send_request(&sub_req).await?;

            let auth_id = runtime.next_request_id();
            let auth_req = build_authorize_request(auth_id, &cfg);
            let _ = runtime.send_request(&auth_req).await?;

            *runtime.protocol.lock().await = ClientProtocol::Stratum;
            if runtime.session_id.lock().await.is_none() {
                *runtime.session_id.lock().await = Some("stratum".to_string());
            }
        }

        // XMRig: request initial job.
        if *runtime.protocol.lock().await == ClientProtocol::Xmrig {
            let getjob_id = runtime.next_request_id();
            let getjob_req = build_getjob_request(getjob_id);
            let _ = runtime.send_request(&getjob_req).await;
        }

        // Wait briefly for at least one job (best-effort).
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), job_rx.changed()).await;

        {
            let mut runtimes = self.runtimes.write().await;
            runtimes.insert(pool_id.to_string(), runtime);
        }
        {
            let mut connections = self.connections.write().await;
            if let Some(conn) = connections.get_mut(pool_id) {
                conn.state = PoolState::Connected;
            }
        }

        Ok(())
    }
    
    /// Disconnect from a pool
    pub async fn disconnect(&self, pool_id: &str) -> anyhow::Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(conn) = connections.get_mut(pool_id) {
            conn.state = PoolState::Disconnected;
        }
        let mut runtimes = self.runtimes.write().await;
        runtimes.remove(pool_id);
        Ok(())
    }
    
    /// Submit share to pool
    pub async fn submit_share(&self, pool_id: &str, share: Share) -> anyhow::Result<bool> {
        let (runtime, cfg) = {
            let runtimes = self.runtimes.read().await;
            let runtime = runtimes
                .get(pool_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Pool {} not connected", pool_id))?;
            let connections = self.connections.read().await;
            let cfg = connections
                .get(pool_id)
                .ok_or_else(|| anyhow::anyhow!("Pool {} not configured", pool_id))?
                .config
                .clone();
            (runtime, cfg)
        };

        let id = runtime.next_request_id();
        let protocol = *runtime.protocol.lock().await;
        let submit_req = if protocol == ClientProtocol::Xmrig {
            let session_id = runtime
                .session_id
                .lock()
                .await
                .clone()
                .ok_or_else(|| anyhow::anyhow!("No active session"))?;
            build_submit_xmrig_request(id, &session_id, &share)
        } else {
            build_submit_stratum_request(id, &cfg, &share)
        };

        let resp = runtime.send_request(&submit_req).await?;
        let accepted = resp.get("result").and_then(|v| v.as_bool()).unwrap_or(false);

        let mut connections = self.connections.write().await;
        let conn = connections
            .get_mut(pool_id)
            .ok_or_else(|| anyhow::anyhow!("Pool {} not connected", pool_id))?;
        
        if accepted {
            conn.accepted_shares += 1;
            conn.last_share_time = Some(std::time::Instant::now());
            
            // Track revenue (estimated)
            let revenue_per_share = share.difficulty * 0.00001;
            let mut revenue = self.revenue.write().await;
            *revenue.entry(pool_id.to_string()).or_insert(0.0) += revenue_per_share;
        } else {
            conn.rejected_shares += 1;
        }
        
        Ok(accepted)
    }
    
    /// Get current job from pool
    pub async fn get_job(&self, pool_id: &str) -> anyhow::Result<Option<MiningJob>> {
        let runtime = {
            let runtimes = self.runtimes.read().await;
            runtimes
                .get(pool_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Pool {} not connected", pool_id))?
        };

        // Best-effort: request a job for XMRig protocol if none yet.
        let mut rx = runtime.job_tx.subscribe();
        if rx.borrow().is_none() {
            if *runtime.protocol.lock().await == ClientProtocol::Xmrig {
                let id = runtime.next_request_id();
                let req = build_getjob_request(id);
                let _ = runtime.send_request(&req).await;
                let _ = tokio::time::timeout(std::time::Duration::from_secs(1), rx.changed()).await;
            }
        }

        let job = rx.borrow().clone();
        Ok(job)
    }
    
    /// Get pool statistics
    pub async fn get_stats(&self, pool_id: &str) -> anyhow::Result<PoolConnection> {
        let connections = self.connections.read().await;
        connections.get(pool_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Pool {} not found", pool_id))
    }
    
    /// Get all active pools
    pub async fn get_active_pools(&self) -> Vec<PoolConnection> {
        let connections = self.connections.read().await;
        connections.values()
            .filter(|c| matches!(c.state, PoolState::Connected | PoolState::Mining))
            .cloned()
            .collect()
    }
    
    /// Get total revenue across all pools
    pub async fn get_total_revenue(&self) -> HashMap<String, f64> {
        self.revenue.read().await.clone()
    }
    
    /// Connect to all configured pools
    pub async fn connect_all(&self) -> anyhow::Result<()> {
        let pool_ids: Vec<String> = {
            let connections = self.connections.read().await;
            connections.keys().cloned().collect()
        };
        
        for pool_id in pool_ids {
            if let Err(e) = self.connect(&pool_id).await {
                log::warn!("Failed to connect to pool {}: {}", pool_id, e);
            }
        }
        Ok(())
    }
    
    /// Disconnect from all pools
    pub async fn disconnect_all(&self) -> anyhow::Result<()> {
        let mut connections = self.connections.write().await;
        for conn in connections.values_mut() {
            conn.state = PoolState::Disconnected;
        }
        Ok(())
    }
}

impl Default for PoolManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AlgorithmType;
    use tokio::net::TcpListener;
    
    #[tokio::test]
    async fn test_pool_manager() {
        let manager = PoolManager::new();

        // Start a local mock pool server (xmrig-style)
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();

            loop {
                line.clear();
                let bytes = reader.read_line(&mut line).await.unwrap();
                if bytes == 0 {
                    break;
                }

                let v: serde_json::Value = match serde_json::from_str(&line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let id = v.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
                let method = v.get("method").and_then(|v| v.as_str()).unwrap_or("");

                if method == "login" {
                    let resp = serde_json::json!({
                        "id": id,
                        "result": {
                            "id": "session-1",
                            "job": {
                                "job_id": "job-1",
                                "blob": "00",
                                "target": "ff",
                                "height": 1,
                                "seed_hash": null
                            }
                        },
                        "error": null
                    });
                    let _ = write_half.write_all(serde_json::to_string(&resp).unwrap().as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                    let _ = write_half.flush().await;
                } else if method == "getjob" {
                    let resp = serde_json::json!({
                        "id": id,
                        "result": {
                            "job_id": "job-1",
                            "blob": "00",
                            "target": "ff",
                            "height": 1,
                            "seed_hash": null
                        },
                        "error": null
                    });
                    let _ = write_half.write_all(serde_json::to_string(&resp).unwrap().as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                    let _ = write_half.flush().await;
                } else if method == "submit" {
                    let resp = serde_json::json!({
                        "id": id,
                        "result": true,
                        "error": null
                    });
                    let _ = write_half.write_all(serde_json::to_string(&resp).unwrap().as_bytes()).await;
                    let _ = write_half.write_all(b"\n").await;
                    let _ = write_half.flush().await;
                }
            }
        });
        
        let config = PoolConfig {
            url: format!("stratum+tcp://{}", addr),
            wallet: "ZION_WALLET".to_string(),
            worker: "test".to_string(),
            password: "x".to_string(),
            algorithm: AlgorithmType::CosmicFusion,
            enabled: true,
        };
        
        manager.add_pool("zion-main", config).await;
        manager.connect("zion-main").await.unwrap();
        
        let stats = manager.get_stats("zion-main").await.unwrap();
        assert_eq!(stats.state, PoolState::Connected);

        let job = manager.get_job("zion-main").await.unwrap();
        assert!(job.is_some());

        let accepted = manager
            .submit_share(
                "zion-main",
                Share {
                    job_id: "job-1".to_string(),
                    nonce: "00000001".to_string(),
                    hash: "deadbeef".to_string(),
                    difficulty: 1.0,
                },
            )
            .await
            .unwrap();
        assert!(accepted);
    }
}
