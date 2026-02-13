/// Sprint 1.3 — IBD (Initial Block Download) Hardening Test Suite
///
/// Tests covering:
///   1.3.1  SyncStatus state machine (enter/exit/abort IBD)
///   1.3.2  IBD threshold logic (should_enter_ibd)
///   1.3.3  Progress tracking (update_progress, progress_report)
///   1.3.4  Stall detection (is_stalled, record_stall)
///   1.3.5  Peer tracking (ibd_peer, is_ibd_peer)
///   1.3.6  JSON snapshot (to_json) for RPC
///   1.3.7  P2P message serialization (GetBlocksIBD, BlocksIBD, HandshakeAck)
///   1.3.8  IBD constants sanity

use zion_core::p2p::sync::{
    SyncStatus, SyncState,
    IBD_THRESHOLD, IBD_BATCH_SIZE, IBD_MAX_MESSAGE_SIZE,
    IBD_STALL_TIMEOUT_SECS, IBD_MAX_STALL_RETRIES,
};
use zion_core::p2p::messages::Message;

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.8  IBD Constants
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_ibd_threshold_is_50() {
    assert_eq!(IBD_THRESHOLD, 50, "IBD kicks in when peer >50 blocks ahead");
}

#[test]
fn test_ibd_batch_size_is_500() {
    assert_eq!(IBD_BATCH_SIZE, 500, "Request 500 blocks per IBD batch");
}

#[test]
fn test_ibd_max_message_size_50mb() {
    assert_eq!(IBD_MAX_MESSAGE_SIZE, 50_000_000, "50MB max IBD message");
}

#[test]
fn test_ibd_stall_timeout_120s() {
    assert_eq!(IBD_STALL_TIMEOUT_SECS, 120, "Stall after 120s of no progress");
}

#[test]
fn test_ibd_max_stall_retries_3() {
    assert_eq!(IBD_MAX_STALL_RETRIES, 3, "Max 3 stall retries before abort");
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.1  SyncStatus State Machine
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_sync_status_initial_state_is_steady() {
    let ss = SyncStatus::new();
    assert_eq!(ss.state(), SyncState::Steady);
    assert!(!ss.is_ibd());
    assert!(!ss.syncing.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_enter_ibd_sets_state() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "127.0.0.1:8334");
    
    assert_eq!(ss.state(), SyncState::IBD);
    assert!(ss.is_ibd());
    assert!(ss.syncing.load(std::sync::atomic::Ordering::Relaxed));
    assert_eq!(ss.target_height.load(std::sync::atomic::Ordering::Relaxed), 1000);
    assert_eq!(ss.blocks_downloaded.load(std::sync::atomic::Ordering::Relaxed), 0);
}

#[test]
fn test_exit_ibd_resets_state() {
    let ss = SyncStatus::new();
    ss.enter_ibd(500, "10.0.0.1:8334");
    ss.update_progress(100);
    
    ss.exit_ibd();
    
    assert_eq!(ss.state(), SyncState::Steady);
    assert!(!ss.is_ibd());
    assert!(!ss.syncing.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_abort_ibd_resets_state() {
    let ss = SyncStatus::new();
    ss.enter_ibd(500, "10.0.0.1:8334");
    ss.update_progress(200);
    
    ss.abort_ibd("test abort");
    
    assert_eq!(ss.state(), SyncState::Steady);
    assert!(!ss.is_ibd());
    assert!(!ss.syncing.load(std::sync::atomic::Ordering::Relaxed));
}

#[test]
fn test_double_enter_ibd_updates_target() {
    let ss = SyncStatus::new();
    // First entry — IBD mode
    ss.enter_ibd(500, "peer1:8334");
    assert_eq!(ss.target_height.load(std::sync::atomic::Ordering::Relaxed), 500);
    
    // Exit and re-enter with higher target
    ss.exit_ibd();
    ss.enter_ibd(1000, "peer2:8334");
    assert_eq!(ss.target_height.load(std::sync::atomic::Ordering::Relaxed), 1000);
    assert!(ss.is_ibd_peer("peer2:8334"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.2  IBD Threshold Logic
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_should_enter_ibd_peer_ahead_by_51() {
    let ss = SyncStatus::new();
    assert!(ss.should_enter_ibd(100, 151), "Peer 51 blocks ahead → should IBD");
}

#[test]
fn test_should_not_enter_ibd_peer_ahead_by_50() {
    let ss = SyncStatus::new();
    assert!(!ss.should_enter_ibd(100, 150), "Peer exactly 50 blocks ahead → no IBD");
}

#[test]
fn test_should_not_enter_ibd_peer_behind() {
    let ss = SyncStatus::new();
    assert!(!ss.should_enter_ibd(100, 50), "Peer behind us → no IBD");
}

#[test]
fn test_should_not_enter_ibd_same_height() {
    let ss = SyncStatus::new();
    assert!(!ss.should_enter_ibd(100, 100), "Same height → no IBD");
}

#[test]
fn test_should_not_enter_ibd_already_in_ibd() {
    let ss = SyncStatus::new();
    ss.enter_ibd(500, "peer:8334");
    assert!(!ss.should_enter_ibd(100, 200), "Already in IBD → false");
}

#[test]
fn test_should_enter_ibd_from_genesis() {
    let ss = SyncStatus::new();
    assert!(ss.should_enter_ibd(0, 100), "New node at height 0, peer at 100 → IBD");
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.3  Progress Tracking
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_update_progress_increments_counter() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "peer:8334");
    
    ss.update_progress(100);
    ss.update_progress(101);
    ss.update_progress(102);
    
    assert_eq!(ss.download_height.load(std::sync::atomic::Ordering::Relaxed), 102);
    assert_eq!(ss.blocks_downloaded.load(std::sync::atomic::Ordering::Relaxed), 3);
}

#[test]
fn test_progress_report_format() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "peer:8334");
    
    // Simulate some progress
    for h in 1..=500 {
        ss.update_progress(h);
    }
    
    let report = ss.progress_report();
    assert!(report.contains("500/1000"), "Report should show 500/1000: {}", report);
    assert!(report.contains("50.0%"), "Report should show 50%: {}", report);
    assert!(report.contains("📥 IBD:"), "Report should have IBD prefix: {}", report);
}

#[test]
fn test_progress_report_when_not_syncing() {
    let ss = SyncStatus::new();
    let report = ss.progress_report();
    // Should not crash, just show 0/0
    assert!(report.contains("0/0"), "Not syncing → 0/0: {}", report);
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.4  Stall Detection
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_not_stalled_when_not_in_ibd() {
    let ss = SyncStatus::new();
    assert!(!ss.is_stalled(), "Not in IBD → not stalled");
}

#[test]
fn test_not_stalled_right_after_entering() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "peer:8334");
    assert!(!ss.is_stalled(), "Just entered IBD → not stalled");
}

#[test]
fn test_not_stalled_after_progress() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "peer:8334");
    ss.update_progress(100);
    assert!(!ss.is_stalled(), "Just got progress → not stalled");
}

#[test]
fn test_record_stall_increments_retries() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "peer:8334");
    
    assert!(!ss.record_stall(), "First stall → not exhausted");
    assert!(!ss.record_stall(), "Second stall → not exhausted");
    assert!(ss.record_stall(), "Third stall → exhausted (max=3)");
}

#[test]
fn test_enter_ibd_resets_stall_retries() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "peer1:8334");
    ss.record_stall();
    ss.record_stall();
    
    ss.exit_ibd();
    ss.enter_ibd(2000, "peer2:8334");
    
    assert_eq!(ss.stall_retries.load(std::sync::atomic::Ordering::Relaxed), 0);
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.5  Peer Tracking
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_ibd_peer_tracking() {
    let ss = SyncStatus::new();
    
    // No peer initially
    assert!(!ss.is_ibd_peer("any"));
    
    // Enter IBD with a specific peer
    ss.enter_ibd(500, "127.0.0.1:8334");
    assert!(ss.is_ibd_peer("127.0.0.1:8334"));
    assert!(!ss.is_ibd_peer("5.78.138.238:8335"));
}

#[test]
fn test_ibd_peer_cleared_on_exit() {
    let ss = SyncStatus::new();
    ss.enter_ibd(500, "127.0.0.1:8334");
    assert!(ss.is_ibd_peer("127.0.0.1:8334"));
    
    ss.exit_ibd();
    assert!(!ss.is_ibd_peer("127.0.0.1:8334"));
}

#[test]
fn test_ibd_peer_cleared_on_abort() {
    let ss = SyncStatus::new();
    ss.enter_ibd(500, "127.0.0.1:8334");
    
    ss.abort_ibd("test");
    assert!(!ss.is_ibd_peer("127.0.0.1:8334"));
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.6  JSON Snapshot (RPC)
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_to_json_steady_state() {
    let ss = SyncStatus::new();
    let snap = ss.to_json();
    
    assert_eq!(snap.state, SyncState::Steady);
    assert!(!snap.syncing);
    assert_eq!(snap.target_height, 0);
    assert_eq!(snap.download_height, 0);
    assert_eq!(snap.blocks_downloaded, 0);
    assert_eq!(snap.stall_retries, 0);
    assert!(snap.ibd_peer.is_none());
}

#[test]
fn test_to_json_ibd_state() {
    let ss = SyncStatus::new();
    ss.enter_ibd(1000, "10.0.0.1:8334");
    
    for h in 1..=250 {
        ss.update_progress(h);
    }
    
    let snap = ss.to_json();
    
    assert_eq!(snap.state, SyncState::IBD);
    assert!(snap.syncing);
    assert_eq!(snap.target_height, 1000);
    assert_eq!(snap.download_height, 250);
    assert_eq!(snap.blocks_downloaded, 250);
    assert!(snap.percent > 24.0 && snap.percent < 26.0, "Should be ~25%: {}", snap.percent);
    assert!(snap.blocks_per_sec > 0.0, "Should have non-zero speed");
    assert_eq!(snap.ibd_peer, Some("10.0.0.1:8334".to_string()));
}

#[test]
fn test_to_json_serializable() {
    let ss = SyncStatus::new();
    ss.enter_ibd(500, "peer:8334");
    ss.update_progress(100);
    
    let snap = ss.to_json();
    let json = serde_json::to_value(&snap).unwrap();
    
    assert!(json.get("state").is_some());
    assert!(json.get("syncing").is_some());
    assert!(json.get("target_height").is_some());
    assert!(json.get("download_height").is_some());
    assert!(json.get("blocks_per_sec").is_some());
    assert!(json.get("ibd_peer").is_some());
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.7  P2P Message Serialization
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_get_blocks_ibd_serialization() {
    let msg = Message::GetBlocksIBD {
        from_height: 100,
        limit: 500,
    };
    
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"GetBlocksIBD\""), "Should be tagged: {}", json);
    assert!(json.contains("\"from_height\":100"), "Should have from_height: {}", json);
    assert!(json.contains("\"limit\":500"), "Should have limit: {}", json);
    
    // Round-trip
    let parsed: Message = serde_json::from_str(&json).unwrap();
    match parsed {
        Message::GetBlocksIBD { from_height, limit } => {
            assert_eq!(from_height, 100);
            assert_eq!(limit, 500);
        }
        _ => panic!("Expected GetBlocksIBD"),
    }
}

#[test]
fn test_blocks_ibd_serialization() {
    let msg = Message::BlocksIBD {
        blocks: vec![],
        remaining: 42,
    };
    
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"BlocksIBD\""), "Should be tagged: {}", json);
    assert!(json.contains("\"remaining\":42"), "Should have remaining: {}", json);
    
    // Round-trip
    let parsed: Message = serde_json::from_str(&json).unwrap();
    match parsed {
        Message::BlocksIBD { blocks, remaining } => {
            assert!(blocks.is_empty());
            assert_eq!(remaining, 42);
        }
        _ => panic!("Expected BlocksIBD"),
    }
}

#[test]
fn test_handshake_ack_serialization() {
    let msg = Message::HandshakeAck {
        version: 1,
        height: 9876,
        nonce: 42,
    };
    
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"HandshakeAck\""), "Should be tagged: {}", json);
    assert!(json.contains("\"height\":9876"), "Should have height: {}", json);
    
    // Round-trip
    let parsed: Message = serde_json::from_str(&json).unwrap();
    match parsed {
        Message::HandshakeAck { version, height, nonce } => {
            assert_eq!(version, 1);
            assert_eq!(height, 9876);
            assert_eq!(nonce, 42);
        }
        _ => panic!("Expected HandshakeAck"),
    }
}

#[test]
fn test_handshake_serialization_with_network() {
    let msg = Message::Handshake {
        version: 1,
        agent: "ZionCore/0.2.0".to_string(),
        height: 500,
        network: "zion-testnet-v1".to_string(),
        nonce: 12345,
    };
    
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"network\":\"zion-testnet-v1\""));
    
    // Round-trip
    let parsed: Message = serde_json::from_str(&json).unwrap();
    match parsed {
        Message::Handshake { version, agent, height, network, nonce } => {
            assert_eq!(version, 1);
            assert_eq!(agent, "ZionCore/0.2.0");
            assert_eq!(height, 500);
            assert_eq!(network, "zion-testnet-v1");
            assert_eq!(nonce, 12345);
        }
        _ => panic!("Expected Handshake"),
    }
}

#[test]
fn test_get_blocks_ibd_default_limit() {
    // When limit is missing from JSON, default should be 500
    let json = r#"{"type":"GetBlocksIBD","from_height":0}"#;
    let parsed: Message = serde_json::from_str(json).unwrap();
    match parsed {
        Message::GetBlocksIBD { from_height, limit } => {
            assert_eq!(from_height, 0);
            assert_eq!(limit, 500, "Default IBD limit should be 500");
        }
        _ => panic!("Expected GetBlocksIBD"),
    }
}

#[test]
fn test_get_blocks_default_limit() {
    // When limit is missing from JSON, default should be 10
    let json = r#"{"type":"GetBlocks","from_height":0}"#;
    let parsed: Message = serde_json::from_str(json).unwrap();
    match parsed {
        Message::GetBlocks { from_height, limit } => {
            assert_eq!(from_height, 0);
            assert_eq!(limit, 10, "Default GetBlocks limit should be 10");
        }
        _ => panic!("Expected GetBlocks"),
    }
}

#[test]
fn test_get_tip_serialization() {
    let msg = Message::GetTip;
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"GetTip\""));
    
    let parsed: Message = serde_json::from_str(&json).unwrap();
    match parsed {
        Message::GetTip => {} // OK
        _ => panic!("Expected GetTip"),
    }
}

#[test]
fn test_tip_serialization() {
    let msg = Message::Tip {
        height: 42,
        hash: "abc123".to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    match parsed {
        Message::Tip { height, hash } => {
            assert_eq!(height, 42);
            assert_eq!(hash, "abc123");
        }
        _ => panic!("Expected Tip"),
    }
}

#[test]
fn test_new_block_serialization() {
    let msg = Message::NewBlock {
        height: 100,
        hash: "deadbeef".to_string(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: Message = serde_json::from_str(&json).unwrap();
    match parsed {
        Message::NewBlock { height, hash } => {
            assert_eq!(height, 100);
            assert_eq!(hash, "deadbeef");
        }
        _ => panic!("Expected NewBlock"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 1.3.8  IBD Lifecycle Integration
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_full_ibd_lifecycle() {
    let ss = SyncStatus::new();
    
    // 1. Start → Steady
    assert_eq!(ss.state(), SyncState::Steady);
    
    // 2. Peer at height 1000, we at 0 → should IBD
    assert!(ss.should_enter_ibd(0, 1000));
    ss.enter_ibd(1000, "seed:8334");
    assert_eq!(ss.state(), SyncState::IBD);
    
    // 3. Download blocks
    for h in 1..=1000 {
        ss.update_progress(h);
    }
    assert_eq!(ss.blocks_downloaded.load(std::sync::atomic::Ordering::Relaxed), 1000);
    
    // 4. Complete → back to Steady
    ss.exit_ibd();
    assert_eq!(ss.state(), SyncState::Steady);
    assert!(!ss.is_ibd());
    
    // 5. Should be able to enter IBD again if new peer appears
    assert!(ss.should_enter_ibd(1000, 2000));
}

#[test]
fn test_ibd_abort_and_retry_lifecycle() {
    let ss = SyncStatus::new();
    
    // Enter IBD
    ss.enter_ibd(1000, "peer1:8334");
    ss.update_progress(100);
    
    // Stall 3 times → abort
    ss.record_stall();
    ss.record_stall();
    assert!(ss.record_stall(), "Third stall exhausts retries");
    
    // Abort
    ss.abort_ibd("stall exceeded");
    assert_eq!(ss.state(), SyncState::Steady);
    
    // New peer appears → can re-enter IBD
    assert!(ss.should_enter_ibd(100, 1000));
    ss.enter_ibd(1000, "peer2:8334");
    assert_eq!(ss.state(), SyncState::IBD);
    assert_eq!(ss.stall_retries.load(std::sync::atomic::Ordering::Relaxed), 0, "Retries reset");
    assert!(ss.is_ibd_peer("peer2:8334"));
}

#[test]
fn test_concurrent_sync_status_access() {
    use std::sync::Arc;
    use std::thread;
    
    let ss = Arc::new(SyncStatus::new());
    ss.enter_ibd(10_000, "peer:8334");
    
    let mut handles = vec![];
    
    // 10 threads updating progress concurrently
    for i in 0..10 {
        let ss_clone = ss.clone();
        handles.push(thread::spawn(move || {
            for h in (i * 100)..((i + 1) * 100) {
                ss_clone.update_progress(h as u64);
            }
        }));
    }
    
    for h in handles {
        h.join().unwrap();
    }
    
    // All 1000 updates should be counted
    assert_eq!(
        ss.blocks_downloaded.load(std::sync::atomic::Ordering::Relaxed),
        1000,
        "All concurrent updates counted"
    );
}
