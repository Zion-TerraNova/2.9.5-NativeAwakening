use std::path::Path;
use anyhow::Result;
use tokio::fs;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PersistedPeer {
    pub addr: String,
    pub last_seen: u64,
    pub success_count: u32,
    pub fail_count: u32,
}

/// Save known peers to disk
pub async fn save_peers(peers: &[PersistedPeer], path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(peers)?;
    fs::write(path, json).await?;
    Ok(())
}

/// Load known peers from disk
pub async fn load_peers(path: &Path) -> Result<Vec<PersistedPeer>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    
    let data = fs::read_to_string(path).await?;
    let peers: Vec<PersistedPeer> = serde_json::from_str(&data)?;
    Ok(peers)
}

/// Convert PeerInfo to PersistedPeer
pub fn to_persisted(info: &crate::p2p::peers::PeerInfo) -> PersistedPeer {
    PersistedPeer {
        addr: info.addr.to_string(),
        last_seen: info.last_seen,
        success_count: 0, // Would need to track this separately
        fail_count: info.failed_attempts,
    }
}

/// Get top peers sorted by reliability (low failures, recent activity)
pub fn get_best_peers(peers: &[PersistedPeer], limit: usize) -> Vec<String> {
    let mut sorted = peers.to_vec();
    
    // Sort by: low fail_count, then high last_seen
    sorted.sort_by(|a, b| {
        let fail_cmp = a.fail_count.cmp(&b.fail_count);
        if fail_cmp == std::cmp::Ordering::Equal {
            b.last_seen.cmp(&a.last_seen) // Higher last_seen first
        } else {
            fail_cmp
        }
    });
    
    sorted.into_iter()
        .take(limit)
        .map(|p| p.addr)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn test_peer_persistence() {
        let temp_path = Path::new("/tmp/zion_test_peers.json");
        
        let peers = vec![
            PersistedPeer {
                addr: "127.0.0.1:8089".to_string(),
                last_seen: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
                success_count: 10,
                fail_count: 0,
            },
        ];
        
        // Save
        save_peers(&peers, temp_path).await.unwrap();
        
        // Load
        let loaded = load_peers(temp_path).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].addr, "127.0.0.1:8089");
        
        // Cleanup
        let _ = tokio::fs::remove_file(temp_path).await;
    }

    #[test]
    fn test_best_peers_sorting() {
        let peers = vec![
            PersistedPeer {
                addr: "bad.peer:8089".to_string(),
                last_seen: 1000,
                success_count: 5,
                fail_count: 10, // High failures
            },
            PersistedPeer {
                addr: "good.peer:8089".to_string(),
                last_seen: 2000,
                success_count: 20,
                fail_count: 0, // No failures
            },
            PersistedPeer {
                addr: "stale.peer:8089".to_string(),
                last_seen: 500, // Old
                success_count: 15,
                fail_count: 0,
            },
        ];
        
        let best = get_best_peers(&peers, 2);
        assert_eq!(best.len(), 2);
        assert_eq!(best[0], "good.peer:8089"); // Best: no failures, recent
        assert_eq!(best[1], "stale.peer:8089"); // Second: no failures, but older
    }
}
