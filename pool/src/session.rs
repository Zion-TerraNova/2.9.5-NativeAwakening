/// Session manager for tracking miner sessions
/// 
/// Manages:
/// - Active sessions with state tracking
/// - Difficulty adjustment per session
/// - Share statistics
/// - Share statistics

use std::net::SocketAddr;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

// Legacy simple session (kept for compatibility)
pub struct Session {
    pub id: u64,
    pub user: Option<String>,
    pub addr: Option<SocketAddr>,
    pub authorized: bool,
}

impl Session {
    pub fn new(id: u64) -> Self { 
        Self { id, user: None, addr: None, authorized: false } 
    }
}

// Enhanced miner session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinerSession {
    /// Session ID
    pub id: String,

    /// Wallet address
    pub wallet: String,

    /// Worker name
    pub worker: Option<String>,

    /// Current difficulty
    pub difficulty: u64,

    /// Mining algorithm
    pub algorithm: String,

    /// Share statistics
    pub shares_submitted: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,

    /// Hashrate (H/s)
    pub hashrate: f64,

    /// Created timestamp
    pub created_at: u64,

    /// Last activity timestamp
    pub last_active: u64,
}

impl MinerSession {
    /// Create new session
    pub fn new(id: String, wallet: String, worker: Option<String>, algorithm: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id,
            wallet,
            worker,
            difficulty: 10_000, // Default difficulty
            algorithm,
            shares_submitted: 0,
            shares_accepted: 0,
            shares_rejected: 0,
            hashrate: 0.0,
            created_at: now,
            last_active: now,
        }
    }

    /// Update last activity
    pub fn touch(&mut self) {
        self.last_active = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    /// Record share submission
    pub fn record_share(&mut self, accepted: bool) {
        self.shares_submitted += 1;
        if accepted {
            self.shares_accepted += 1;
        } else {
            self.shares_rejected += 1;
        }
        self.touch();
    }

    /// Record share outcome without applying XP locally.
    ///
    /// Use this when XP is awarded via Redis-backed XPTracker.
    pub fn record_share_outcome(&mut self, accepted: bool) {
        self.shares_submitted += 1;
        if accepted {
            self.shares_accepted += 1;
        } else {
            self.shares_rejected += 1;
        }
        self.touch();
    }

    /// Get worker identifier
    pub fn worker_id(&self) -> String {
        match &self.worker {
            Some(worker) => format!("{}.{}", self.wallet, worker),
            None => self.wallet.clone(),
        }
    }
}

pub struct SessionManager {
    /// In-memory session cache
    sessions: Arc<RwLock<HashMap<String, MinerSession>>>,
}

impl SessionManager {
    /// Create new session manager
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create or get session
    pub async fn get_or_create(
        &self,
        session_id: String,
        wallet: String,
        worker: Option<String>,
        algorithm: String,
    ) -> MinerSession {
        // Check cache first
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&session_id) {
                return session.clone();
            }
        }

        // Create new session
        let session = MinerSession::new(session_id.clone(), wallet, worker, algorithm);

        // Cache it
        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), session.clone());
        }

        session
    }

    /// Update session
    pub async fn update(&self, session: &MinerSession) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());
    }

    /// Get session by ID
    pub async fn get(&self, session_id: &str) -> Option<MinerSession> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Remove session
    pub async fn remove(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
    }

    /// Get all active sessions
    pub async fn list_active(&self) -> Vec<MinerSession> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Clean up stale sessions (not active for >1 hour)
    pub async fn cleanup_stale(&self) -> usize {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut to_remove = Vec::new();

        {
            let sessions = self.sessions.read().await;
            for (id, session) in sessions.iter() {
                if now - session.last_active > 3600 {
                    // 1 hour
                    to_remove.push(id.clone());
                }
            }
        }

        let count = to_remove.len();

        {
            let mut sessions = self.sessions.write().await;
            for session_id in to_remove {
                sessions.remove(&session_id);
            }
        }

        if count > 0 {
            tracing::info!("🧹 Cleaned up {} stale sessions", count);
        }

        count
    }
}
