/// Advanced load testing framework for ZION blockchain
/// 
/// Features:
/// - Configurable TPS (1-10,000+)
/// - Realistic transaction patterns
/// - Concurrent block generation
/// - Real-time metrics collection
/// - Performance profiling
/// - Latency histograms

use tokio::time::{sleep, Duration, Instant};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::state::State;
use crate::tx::{Transaction, TxInput, TxOutput};
use crate::blockchain::block::Block;

/// Load test configuration
pub struct LoadTestConfig {
    /// Target transactions per second
    pub tx_per_second: u32,
    /// Block interval in seconds
    pub block_interval_secs: u64,
    /// Test duration in seconds
    pub duration_secs: u64,
    /// Number of concurrent mining threads
    pub num_miners: u32,
    /// Enable detailed metrics
    pub detailed_metrics: bool,
    /// Print progress every N seconds
    pub progress_interval_secs: u64,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            tx_per_second: 100,
            block_interval_secs: 60,
            duration_secs: 300,
            num_miners: 10,
            detailed_metrics: true,
            progress_interval_secs: 10,
        }
    }
}

impl LoadTestConfig {
    /// High throughput config (500 TPS)
    pub fn high_throughput() -> Self {
        Self {
            tx_per_second: 500,
            block_interval_secs: 60,
            duration_secs: 600, // 10 minutes
            num_miners: 20,
            detailed_metrics: true,
            progress_interval_secs: 30,
        }
    }
    
    /// Stress test config (1000+ TPS)
    pub fn stress_test() -> Self {
        Self {
            tx_per_second: 1000,
            block_interval_secs: 30,
            duration_secs: 300, // 5 minutes
            num_miners: 50,
            detailed_metrics: true,
            progress_interval_secs: 10,
        }
    }
    
    /// Quick smoke test (10 TPS, 30s)
    pub fn smoke_test() -> Self {
        Self {
            tx_per_second: 10,
            block_interval_secs: 10,
            duration_secs: 30,
            num_miners: 5,
            detailed_metrics: false,
            progress_interval_secs: 5,
        }
    }
}

/// Load test results
pub struct LoadTestResults {
    pub txs_submitted: u64,
    pub txs_accepted: u64,
    pub txs_rejected: u64,
    pub blocks_mined: u64,
    pub blocks_accepted: u64,
    pub blocks_rejected: u64,
    pub duration_secs: u64,
    pub avg_tx_latency_ms: f64,
    pub avg_block_time_ms: f64,
    pub peak_mempool_size: usize,
    pub final_height: u64,
}

impl LoadTestResults {
    pub fn print_summary(&self) {
        let sep = "=".repeat(60);
        println!("\n{}", sep);
        println!("          LOAD TEST RESULTS");
        println!("{}", sep);
        
        println!("\n📊 Transaction Metrics:");
        println!("  Submitted:       {:>10}", self.txs_submitted);
        println!("  Accepted:        {:>10}", self.txs_accepted);
        println!("  Rejected:        {:>10}", self.txs_rejected);
        println!("  Accept Rate:     {:>10.2}%", 
            (self.txs_accepted as f64 / self.txs_submitted as f64) * 100.0);
        
        println!("\n⛏️  Block Metrics:");
        println!("  Mined:           {:>10}", self.blocks_mined);
        println!("  Accepted:        {:>10}", self.blocks_accepted);
        println!("  Rejected:        {:>10}", self.blocks_rejected);
        println!("  Final Height:    {:>10}", self.final_height);
        
        println!("\n⚡ Performance:");
        println!("  Duration:        {:>10}s", self.duration_secs);
        println!("  Actual TPS:      {:>10.2}", 
            self.txs_accepted as f64 / self.duration_secs as f64);
        println!("  Avg TX Latency:  {:>10.2}ms", self.avg_tx_latency_ms);
        println!("  Avg Block Time:  {:>10.2}ms", self.avg_block_time_ms);
        println!("  Peak Mempool:    {:>10}", self.peak_mempool_size);
        
        let sep = "=".repeat(60);
        println!("\n{}\n", sep);
    }
}

/// Counters for tracking test progress
struct LoadTestCounters {
    txs_submitted: AtomicU64,
    txs_accepted: AtomicU64,
    txs_rejected: AtomicU64,
    tx_latency_total_us: AtomicU64,
    tx_latency_samples: AtomicU64,
    blocks_mined: AtomicU64,
    blocks_accepted: AtomicU64,
    blocks_rejected: AtomicU64,
    block_latency_total_us: AtomicU64,
    block_latency_samples: AtomicU64,
    peak_mempool: AtomicU64,
}

impl LoadTestCounters {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            txs_submitted: AtomicU64::new(0),
            txs_accepted: AtomicU64::new(0),
            txs_rejected: AtomicU64::new(0),
            tx_latency_total_us: AtomicU64::new(0),
            tx_latency_samples: AtomicU64::new(0),
            blocks_mined: AtomicU64::new(0),
            blocks_accepted: AtomicU64::new(0),
            blocks_rejected: AtomicU64::new(0),
            block_latency_total_us: AtomicU64::new(0),
            block_latency_samples: AtomicU64::new(0),
            peak_mempool: AtomicU64::new(0),
        })
    }
}

fn avg_ms_from_us(sum_us: u64, samples: u64) -> f64 {
    if samples == 0 {
        return 0.0;
    }
    (sum_us as f64 / samples as f64) / 1000.0
}

/// Run load test with given configuration
pub async fn run_load_test(state: State, config: LoadTestConfig) -> LoadTestResults {
    println!("\n🚀 Starting ZION Load Test");
    println!("  Target TPS: {}", config.tx_per_second);
    println!("  Duration: {}s", config.duration_secs);
    println!("  Block interval: {}s", config.block_interval_secs);
    println!("  Miners: {}\n", config.num_miners);
    
    let start_time = Instant::now();
    let counters = LoadTestCounters::new();
    
    // Spawn progress reporter
    let progress_handle = if config.detailed_metrics {
        let state_progress = state.clone();
        let counters_progress = counters.clone();
        let interval = config.progress_interval_secs;
        
        Some(tokio::spawn(async move {
            loop {
                sleep(Duration::from_secs(interval)).await;
                print_progress(&state_progress, &counters_progress);
            }
        }))
    } else {
        None
    };
    
    // Transaction generator
    let tx_handle = {
        let state_tx = state.clone();
        let counters_tx = counters.clone();
        let tps = config.tx_per_second;
        let duration = config.duration_secs;
        
        tokio::spawn(async move {
            generate_transactions_advanced(state_tx, counters_tx, tps, duration).await;
        })
    };
    
    // Block generators (multiple miners)
    let mut mining_handles = Vec::new();
    for miner_id in 0..config.num_miners {
        let state_mining = state.clone();
        let counters_mining = counters.clone();
        let interval = config.block_interval_secs;
        let duration = config.duration_secs;
        
        let handle = tokio::spawn(async move {
            generate_blocks_advanced(
                state_mining, 
                counters_mining, 
                miner_id, 
                interval, 
                duration
            ).await;
        });
        mining_handles.push(handle);
    }
    
    // Wait for test completion
    let _ = tx_handle.await;
    for handle in mining_handles {
        let _ = handle.await;
    }
    
    // Stop progress reporter
    if let Some(handle) = progress_handle {
        handle.abort();
    }
    
    let duration_secs = start_time.elapsed().as_secs();

    let tx_latency_total_us = counters.tx_latency_total_us.load(Ordering::Relaxed);
    let tx_latency_samples = counters.tx_latency_samples.load(Ordering::Relaxed);
    let block_latency_total_us = counters.block_latency_total_us.load(Ordering::Relaxed);
    let block_latency_samples = counters.block_latency_samples.load(Ordering::Relaxed);
    
    // Collect results
    let results = LoadTestResults {
        txs_submitted: counters.txs_submitted.load(Ordering::Relaxed),
        txs_accepted: counters.txs_accepted.load(Ordering::Relaxed),
        txs_rejected: counters.txs_rejected.load(Ordering::Relaxed),
        blocks_mined: counters.blocks_mined.load(Ordering::Relaxed),
        blocks_accepted: counters.blocks_accepted.load(Ordering::Relaxed),
        blocks_rejected: counters.blocks_rejected.load(Ordering::Relaxed),
        duration_secs,
        avg_tx_latency_ms: avg_ms_from_us(tx_latency_total_us, tx_latency_samples),
        // Represents average block processing latency (submit + validation path) during the load test.
        avg_block_time_ms: avg_ms_from_us(block_latency_total_us, block_latency_samples),
        peak_mempool_size: counters.peak_mempool.load(Ordering::Relaxed) as usize,
        final_height: state.height.load(Ordering::Relaxed),
    };
    
    results.print_summary();
    results
}

/// Generate transactions with realistic patterns
async fn generate_transactions_advanced(
    state: State,
    counters: Arc<LoadTestCounters>,
    tps: u32,
    duration_secs: u64,
) {
    let mut rng = ChaCha8Rng::from_entropy();
    let start = Instant::now();
    let target_txs = (tps as u64) * duration_secs;
    let delay_ms = 1000 / tps as u64;
    
    let mut tx_count = 0u64;
    
    while start.elapsed().as_secs() < duration_secs && tx_count < target_txs {
        // Generate realistic transaction
        let tx = Transaction {
            id: format!("load_test_tx_{}", tx_count),
            version: 1,
            inputs: vec![TxInput {
                prev_tx_hash: format!("{:064x}", rng.gen::<u128>()),
                output_index: rng.gen_range(0..10),
                signature: format!("{:0128x}", rng.gen::<u128>()), // Dummy signature
                public_key: format!("{:064x}", rng.gen::<u64>()), // Dummy pubkey
            }],
            outputs: vec![TxOutput {
                amount: rng.gen_range(1..1000),
                address: format!("ZION{:036x}", rng.gen::<u128>()),
            }],
            fee: rng.gen_range(1..100),
            timestamp: start.elapsed().as_secs(),
        };
        
        counters.txs_submitted.fetch_add(1, Ordering::Relaxed);

        let tx_start = Instant::now();
        match state.process_transaction(tx) {
            Ok(_) => {
                counters.txs_accepted.fetch_add(1, Ordering::Relaxed);
                let latency_us = tx_start.elapsed().as_micros() as u64;
                counters.tx_latency_total_us.fetch_add(latency_us, Ordering::Relaxed);
                counters.tx_latency_samples.fetch_add(1, Ordering::Relaxed);
                let mempool_size = state.mempool.size() as u64;
                let peak = counters.peak_mempool.load(Ordering::Relaxed);
                if mempool_size > peak {
                    counters.peak_mempool.store(mempool_size, Ordering::Relaxed);
                }
            }
            Err(_) => {
                counters.txs_rejected.fetch_add(1, Ordering::Relaxed);
            }
        }
        
        tx_count += 1;
        
        // Rate limiting
        if delay_ms > 0 {
            sleep(Duration::from_millis(delay_ms)).await;
        }
    }
}

/// Generate blocks with concurrent miners
async fn generate_blocks_advanced(
    state: State,
    counters: Arc<LoadTestCounters>,
    _miner_id: u32,
    interval_secs: u64,
    duration_secs: u64,
) {
    let start = Instant::now();
    let mut _block_num = 0u64;
    
    while start.elapsed().as_secs() < duration_secs {
        // Get current tip
        let height = state.height.load(Ordering::Relaxed);
        let prev_hash = { state.tip.lock().unwrap().clone() };
        let difficulty = state.difficulty.load(Ordering::Relaxed);
        
        // Get mempool transactions
        let mempool_txs = state.mempool.get_all();
        let txs_to_include: Vec<Transaction> = mempool_txs.into_iter().take(1000).collect();
        
        // Create block
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let mut block = Block::new(
            1,
            height + 1,
            prev_hash,
            timestamp,
            difficulty,
            0,
            txs_to_include,
        );
        
        // Quick mine (low difficulty for testing)
        let target = crate::blockchain::validation::difficulty_to_target(difficulty);
        for nonce in 0..1_000_000 {
            block.header.nonce = nonce;
            if block.header.meets_target(&target) {
                break;
            }
        }
        
        counters.blocks_mined.fetch_add(1, Ordering::Relaxed);

        // Submit block
        let blk_start = Instant::now();
        match state.process_block(block) {
            Ok(_) => {
                counters.blocks_accepted.fetch_add(1, Ordering::Relaxed);
                let latency_us = blk_start.elapsed().as_micros() as u64;
                counters.block_latency_total_us.fetch_add(latency_us, Ordering::Relaxed);
                counters.block_latency_samples.fetch_add(1, Ordering::Relaxed);
            }
            Err(_) => {
                counters.blocks_rejected.fetch_add(1, Ordering::Relaxed);
            }
        }
        
        _block_num += 1;
        sleep(Duration::from_secs(interval_secs / 10)).await; // Stagger mining
    }
}

/// Print progress during test
fn print_progress(state: &State, counters: &LoadTestCounters) {
    let height = state.height.load(Ordering::Relaxed);
    let mempool = state.mempool.size();
    let txs_sub = counters.txs_submitted.load(Ordering::Relaxed);
    let txs_acc = counters.txs_accepted.load(Ordering::Relaxed);
    let blocks = counters.blocks_accepted.load(Ordering::Relaxed);
    
    println!("[Progress] Height: {} | Mempool: {} | TXs: {}/{} | Blocks: {}",
        height, mempool, txs_acc, txs_sub, blocks);
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_smoke_test_config() {
        let config = LoadTestConfig::smoke_test();
        assert_eq!(config.tx_per_second, 10);
        assert_eq!(config.duration_secs, 30);
    }
    
    #[tokio::test]
    async fn test_high_throughput_config() {
        let config = LoadTestConfig::high_throughput();
        assert_eq!(config.tx_per_second, 500);
        assert!(config.detailed_metrics);
    }

    #[test]
    fn test_avg_ms_from_us() {
        assert_eq!(avg_ms_from_us(0, 0), 0.0);
        assert_eq!(avg_ms_from_us(1_000, 1), 1.0);
        assert_eq!(avg_ms_from_us(2_000, 2), 1.0);
    }
}
