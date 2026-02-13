//! IBD (Initial Block Download) Sync Manager
//!
//! Handles full chain synchronization from peers when node is far behind.
//! Uses large batch requests (500 blocks) with progress tracking.
//!
//! Sprint 1.3 hardening:
//! - IBD stall detection (timeout after 120s of no progress)
//! - Peer tracking (which peer we are syncing from)
//! - Retry counter with max attempts
//! - RPC-visible sync info via to_json()

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use serde::Serialize;

/// Threshold: if peer is more than 50 blocks ahead, enter IBD mode.
pub const IBD_THRESHOLD: u64 = 50;

/// How many blocks to request per IBD batch.
pub const IBD_BATCH_SIZE: u32 = 500;

/// Maximum message size during IBD (50MB — blocks can be large).
pub const IBD_MAX_MESSAGE_SIZE: usize = 50_000_000;

/// How long to wait for an IBD batch before declaring a stall (seconds).
pub const IBD_STALL_TIMEOUT_SECS: u64 = 120;

/// Maximum stall retries before giving up on a peer.
pub const IBD_MAX_STALL_RETRIES: u32 = 3;

/// Sync state for a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SyncState {
    /// Normal operation — gossip-based block propagation
    Steady,
    /// Initial Block Download — pulling large batches from peers
    IBD,
}

/// Serializable snapshot of sync status for RPC.
#[derive(Debug, Clone, Serialize)]
pub struct SyncSnapshot {
    pub state: SyncState,
    pub syncing: bool,
    pub target_height: u64,
    pub download_height: u64,
    pub blocks_downloaded: u64,
    pub elapsed_secs: f64,
    pub blocks_per_sec: f64,
    pub eta_secs: f64,
    pub percent: f64,
    pub stall_retries: u32,
    pub ibd_peer: Option<String>,
}

/// Shared sync status accessible across tasks.
#[derive(Clone)]
pub struct SyncStatus {
    /// Current sync state
    state: Arc<std::sync::Mutex<SyncState>>,
    /// Are we currently syncing?
    pub syncing: Arc<AtomicBool>,
    /// Target height (best peer height)
    pub target_height: Arc<AtomicU64>,
    /// Current download progress
    pub download_height: Arc<AtomicU64>,
    /// Blocks downloaded in current IBD session
    pub blocks_downloaded: Arc<AtomicU64>,
    /// IBD start time (for speed calculation)
    pub ibd_start_time: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    /// Last time we received an IBD batch (for stall detection)
    pub last_batch_time: Arc<std::sync::Mutex<Option<std::time::Instant>>>,
    /// How many times IBD stalled (for retry/abort logic)
    pub stall_retries: Arc<AtomicU64>,
    /// Address of the peer we are currently syncing from
    pub ibd_peer: Arc<std::sync::Mutex<Option<String>>>,
}

impl SyncStatus {
    pub fn new() -> Self {
        Self {
            state: Arc::new(std::sync::Mutex::new(SyncState::Steady)),
            syncing: Arc::new(AtomicBool::new(false)),
            target_height: Arc::new(AtomicU64::new(0)),
            download_height: Arc::new(AtomicU64::new(0)),
            blocks_downloaded: Arc::new(AtomicU64::new(0)),
            ibd_start_time: Arc::new(std::sync::Mutex::new(None)),
            last_batch_time: Arc::new(std::sync::Mutex::new(None)),
            stall_retries: Arc::new(AtomicU64::new(0)),
            ibd_peer: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    pub fn state(&self) -> SyncState {
        *self.state.lock().unwrap()
    }

    pub fn is_ibd(&self) -> bool {
        self.state() == SyncState::IBD
    }

    pub fn enter_ibd(&self, target: u64, peer_addr: &str) {
        *self.state.lock().unwrap() = SyncState::IBD;
        self.syncing.store(true, Ordering::Relaxed);
        self.target_height.store(target, Ordering::Relaxed);
        self.blocks_downloaded.store(0, Ordering::Relaxed);
        self.stall_retries.store(0, Ordering::Relaxed);
        *self.ibd_start_time.lock().unwrap() = Some(std::time::Instant::now());
        *self.last_batch_time.lock().unwrap() = Some(std::time::Instant::now());
        *self.ibd_peer.lock().unwrap() = Some(peer_addr.to_string());
        println!("📥 Entering IBD mode — target height: {} (peer: {})", target, peer_addr);
    }

    pub fn exit_ibd(&self) {
        let downloaded = self.blocks_downloaded.load(Ordering::Relaxed);
        let elapsed = self.ibd_start_time.lock().unwrap()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(1.0);
        let bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
        
        println!(
            "✅ IBD complete — {} blocks downloaded in {:.1}s ({:.0} blocks/sec)",
            downloaded, elapsed, bps
        );
        
        *self.state.lock().unwrap() = SyncState::Steady;
        self.syncing.store(false, Ordering::Relaxed);
        *self.ibd_start_time.lock().unwrap() = None;
        *self.last_batch_time.lock().unwrap() = None;
        *self.ibd_peer.lock().unwrap() = None;
    }

    /// Abort IBD without "complete" message (e.g. stall or peer disconnect).
    pub fn abort_ibd(&self, reason: &str) {
        let downloaded = self.blocks_downloaded.load(Ordering::Relaxed);
        println!(
            "⚠️ IBD aborted — {} blocks downloaded, reason: {}",
            downloaded, reason
        );
        *self.state.lock().unwrap() = SyncState::Steady;
        self.syncing.store(false, Ordering::Relaxed);
        *self.ibd_start_time.lock().unwrap() = None;
        *self.last_batch_time.lock().unwrap() = None;
        *self.ibd_peer.lock().unwrap() = None;
    }

    pub fn update_progress(&self, height: u64) {
        self.download_height.store(height, Ordering::Relaxed);
        self.blocks_downloaded.fetch_add(1, Ordering::Relaxed);
        *self.last_batch_time.lock().unwrap() = Some(std::time::Instant::now());
    }

    /// Check if IBD has stalled (no progress for IBD_STALL_TIMEOUT_SECS).
    pub fn is_stalled(&self) -> bool {
        if !self.is_ibd() {
            return false;
        }
        let last = self.last_batch_time.lock().unwrap();
        match *last {
            Some(t) => t.elapsed().as_secs() > IBD_STALL_TIMEOUT_SECS,
            None => false,
        }
    }

    /// Record a stall event. Returns true if retries exceeded.
    pub fn record_stall(&self) -> bool {
        let retries = self.stall_retries.fetch_add(1, Ordering::Relaxed) + 1;
        println!("⚠️ IBD stall detected (retry {}/{})", retries, IBD_MAX_STALL_RETRIES);
        retries >= IBD_MAX_STALL_RETRIES as u64
    }

    /// Is the given peer the one we're currently syncing from?
    pub fn is_ibd_peer(&self, addr: &str) -> bool {
        let peer = self.ibd_peer.lock().unwrap();
        peer.as_deref() == Some(addr)
    }

    pub fn progress_report(&self) -> String {
        let current = self.download_height.load(Ordering::Relaxed);
        let target = self.target_height.load(Ordering::Relaxed);
        let downloaded = self.blocks_downloaded.load(Ordering::Relaxed);
        let elapsed = self.ibd_start_time.lock().unwrap()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        
        let bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
        let remaining = target.saturating_sub(current);
        let eta = if bps > 0.0 { remaining as f64 / bps } else { 0.0 };
        
        let pct = if target > 0 { (current as f64 / target as f64 * 100.0).min(100.0) } else { 0.0 };
        
        format!(
            "📥 IBD: {}/{} ({:.1}%) | {:.0} blocks/sec | ETA: {:.0}s",
            current, target, pct, bps, eta
        )
    }

    /// Produce a serializable snapshot for RPC/metrics.
    pub fn to_json(&self) -> SyncSnapshot {
        let current = self.download_height.load(Ordering::Relaxed);
        let target = self.target_height.load(Ordering::Relaxed);
        let downloaded = self.blocks_downloaded.load(Ordering::Relaxed);
        let elapsed = self.ibd_start_time.lock().unwrap()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        let bps = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
        let remaining = target.saturating_sub(current);
        let eta = if bps > 0.0 { remaining as f64 / bps } else { 0.0 };
        let pct = if target > 0 { (current as f64 / target as f64 * 100.0).min(100.0) } else { 0.0 };
        
        SyncSnapshot {
            state: self.state(),
            syncing: self.syncing.load(Ordering::Relaxed),
            target_height: target,
            download_height: current,
            blocks_downloaded: downloaded,
            elapsed_secs: elapsed,
            blocks_per_sec: bps,
            eta_secs: eta,
            percent: pct,
            stall_retries: self.stall_retries.load(Ordering::Relaxed) as u32,
            ibd_peer: self.ibd_peer.lock().unwrap().clone(),
        }
    }

    /// Check if we should enter IBD based on peer height vs our height.
    pub fn should_enter_ibd(&self, our_height: u64, peer_height: u64) -> bool {
        if self.is_ibd() {
            return false; // Already in IBD
        }
        peer_height > our_height + IBD_THRESHOLD
    }
}
