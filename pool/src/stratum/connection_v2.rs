/// Connection state management for Stratum miners
/// 
/// Tracks individual miner connections with:
/// - Authentication state
/// - Session metadata
/// - Activity tracking
/// - Protocol detection

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::vardiff::VarDiffState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    /// Initial state after connection
    Connected,
    /// After successful subscribe (Stratum)
    Subscribed,
    /// After successful login/authorize
    Authenticated,
    /// Connection being closed
    Disconnecting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Unknown,
    XMRig,   // XMRig JSON-RPC protocol
    Stratum, // Standard Stratum protocol
}

pub struct Connection {
    /// Unique session ID
    pub session_id: String,

    /// Peer address
    pub peer_addr: SocketAddr,

    /// Current connection state
    pub state: ConnectionState,

    /// Detected protocol
    pub protocol: Protocol,

    /// Wallet address (after authentication)
    pub wallet_address: Option<String>,

    /// Worker name/ID
    pub worker_name: Option<String>,

    /// Mining algorithm
    pub algorithm: Option<String>,

    /// User agent string
    pub user_agent: Option<String>,

    /// Subscription ID (Stratum)
    pub subscription_id: Option<String>,

    /// P1-25: Unique per-session extranonce1 (4-byte hex)
    pub extranonce1: String,

    /// Current difficulty
    pub difficulty: u64,

    /// Last activity timestamp
    last_activity: Instant,

    /// Connection established time
    connected_at: Instant,

    /// Share statistics
    pub shares_submitted: u64,
    pub shares_accepted: u64,
    pub shares_rejected: u64,

    /// Current job ID
    pub current_job_id: Option<String>,

    /// VarDiff state for this connection
    pub vardiff: VarDiffState,

    /// Outbound writer channel (server -> miner)
    pub outbound: Option<mpsc::UnboundedSender<String>>,
}

impl Connection {
    /// Create new connection
    pub fn new(session_id: String, peer_addr: SocketAddr) -> Self {
        let now = Instant::now();

        // P1-25: Derive a unique 4-byte extranonce1 from session_id before moving it
        let extranonce1 = {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            session_id.hash(&mut hasher);
            let h = hasher.finish();
            format!("{:08x}", (h & 0xFFFF_FFFF) as u32)
        };

        Self {
            session_id,
            peer_addr,
            state: ConnectionState::Connected,
            protocol: Protocol::Unknown,
            wallet_address: None,
            worker_name: None,
            algorithm: None,
            user_agent: None,
            subscription_id: None,
            extranonce1,
            difficulty: 500_000, // Default share difficulty for Cosmic Harmony
            // GPU miners at 2-3 MH/s find ~1 share/15s at diff ~38M.
            // Start lower so VarDiff can tune up quickly.
            last_activity: now,
            connected_at: now,
            shares_submitted: 0,
            shares_accepted: 0,
            shares_rejected: 0,
            current_job_id: None,
            vardiff: VarDiffState::new(None),
            outbound: None,
        }
    }

    /// Update difficulty based on VarDiff window.
    /// Returns Some(new_difficulty) when changed.
    pub fn vardiff_on_share(&mut self, accepted: bool) -> Option<u64> {
        let now = Instant::now();
        self.vardiff.on_share(now, accepted, self.difficulty)
    }

    /// Update last activity timestamp
    pub fn update_activity(&mut self) {
        self.last_activity = Instant::now();
    }

    /// Check if connection is stale (inactive for too long)
    pub fn is_stale(&self, timeout: Duration) -> bool {
        self.last_activity.elapsed() > timeout
    }

    /// Get connection uptime
    pub fn uptime(&self) -> Duration {
        self.connected_at.elapsed()
    }

    /// Check if authenticated
    pub fn is_authenticated(&self) -> bool {
        self.state == ConnectionState::Authenticated
    }

    /// Get worker identifier (wallet.worker or just wallet)
    pub fn worker_id(&self) -> Option<String> {
        match (&self.wallet_address, &self.worker_name) {
            (Some(wallet), Some(worker)) => Some(format!("{}.{}", wallet, worker)),
            (Some(wallet), None) => Some(wallet.clone()),
            _ => None,
        }
    }

    /// Record share submission
    pub fn record_share(&mut self, accepted: bool) {
        self.shares_submitted += 1;
        if accepted {
            self.shares_accepted += 1;
        } else {
            self.shares_rejected += 1;
        }
    }

    /// Get share acceptance rate (0.0 - 1.0)
    pub fn acceptance_rate(&self) -> f64 {
        if self.shares_submitted == 0 {
            return 0.0;
        }
        self.shares_accepted as f64 / self.shares_submitted as f64
    }

    /// Detect protocol from first message
    pub fn detect_protocol(&mut self, method: &str) {
        if self.protocol != Protocol::Unknown {
            return;
        }

        self.protocol = match method {
            "login" | "keepalived" | "getjob" => Protocol::XMRig,
            "mining.subscribe" | "mining.authorize" | "mining.submit" => Protocol::Stratum,
            _ => Protocol::Unknown,
        };

        if self.protocol != Protocol::Unknown {
            tracing::debug!(
                "🔍 Detected protocol: {:?} for session {}",
                self.protocol,
                self.session_id
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn test_connection_creation() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
        let conn = Connection::new("test-session".to_string(), addr);

        assert_eq!(conn.state, ConnectionState::Connected);
        assert_eq!(conn.protocol, Protocol::Unknown);
        assert!(conn.wallet_address.is_none());
        assert_eq!(conn.shares_submitted, 0);
    }

    #[test]
    fn test_worker_id() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
        let mut conn = Connection::new("test".to_string(), addr);

        assert!(conn.worker_id().is_none());

        conn.wallet_address = Some("ZION123".to_string());
        assert_eq!(conn.worker_id(), Some("ZION123".to_string()));

        conn.worker_name = Some("miner1".to_string());
        assert_eq!(conn.worker_id(), Some("ZION123.miner1".to_string()));
    }

    #[test]
    fn test_share_tracking() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
        let mut conn = Connection::new("test".to_string(), addr);

        conn.record_share(true);
        conn.record_share(true);
        conn.record_share(false);

        assert_eq!(conn.shares_submitted, 3);
        assert_eq!(conn.shares_accepted, 2);
        assert_eq!(conn.shares_rejected, 1);
        assert!((conn.acceptance_rate() - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_protocol_detection() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
        let mut conn = Connection::new("test".to_string(), addr);

        conn.detect_protocol("login");
        assert_eq!(conn.protocol, Protocol::XMRig);

        let mut conn2 = Connection::new("test2".to_string(), addr);
        conn2.detect_protocol("mining.subscribe");
        assert_eq!(conn2.protocol, Protocol::Stratum);
    }

    #[test]
    fn test_stale_detection() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
        let conn = Connection::new("test".to_string(), addr);

        assert!(!conn.is_stale(Duration::from_secs(60)));
        // Note: Can't easily test true case without waiting or mocking time
    }
}
