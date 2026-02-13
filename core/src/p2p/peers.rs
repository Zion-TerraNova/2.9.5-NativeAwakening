use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use std::path::Path;
use tokio::sync::mpsc;
use crate::p2p::messages::Message;
use crate::p2p::persistence::{self, PersistedPeer};

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub addr: SocketAddr,
    pub height: u64,
    pub sub_version: String,
    pub last_seen: u64,
    pub failed_attempts: u32,
}

/// P1-07: Peer direction — prevents eclipse attack by reserving outbound slots
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeerDirection {
    Inbound,
    Outbound,
}

#[derive(Clone)]
pub struct PeerManager {
    // Info about known peers (metadata)
    pub known_peers: Arc<Mutex<HashMap<SocketAddr, PeerInfo>>>,
    // Active connections (channels to write loop)
    pub active_peers: Arc<Mutex<HashMap<SocketAddr, mpsc::Sender<Message>>>>,
    // P1-07: Track peer direction (inbound vs outbound)
    peer_directions: Arc<Mutex<HashMap<SocketAddr, PeerDirection>>>,
}

impl PeerManager {
    pub fn new() -> Self {
        Self {
            known_peers: Arc::new(Mutex::new(HashMap::new())),
            active_peers: Arc::new(Mutex::new(HashMap::new())),
            peer_directions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn add_peer(&self, addr: SocketAddr, mut info: PeerInfo) {
        info.failed_attempts = 0;
        let mut peers = self.known_peers.lock().unwrap();
        peers.insert(addr, info);
    }

    pub fn register_connection(&self, addr: SocketAddr, sender: mpsc::Sender<Message>) {
        let mut active = self.active_peers.lock().unwrap();
        active.insert(addr, sender);
    }

    /// P1-07: Register connection with direction tracking
    pub fn register_connection_with_direction(&self, addr: SocketAddr, sender: mpsc::Sender<Message>, direction: PeerDirection) {
        let mut active = self.active_peers.lock().unwrap();
        active.insert(addr, sender);
        let mut dirs = self.peer_directions.lock().unwrap();
        dirs.insert(addr, direction);
    }
    
    pub fn remove_peer(&self, addr: &SocketAddr) {
        let mut peers = self.known_peers.lock().unwrap();
        peers.remove(addr);
        let mut active = self.active_peers.lock().unwrap();
        active.remove(addr);
        let mut dirs = self.peer_directions.lock().unwrap();
        dirs.remove(addr);
    }
    
    pub async fn broadcast(&self, msg: Message) {
        // Clone senders to avoid holding lock while sending
        let senders: Vec<mpsc::Sender<Message>> = {
            let active = self.active_peers.lock().unwrap();
            active.values().cloned().collect()
        };
        
        for tx in senders {
            // Check if capacity exists, otherwise drop (or block if critical?)
            // For P2P gossip, drop is usually acceptable if peer is slow.
            let _ = tx.send(msg.clone()).await; 
            // Note: Message needs to be Clone
        }
    }
    
    pub fn get_peers(&self) -> Vec<PeerInfo> {
        let peers = self.known_peers.lock().unwrap();
        peers.values().cloned().collect()
    }

    pub fn active_count(&self) -> usize {
        let active = self.active_peers.lock().unwrap();
        active.len()
    }

    pub fn get_stale_peers(&self, timeout_secs: u64) -> Vec<SocketAddr> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let peers = self.known_peers.lock().unwrap();
        peers
            .iter()
            .filter(|(_, info)| now - info.last_seen > timeout_secs)
            .map(|(addr, _)| *addr)
            .collect()
    }

    pub fn update_last_seen(&self, addr: &SocketAddr) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut peers = self.known_peers.lock().unwrap();
        if let Some(info) = peers.get_mut(addr) {
            info.last_seen = now;
        }
    }

    pub fn update_peer_height(&self, addr: &SocketAddr, height: u64) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let mut peers = self.known_peers.lock().unwrap();
        if let Some(info) = peers.get_mut(addr) {
            info.height = height;
            info.last_seen = now;
        }
    }

    pub fn increment_failures(&self, addr: &SocketAddr) {
        let mut peers = self.known_peers.lock().unwrap();
        if let Some(info) = peers.get_mut(addr) {
            info.failed_attempts += 1;
        }
    }

    pub fn reset_failures(&self, addr: &SocketAddr) {
        let mut peers = self.known_peers.lock().unwrap();
        if let Some(info) = peers.get_mut(addr) {
            info.failed_attempts = 0;
        }
    }

    /// P2-03: Get failure count for backoff calculation
    pub fn get_failures(&self, addr: &SocketAddr) -> u32 {
        let peers = self.known_peers.lock().unwrap();
        peers.get(addr).map(|info| info.failed_attempts).unwrap_or(0)
    }

    /// P2-03: Get last seen timestamp for backoff timing
    pub fn get_last_seen(&self, addr: &SocketAddr) -> u64 {
        let peers = self.known_peers.lock().unwrap();
        peers.get(addr).map(|info| info.last_seen).unwrap_or(0)
    }

    pub fn is_connected(&self, addr: &SocketAddr) -> bool {
        let active = self.active_peers.lock().unwrap();
        active.contains_key(addr)
    }

    /// Save peers to disk for future bootstrap
    pub async fn save_to_disk(&self, path: &Path) -> anyhow::Result<()> {
        let peers_to_save: Vec<PersistedPeer> = {
            let peers = self.known_peers.lock().unwrap();
            peers.values()
                .map(|info| persistence::to_persisted(info))
                .collect()
        };
        
        persistence::save_peers(&peers_to_save, path).await?;
        println!("[P2P] Saved {} peers to {:?}", peers_to_save.len(), path);
        Ok(())
    }

    /// Load peers from disk on startup
    pub async fn load_from_disk(&self, path: &Path) -> anyhow::Result<Vec<String>> {
        let persisted = persistence::load_peers(path).await?;
        
        if persisted.is_empty() {
            println!("[P2P] No saved peers found");
            return Ok(Vec::new());
        }
        
        // Get best peers (low failures, recent)
        let best = persistence::get_best_peers(&persisted, 10);
        println!("[P2P] Loaded {} peers from disk, using {} best", persisted.len(), best.len());
        
        Ok(best)
    }

    // --- P1-07: Inbound/Outbound slot separation ---

    /// Count how many inbound connections are active.
    pub fn inbound_count(&self) -> usize {
        let dirs = self.peer_directions.lock().unwrap();
        dirs.values().filter(|d| **d == PeerDirection::Inbound).count()
    }

    /// Count how many outbound connections are active.
    pub fn outbound_count(&self) -> usize {
        let dirs = self.peer_directions.lock().unwrap();
        dirs.values().filter(|d| **d == PeerDirection::Outbound).count()
    }

    /// P1-07: Check if an inbound connection can be accepted.
    /// Reserves `min_outbound_slots` slots exclusively for outbound peers
    /// to prevent an eclipse attack where all slots are filled by attacker
    /// inbound connections.
    pub fn allow_inbound(&self, max_total: usize, min_outbound_slots: usize) -> bool {
        let active = self.active_peers.lock().unwrap();
        let total = active.len();
        if total >= max_total {
            return false;
        }
        // Reserve slots for outbound connections
        let max_inbound = max_total.saturating_sub(min_outbound_slots);
        let inbound = self.inbound_count();
        inbound < max_inbound
    }
}
