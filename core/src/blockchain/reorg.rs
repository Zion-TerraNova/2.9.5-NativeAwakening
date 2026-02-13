/// Minimal chain reorganization utilities
///
/// ZION uses simple best-chain-wins based on cumulative difficulty.
/// If we receive a chain that has more total work than our tip, we reorg.

use crate::blockchain::block::Block;
use crate::storage::ZionStorage;
use anyhow::{anyhow, Result};

/// Computes cumulative difficulty from genesis up to given height.
pub fn cumulative_difficulty(storage: &ZionStorage, height: u64) -> Result<u64> {
    let mut total: u64 = 0;
    for h in 0..=height {
        if let Some(block) = storage.get_block_by_height(h)? {
            total = total.saturating_add(block.difficulty());
        } else {
            break;
        }
    }
    Ok(total)
}

/// Check if a new chain starting at fork_point is stronger than our current tip.
/// Returns true if the new chain should replace the current one.
pub fn is_stronger_chain(
    storage: &ZionStorage,
    fork_point: u64,
    new_chain: &[Block],
) -> Result<bool> {
    if new_chain.is_empty() {
        return Ok(false);
    }

    // Cumulative work on current chain
    let current_tip = storage.get_tip()?.0;
    let current_work = cumulative_difficulty(storage, current_tip)?;

    // Cumulative work up to and INCLUDING fork point (shared ancestor).
    // The fork_point block is the same on both chains, so its work counts
    // toward BOTH chains.  Previously we used `fork_point - 1` which
    // systematically under-counted the new chain by one block's difficulty,
    // causing the node to reject valid reorgs.
    let work_before_fork = cumulative_difficulty(storage, fork_point)?;

    let new_chain_work: u64 = new_chain
        .iter()
        .map(|b| b.difficulty())
        .fold(0u64, |acc, d| acc.saturating_add(d));

    let total_new_work = work_before_fork.saturating_add(new_chain_work);

    let new_tip_height = new_chain.last().map(|b| b.height()).unwrap_or(0);
    let height_advantage = new_tip_height.saturating_sub(current_tip);

    eprintln!("🔀 is_stronger_chain: fork_point={} current_tip={} new_tip={} our_work={} new_work={} (pre_fork={} + chain={}) new_blocks={} height_adv={}",
        fork_point, current_tip, new_tip_height, current_work, total_new_work,
        work_before_fork, new_chain_work, new_chain.len(), height_advantage);

    // Primary: more cumulative work wins
    if total_new_work > current_work {
        return Ok(true);
    }

    // Secondary: equal work but longer chain — prefer more confirmations
    if total_new_work == current_work && new_tip_height > current_tip {
        eprintln!("🔀 is_stronger_chain: equal work but new chain is taller ({} > {}), accepting",
            new_tip_height, current_tip);
        return Ok(true);
    }

    // AUDIT-FIX P0-07: Removed tertiary fork-choice rule that accepted chains
    // with only 90% of current work. This violated Nakamoto consensus (most-work
    // chain wins) and was exploitable for chain-replacement attacks.

    Ok(false)
}

/// Roll back blocks from current tip to fork_point (exclusive).
/// Returns rolled back blocks in reverse order (tip first).
///
/// This implementation now RESTORES UTXOs during rollback.
pub fn rollback_to_height(storage: &ZionStorage, target_height: u64) -> Result<Vec<Block>> {
    let (current_tip, _) = storage.get_tip()?;
    if target_height >= current_tip {
        return Ok(Vec::new());
    }

    let mut rolled_back = Vec::new();
    for h in (target_height + 1..=current_tip).rev() {
        if let Some(block) = storage.get_block_by_height(h)? {
            // Rollback UTXOs before deleting block
            storage.rollback_block_utxos(&block)?;

            rolled_back.push(block);
            storage.delete_block_at_height(h)?;
        }
    }

    Ok(rolled_back)
}

/// Finds common ancestor height between local chain and incoming blocks.
/// Assumes incoming blocks are contiguous and sorted by height.
pub fn find_fork_point(storage: &ZionStorage, incoming: &[Block]) -> Result<u64> {
    if incoming.is_empty() {
        let (tip, _) = storage.get_tip()?;
        return Ok(tip);
    }

    // Walk backwards from first incoming block's prev_hash
    let first = &incoming[0];
    if first.height() == 0 {
        return Ok(0);
    }

    let mut check_hash = first.prev_hash().to_string();
    let mut fork_height = first.height().saturating_sub(1);

    eprintln!("🔍 find_fork_point: first incoming height={}, prev_hash={}",
        first.height(), &check_hash[..16.min(check_hash.len())]);

    loop {
        if let Some(local_block) = storage.get_block_by_height(fork_height)? {
            // CRITICAL: Use Block::calculate_hash() (not hex::encode(header.calculate_hash()))
            // to be consistent with how prev_hash is set in validation.rs process_block.
            let local_hash_hex = local_block.calculate_hash();
            if local_hash_hex == check_hash {
                eprintln!("🔍 find_fork_point: MATCH at height {}", fork_height);
                return Ok(fork_height);
            } else {
                eprintln!("🔍 find_fork_point: h{} MISMATCH local={} vs check={}",
                    fork_height, &local_hash_hex[..16.min(local_hash_hex.len())],
                    &check_hash[..16.min(check_hash.len())]);
                if fork_height == 0 {
                    return Err(anyhow!(
                        "No common ancestor found - genesis mismatch: local_genesis_hash={} vs check_hash={}",
                        local_hash_hex, check_hash
                    ));
                }
                check_hash = local_block.prev_hash().to_string();
                fork_height -= 1;
            }
        } else {
            if fork_height == 0 {
                return Ok(0);
            }
            fork_height -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(prefix: &str) -> std::path::PathBuf {
        let mut p = std::env::temp_dir();
        let uniq = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("{prefix}-{uniq}"));
        p
    }

    #[test]
    fn test_cumulative_difficulty() {
        let dir = temp_path("reorg-cumulative");
        let storage = ZionStorage::open(&dir).unwrap();

        let genesis = Block::genesis();
        storage.save_block(&genesis).unwrap();

        let block1 = Block::new(
            1,
            1,
            genesis.calculate_hash(),
            1704067260,
            2000,
            123,
            vec![],
        );
        storage.save_block(&block1).unwrap();

        let cum = cumulative_difficulty(&storage, 1).unwrap();
        assert_eq!(cum, genesis.difficulty() + 2000);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_is_stronger_chain() {
        let dir = temp_path("reorg-stronger");
        let storage = ZionStorage::open(&dir).unwrap();

        let genesis = Block::genesis();
        storage.save_block(&genesis).unwrap();

        // fork_point=0 means we fork right after genesis.
        // new_chain contains block at height 1 with higher difficulty.
        let new_chain = vec![Block::new(
            1,
            1,
            genesis.calculate_hash(),
            1704067260,
            genesis.difficulty() + 10_000,
            123,
            vec![],
        )];

        // Our chain is just genesis (tip=0).  New chain: genesis + block1.
        // work_before_fork = cumulative(0..=0) = genesis.difficulty()
        // total_new = genesis.difficulty() + (genesis.difficulty()+10000) > genesis.difficulty()
        let stronger = is_stronger_chain(&storage, 0, &new_chain).unwrap();
        assert!(stronger);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
