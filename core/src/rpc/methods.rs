use axum::{Json, extract::State as AxumState};
use axum::extract::{Path, Query};
use serde::{Deserialize, Serialize};
use crate::state::State;
use crate::blockchain::reward;
use crate::blockchain::consensus;
use crate::premine;
use crate::crypto::keys;
use crate::tx::Transaction;
use crate::blockchain::block::Block;

#[derive(Serialize)]
pub struct Template {
    pub version: u32,
    pub height: u64,
    pub difficulty: u64,
    pub prev_hash: String,
    pub target: String,
    pub reward_atomic: u64,
    pub timestamp: u64,
    pub blob: String,
}

#[derive(Deserialize)]
pub struct Submit { pub data: String } // Deprecated/Unused if we switch to Json<Block>

pub async fn health() -> &'static str { "ok" }

pub async fn stats(AxumState(state): AxumState<State>) -> Json<serde_json::Value> {
    let h = state.height.load(std::sync::atomic::Ordering::Relaxed);
    let d = state.difficulty.load(std::sync::atomic::Ordering::Relaxed);
    let tip = { state.tip.lock().unwrap().clone() };
    let health = state.metrics.health_check();
    let sync_snap = crate::p2p::get_sync_status().to_json();
    Json(serde_json::json!({
        "tps": 0,
        "network": health.network,
        "height": h,
        "difficulty": d,
        "tip": tip,
        "peers_connected": health.peers_connected,
        "mempool_size": health.mempool_size,
        "time_since_last_block": health.time_since_last_block,
        "status": health.status,
        "sync": sync_snap,
    }))
}

pub async fn get_block_template(AxumState(state): AxumState<State>) -> Json<Template> {
    let tip_h = state.height.load(std::sync::atomic::Ordering::Relaxed) as u64;
    let h = tip_h.saturating_add(1);
    let d = state.difficulty.load(std::sync::atomic::Ordering::Relaxed) as u64;
    let prev = { state.tip.lock().unwrap().clone() };
    // Use 256-bit target for proper mining
    let target = consensus::target_from_difficulty_256(d);
    let reward_atomic = reward::calculate(h, d);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    // This endpoint does not include a wallet address; keep a deterministic placeholder
    // so callers can still round-trip submitBlock(blob, nonce, wallet).
    // In practice, the pool uses the JSON-RPC getBlockTemplate path.
    let merkle_root = crate::blockchain::block::Block::calculate_merkle_root(&[]);
    let blob = Block::build_template_blob(1, h, &prev, &merkle_root, timestamp, d);
    Json(Template {
        version: 1,
        height: h,
        difficulty: d,
        prev_hash: prev,
        target,
        reward_atomic,
        timestamp,
        blob,
    })
}

pub async fn get_premine_total() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "total_zion": premine::PREMINE_TOTAL / 1_000_000,
        "total_atomic": premine::PREMINE_TOTAL,
        "total_supply": premine::TOTAL_SUPPLY / 1_000_000
    }))
}

pub async fn get_premine_summary() -> Json<serde_json::Value> {
    let all = premine::get_all_premine_addresses();
    let by_category = all.iter().fold(std::collections::HashMap::new(), |mut map, addr| {
        let cat = addr.category.clone();
        let entry = map.entry(cat).or_insert((0u64, 0usize));
        entry.0 += addr.amount;
        entry.1 += 1;
        map
    });
    let summary: Vec<_> = by_category.iter().map(|(cat, (amt, cnt))| 
        serde_json::json!({
            "category": cat,
            "count": cnt,
            "total_atomic": amt,
            "total_zion": amt / 1_000_000
        })
    ).collect();
    Json(serde_json::json!({"categories": summary}))
}

pub async fn submit_tx(AxumState(state): AxumState<State>, Json(tx): Json<Transaction>) -> Json<serde_json::Value> {
    let tx_id = tx.id.clone();
    println!("RPC: submit_tx received {}", tx_id);
    
    // 1. Verify Signatures (Stateless)
    if !tx.verify_signatures() {
        return Json(serde_json::json!({"status": "error", "message": "Invalid signatures or ID mismatch"}));
    }

    // 2. Verify UTXOs (Context uses Storage)
    {
        for input in &tx.inputs {
            // Check for Coinbase-like inputs?
            let zero_hash = "0000000000000000000000000000000000000000000000000000000000000000";
            if input.prev_tx_hash == zero_hash { continue; }

            let key = format!("{}:{}", input.prev_tx_hash, input.output_index);
            // Storage access (Blocking I/O)
            match state.storage.get_utxo(&key).unwrap_or(None) {
                Some(output) => {
                    // Check ownership
                    let derived = keys::address_from_public_key(&input.public_key);
                    if derived.is_none() || derived.unwrap() != output.address {
                         return Json(serde_json::json!({"status": "error", "message": "Input signature does not match UTXO owner"}));
                    }
                },
                None => {
                     return Json(serde_json::json!({"status": "error", "message": format!("UTXO not found: {}", key)}));
                }
            }
        }
    }

    // 3. Add to mempool via unified pipeline (metrics + broadcast)
    match state.process_transaction(tx) {
        Ok(()) => Json(serde_json::json!({"status": "ok", "tx_id": tx_id})),
        Err(e) => Json(serde_json::json!({"status": "error", "message": e})),
    }
}

pub async fn submit_block(AxumState(state): AxumState<State>, Json(block): Json<Block>) -> Json<serde_json::Value> {
    println!("RPC: submit_block received height={} hash={} txs={}", block.height(), block.calculate_hash(), block.transactions.len());

    match state.process_block(block) {
        Ok((height, hash)) => Json(serde_json::json!({"status": "ok", "height": height, "hash": hash})),
        Err(e) => Json(serde_json::json!({"status": "error", "message": e}))
    }
}

pub async fn get_premine_list() -> Json<Vec<serde_json::Value>> {
    let all = premine::get_all_premine_addresses();
    let list = all.iter().map(|addr| {
        serde_json::json!({
            "address": addr.address,
            "purpose": addr.purpose,
            "amount_atomic": addr.amount,
            "amount_zion": addr.amount / 1_000_000,
            "category": addr.category,
            "unlock_height": addr.unlock_height,
        })
    }).collect();
    Json(list)
}

// --- REST-style API handlers ---

pub async fn get_block_by_hash_rest(
    AxumState(state): AxumState<State>,
    Path(hash): Path<String>,
) -> Json<serde_json::Value> {
    match state.storage.get_block(&hash) {
        Ok(Some(block)) => Json(serde_json::json!({
            "status": "ok",
            "block": block
        })),
        Ok(None) => Json(serde_json::json!({
            "status": "error",
            "message": "Block not found"
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Storage error: {}", e)
        })),
    }
}

pub async fn get_block_by_height_rest(
    AxumState(state): AxumState<State>,
    Path(height): Path<u64>,
) -> Json<serde_json::Value> {
    match state.storage.get_block_by_height(height) {
        Ok(Some(block)) => Json(serde_json::json!({
            "status": "ok",
            "block": block
        })),
        Ok(None) => Json(serde_json::json!({
            "status": "error",
            "message": "Block not found"
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Storage error: {}", e)
        })),
    }
}

/// GET /api/blocks/range/:start/:end — batch block headers in one read txn
pub async fn get_blocks_range_rest(
    AxumState(state): AxumState<State>,
    Path((start, end)): Path<(u64, u64)>,
) -> Json<serde_json::Value> {
    match state.storage.get_blocks_in_range(start, end) {
        Ok(blocks) => {
            let headers: Vec<serde_json::Value> = blocks.iter().map(|b| {
                serde_json::json!({
                    "height": b.height(),
                    "hash": b.calculate_hash(),
                    "prev_hash": b.header.prev_hash,
                    "timestamp": b.header.timestamp,
                    "difficulty": b.header.difficulty,
                    "nonce": b.header.nonce,
                    "version": b.header.version,
                    "num_txes": b.transactions.len(),
                    "reward": b.transactions.first()
                        .and_then(|tx| tx.outputs.first())
                        .map(|o| o.amount)
                        .unwrap_or(0),
                })
            }).collect();
            Json(serde_json::json!({
                "status": "ok",
                "count": headers.len(),
                "headers": headers
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Storage error: {}", e)
        })),
    }
}

pub async fn get_tx_rest(
    AxumState(state): AxumState<State>,
    Path(txid): Path<String>,
) -> Json<serde_json::Value> {
    // Check mempool first
    if let Some(tx) = state.mempool.get_transaction(&txid) {
        return Json(serde_json::json!({
            "status": "ok",
            "tx": tx,
            "in_mempool": true
        }));
    }

    // Check storage via tx->block index
    match state.storage.get_block_hash_for_tx(&txid) {
        Ok(Some(block_hash)) => match state.storage.get_block(&block_hash) {
            Ok(Some(block)) => {
                let tx = block.transactions.into_iter().find(|t| t.id == txid);
                match tx {
                    Some(found) => Json(serde_json::json!({
                        "status": "ok",
                        "tx": found,
                        "in_mempool": false,
                        "block_hash": block_hash
                    })),
                    None => Json(serde_json::json!({
                        "status": "error",
                        "message": "Transaction not found in block"
                    })),
                }
            }
            Ok(None) => Json(serde_json::json!({
                "status": "error",
                "message": "Block not found"
            })),
            Err(e) => Json(serde_json::json!({
                "status": "error",
                "message": format!("Storage error: {}", e)
            })),
        },
        Ok(None) => Json(serde_json::json!({
            "status": "error",
            "message": "Transaction not found"
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Storage error: {}", e)
        })),
    }
}

pub async fn get_mempool_info_rest(AxumState(state): AxumState<State>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "size": state.mempool.size(),
        "transactions": state.mempool.get_all().iter().map(|tx| &tx.id).collect::<Vec<_>>()
    }))
}

#[derive(Deserialize)]
pub struct PaginationParams {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

pub async fn get_address_balance_rest(
    AxumState(state): AxumState<State>,
    Path(address): Path<String>,
) -> Json<serde_json::Value> {
    match state.storage.get_balance_for_address(&address) {
        Ok((total, count)) => Json(serde_json::json!({
            "status": "ok",
            "address": address,
            "utxo_count": count,
            "balance_atomic": total,
            "balance_zion": total / 1_000_000,
        })),
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Storage error: {}", e)
        })),
    }
}

pub async fn get_address_utxos_rest(
    AxumState(state): AxumState<State>,
    Path(address): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(100).clamp(1, 500);
    let offset = params.offset.unwrap_or(0);

    match state.storage.get_utxos_for_address(&address, limit, offset) {
        Ok(utxos) => {
            let list: Vec<serde_json::Value> = utxos
                .into_iter()
                .map(|(key, output)| {
                    serde_json::json!({
                        "key": key,
                        "amount": output.amount,
                        "amount_atomic": output.amount,
                        "amount_zion": output.amount / 1_000_000,
                        "address": output.address,
                    })
                })
                .collect();
            Json(serde_json::json!({
                "status": "ok",
                "address": address,
                "count": list.len(),
                "limit": limit,
                "offset": offset,
                "utxos": list,
            }))
        }
        Err(e) => Json(serde_json::json!({
            "status": "error",
            "message": format!("Storage error: {}", e)
        })),
    }
}

/// GET /api/sync/status — IBD / sync progress (no State needed, reads global)
pub async fn get_sync_status_rest() -> Json<serde_json::Value> {
    let snap = crate::p2p::get_sync_status().to_json();
    Json(serde_json::json!({
        "status": "ok",
        "sync": snap,
    }))
}
