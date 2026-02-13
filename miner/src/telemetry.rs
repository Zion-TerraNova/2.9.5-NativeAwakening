//! Real-time monitoring and telemetry for ZION miner
//!
//! Tracks detailed performance metrics and system health.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::System;
use tokio::sync::RwLock;

/// Miner telemetry data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Telemetry {
    /// Total hashes computed
    pub total_hashes: u64,
    
    /// Current hashrate (hashes/second)
    pub current_hashrate: f64,
    
    /// Average hashrate over last minute
    pub avg_hashrate_1m: f64,
    
    /// Average hashrate over last hour
    pub avg_hashrate_1h: f64,
    
    /// Shares submitted
    pub shares_submitted: u64,
    
    /// Shares accepted
    pub shares_accepted: u64,
    
    /// Shares rejected
    pub shares_rejected: u64,
    
    /// Blocks found
    pub blocks_found: u64,
    
    /// Current difficulty
    pub current_difficulty: f64,
    
    /// Uptime in seconds
    pub uptime_seconds: u64,
    
    /// CPU usage percentage
    pub cpu_usage: f64,
    
    /// Memory usage in MB
    pub memory_mb: u64,
    
    /// Temperature (if available)
    pub temperature_c: Option<f32>,
    
    /// Power consumption (if available)
    pub power_watts: Option<f32>,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            total_hashes: 0,
            current_hashrate: 0.0,
            avg_hashrate_1m: 0.0,
            avg_hashrate_1h: 0.0,
            shares_submitted: 0,
            shares_accepted: 0,
            shares_rejected: 0,
            blocks_found: 0,
            current_difficulty: 1.0,
            uptime_seconds: 0,
            cpu_usage: 0.0,
            memory_mb: 0,
            temperature_c: None,
            power_watts: None,
        }
    }
}

impl Telemetry {
    /// Calculate acceptance rate percentage
    pub fn acceptance_rate(&self) -> f64 {
        if self.shares_submitted == 0 {
            return 0.0;
        }
        (self.shares_accepted as f64 / self.shares_submitted as f64) * 100.0
    }
    
    /// Calculate rejection rate percentage
    pub fn rejection_rate(&self) -> f64 {
        if self.shares_submitted == 0 {
            return 0.0;
        }
        (self.shares_rejected as f64 / self.shares_submitted as f64) * 100.0
    }
    
    /// Estimated earnings per hour (based on current hashrate)
    pub fn estimated_hourly_earnings(&self, block_reward: f64, network_hashrate: f64) -> f64 {
        if network_hashrate == 0.0 {
            return 0.0;
        }
        
        // Simple calculation: (your_hashrate / network_hashrate) * blocks_per_hour * reward
        let blocks_per_hour = 60.0; // 1 block per minute
        (self.current_hashrate / network_hashrate) * blocks_per_hour * block_reward
    }
}

/// Hashrate sample for averaging
#[derive(Debug, Clone)]
struct HashrateSample {
    timestamp: Instant,
    hashes: u64,
}

/// Telemetry collector
pub struct TelemetryCollector {
    telemetry: Arc<RwLock<Telemetry>>,
    start_time: Instant,
    samples_1m: Arc<RwLock<Vec<HashrateSample>>>,
    samples_1h: Arc<RwLock<Vec<HashrateSample>>>,
    system: Arc<RwLock<System>>,
}

impl TelemetryCollector {
    pub fn new() -> Self {
        Self {
            telemetry: Arc::new(RwLock::new(Telemetry::default())),
            start_time: Instant::now(),
            samples_1m: Arc::new(RwLock::new(Vec::new())),
            samples_1h: Arc::new(RwLock::new(Vec::new())),
            system: Arc::new(RwLock::new(System::new_all())),
        }
    }
    
    /// Get current telemetry snapshot
    pub async fn snapshot(&self) -> Telemetry {
        self.telemetry.read().await.clone()
    }
    
    /// Record hashes computed
    pub async fn record_hashes(&self, count: u64) {
        let mut telemetry = self.telemetry.write().await;
        telemetry.total_hashes += count;
        
        // Add sample
        let sample = HashrateSample {
            timestamp: Instant::now(),
            hashes: telemetry.total_hashes,
        };
        
        self.samples_1m.write().await.push(sample.clone());
        self.samples_1h.write().await.push(sample);
    }
    
    /// Record share submission
    pub async fn record_share(&self, accepted: bool) {
        let mut telemetry = self.telemetry.write().await;
        telemetry.shares_submitted += 1;
        
        if accepted {
            telemetry.shares_accepted += 1;
        } else {
            telemetry.shares_rejected += 1;
        }
    }
    
    /// Record block found
    pub async fn record_block(&self) {
        let mut telemetry = self.telemetry.write().await;
        telemetry.blocks_found += 1;
    }
    
    /// Update difficulty
    pub async fn update_difficulty(&self, difficulty: f64) {
        let mut telemetry = self.telemetry.write().await;
        telemetry.current_difficulty = difficulty;
    }
    
    /// Update system metrics
    pub async fn update_system_metrics(&self) {
        let mut telemetry = self.telemetry.write().await;
        
        // Update uptime
        telemetry.uptime_seconds = self.start_time.elapsed().as_secs();
        
        // Calculate current hashrate
        if let Some(avg) = self.calculate_avg_hashrate(Duration::from_secs(10)).await {
            telemetry.current_hashrate = avg;
        }
        
        // Calculate 1m average
        if let Some(avg) = self.calculate_avg_hashrate(Duration::from_secs(60)).await {
            telemetry.avg_hashrate_1m = avg;
        }
        
        // Calculate 1h average
        if let Some(avg) = self.calculate_avg_hashrate(Duration::from_secs(3600)).await {
            telemetry.avg_hashrate_1h = avg;
        }

        // Update CPU/memory (cross-platform via sysinfo)
        let mut system = self.system.write().await;
        system.refresh_cpu_usage();
        system.refresh_memory();

        telemetry.cpu_usage = system.global_cpu_info().cpu_usage() as f64;

        // sysinfo reports memory in KiB; convert to MiB.
        telemetry.memory_mb = system.used_memory() / 1024;
    }
    
    /// Calculate average hashrate over duration
    async fn calculate_avg_hashrate(&self, duration: Duration) -> Option<f64> {
        let samples = self.samples_1m.read().await;
        let now = Instant::now();
        let cutoff = now - duration;
        
        // Find first and last samples within window
        let recent: Vec<_> = samples.iter()
            .filter(|s| s.timestamp > cutoff)
            .collect();
        
        if recent.len() < 2 {
            return None;
        }
        
        let first = recent.first()?;
        let last = recent.last()?;
        
        let time_diff = last.timestamp.duration_since(first.timestamp).as_secs_f64();
        if time_diff == 0.0 {
            return None;
        }
        
        let hash_diff = last.hashes.saturating_sub(first.hashes) as f64;
        Some(hash_diff / time_diff)
    }
    
    /// Clean old samples
    pub async fn cleanup_samples(&self) {
        let now = Instant::now();
        
        // Keep only last 1 minute for 1m samples
        let mut samples_1m = self.samples_1m.write().await;
        samples_1m.retain(|s| now.duration_since(s.timestamp) < Duration::from_secs(60));
        
        // Keep only last 1 hour for 1h samples
        let mut samples_1h = self.samples_1h.write().await;
        samples_1h.retain(|s| now.duration_since(s.timestamp) < Duration::from_secs(3600));
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_telemetry_basics() {
        let collector = TelemetryCollector::new();
        
        // Record some hashes
        collector.record_hashes(1000).await;
        collector.record_hashes(1000).await;
        
        let snapshot = collector.snapshot().await;
        assert_eq!(snapshot.total_hashes, 2000);
    }
    
    #[tokio::test]
    async fn test_share_tracking() {
        let collector = TelemetryCollector::new();
        
        // Submit shares
        collector.record_share(true).await;
        collector.record_share(true).await;
        collector.record_share(false).await;
        
        let snapshot = collector.snapshot().await;
        assert_eq!(snapshot.shares_submitted, 3);
        assert_eq!(snapshot.shares_accepted, 2);
        assert_eq!(snapshot.shares_rejected, 1);
        assert_eq!(snapshot.acceptance_rate(), 66.66666666666666);
    }
    
    #[tokio::test]
    async fn test_block_found() {
        let collector = TelemetryCollector::new();
        
        collector.record_block().await;
        
        let snapshot = collector.snapshot().await;
        assert_eq!(snapshot.blocks_found, 1);
    }

    #[tokio::test]
    async fn test_system_metrics_update_does_not_panic() {
        let collector = TelemetryCollector::new();

        collector.update_system_metrics().await;
        let snapshot = collector.snapshot().await;

        assert!(snapshot.cpu_usage >= 0.0);
        assert!(snapshot.cpu_usage <= 100.0);
        assert!(snapshot.memory_mb > 0);
    }
}
