/// Metrics and observability module for ZION blockchain
/// 
/// Provides:
/// - Prometheus metrics exporter
/// - Real-time performance counters
/// - Health check status
/// - System resource monitoring

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Global metrics collector
pub struct Metrics {
    // Blockchain metrics
    pub blocks_processed: AtomicU64,
    pub blocks_rejected: AtomicU64,
    pub current_height: AtomicU64,
    pub current_difficulty: AtomicU64,
    
    // Transaction metrics
    pub txs_submitted: AtomicU64,
    pub txs_accepted: AtomicU64,
    pub txs_rejected: AtomicU64,
    pub txs_in_mempool: AtomicUsize,
    
    // Mempool metrics
    pub mempool_size_bytes: AtomicUsize,
    pub mempool_evictions: AtomicU64,
    
    // P2P metrics
    pub peers_connected: AtomicUsize,
    pub peers_total: AtomicUsize,
    pub messages_sent: AtomicU64,
    pub messages_received: AtomicU64,
    
    // Performance metrics
    pub validation_time_us: AtomicU64, // microseconds
    pub pow_time_us: AtomicU64,
    pub storage_writes: AtomicU64,
    pub storage_reads: AtomicU64,
    
    // System metrics
    pub start_time: Instant,
    pub last_block_time: AtomicU64, // Unix timestamp
}

impl Metrics {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            blocks_processed: AtomicU64::new(0),
            blocks_rejected: AtomicU64::new(0),
            current_height: AtomicU64::new(0),
            current_difficulty: AtomicU64::new(0),
            
            txs_submitted: AtomicU64::new(0),
            txs_accepted: AtomicU64::new(0),
            txs_rejected: AtomicU64::new(0),
            txs_in_mempool: AtomicUsize::new(0),
            
            mempool_size_bytes: AtomicUsize::new(0),
            mempool_evictions: AtomicU64::new(0),
            
            peers_connected: AtomicUsize::new(0),
            peers_total: AtomicUsize::new(0),
            messages_sent: AtomicU64::new(0),
            messages_received: AtomicU64::new(0),
            
            validation_time_us: AtomicU64::new(0),
            pow_time_us: AtomicU64::new(0),
            storage_writes: AtomicU64::new(0),
            storage_reads: AtomicU64::new(0),
            
            start_time: Instant::now(),
            last_block_time: AtomicU64::new(0),
        })
    }
    
    /// Export metrics in Prometheus format
    pub fn prometheus_export(&self) -> String {
        let uptime_secs = self.start_time.elapsed().as_secs();
        let last_block = self.last_block_time.load(Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let time_since_last_block = if last_block > 0 {
            now.saturating_sub(last_block)
        } else {
            0
        };
        
        format!(
r#"# HELP zion_blocks_processed_total Total blocks processed
# TYPE zion_blocks_processed_total counter
zion_blocks_processed_total {}

# HELP zion_blocks_rejected_total Total blocks rejected
# TYPE zion_blocks_rejected_total counter
zion_blocks_rejected_total {}

# HELP zion_blockchain_height Current blockchain height
# TYPE zion_blockchain_height gauge
zion_blockchain_height {}

# HELP zion_blockchain_difficulty Current mining difficulty
# TYPE zion_blockchain_difficulty gauge
zion_blockchain_difficulty {}

# HELP zion_txs_submitted_total Total transactions submitted
# TYPE zion_txs_submitted_total counter
zion_txs_submitted_total {}

# HELP zion_txs_accepted_total Total transactions accepted
# TYPE zion_txs_accepted_total counter
zion_txs_accepted_total {}

# HELP zion_txs_rejected_total Total transactions rejected
# TYPE zion_txs_rejected_total counter
zion_txs_rejected_total {}

# HELP zion_mempool_size Number of transactions in mempool
# TYPE zion_mempool_size gauge
zion_mempool_size {}

# HELP zion_mempool_size_bytes Mempool size in bytes
# TYPE zion_mempool_size_bytes gauge
zion_mempool_size_bytes {}

# HELP zion_mempool_evictions_total Total mempool evictions
# TYPE zion_mempool_evictions_total counter
zion_mempool_evictions_total {}

# HELP zion_peers_connected Currently connected peers
# TYPE zion_peers_connected gauge
zion_peers_connected {}

# HELP zion_peers_total Total known peers
# TYPE zion_peers_total gauge
zion_peers_total {}

# HELP zion_p2p_messages_sent_total P2P messages sent
# TYPE zion_p2p_messages_sent_total counter
zion_p2p_messages_sent_total {}

# HELP zion_p2p_messages_received_total P2P messages received
# TYPE zion_p2p_messages_received_total counter
zion_p2p_messages_received_total {}

# HELP zion_validation_time_us Average block validation time (microseconds)
# TYPE zion_validation_time_us gauge
zion_validation_time_us {}

# HELP zion_pow_time_us Average PoW validation time (microseconds)
# TYPE zion_pow_time_us gauge
zion_pow_time_us {}

# HELP zion_storage_writes_total Total storage write operations
# TYPE zion_storage_writes_total counter
zion_storage_writes_total {}

# HELP zion_storage_reads_total Total storage read operations
# TYPE zion_storage_reads_total counter
zion_storage_reads_total {}

# HELP zion_uptime_seconds Node uptime in seconds
# TYPE zion_uptime_seconds gauge
zion_uptime_seconds {}

# HELP zion_time_since_last_block_seconds Time since last block
# TYPE zion_time_since_last_block_seconds gauge
zion_time_since_last_block_seconds {}
"#,
            self.blocks_processed.load(Ordering::Relaxed),
            self.blocks_rejected.load(Ordering::Relaxed),
            self.current_height.load(Ordering::Relaxed),
            self.current_difficulty.load(Ordering::Relaxed),
            
            self.txs_submitted.load(Ordering::Relaxed),
            self.txs_accepted.load(Ordering::Relaxed),
            self.txs_rejected.load(Ordering::Relaxed),
            self.txs_in_mempool.load(Ordering::Relaxed),
            self.mempool_size_bytes.load(Ordering::Relaxed),
            self.mempool_evictions.load(Ordering::Relaxed),
            
            self.peers_connected.load(Ordering::Relaxed),
            self.peers_total.load(Ordering::Relaxed),
            self.messages_sent.load(Ordering::Relaxed),
            self.messages_received.load(Ordering::Relaxed),
            
            self.validation_time_us.load(Ordering::Relaxed),
            self.pow_time_us.load(Ordering::Relaxed),
            self.storage_writes.load(Ordering::Relaxed),
            self.storage_reads.load(Ordering::Relaxed),
            
            uptime_secs,
            time_since_last_block,
        )
    }
    
    /// Get health check status
    pub fn health_check(&self) -> HealthStatus {
        let uptime = self.start_time.elapsed().as_secs();
        let last_block = self.last_block_time.load(Ordering::Relaxed);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let time_since_last_block = if last_block > 0 {
            now.saturating_sub(last_block)
        } else {
            0
        };
        
        // Health criteria — 15 min threshold for testnet (low hashrate = longer gaps)
        let is_healthy = time_since_last_block < 900; // 15 minutes
        let peers_ok = self.peers_connected.load(Ordering::Relaxed) > 0;
        let mempool_ok = self.txs_in_mempool.load(Ordering::Relaxed) < 50_000;
        
        let status = if is_healthy && peers_ok && mempool_ok {
            "healthy"
        } else if is_healthy {
            "degraded"
        } else {
            "unhealthy"
        };
        
        HealthStatus {
            status: status.to_string(),
            network: crate::network::get_network().name().to_string(),
            uptime_seconds: uptime,
            height: self.current_height.load(Ordering::Relaxed),
            difficulty: self.current_difficulty.load(Ordering::Relaxed),
            peers_connected: self.peers_connected.load(Ordering::Relaxed),
            mempool_size: self.txs_in_mempool.load(Ordering::Relaxed),
            time_since_last_block,
            blocks_processed: self.blocks_processed.load(Ordering::Relaxed),
            blocks_rejected: self.blocks_rejected.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, serde::Serialize)]
pub struct HealthStatus {
    pub status: String, // "healthy" | "degraded" | "unhealthy"
    pub network: String, // "testnet" | "mainnet"
    pub uptime_seconds: u64,
    pub height: u64,
    pub difficulty: u64,
    pub peers_connected: usize,
    pub mempool_size: usize,
    pub time_since_last_block: u64,
    pub blocks_processed: u64,
    pub blocks_rejected: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_metrics_initialization() {
        let metrics = Metrics::new();
        assert_eq!(metrics.blocks_processed.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.txs_in_mempool.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.peers_connected.load(Ordering::Relaxed), 0);
    }
    
    #[test]
    fn test_prometheus_export_format() {
        let metrics = Metrics::new();
        metrics.blocks_processed.store(100, Ordering::Relaxed);
        metrics.current_height.store(42, Ordering::Relaxed);
        
        let export = metrics.prometheus_export();
        assert!(export.contains("zion_blocks_processed_total 100"));
        assert!(export.contains("zion_blockchain_height 42"));
        assert!(export.contains("# TYPE zion_blocks_processed_total counter"));
    }
    
    #[test]
    fn test_health_check_status() {
        let metrics = Metrics::new();
        metrics.current_height.store(100, Ordering::Relaxed);
        metrics.peers_connected.store(5, Ordering::Relaxed);
        metrics.last_block_time.store(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            Ordering::Relaxed
        );
        
        let health = metrics.health_check();
        assert_eq!(health.status, "healthy");
        assert_eq!(health.height, 100);
        assert_eq!(health.peers_connected, 5);
    }
}
