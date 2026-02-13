mod messages;
pub mod ethstratum;

pub use messages::Job;

use anyhow::{anyhow, Result};
use log::debug;
use std::collections::HashMap;
use std::sync::{Arc, atomic::{AtomicU64, Ordering}};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, tcp::{OwnedReadHalf, OwnedWriteHalf}};
use tokio::sync::{Mutex, oneshot, watch};
use tokio::time::{timeout, Duration};

use self::messages::{StratumRequest, StratumResponse};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClientProtocol {
    Xmrig,
    Stratum,
}

#[derive(Clone)]
pub struct StratumClient {
    pool_url: String,
    wallet_address: String,
    worker_name: String,
    algorithm: String,
    difficulty: Option<u64>,
    writer: Arc<Mutex<Option<OwnedWriteHalf>>>,
    session_id: Arc<Mutex<Option<String>>>,
    protocol: Arc<Mutex<ClientProtocol>>,
    job_tx: watch::Sender<Option<Job>>,
    job_rx: Arc<Mutex<watch::Receiver<Option<Job>>>>,
    pending: Arc<Mutex<HashMap<u64, oneshot::Sender<StratumResponse>>>>,
    next_id: Arc<AtomicU64>,
    connected: Arc<std::sync::atomic::AtomicBool>,
}

impl StratumClient {
    pub fn new(
        pool_url: &str,
        wallet_address: &str,
        worker_name: &str,
        algorithm: &str,
        difficulty: Option<u64>,
    ) -> Result<Self> {
        // Accept either `stratum+tcp://host:port` or bare `host:port`.
        let url = pool_url
            .strip_prefix("stratum+tcp://")
            .or_else(|| pool_url.strip_prefix("tcp://"))
            .unwrap_or(pool_url)
            .trim();

        if url.is_empty() || !url.contains(':') {
            return Err(anyhow!(
                "Invalid pool URL format. Expected: host:port or stratum+tcp://host:port"
            ));
        }

        let (job_tx, job_rx) = watch::channel(None);

        Ok(Self {
            pool_url: url.to_string(),
            wallet_address: wallet_address.to_string(),
            worker_name: worker_name.to_string(),
            algorithm: algorithm.to_string(),
            difficulty,
            writer: Arc::new(Mutex::new(None)),
            session_id: Arc::new(Mutex::new(None)),
            protocol: Arc::new(Mutex::new(ClientProtocol::Xmrig)),
            job_tx,
            job_rx: Arc::new(Mutex::new(job_rx)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            connected: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }

    fn password(&self) -> String {
        match self.difficulty {
            Some(d) => format!("algo={},d={}", self.algorithm, d),
            None => format!("algo={}", self.algorithm),
        }
    }

    pub async fn connect(&self) -> Result<()> {
        debug!("Connecting to pool: {}", self.pool_url);

        let stream = TcpStream::connect(&self.pool_url).await?;

        debug!("Connected to pool");

        let (read_half, write_half) = stream.into_split();
        *self.writer.lock().await = Some(write_half);

        // Start reader loop
        let session_id = self.session_id.clone();
        let job_tx = self.job_tx.clone();
        let pending = self.pending.clone();
        let default_algo = self.algorithm.clone();
        let connected_flag = self.connected.clone();
        tokio::spawn(async move {
            if let Err(e) = Self::read_loop(read_half, session_id, job_tx, pending, default_algo).await {
                debug!("Stratum read loop ended: {}", e);
            }
            connected_flag.store(false, std::sync::atomic::Ordering::Relaxed);
            debug!("Connection to pool LOST — miner will reconnect");
        });

        // Perform login (XMRig). If it fails, fallback to Stratum subscribe/authorize.
        if let Err(e) = self.login().await {
            debug!("XMRig login failed ({}). Falling back to Stratum subscribe/authorize", e);
            self.subscribe_and_authorize().await?;
        }

        // Wait for session_id
        let mut attempts = 0;
        while attempts < 20 {
            if self.session_id.lock().await.is_some() {
                debug!("Logged in as: {}", self.worker_name);
                self.connected.store(true, std::sync::atomic::Ordering::Relaxed);
                self.start_keepalive_loop();
                return Ok(());
            }
            attempts += 1;
            tokio::time::sleep(Duration::from_millis(250)).await;
        }

        Err(anyhow!("Login timeout: no session_id received"))
    }

    /// Check if still connected to pool
    pub fn is_connected(&self) -> bool {
        self.connected.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Connect with exponential backoff retry
    pub async fn connect_with_retry(&self, max_attempts: u32) -> Result<()> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            match self.connect().await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if attempt >= max_attempts {
                        return Err(anyhow!("Failed to connect after {} attempts: {}", max_attempts, e));
                    }
                    let delay = std::cmp::min(2u64.pow(attempt), 30);
                    debug!("Connection attempt {}/{} failed: {} — retrying in {}s", attempt, max_attempts, e, delay);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                }
            }
        }
    }

    /// Reconnect after connection loss
    pub async fn reconnect(&self) -> Result<()> {
        self.connected.store(false, std::sync::atomic::Ordering::Relaxed);
        debug!("Reconnecting to pool {}...", self.pool_url);
        // Clear stale state
        *self.writer.lock().await = None;
        *self.session_id.lock().await = None;
        self.pending.lock().await.clear();
        // Reconnect with retry
        self.connect_with_retry(10).await
    }

    pub async fn get_session_id(&self) -> Option<String> {
        self.session_id.lock().await.clone()
    }

    pub async fn subscribe_jobs(&self) -> watch::Receiver<Option<Job>> {
        // Return a clone of the stored receiver (not a new one from subscribe())
        // This ensures we get the same receiver that sees all updates
        self.job_rx.lock().await.clone()
    }

    async fn login(&self) -> Result<()> {
        let id = self.next_request_id();
        let pass = self.password();
        let login_request = StratumRequest::login(
            id,
            &self.wallet_address,
            &self.worker_name,
            &pass,
            &self.algorithm,
        );

        let _ = self.send_request(&login_request).await?;
        *self.protocol.lock().await = ClientProtocol::Xmrig;
        Ok(())
    }

    async fn subscribe_and_authorize(&self) -> Result<()> {
        // Step 1: mining.subscribe
        let subscribe_id = self.next_request_id();
        let subscribe_req = StratumRequest::subscribe(subscribe_id);
        let subscribe_resp = self.send_request(&subscribe_req).await?;
        debug!("mining.subscribe response received");

        // Parse extranonce from subscribe response (if available)
        // Standard response: [[subscriptions], extranonce1, extranonce2_size]
        if let Some(result) = &subscribe_resp.result {
            if let Some(arr) = result.as_array() {
                if arr.len() >= 2 {
                    let extranonce1 = arr.get(1).and_then(|v| v.as_str()).unwrap_or("");
                    let extranonce2_size = arr.get(2).and_then(|v| v.as_u64()).unwrap_or(4);
                    debug!("📡 Extranonce1: {}, Extranonce2 size: {}", extranonce1, extranonce2_size);
                }
            }
        }

        // Step 2: mining.authorize
        let username = if self.worker_name.is_empty() {
            self.wallet_address.clone()
        } else {
            format!("{}.{}", self.wallet_address, self.worker_name)
        };
        let password = self.password();
        let auth_id = self.next_request_id();
        let auth_req = StratumRequest::authorize(auth_id, &username, &password);
        let auth_resp = self.send_request(&auth_req).await?;

        // Validate authorize response: result should be true
        let authorized = match &auth_resp.result {
            Some(v) if v.as_bool() == Some(true) => true,
            Some(v) if v.as_str().map(|s| s.eq_ignore_ascii_case("ok")).unwrap_or(false) => true,
            _ => false,
        };

        if !authorized {
            debug!("Pool rejected authorization (result={:?})", auth_resp.result);
            // Continue anyway — some pools don't return true but still work
        } else {
            debug!("mining.authorize accepted (worker={})", username);
        }

        *self.protocol.lock().await = ClientProtocol::Stratum;

        // Set session_id so connect() can proceed — for Stratum v1 there is no
        // real session_id, but the connect() loop waits for it.
        *self.session_id.lock().await = Some(format!("stratum-{}", username));

        // Wait up to 15s for the first mining.notify job from pool
        debug!("Waiting for first mining.notify from pool...");
        let mut job_rx = self.job_rx.lock().await.clone();
        let wait_result = timeout(Duration::from_secs(15), async {
            loop {
                if job_rx.changed().await.is_err() {
                    return false;
                }
                if job_rx.borrow().is_some() {
                    return true;
                }
            }
        }).await;

        match wait_result {
            Ok(true) => debug!("First mining job received — ready to mine"),
            Ok(false) => debug!("Job channel closed while waiting for first job"),
            Err(_) => debug!("No mining.notify received within 15s — pool may not be sending jobs yet"),
        }

        Ok(())
    }

    fn start_keepalive_loop(&self) {
        let session_id = self.session_id.clone();
        let writer = self.writer.clone();
        let pending = self.pending.clone();
        let next_id = self.next_id.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            loop {
                interval.tick().await;

                let Some(active_session) = session_id.lock().await.clone() else {
                    continue;
                };

                let id = next_id.fetch_add(1, Ordering::Relaxed);
                let req = StratumRequest::keepalive(id, &active_session);

                let json = match serde_json::to_string(&req) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                    if let Some(stream) = writer.lock().await.as_mut() {
                    let _ = stream.write_all(json.as_bytes()).await;
                    let _ = stream.write_all(b"\n").await;
                    let _ = stream.flush().await;
                }

                // Clear any pending keepalive response to avoid leaking pending map.
                let _ = pending.lock().await.remove(&id);
            }
        });
    }

    async fn send_request(&self, request: &StratumRequest) -> Result<StratumResponse> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request.id, tx);

        let json = serde_json::to_string(request)?;
        debug!("→ {}", json);

        let mut writer = self.writer.lock().await;
        if let Some(stream) = writer.as_mut() {
            stream.write_all(json.as_bytes()).await?;
            stream.write_all(b"\n").await?;
            stream.flush().await?;
        } else {
            return Err(anyhow!("Not connected to pool"));
        }

        match timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(resp)) => {
                if let Some(err) = &resp.error {
                    return Err(anyhow!("Stratum error {}: {}", err.code, err.message));
                }
                Ok(resp)
            }
            Ok(Err(_)) => Err(anyhow!("Request cancelled")),
            Err(_) => Err(anyhow!("Request timeout")),
        }
    }

    pub async fn send_custom_value(&self, request: serde_json::Value) -> Result<StratumResponse> {
        let req: StratumRequest = serde_json::from_value(request)?;
        self.send_request(&req).await
    }

    pub async fn submit_share(&self, job_id: &str, nonce: u32, result: &str) -> Result<bool> {
        let id = self.next_request_id();
        let protocol = *self.protocol.lock().await;
        let submit_request = if protocol == ClientProtocol::Xmrig {
            let session_id = self.session_id.lock().await
                .as_ref()
                .ok_or_else(|| anyhow!("No active session"))?
                .clone();
            StratumRequest::submit(id, &session_id, job_id, nonce, result)
        } else {
            let worker = if self.worker_name.is_empty() {
                self.wallet_address.clone()
            } else {
                format!("{}.{}", self.wallet_address, self.worker_name)
            };
            StratumRequest::submit_stratum(id, &worker, job_id, &format!("{:08x}", nonce), result)
        };

        let response = self.send_request(&submit_request).await?;

        // Pools differ in submit response shape:
        // - Stratum: {"result": true|false}
        // - XMRig:   {"result": {"status": "OK"}} (and rejects via JSON-RPC error)
        // Some implementations also return {"result": "OK"}.
        let accepted = match response.result.as_ref() {
            Some(v) if v.is_boolean() => v.as_bool().unwrap_or(false),
            Some(v) if v.is_string() => {
                matches!(v.as_str().unwrap_or("").to_ascii_uppercase().as_str(), "OK" | "ACCEPTED")
            }
            Some(v) if v.is_object() => {
                let status = v
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                matches!(status.to_ascii_uppercase().as_str(), "OK" | "ACCEPTED")
            }
            Some(v) => {
                // If we see a weird shape here, surface it at info level so we can debug without
                // relying on debug logs being enabled in the build.
                debug!("Unexpected submit result shape (treating as rejected): {:?}", v);
                false
            }
            None => false,
        };
        Ok(accepted)
    }

    pub async fn request_job(&self) -> Result<()> {
        if *self.protocol.lock().await != ClientProtocol::Xmrig {
            return Ok(());
        }
        let id = self.next_request_id();
        let req = StratumRequest::getjob(id);
        let _ = self.send_request(&req).await?;
        Ok(())
    }

    pub async fn is_xmrig(&self) -> bool {
        *self.protocol.lock().await == ClientProtocol::Xmrig
    }

    pub async fn disconnect(&self) {
        debug!("Disconnecting from pool");
        *self.writer.lock().await = None;
        *self.session_id.lock().await = None;
    }

    pub fn next_request_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    async fn read_loop(
        read_half: OwnedReadHalf,
        session_id: Arc<Mutex<Option<String>>>,
        job_tx: watch::Sender<Option<Job>>,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<StratumResponse>>>>,
        default_algo: String,
    ) -> Result<()> {
        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        // Revenue lock: when we start mining an ext-* (Revenue) job, record the time.
        // We ignore ZION/NCL push notifications for LOCK_SECS seconds to give slow
        // RandomX enough time to find a share at the pool's difficulty.
        let mut revenue_lock_start: Option<std::time::Instant> = None;
        let revenue_lock_secs: u64 = std::env::var("ZION_REVENUE_LOCK_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(120); // 2 min default (was 1200)

        loop {
            line.clear();
            let bytes = reader.read_line(&mut line).await?;
            if bytes == 0 {
                break;
            }

            let parsed: StratumResponse = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(e) => {
                    debug!("Invalid stratum response: {}", e);
                    continue;
                }
            };

            // Handle notifications (mining.notify)
            // Pool sends: [job_id, blob, target, height, algo, seed_hash, clean_jobs]
            if let Some(method) = &parsed.method {
                if method == "mining.notify" {
                    if let Some(params) = &parsed.params {
                        if let Some(arr) = params.as_array() {
                            if arr.len() >= 4 {
                                let algo = arr
                                    .get(4)
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string())
                                    .unwrap_or_else(|| default_algo.clone());
                                let seed_hash = arr
                                    .get(5)
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                let job_id = arr.get(0).and_then(|v| v.as_str()).unwrap_or("").to_string();
                                debug!("mining.notify: job={}, algo={}, height={}", 
                                    job_id, algo, arr.get(3).and_then(|v| v.as_u64()).unwrap_or(0));
                                let job = Job {
                                    job_id: job_id.clone(),
                                    blob: arr.get(1).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    target: arr.get(2).and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                    height: arr.get(3).and_then(|v| v.as_u64()).unwrap_or(0),
                                    seed_hash,
                                    algo: Some(algo),
                                    coin: None,
                                    cosmic_state0_endian: None,
                                };

                                // Revenue lock: CPU-only RandomX runs at ~4 H/s.
                                // At diff 10000 that's ~40 min per share.  TimeSplit
                                // rotates every ~2 min which is far too short.
                                // Keep mining the Revenue job for LOCK_SECS (1200 s =
                                // 20 min) before allowing a switch back to ZION/NCL.
                                if !job.job_id.starts_with("ext-") {
                                    if let Some(cur) = &*job_tx.borrow() {
                                        if cur.job_id.starts_with("ext-") {
                                            let locked_elapsed = revenue_lock_start
                                                .map(|t| t.elapsed().as_secs())
                                                .unwrap_or(0);
                                            let lock_secs = revenue_lock_secs;
                                            if locked_elapsed < lock_secs {
                                                log::info!("🔒 Revenue lock ({}/{}s): ignoring ZION/NCL notify {} — staying on {}",
                                                    locked_elapsed, lock_secs, job.job_id, cur.job_id);
                                                continue;
                                            } else {
                                                log::info!("🔓 Revenue lock expired ({}s) — switching to {}",
                                                    locked_elapsed, job.job_id);
                                                revenue_lock_start = None;
                                            }
                                        }
                                    }
                                } else if revenue_lock_start.is_none() {
                                    // Starting a new Revenue (ext-*) job → arm the lock timer
                                    revenue_lock_start = Some(std::time::Instant::now());
                                    log::info!("🔒 Revenue lock armed for ext-* job: {}", job.job_id);
                                }

                                let _ = job_tx.send(Some(job));
                            }
                        }
                    }
                } else if method == "mining.set_difficulty" {
                    if let Some(params) = &parsed.params {
                        if let Some(arr) = params.as_array() {
                            if let Some(diff) = arr.get(0).and_then(|v| v.as_u64()) {
                                debug!("Pool difficulty updated: {} → target will apply on next job", diff);
                                // Convert difficulty to 32-byte target and update current job.
                                // target = 0xFFFFFFFF / difficulty (stored as 8-hex-char BE).
                                let target_u32 = if diff > 0 {
                                    (0xFFFF_FFFFu64 / diff) as u32
                                } else {
                                    0xFFFF_FFFFu32
                                };
                                let target_hex = format!("{:08x}", target_u32);
                                // Update the current job target so GPU/CPU use the new difficulty
                                // immediately.  The next mining.notify from the pool will carry
                                // the canonical target, so this is only a bridge.
                                if let Some(current) = &*job_tx.borrow() {
                                    let mut updated = current.clone();
                                    updated.target = target_hex;
                                    let _ = job_tx.send(Some(updated));
                                    debug!("Applied new target for diff={}", diff);
                                }
                            }
                        }
                    }
                } else if method == "job" {
                    // XMRig-format job notification: params is an object, not an array.
                    // Pool sends this during Revenue/NCL stream phases with external algo jobs.
                    if let Some(params) = &parsed.params {
                        if let Some(obj) = params.as_object() {
                            let raw_job_id = obj
                                .get("job_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();

                            let mut algo = obj
                                .get("algo")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| default_algo.clone());

                            // Normalize CryptoNote algo names
                            if algo == "rx/0" { algo = "randomx".to_string(); }

                            // Append algo suffix to job_id if missing (same logic as login response handler)
                            // BUT skip ext-* jobs — they already have a well-formed id from StreamScheduler
                            let mut job_id = raw_job_id.clone();
                            if !raw_job_id.starts_with("ext-") {
                                let raw_parts: Vec<&str> = raw_job_id.split('-').collect();
                                let is_legacy_base = raw_parts.len() == 2;
                                let is_timestamp_base = raw_parts.len() == 3
                                    && raw_parts[2].chars().all(|c| c.is_ascii_digit());
                                if is_legacy_base || is_timestamp_base {
                                    job_id = format!("{}-{}", job_id, algo);
                                }
                            }

                            let job = Job {
                                job_id: job_id.clone(),
                                blob: obj.get("blob").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                target: obj.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                                height: obj.get("height").and_then(|v| v.as_u64()).unwrap_or(0),
                                seed_hash: obj.get("seed_hash").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                algo: Some(algo.clone()),
                                coin: obj.get("coin").and_then(|v| v.as_str()).map(|s| s.to_string()),
                                cosmic_state0_endian: obj
                                    .get("cosmic_state0_endian")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string()),
                            };
                            debug!("📦 XMRig job notification: id={}, algo={}, height={}, seed_hash={:?}",
                                job_id, algo, job.height, job.seed_hash);

                            // ── Revenue Lock (same logic as mining.notify handler) ──
                            if !job.job_id.starts_with("ext-") {
                                if let Some(cur) = &*job_tx.borrow() {
                                    if cur.job_id.starts_with("ext-") {
                                        let locked_elapsed = revenue_lock_start
                                            .map(|t| t.elapsed().as_secs())
                                            .unwrap_or(0);
                                        let lock_secs = revenue_lock_secs;
                                        if locked_elapsed < lock_secs {
                                            log::info!("🔒 Revenue lock ({}/{}s): ignoring XMRig ZION job {} — staying on {}",
                                                locked_elapsed, lock_secs, job.job_id, cur.job_id);
                                            continue;
                                        } else {
                                            log::info!("🔓 Revenue lock expired ({}s) — switching to {}",
                                                locked_elapsed, job.job_id);
                                            revenue_lock_start = None;
                                        }
                                    }
                                }
                            } else if revenue_lock_start.is_none() {
                                revenue_lock_start = Some(std::time::Instant::now());
                                log::info!("🔒 Revenue lock armed for ext-* XMRig job: {}", job.job_id);
                            }

                            let _ = job_tx.send(Some(job));
                        }
                    }
                }
            }

            // Handle login/getjob responses
            if let Some(result) = &parsed.result {
                if let Some(obj) = result.as_object() {
                    if let Some(id_val) = obj.get("id") {
                        let session = if let Some(id_str) = id_val.as_str() {
                            Some(id_str.to_string())
                        } else if let Some(id_num) = id_val.as_u64() {
                            Some(id_num.to_string())
                        } else {
                            None
                        };

                        if let Some(session) = session {
                            *session_id.lock().await = Some(session);
                        }
                    }
                    if let Some(job_val) = obj.get("job") {
                        debug!("📦 Received job object: {:?}", job_val);
                        match serde_json::from_value::<Job>(job_val.clone()) {
                            Ok(job) => {
                                debug!("Job parsed: id={}, height={}, target={}", job.job_id, job.height, job.target);
                                let _ = job_tx.send(Some(job));
                            }
                            Err(e) => {
                                debug!("Failed to parse job: {} - raw: {}", e, job_val);
                            }
                        }
                    } else if obj.get("job_id").is_some() && obj.get("blob").is_some() {
                        let raw_job_id = obj
                            .get("job_id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        // Some pool builds return XMRig `getjob` results with a base job_id
                        // (e.g. "h8-89431800") that omits the algorithm suffix used by `login`/`submit`.
                        // If we mine on the getjob blob/target but keep submitting under the old job_id,
                        // the pool will validate against a different template and reject shares.
                        // BUT skip ext-* jobs — they already have a well-formed id from StreamScheduler.
                        let mut job_id = raw_job_id.clone();
                        if !raw_job_id.starts_with("ext-") {
                            let raw_parts: Vec<&str> = raw_job_id.split('-').collect();
                            let is_legacy_base = raw_parts.len() == 2;
                            let is_timestamp_base = raw_parts.len() == 3
                                && raw_parts[2].chars().all(|c| c.is_ascii_digit());
                            if is_legacy_base || is_timestamp_base {
                                job_id = format!("{}-{}", job_id, default_algo.clone());
                            }
                        }

                        let algo = obj
                            .get("algo")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| default_algo.clone());

                        let job = Job {
                            job_id,
                            blob: obj.get("blob").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            target: obj.get("target").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                            height: obj.get("height").and_then(|v| v.as_u64()).unwrap_or(0),
                            seed_hash: obj.get("seed_hash").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            algo: Some(algo.clone()),
                            coin: obj.get("coin").and_then(|v| v.as_str()).map(|s| s.to_string()),
                            cosmic_state0_endian: obj
                                .get("cosmic_state0_endian")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string()),
                        };

                        // Guard: Don't let a fallback cosmic_harmony getjob response
                        // override an active Revenue/external job (ext-*).
                        // The pool TimeSplit rotates ZION/Revenue/NCL in ~10-15s intervals.
                        // If we're mining an ext-* job (RandomX for MoneroOcean), we should
                        // keep it until the pool sends a NEW push notification — not let
                        // getjob polling clobber it with a zero-height cosmic_harmony fallback.
                        if !job.job_id.starts_with("ext-") {
                            let current = job_tx.borrow().clone();
                            if let Some(ref cur) = current {
                                if cur.job_id.starts_with("ext-") {
                                    let locked_elapsed = revenue_lock_start
                                        .map(|t| t.elapsed().as_secs())
                                        .unwrap_or(0);
                                    let lock_secs = revenue_lock_secs;
                                    if locked_elapsed < lock_secs {
                                        log::info!("📋 getjob: Revenue lock ({}/{}s) — ignoring {} — keeping Revenue job {}",
                                            locked_elapsed, lock_secs, job.job_id, cur.job_id);
                                        continue;
                                    }
                                }
                            }
                        } else if revenue_lock_start.is_none() {
                            revenue_lock_start = Some(std::time::Instant::now());
                            log::info!("🔒 Revenue lock armed via getjob ext-* response: {}", job.job_id);
                        }

                        let _ = job_tx.send(Some(job));
                    }
                }
            }

            // Fulfill pending requests
            if let Some(id) = parsed.id {
                debug!("📬 Response for id={}: result={:?}, error={:?}", id, parsed.result, parsed.error);
                if let Some(tx) = pending.lock().await.remove(&id) {
                    debug!("📬 Found pending request for id={}, fulfilling", id);
                    let _ = tx.send(parsed);
                } else {
                    debug!("⚠️ No pending request for id={}", id);
                }
            } else {
                debug!("📬 Response without id: {:?}", parsed);
            }
        }

        Ok(())
    }
}
