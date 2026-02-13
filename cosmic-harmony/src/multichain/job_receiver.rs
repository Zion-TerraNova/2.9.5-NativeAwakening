//! External Job Receiver - Connects to external mining pools
//!
//! Receives `mining.notify` jobs from external pools and queues them
//! for processing by algorithm workers.

use super::ExternalChain;
use anyhow::{Result, Context, anyhow};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock, watch};

/// Mining job from external pool
#[derive(Debug, Clone)]
pub struct MiningJob {
    pub chain: ExternalChain,
    pub job_id: String,
    pub header_hash: Vec<u8>,
    pub seed_hash: Option<Vec<u8>>,
    pub target: Vec<u8>,
    pub height: u64,
    pub timestamp: u64,
    pub extra_data: HashMap<String, serde_json::Value>,
}

/// Pool connection state
#[derive(Debug, Clone, PartialEq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Authenticated,
    Mining,
    Error(String),
}

/// Single pool connection
pub struct PoolConnection {
    pub chain: ExternalChain,
    pub host: String,
    pub port: u16,
    pub wallet: String,
    pub worker: String,
    pub state: ConnectionState,
    writer: Option<tokio::io::WriteHalf<TcpStream>>,
    job_tx: watch::Sender<Option<MiningJob>>,
    job_rx: watch::Receiver<Option<MiningJob>>,
    next_id: std::sync::atomic::AtomicU64,
}

impl PoolConnection {
    pub fn new(chain: ExternalChain, host: &str, port: u16, wallet: &str) -> Self {
        let (job_tx, job_rx) = watch::channel(None);
        Self {
            chain,
            host: host.to_string(),
            port,
            wallet: wallet.to_string(),
            worker: format!("zion-ch3-{:?}", chain).to_lowercase(),
            state: ConnectionState::Disconnected,
            writer: None,
            job_tx,
            job_rx,
            next_id: std::sync::atomic::AtomicU64::new(1),
        }
    }

    /// Connect to pool
    pub async fn connect(&mut self) -> Result<()> {
        self.state = ConnectionState::Connecting;

        let addr = format!("{}:{}", self.host, self.port);
        let stream = TcpStream::connect(&addr)
            .await
            .context(format!("Failed to connect to {}", addr))?;

        let (read_half, write_half) = tokio::io::split(stream);
        self.writer = Some(write_half);
        self.state = ConnectionState::Connected;

        // Start read loop
        let chain = self.chain;
        let job_tx = self.job_tx.clone();
        tokio::spawn(async move {
            Self::read_loop(chain, read_half, job_tx).await;
        });

        // Authenticate
        self.authenticate().await?;

        Ok(())
    }

    /// Authenticate with pool
    async fn authenticate(&mut self) -> Result<()> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        
        // Different protocols for different algos
        let request = match self.chain {
            ExternalChain::ETC => {
                // Ethash/Stratum protocol
                serde_json::json!({
                    "id": id,
                    "method": "eth_submitLogin",
                    "params": [self.wallet, "x"]
                })
            }
            ExternalChain::RVN | ExternalChain::CLORE => {
                // KawPow uses XMRig-like login
                serde_json::json!({
                    "id": id,
                    "method": "mining.authorize",
                    "params": [format!("{}.{}", self.wallet, self.worker), "x"]
                })
            }
            ExternalChain::ERG => {
                // Ergo Stratum
                serde_json::json!({
                    "id": id,
                    "method": "mining.authorize",
                    "params": [self.wallet, "x"]
                })
            }
            _ => {
                // Generic Stratum
                serde_json::json!({
                    "id": id,
                    "method": "mining.authorize",
                    "params": [format!("{}.{}", self.wallet, self.worker), "x"]
                })
            }
        };

        self.send_json(&request).await?;
        self.state = ConnectionState::Authenticated;

        // Subscribe for jobs
        self.subscribe().await?;

        Ok(())
    }

    /// Subscribe for mining jobs
    async fn subscribe(&mut self) -> Result<()> {
        let id = self.next_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        
        let request = match self.chain {
            ExternalChain::ETC => {
                serde_json::json!({
                    "id": id,
                    "method": "eth_getWork",
                    "params": []
                })
            }
            _ => {
                serde_json::json!({
                    "id": id,
                    "method": "mining.subscribe",
                    "params": ["zion-ch3/2.9.5"]
                })
            }
        };

        self.send_json(&request).await?;
        self.state = ConnectionState::Mining;

        Ok(())
    }

    /// Send JSON message
    async fn send_json(&mut self, msg: &serde_json::Value) -> Result<()> {
        if let Some(writer) = &mut self.writer {
            let payload = serde_json::to_string(msg)? + "\n";
            writer.write_all(payload.as_bytes()).await?;
            writer.flush().await?;
        }
        Ok(())
    }

    /// Read loop - receives and parses pool messages
    async fn read_loop(
        chain: ExternalChain,
        read_half: tokio::io::ReadHalf<TcpStream>,
        job_tx: watch::Sender<Option<MiningJob>>,
    ) {
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    if let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) {
                        if let Some(job) = Self::parse_job(chain, &msg) {
                            let _ = job_tx.send(Some(job));
                        }
                    }
                }
                Err(e) => {
                    log::error!("Read error from {:?} pool: {}", chain, e);
                    break;
                }
            }
        }
    }

    /// Parse job from pool message
    fn parse_job(chain: ExternalChain, msg: &serde_json::Value) -> Option<MiningJob> {
        let method = msg.get("method").and_then(|v| v.as_str())?;
        
        if method != "mining.notify" && method != "eth_getWork" {
            return None;
        }

        let params = msg.get("params").or_else(|| msg.get("result"))?;

        match chain {
            ExternalChain::ETC => Self::parse_ethash_job(chain, params),
            ExternalChain::RVN | ExternalChain::CLORE => Self::parse_kawpow_job(chain, params),
            ExternalChain::ERG => Self::parse_autolykos_job(chain, params),
            ExternalChain::KAS => Self::parse_kheavyhash_job(chain, params),
            _ => Self::parse_generic_job(chain, params),
        }
    }

    fn parse_ethash_job(chain: ExternalChain, params: &serde_json::Value) -> Option<MiningJob> {
        let arr = params.as_array()?;
        if arr.len() < 3 {
            return None;
        }

        Some(MiningJob {
            chain,
            job_id: arr.get(0)?.as_str()?.to_string(),
            header_hash: hex::decode(arr.get(0)?.as_str()?.trim_start_matches("0x")).ok()?,
            seed_hash: arr.get(1).and_then(|v| v.as_str())
                .and_then(|s| hex::decode(s.trim_start_matches("0x")).ok()),
            target: hex::decode(arr.get(2)?.as_str()?.trim_start_matches("0x")).ok()?,
            height: arr.get(3).and_then(|v| v.as_u64()).unwrap_or(0),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            extra_data: HashMap::new(),
        })
    }

    fn parse_kawpow_job(chain: ExternalChain, params: &serde_json::Value) -> Option<MiningJob> {
        let arr = params.as_array()?;
        
        Some(MiningJob {
            chain,
            job_id: arr.get(0)?.as_str()?.to_string(),
            header_hash: hex::decode(arr.get(1)?.as_str()?.trim_start_matches("0x")).ok()?,
            seed_hash: arr.get(2).and_then(|v| v.as_str())
                .and_then(|s| hex::decode(s.trim_start_matches("0x")).ok()),
            target: hex::decode(arr.get(3)?.as_str()?.trim_start_matches("0x")).ok()?,
            height: arr.get(4).and_then(|v| v.as_u64()).unwrap_or(0),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            extra_data: HashMap::new(),
        })
    }

    fn parse_autolykos_job(chain: ExternalChain, params: &serde_json::Value) -> Option<MiningJob> {
        let obj = params.as_object().or_else(|| {
            params.as_array().and_then(|arr| arr.get(0)?.as_object())
        })?;

        Some(MiningJob {
            chain,
            job_id: obj.get("jobId")?.as_str()?.to_string(),
            header_hash: hex::decode(obj.get("msg")?.as_str()?).ok()?,
            seed_hash: None,
            target: hex::decode(obj.get("b")?.as_str()?).ok()?,
            height: obj.get("height").and_then(|v| v.as_u64()).unwrap_or(0),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            extra_data: HashMap::new(),
        })
    }

    fn parse_kheavyhash_job(chain: ExternalChain, params: &serde_json::Value) -> Option<MiningJob> {
        let arr = params.as_array()?;

        Some(MiningJob {
            chain,
            job_id: arr.get(0)?.as_str()?.to_string(),
            header_hash: hex::decode(arr.get(1)?.as_str()?).ok()?,
            seed_hash: None,
            target: hex::decode(arr.get(2)?.as_str()?).ok()?,
            height: arr.get(3).and_then(|v| v.as_u64()).unwrap_or(0),
            timestamp: arr.get(4).and_then(|v| v.as_u64()).unwrap_or_else(|| {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            }),
            extra_data: HashMap::new(),
        })
    }

    fn parse_generic_job(chain: ExternalChain, params: &serde_json::Value) -> Option<MiningJob> {
        let arr = params.as_array()?;

        Some(MiningJob {
            chain,
            job_id: arr.get(0)?.as_str()?.to_string(),
            header_hash: arr.get(1).and_then(|v| v.as_str())
                .and_then(|s| hex::decode(s).ok())
                .unwrap_or_default(),
            seed_hash: arr.get(2).and_then(|v| v.as_str())
                .and_then(|s| hex::decode(s).ok()),
            target: arr.get(3).and_then(|v| v.as_str())
                .and_then(|s| hex::decode(s).ok())
                .unwrap_or_default(),
            height: arr.get(4).and_then(|v| v.as_u64()).unwrap_or(0),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            extra_data: HashMap::new(),
        })
    }

    /// Get current job
    pub fn current_job(&self) -> Option<MiningJob> {
        self.job_rx.borrow().clone()
    }

    /// Subscribe to job updates
    pub fn job_receiver(&self) -> watch::Receiver<Option<MiningJob>> {
        self.job_rx.clone()
    }

    /// Disconnect
    pub async fn disconnect(&mut self) {
        self.writer = None;
        self.state = ConnectionState::Disconnected;
    }
}

/// Manages connections to multiple external pools
pub struct ExternalJobReceiver {
    connections: HashMap<ExternalChain, PoolConnection>,
}

impl ExternalJobReceiver {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    /// Connect to external pool
    pub async fn connect_pool(
        &mut self,
        chain: ExternalChain,
        host: &str,
        port: u16,
        wallet: &str,
    ) -> Result<()> {
        let mut conn = PoolConnection::new(chain, host, port, wallet);
        conn.connect().await?;
        self.connections.insert(chain, conn);
        
        log::info!(
            "ch3_external_pool_connected chain={:?} host={}:{}",
            chain, host, port
        );
        
        Ok(())
    }

    /// Get current job for chain
    pub fn get_job(&self, chain: ExternalChain) -> Option<MiningJob> {
        self.connections.get(&chain)?.current_job()
    }

    /// Get all current jobs
    pub fn get_all_jobs(&self) -> HashMap<ExternalChain, MiningJob> {
        self.connections
            .iter()
            .filter_map(|(chain, conn)| {
                conn.current_job().map(|job| (*chain, job))
            })
            .collect()
    }

    /// Disconnect all pools
    pub async fn disconnect_all(&mut self) {
        for (chain, conn) in self.connections.iter_mut() {
            conn.disconnect().await;
            log::info!("ch3_external_pool_disconnected chain={:?}", chain);
        }
        self.connections.clear();
    }

    /// Get connection state for chain
    pub fn connection_state(&self, chain: ExternalChain) -> ConnectionState {
        self.connections
            .get(&chain)
            .map(|c| c.state.clone())
            .unwrap_or(ConnectionState::Disconnected)
    }
}

impl Default for ExternalJobReceiver {
    fn default() -> Self {
        Self::new()
    }
}
