use tokio::time::{interval, Duration};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::net::TcpStream;
use std::net::SocketAddr;

use crate::state::State;
use crate::p2p::peers::PeerManager;
use crate::p2p::messages::Message;
use crate::p2p::security::Blacklist;
use crate::p2p::get_sync_status;
use crate::p2p::sync;

/// P2P heartbeat monitor
/// 
/// Periodically:
/// 1. Checks for stale peers (no activity > 60s)
/// 2. Attempts reconnection to known peers
/// 3. Sends ping messages to keep connections alive
/// 4. Detects IBD stalls and aborts if peer is unresponsive
pub async fn start_heartbeat(
    state: State,
    peers: Arc<PeerManager>,
    blacklist: Arc<Blacklist>,
    msg_rate_limiter: Arc<super::security::MessageRateLimiter>,
    initial_peers: Vec<String>,
) {
    let mut tick = interval(Duration::from_secs(30));
    
    loop {
        tick.tick().await;
        
        // === IBD stall detection ===
        let sync_status = get_sync_status();
        if sync_status.is_ibd() && sync_status.is_stalled() {
            let exhausted = sync_status.record_stall();
            if exhausted {
                // Too many stalls — abort IBD, will retry on next Tip/Handshake
                sync_status.abort_ibd("max stall retries exceeded");
                println!("⚠️ IBD stalled {} times, aborting. Will retry on next peer.", sync::IBD_MAX_STALL_RETRIES);
            } else {
                // Re-request from where we left off
                let from = sync_status.download_height.load(std::sync::atomic::Ordering::Relaxed) + 1;
                println!("⚠️ IBD stalled, re-requesting from height {}", from);
                peers.broadcast(Message::GetBlocksIBD {
                    from_height: from,
                    limit: sync::IBD_BATCH_SIZE,
                }).await;
            }
        }
        
        // Check for stale peers
        let stale = peers.get_stale_peers(60);
        for addr in stale {
            println!("Peer {} is stale, disconnecting", addr);
            peers.remove_peer(&addr);
        }
        
        // Reconnect to initial peers if disconnected
        // P2-03: Exponential backoff based on failure count (30s → 60s → 120s → max 300s)
        for peer_addr in &initial_peers {
            if let Ok(socket_addr) = peer_addr.parse::<SocketAddr>() {
                if !peers.is_connected(&socket_addr) {
                    // Check backoff: skip if not enough time has passed
                    let failures = peers.get_failures(&socket_addr);
                    let backoff_secs = std::cmp::min(30u64 * 2u64.saturating_pow(failures), 300);
                    let last_seen = peers.get_last_seen(&socket_addr);
                    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
                    if failures > 0 && now.saturating_sub(last_seen) < backoff_secs {
                        continue; // Not enough time since last attempt
                    }
                    
                    println!("Reconnecting to {} (attempt {}, backoff {}s)", peer_addr, failures + 1, backoff_secs);
                    
                    let state_clone = state.clone();
                    let peers_clone = peers.clone();
                    let blacklist_clone = blacklist.clone();
                    let msg_rate_clone = msg_rate_limiter.clone();
                    let peer_str = peer_addr.clone();
                    
                    tokio::spawn(async move {
                        match TcpStream::connect(&peer_str).await {
                            Ok(stream) => {
                                peers_clone.reset_failures(&socket_addr);
                                println!("Reconnected to {}", peer_str);
                                if let Err(e) = crate::p2p::handle_connection(
                                    stream,
                                    socket_addr,
                                    state_clone,
                                    peers_clone,
                                    blacklist_clone,
                                    msg_rate_clone,
                                    crate::p2p::peers::PeerDirection::Outbound,
                                ).await {
                                    println!("Reconnection to {} failed: {}", peer_str, e);
                                }
                            }
                            Err(e) => {
                                peers_clone.increment_failures(&socket_addr);
                                println!("Failed to reconnect to {}: {}", peer_str, e);
                            }
                        }
                    });
                }
            }
        }

        // Keepalive: ask peers for tip to keep connections active
        peers.broadcast(Message::GetTip).await;
    }
}
