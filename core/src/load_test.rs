/// Load test simulator for ZION blockchain core
/// 
/// Simulates high transaction load and mining activity to stress test:
/// - Mempool capacity and eviction
/// - Block validation throughput
/// - P2P propagation
/// - Storage I/O

use tokio::time::{sleep, Duration};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

use crate::state::State;
use crate::tx::{Transaction, TxInput, TxOutput};
use crate::blockchain::block::Block;

pub struct LoadTestConfig {
    pub tx_per_second: u32,
    pub block_interval_secs: u64,
    pub duration_secs: u64,
    pub num_miners: u32,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            tx_per_second: 100,
            block_interval_secs: 60,
            duration_secs: 300, // 5 minutes
            num_miners: 10,
        }
    }
}

pub async fn run_load_test(state: State, config: LoadTestConfig) {
    println!("[LoadTest] Starting with {} TPS for {}s", 
        config.tx_per_second, config.duration_secs);

    let state_tx = state.clone();
    let state_mining = state.clone();

    // Transaction flood
    let tx_handle = tokio::spawn(async move {
        generate_transactions(state_tx, config.tx_per_second, config.duration_secs).await;
    });

    // Block generation
    let mining_handle = tokio::spawn(async move {
        generate_blocks(state_mining, config.block_interval_secs, config.duration_secs).await;
    });

    // Wait for completion
    let _ = tokio::join!(tx_handle, mining_handle);

    println!("[LoadTest] Completed");
    print_statistics(&state);
}

async fn generate_transactions(state: State, tps: u32, duration_secs: u64) {
    let interval_ms = 1000 / tps as u64;
    let total_txs = tps as u64 * duration_secs;
    let mut rng = ChaCha8Rng::from_entropy();

    for i in 0..total_txs {
        let tx = Transaction {
            id: format!("loadtest_tx_{}", i),
            version: 1,
            inputs: vec![TxInput {
                prev_tx_hash: format!("prev_{}", rng.gen::<u32>()),
                output_index: 0,
                signature: "fake_sig".to_string(),
                public_key: "fake_pubkey".to_string(),
            }],
            outputs: vec![TxOutput {
                amount: rng.gen_range(100..10000),
                address: format!("ZION_loadtest_{}", rng.gen::<u32>()),
            }],
            fee: rng.gen_range(10..100),
            timestamp: i,
        };

        let _ = state.process_transaction(tx);

        if i % 100 == 0 {
            println!("[LoadTest] Generated {} transactions, mempool size: {}", 
                i, state.mempool.size());
        }

        sleep(Duration::from_millis(interval_ms)).await;
    }
}

async fn generate_blocks(state: State, interval_secs: u64, duration_secs: u64) {
    let num_blocks = duration_secs / interval_secs;

    for i in 0..num_blocks {
        sleep(Duration::from_secs(interval_secs)).await;

        let current_height = state.height.load(std::sync::atomic::Ordering::Relaxed);
        let prev_hash = { state.tip.lock().unwrap().clone() };
        let difficulty = state.difficulty.load(std::sync::atomic::Ordering::Relaxed);

        // Get transactions from mempool
        let txs = state.mempool.get_all();
        let block_txs = txs.into_iter().take(1000).collect(); // Max 1000 tx/block

        let block = Block::new(
            1,
            current_height + 1,
            prev_hash,
            i * interval_secs,
            difficulty,
            i, // nonce
            block_txs,
        );

        match state.process_block(block) {
            Ok((h, hash)) => {
                println!("[LoadTest] Mined block {} with hash {}", h, &hash[..16]);
            }
            Err(e) => {
                println!("[LoadTest] Block {} rejected: {}", current_height + 1, e);
            }
        }
    }
}

fn print_statistics(state: &State) {
    let height = state.height.load(std::sync::atomic::Ordering::Relaxed);
    let mempool_size = state.mempool.size();
    
    println!("\n[LoadTest] Final Statistics:");
    println!("  Blockchain height: {}", height);
    println!("  Mempool size: {}", mempool_size);
    println!("  Tip: {}", state.tip.lock().unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_test_smoke() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut path = std::env::temp_dir();
        path.push(format!("zion_load_test_{}", nanos));
        let state = crate::state::Inner::new(&path.to_string_lossy());
        let config = LoadTestConfig {
            tx_per_second: 10,
            block_interval_secs: 5,
            duration_secs: 10,
            num_miners: 1,
        };

        run_load_test(state, config).await;

        let _ = std::fs::remove_dir_all(&path);
    }
}
