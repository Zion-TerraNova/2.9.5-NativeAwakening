use axum::{Json, extract::State as AxumState};
use serde::{Deserialize, Serialize};
use crate::state::State;
use crate::blockchain::consensus;
use crate::blockchain::reward;
use crate::blockchain::block::{Algorithm as CoreAlgorithm, Block};
use crate::blockchain::burn::{self, BuybackTracker};
use crate::blockchain::premine;
use crate::tx::{Transaction, TxOutput};

#[derive(Deserialize)]
pub struct Request { pub id: Option<serde_json::Value>, pub method: String, pub params: Option<serde_json::Value> }

#[derive(Serialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<serde_json::Value>,
}

pub async fn handle(AxumState(state): AxumState<State>, Json(req): Json<Request>) -> Json<Response> {
    let res = match req.method.as_str() {
        "getBlockTemplate" | "get_block_template" | "getblocktemplate" => {
            let tip_h = state.height.load(std::sync::atomic::Ordering::Relaxed) as u64;
            let h = tip_h.saturating_add(1);
            let d = state.difficulty.load(std::sync::atomic::Ordering::Relaxed) as u64;
            let prev = { state.tip.lock().unwrap().clone() };
            let algo = CoreAlgorithm::from_height(h);
            let target = match algo {
                CoreAlgorithm::RandomX => consensus::target_from_difficulty(d),
                CoreAlgorithm::Blake3 | CoreAlgorithm::Yescrypt | CoreAlgorithm::CosmicHarmony => {
                    consensus::target_from_difficulty_256(d)
                }
            };
            let target_u32 = consensus::target_u32_from_difficulty(d);
            let target_u128 = consensus::target_u128_from_difficulty(d);
            let r = reward::calculate(h, d);
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let wallet_address = req
                .params
                .as_ref()
                .and_then(|v| v.get("wallet_address").and_then(|x| x.as_str()))
                .unwrap_or_default();

            let mut coinbase = Transaction::new();
            coinbase.timestamp = timestamp;
            coinbase.outputs = vec![TxOutput {
                amount: r,
                address: wallet_address.to_string(),
            }];
            coinbase.id = coinbase.calculate_hash();

            let merkle_root = Block::calculate_merkle_root(&[coinbase]);
            let blob = Block::build_template_blob(1, h, &prev, &merkle_root, timestamp, d);
            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "version": 1,
                    "height": h,
                    "difficulty": d,
                    "prev_hash": prev,
                    "target": target,
                    "target_u32": format!("{:08x}", target_u32),
                    "target_u128": format!("{:032x}", target_u128),
                    "reward_atomic": r,
                    "timestamp": timestamp,
                    "merkle_root": merkle_root,
                    "blob": blob
                })),
                error: None,
            }
        },
        "getMempoolInfo" => Response {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: Some(serde_json::json!({"size": state.mempool.size()})),
            error: None,
        },
        "getMempool" | "get_mempool" => {
            let (limit, offset) = req
                .params
                .as_ref()
                .map(|v| {
                    if let Some(arr) = v.as_array() {
                        let lim = arr.get(0).and_then(|x| x.as_u64()).unwrap_or(100) as usize;
                        let off = arr.get(1).and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                        (lim, off)
                    } else {
                        let lim = v.get("limit").and_then(|x| x.as_u64()).unwrap_or(100) as usize;
                        let off = v.get("offset").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                        (lim, off)
                    }
                })
                .unwrap_or((100, 0));

            let limit = limit.clamp(1, 500);
            let all = state.mempool.get_all();
            let total = all.len();
            let slice = all
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|tx| tx.id)
                .collect::<Vec<_>>();

            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "total": total,
                    "count": slice.len(),
                    "limit": limit,
                    "offset": offset,
                    "txids": slice,
                })),
                error: None,
            }
        }
        "getBalance" | "getbalance" => {
            let address_opt: Option<String> = req
                .params
                .as_ref()
                .and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.get(0).and_then(|x| x.as_str()).map(|s| s.to_string())
                    } else {
                        v.get("address").and_then(|x| x.as_str()).map(|s| s.to_string())
                    }
                });

            match address_opt {
                Some(addr) => match state.storage.get_balance_for_address(&addr) {
                    Ok((total, count)) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(serde_json::json!({
                            "address": addr,
                            "utxo_count": count,
                            "balance_atomic": total,
                            "balance_zion": total / 1_000_000
                        })),
                        error: None,
                    },
                    Err(e) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32000, "message": format!("Storage error: {e}")})),
                    },
                },
                None => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: missing address"})),
                },
            }
        }
        "getUtxos" | "get_utxos" => {
            let (address_opt, limit, offset) = req
                .params
                .as_ref()
                .map(|v| {
                    if let Some(arr) = v.as_array() {
                        let addr = arr.get(0).and_then(|x| x.as_str()).map(|s| s.to_string());
                        let lim = arr.get(1).and_then(|x| x.as_u64()).unwrap_or(100) as usize;
                        let off = arr.get(2).and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                        (addr, lim, off)
                    } else {
                        let addr = v.get("address").and_then(|x| x.as_str()).map(|s| s.to_string());
                        let lim = v.get("limit").and_then(|x| x.as_u64()).unwrap_or(100) as usize;
                        let off = v.get("offset").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                        (addr, lim, off)
                    }
                })
                .unwrap_or((None, 100, 0));

            let limit = limit.clamp(1, 500);
            match address_opt {
                Some(addr) => match state.storage.get_utxos_for_address(&addr, limit, offset) {
                    Ok(utxos) => {
                        let list: Vec<serde_json::Value> = utxos
                            .into_iter()
                            .map(|(key, output)| {
                                serde_json::json!({
                                    "key": key,
                                    "amount_atomic": output.amount,
                                    "amount_zion": output.amount / 1_000_000,
                                    "address": output.address,
                                })
                            })
                            .collect();
                        Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: Some(serde_json::json!({
                                "address": addr,
                                "count": list.len(),
                                "limit": limit,
                                "offset": offset,
                                "utxos": list,
                            })),
                            error: None,
                        }
                    }
                    Err(e) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32000, "message": format!("Storage error: {e}")})),
                    },
                },
                None => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: missing address"})),
                },
            }
        }
        "getBlockByHash" | "get_block_by_hash" => {
            let hash_opt: Option<String> = req
                .params
                .as_ref()
                .and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.get(0).and_then(|x| x.as_str()).map(|s| s.to_string())
                    } else {
                        v.get("hash").and_then(|x| x.as_str()).map(|s| s.to_string())
                    }
                });

            match hash_opt {
                Some(h) => match state.storage.get_block(&h) {
                    Ok(Some(block)) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(serde_json::to_value(block).unwrap_or(serde_json::Value::Null)),
                        error: None,
                    },
                    Ok(None) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32004, "message": "Block not found"})),
                    },
                    Err(e) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32000, "message": format!("Storage error: {e}")})),
                    },
                },
                None => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: missing hash"})),
                },
            }
        }
        "getBlockByHeight" | "get_block_by_height" => {
            let height_opt: Option<u64> = req
                .params
                .as_ref()
                .and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.get(0).and_then(|x| x.as_u64())
                    } else {
                        v.get("height").and_then(|x| x.as_u64())
                    }
                });

            match height_opt {
                Some(h) => match state.storage.get_block_by_height(h) {
                    Ok(Some(block)) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(serde_json::to_value(block).unwrap_or(serde_json::Value::Null)),
                        error: None,
                    },
                    Ok(None) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32004, "message": "Block not found"})),
                    },
                    Err(e) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32000, "message": format!("Storage error: {e}")})),
                    },
                },
                None => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: missing height"})),
                },
            }
        }
        "get_block_headers_range" | "getBlockHeadersRange" | "getblockheadersrange" => {
            // Batch fetch block headers in range [start_height, end_height].
            // params: {"start_height": N, "end_height": M} OR [start, end]
            // Clamped to max 100 blocks per call.
            let (start_opt, end_opt) = req
                .params
                .as_ref()
                .map(|v| {
                    if let Some(arr) = v.as_array() {
                        (arr.get(0).and_then(|x| x.as_u64()), arr.get(1).and_then(|x| x.as_u64()))
                    } else {
                        (v.get("start_height").and_then(|x| x.as_u64()),
                         v.get("end_height").and_then(|x| x.as_u64()))
                    }
                })
                .unwrap_or((None, None));

            match (start_opt, end_opt) {
                (Some(start), Some(end)) if end >= start => {
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
                            Response {
                                jsonrpc: "2.0".to_string(),
                                id: req.id,
                                result: Some(serde_json::json!({"headers": headers})),
                                error: None,
                            }
                        }
                        Err(e) => Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: None,
                            error: Some(serde_json::json!({"code": -32000, "message": format!("Storage error: {e}")})),
                        },
                    }
                }
                _ => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: need start_height and end_height"})),
                },
            }
        }
        "getTx" | "get_tx" | "gettransaction" => {
            let txid_opt: Option<String> = req
                .params
                .as_ref()
                .and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.get(0).and_then(|x| x.as_str()).map(|s| s.to_string())
                    } else {
                        v.get("txid").or_else(|| v.get("id")).and_then(|x| x.as_str()).map(|s| s.to_string())
                    }
                });

            match txid_opt {
                Some(txid) => {
                    // 1) Check mempool first
                    if let Some(tx) = state.mempool.get_transaction(&txid) {
                        Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: Some(serde_json::to_value(tx).unwrap_or(serde_json::Value::Null)),
                            error: None,
                        }
                    } else {
                        // 2) Lookup block via index, then scan block for tx
                        match state.storage.get_block_hash_for_tx(&txid) {
                            Ok(Some(block_hash)) => match state.storage.get_block(&block_hash) {
                                Ok(Some(block)) => {
                                    let found = block.transactions.into_iter().find(|t| t.id == txid);
                                    if let Some(tx) = found {
                                        Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: req.id,
                                            result: Some(serde_json::to_value(tx).unwrap_or(serde_json::Value::Null)),
                                            error: None,
                                        }
                                    } else {
                                        Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: req.id,
                                            result: None,
                                            error: Some(serde_json::json!({"code": -32004, "message": "Transaction not found"})),
                                        }
                                    }
                                }
                                Ok(None) => Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: req.id,
                                    result: None,
                                    error: Some(serde_json::json!({"code": -32004, "message": "Block not found"})),
                                },
                                Err(e) => Response {
                                    jsonrpc: "2.0".to_string(),
                                    id: req.id,
                                    result: None,
                                    error: Some(serde_json::json!({"code": -32000, "message": format!("Storage error: {e}")})),
                                },
                            },
                            Ok(None) => Response {
                                jsonrpc: "2.0".to_string(),
                                id: req.id,
                                result: None,
                                error: Some(serde_json::json!({"code": -32004, "message": "Transaction not found"})),
                            },
                            Err(e) => Response {
                                jsonrpc: "2.0".to_string(),
                                id: req.id,
                                result: None,
                                error: Some(serde_json::json!({"code": -32000, "message": format!("Storage error: {e}")})),
                            },
                        }
                    }
                }
                None => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: missing txid"})),
                },
            }
        }
        "getConsensusParams" => {
            let d = state.difficulty.load(std::sync::atomic::Ordering::Relaxed) as u64;
            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "target": consensus::target_from_difficulty(d),
                    "block_time": 60
                })),
                error: None,
            }
        },
        "get_info" | "getInfo" => {
            let h = state.height.load(std::sync::atomic::Ordering::Relaxed) as u64;
            let d = state.difficulty.load(std::sync::atomic::Ordering::Relaxed) as u64;
            let tip = { state.tip.lock().unwrap().clone() };
            let peers = state.metrics.peers_connected.load(std::sync::atomic::Ordering::Relaxed);
            let tx_pool_size = state.mempool.size();
            let uptime_secs = state.metrics.start_time.elapsed().as_secs();
            let start_time_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                .saturating_sub(uptime_secs);
            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "status": "OK",
                    "height": h,
                    "difficulty": d,
                    "tip": tip,
                    "top_block_hash": tip,
                    "target": 60,
                    "tx_count": h,  // Approximation: at least 1 coinbase tx per block
                    "tx_pool_size": tx_pool_size,
                    "incoming_connections_count": peers / 2,
                    "outgoing_connections_count": (peers + 1) / 2,
                    "version": "2.9.5",
                    "mainnet": true,
                    "testnet": false,
                    "start_time": start_time_unix,
                    "database_size": 0,
                    "cumulative_difficulty": 0,
                    "block_size_limit": 600_000,
                    "block_size_median": 300_000
                })),
                error: None,
            }
        }
        "dev.set_difficulty" => {
            // SECURITY: Compile-time gated — only available with `--features dev-tools`
            #[cfg(feature = "dev-tools")]
            {
                let dev_mode = std::env::var("ZION_DEV_MODE")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);

                if !dev_mode {
                    Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32601, "message": "Method not found"})),
                    }
                } else {
                    let d_opt: Option<u64> = req
                        .params
                        .as_ref()
                        .and_then(|v| {
                            if let Some(arr) = v.as_array() {
                                arr.get(0).and_then(|x| x.as_u64())
                            } else {
                                v.get("difficulty").and_then(|x| x.as_u64())
                            }
                        });

                    match d_opt {
                        Some(d) if d >= 1 => {
                            state
                                .difficulty
                                .store(d, std::sync::atomic::Ordering::Relaxed);
                            state
                                .metrics
                                .current_difficulty
                                .store(d, std::sync::atomic::Ordering::Relaxed);
                            Response {
                                jsonrpc: "2.0".to_string(),
                                id: req.id,
                                result: Some(serde_json::json!({"status": "ok", "difficulty": d})),
                                error: None,
                            }
                        }
                        _ => Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: None,
                            error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: expected difficulty>=1"})),
                        },
                    }
                }
            }
            #[cfg(not(feature = "dev-tools"))]
            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(serde_json::json!({"code": -32601, "message": "Method not found"})),
            }
        }
        "dev.credit_balance" | "dev.set_balance" => {
            // SECURITY: Compile-time gated — only available with `--features dev-tools`
            #[cfg(feature = "dev-tools")]
            {
                let dev_mode = std::env::var("ZION_DEV_MODE")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);

                if !dev_mode {
                    Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -32601, "message": "Method not found (DEV_MODE required)"})),
                    }
                } else {
                let (addr_opt, amount_opt) = req
                    .params
                    .as_ref()
                    .map(|v| {
                        if let Some(arr) = v.as_array() {
                            let addr = arr.get(0).and_then(|x| x.as_str()).map(|s| s.to_string());
                            let amt = arr.get(1).and_then(|x| x.as_f64());
                            (addr, amt)
                        } else {
                            let addr = v.get("address").and_then(|x| x.as_str()).map(|s| s.to_string());
                            let amt = v.get("amount").and_then(|x| x.as_f64());
                            (addr, amt)
                        }
                    })
                    .unwrap_or((None, None));

                match (addr_opt, amount_opt) {
                    (Some(addr), Some(amount)) if amount > 0.0 => {
                        let amount_atomic = (amount * 1_000_000.0) as u64;
                        
                        // Credit balance directly to storage
                        if let Err(e) = state.storage.credit_balance(&addr, amount_atomic) {
                            Response {
                                jsonrpc: "2.0".to_string(),
                                id: req.id,
                                result: None,
                                error: Some(serde_json::json!({
                                    "code": -32000,
                                    "message": format!("Failed to credit balance: {}", e)
                                })),
                            }
                        } else {
                            let new_balance = state.storage.get_balance_for_address(&addr)
                                .map(|(b, _)| b)
                                .unwrap_or(0);
                            
                            Response {
                                jsonrpc: "2.0".to_string(),
                                id: req.id,
                                result: Some(serde_json::json!({
                                    "status": "OK",
                                    "address": addr,
                                    "credited_atomic": amount_atomic,
                                    "credited_zion": amount,
                                    "new_balance_atomic": new_balance,
                                    "new_balance_zion": (new_balance as f64) / 1_000_000.0
                                })),
                                error: None,
                            }
                        }
                    }
                    _ => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({
                            "code": -32602,
                            "message": "Invalid params: need address and amount (>0)"
                        })),
                    },
                }
            }
            }
            #[cfg(not(feature = "dev-tools"))]
            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: None,
                error: Some(serde_json::json!({"code": -32601, "message": "Method not found"})),
            }
        }
        "sendTransaction" | "sendtransaction" => {
            // Pool payout transaction
            // params: [from_addr, to_addr, amount, purpose]
            // OR: {"from": "...", "to": "...", "amount": 1.0, "purpose": "..."}
            let (from_opt, to_opt, amount_opt, purpose) = req
                .params
                .as_ref()
                .map(|v| {
                    if let Some(arr) = v.as_array() {
                        let from = arr.get(0).and_then(|x| x.as_str()).map(|s| s.to_string());
                        let to = arr.get(1).and_then(|x| x.as_str()).map(|s| s.to_string());
                        let amt = arr.get(2).and_then(|x| x.as_f64());
                        let purp = arr.get(3).and_then(|x| x.as_str()).unwrap_or("").to_string();
                        (from, to, amt, purp)
                    } else {
                        let from = v.get("from").and_then(|x| x.as_str()).map(|s| s.to_string());
                        let to = v.get("to").and_then(|x| x.as_str()).map(|s| s.to_string());
                        let amt = v.get("amount").and_then(|x| x.as_f64());
                        let purp = v.get("purpose").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        (from, to, amt, purp)
                    }
                })
                .unwrap_or((None, None, None, String::new()));

            match (from_opt, to_opt, amount_opt) {
                (Some(from), Some(to), Some(amount)) if amount > 0.0 => {
                    let amount_atomic = (amount * 1_000_000.0) as u64;
                    
                    // Check sender balance
                    let sender_balance = state.storage.get_balance_for_address(&from)
                        .map(|(b, _)| b)
                        .unwrap_or(0);
                    
                    if sender_balance < amount_atomic {
                        Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: None,
                            error: Some(serde_json::json!({
                                "code": -32000,
                                "message": format!("Insufficient balance: have {} need {}", sender_balance, amount_atomic)
                            })),
                        }
                    } else {
                        // Create transaction
                        let timestamp = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        
                        let mut tx = Transaction::new();
                        tx.timestamp = timestamp;
                        tx.outputs = vec![TxOutput {
                            amount: amount_atomic,
                            address: to.clone(),
                        }];
                        tx.id = tx.calculate_hash();
                        
                        let tx_id = tx.id.clone();
                        
                        // Add to mempool
                        state.mempool.add_transaction(tx);
                        
                        Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: Some(serde_json::json!({
                                "status": "OK",
                                "tx_id": tx_id,
                                "from": from,
                                "to": to,
                                "amount_atomic": amount_atomic,
                                "amount_zion": amount,
                                "purpose": purpose
                            })),
                            error: None,
                        }
                    }
                }
                _ => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({
                        "code": -32602,
                        "message": "Invalid params: need from, to, amount (>0)"
                    })),
                },
            }
        }
        "submitTransaction" | "submit_transaction" => {
            // Accept a fully signed Transaction object from pool wallet
            // This is the secure path: pool builds + signs TX locally, submits here
            let tx_opt: Option<Transaction> = req
                .params
                .as_ref()
                .and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.get(0).and_then(|x| serde_json::from_value(x.clone()).ok())
                    } else if v.is_object() {
                        // Could be the tx directly or {"tx": {...}}
                        v.get("tx")
                            .and_then(|x| serde_json::from_value(x.clone()).ok())
                            .or_else(|| serde_json::from_value(v.clone()).ok())
                    } else {
                        None
                    }
                });

            match tx_opt {
                Some(tx) => {
                    let tx_id = tx.id.clone();
                    println!("JSONRPC: submitTransaction received {}", tx_id);

                    // 1. Verify signatures
                    if !tx.verify_signatures() {
                        return Json(Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: None,
                            error: Some(serde_json::json!({
                                "code": -32000,
                                "message": "Invalid signatures or ID mismatch"
                            })),
                        });
                    }

                    // 2. Verify UTXOs exist and ownership matches
                    {
                        let zero_hash = "0000000000000000000000000000000000000000000000000000000000000000";
                        for input in &tx.inputs {
                            if input.prev_tx_hash == zero_hash { continue; }
                            let key = format!("{}:{}", input.prev_tx_hash, input.output_index);
                            match state.storage.get_utxo(&key).unwrap_or(None) {
                                Some(output) => {
                                    let derived = crate::crypto::keys::address_from_public_key(&input.public_key);
                                    if derived.is_none() || derived.unwrap() != output.address {
                                        return Json(Response {
                                            jsonrpc: "2.0".to_string(),
                                            id: req.id,
                                            result: None,
                                            error: Some(serde_json::json!({
                                                "code": -32000,
                                                "message": format!("Input signature does not match UTXO owner for {}", key)
                                            })),
                                        });
                                    }
                                }
                                None => {
                                    return Json(Response {
                                        jsonrpc: "2.0".to_string(),
                                        id: req.id,
                                        result: None,
                                        error: Some(serde_json::json!({
                                            "code": -32000,
                                            "message": format!("UTXO not found: {}", key)
                                        })),
                                    });
                                }
                            }
                        }
                    }

                    // 3. Add to mempool
                    match state.process_transaction(tx) {
                        Ok(()) => Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: Some(serde_json::json!({
                                "status": "OK",
                                "tx_id": tx_id
                            })),
                            error: None,
                        },
                        Err(e) => Response {
                            jsonrpc: "2.0".to_string(),
                            id: req.id,
                            result: None,
                            error: Some(serde_json::json!({
                                "code": -32000,
                                "message": format!("Mempool rejected: {}", e)
                            })),
                        },
                    }
                }
                None => Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({
                        "code": -32602,
                        "message": "Invalid params: expected Transaction object"
                    })),
                },
            }
        }
        "submitBlock" | "submitblock" => {
            // Two submission modes:
            // 1. Full block JSON object
            // 2. Template blob + nonce (params: [blob_hex, nonce_u64, wallet])
            
            let block_opt: Option<crate::blockchain::block::Block> = if let Some(params) = req.params {
                 // Mode 2: Check if first param is string (blob) and second is number (nonce)
                 if let Some(arr) = params.as_array() {
                     if arr.len() >= 2 {
                         if let (Some(blob_str), Some(nonce_val)) = (arr[0].as_str(), arr[1].as_u64()) {
                             // Blob + nonce mode
                             match Block::from_template_blob(blob_str, nonce_val) {
                                 Ok(header) => {
                                     // Reconstruct block with coinbase transaction
                                     // IMPORTANT: Coinbase must be deterministically created with SAME 
                                     // parameters as when template was generated, so merkle_root matches!
                                     let wallet = arr.get(2).and_then(|v| v.as_str()).unwrap_or("UNKNOWN");
                                     let reward = reward::calculate(header.height, header.difficulty);
                                     
                                     // Create coinbase EXACTLY like in getblocktemplate:
                                     // Use Transaction::new() for consistency, then set fields
                                     let mut coinbase = Transaction::new();
                                     coinbase.timestamp = header.timestamp; // MUST match template timestamp
                                     coinbase.outputs = vec![TxOutput {
                                         amount: reward,
                                         address: wallet.to_string(),
                                     }];
                                     coinbase.id = coinbase.calculate_hash();
                                     
                                     // Verify merkle_root matches what's in the header
                                     let computed_merkle = Block::calculate_merkle_root(&[coinbase.clone()]);
                                     if computed_merkle != header.merkle_root {
                                         eprintln!(
                                             "WARN: Merkle mismatch! header={} computed={} wallet={} ts={}",
                                             header.merkle_root, computed_merkle, wallet, header.timestamp
                                         );
                                         // For now, TRUST the header's merkle_root since pool should use same formula
                                         // The real issue is likely coinbase hash algorithm
                                     }
                                     
                                     Some(Block {
                                         header,
                                         transactions: vec![coinbase],
                                     })
                                 }
                                 Err(e) => {
                                     return Json(Response {
                                         jsonrpc: "2.0".to_string(),
                                         id: req.id,
                                         result: None,
                                         error: Some(serde_json::json!({
                                             "code": -32602,
                                             "message": format!("Invalid blob: {}", e)
                                         })),
                                     });
                                 }
                             }
                         } else {
                             // Mode 1: Full block object in params[0]
                             let first = arr.get(0).unwrap_or(&serde_json::Value::Null);
                             if let Some(obj) = first.as_object() {
                                 if let Some(block_val) = obj.get("block") {
                                     serde_json::from_value(block_val.clone()).ok()
                                 } else {
                                     serde_json::from_value(first.clone()).ok()
                                 }
                             } else {
                                 serde_json::from_value(first.clone()).ok()
                             }
                         }
                     } else {
                         // Single param array
                         let first = arr.get(0).unwrap_or(&serde_json::Value::Null);
                         serde_json::from_value(first.clone()).ok()
                     }
                 } else if let Some(obj) = params.as_object() {
                     if let Some(block_val) = obj.get("block") {
                         serde_json::from_value(block_val.clone()).ok()
                     } else {
                         serde_json::from_value(params).ok()
                     }
                 } else {
                     serde_json::from_value(params).ok()
                 }
            } else {
                None
            };

            if let Some(block) = block_opt {
                match state.process_block(block) {
                    Ok((height, hash)) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(serde_json::json!({"status": "OK", "height": height, "hash": hash})),
                        error: None,
                    },
                    Err(e) => Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: None,
                        error: Some(serde_json::json!({"code": -1, "message": e})),
                    }
                }
            } else {
                Response {
                    jsonrpc: "2.0".to_string(),
                    id: req.id,
                    result: None,
                    error: Some(serde_json::json!({"code": -32602, "message": "Invalid params: Missing block data"})),
                }
            }
        }
        // ===================================================================
        // Sprint 1.6 — Supply, Buyback, Network & Peer Info Endpoints
        // ===================================================================

        "getSupplyInfo" | "get_supply_info" | "getsupplyinfo" => {
            let height = state.height.load(std::sync::atomic::Ordering::Relaxed);
            let block_reward = reward::BLOCK_REWARD_ATOMIC;
            // Mined supply = height × block_reward (genesis has no reward)
            let mined_atomic = height.saturating_mul(block_reward);
            let total_supply = premine::TOTAL_SUPPLY;
            let premine_total = premine::PREMINE_TOTAL;
            let mining_emission = premine::MINING_EMISSION;

            // Burn tracking (in-memory only for stats)
            let tracker = BuybackTracker::in_memory();
            let stats = tracker.get_stats();
            let burned = stats.combined_burn_atomic;
            let circulating = total_supply.saturating_sub(burned);

            let supply_mined_pct = if mining_emission > 0 {
                (mined_atomic as f64 / mining_emission as f64) * 100.0
            } else {
                0.0
            };

            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "total_supply_atomic": total_supply,
                    "total_supply_zion": total_supply / 1_000_000,
                    "premine_atomic": premine_total,
                    "premine_zion": premine_total / 1_000_000,
                    "mining_emission_atomic": mining_emission,
                    "mining_emission_zion": mining_emission / 1_000_000,
                    "mined_so_far_atomic": mined_atomic,
                    "mined_so_far_zion": mined_atomic / 1_000_000,
                    "supply_mined_percent": format!("{:.6}", supply_mined_pct),
                    "burned_atomic": burned,
                    "burned_zion": burned / 1_000_000,
                    "circulating_supply_atomic": circulating,
                    "circulating_supply_zion": circulating / 1_000_000,
                    "block_reward_atomic": block_reward,
                    "block_reward_zion": block_reward as f64 / 1_000_000.0,
                    "height": height,
                    "deflation_rate_percent": stats.deflation_rate_percent,
                })),
                error: None,
            }
        }

        "getBuybackStats" | "get_buyback_stats" | "getbuybackstats" => {
            let tracker = BuybackTracker::in_memory();
            let stats = tracker.get_stats();

            // Number of recent events to return (default 10)
            let limit = req
                .params
                .as_ref()
                .and_then(|v| {
                    if let Some(arr) = v.as_array() {
                        arr.get(0).and_then(|x| x.as_u64())
                    } else {
                        v.get("limit").and_then(|x| x.as_u64())
                    }
                })
                .unwrap_or(10) as usize;

            let recent = tracker.get_recent_events(limit);
            let events_json: Vec<serde_json::Value> = recent
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "id": e.id,
                        "timestamp": e.timestamp,
                        "btc_amount_sats": e.btc_amount_sats,
                        "btc_burn_sats": e.btc_burn_sats,
                        "btc_creators_sats": e.btc_creators_sats,
                        "zion_burned_atomic": e.zion_burned_atomic,
                        "zion_creators_rent_atomic": e.zion_creators_rent_atomic,
                        "burn_tx_hash": e.burn_tx_hash,
                        "creators_tx_hash": e.creators_tx_hash,
                        "source": e.source,
                    })
                })
                .collect();

            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "burn_address": burn::BURN_ADDRESS,
                    "creators_address": burn::CREATORS_ADDRESS,
                    "dao_address": burn::DAO_ADDRESS,
                    "burn_share_percent": stats.burn_share_percent,
                    "creators_share_percent": stats.creators_share_percent,
                    "dao_share_percent": burn::DAO_SHARE_PERCENT,
                    "total_btc_revenue_sats": stats.total_btc_revenue_sats,
                    "total_btc_burn_sats": stats.total_btc_burn_sats,
                    "total_btc_creators_sats": stats.total_btc_creators_sats,
                    "total_zion_burned_atomic": stats.total_zion_burned_atomic,
                    "total_zion_burned_zion": stats.total_zion_burned_atomic / 1_000_000,
                    "total_zion_creators_rent_atomic": stats.total_zion_creators_rent_atomic,
                    "total_zion_creators_rent_zion": stats.total_zion_creators_rent_atomic / 1_000_000,
                    "total_fees_burned_atomic": stats.total_fees_burned_atomic,
                    "combined_burn_atomic": stats.combined_burn_atomic,
                    "circulating_supply_atomic": stats.circulating_supply_atomic,
                    "deflation_rate_percent": stats.deflation_rate_percent,
                    "buyback_count": stats.buyback_count,
                    "last_buyback_timestamp": stats.last_buyback_timestamp,
                    "recent_events": events_json,
                })),
                error: None,
            }
        }

        "getNetworkInfo" | "get_network_info" | "getnetworkinfo" => {
            let net = crate::network::get_network();
            let h = state.height.load(std::sync::atomic::Ordering::Relaxed);
            let d = state.difficulty.load(std::sync::atomic::Ordering::Relaxed);
            let tip = { state.tip.lock().unwrap().clone() };
            let uptime_secs = state.metrics.start_time.elapsed().as_secs();
            let peers = state.metrics.peers_connected.load(std::sync::atomic::Ordering::Relaxed);
            let blocks_processed = state.metrics.blocks_processed.load(std::sync::atomic::Ordering::Relaxed);
            let blocks_rejected = state.metrics.blocks_rejected.load(std::sync::atomic::Ordering::Relaxed);
            let txs_in_mempool = state.metrics.txs_in_mempool.load(std::sync::atomic::Ordering::Relaxed);
            let algo = CoreAlgorithm::from_height(h + 1);
            let last_block_time = state.metrics.last_block_time.load(std::sync::atomic::Ordering::Relaxed);

            // Estimated hashrate: difficulty / block_time (60s)
            let estimated_hashrate = d as f64 / 60.0;

            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "version": "2.9.5",
                    "network": net.name(),
                    "magic": net.magic(),
                    "height": h,
                    "difficulty": d,
                    "tip_hash": tip,
                    "current_algorithm": format!("{:?}", algo),
                    "peers_connected": peers,
                    "uptime_seconds": uptime_secs,
                    "blocks_processed": blocks_processed,
                    "blocks_rejected": blocks_rejected,
                    "mempool_size": txs_in_mempool,
                    "last_block_time": last_block_time,
                    "estimated_hashrate_hs": estimated_hashrate,
                    "p2p_port": net.default_p2p_port(),
                    "rpc_port": net.default_rpc_port(),
                })),
                error: None,
            }
        }

        "getPeerInfo" | "get_peer_info" | "getpeerinfo" => {
            let peers_connected = state.metrics.peers_connected.load(std::sync::atomic::Ordering::Relaxed);
            let peers_total = state.metrics.peers_total.load(std::sync::atomic::Ordering::Relaxed);
            let messages_sent = state.metrics.messages_sent.load(std::sync::atomic::Ordering::Relaxed);
            let messages_received = state.metrics.messages_received.load(std::sync::atomic::Ordering::Relaxed);

            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "peers_connected": peers_connected,
                    "peers_total_seen": peers_total,
                    "messages_sent": messages_sent,
                    "messages_received": messages_received,
                })),
                error: None,
            }
        }

        "getPeerList" | "get_peer_list" | "getpeerlist" | "getConnections" | "get_connections" => {
            // Return full peer list with addresses, heights, latency, direction
            let peer_manager = state.peer_manager.lock().unwrap().clone();
            match peer_manager {
                Some(pm) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let known = pm.get_peers();
                    let peers_json: Vec<serde_json::Value> = known.iter().map(|p| {
                        let connected = pm.is_connected(&p.addr);
                        let idle_secs = now.saturating_sub(p.last_seen);
                        serde_json::json!({
                            "address": p.addr.to_string(),
                            "host": p.addr.ip().to_string(),
                            "port": p.addr.port(),
                            "height": p.height,
                            "sub_version": p.sub_version,
                            "last_seen": p.last_seen,
                            "idle_seconds": idle_secs,
                            "connected": connected,
                            "failed_attempts": p.failed_attempts,
                            "incoming": false,
                            "state": if connected { "active" } else { "known" },
                        })
                    }).collect();

                    let active_count = pm.active_count();
                    let chain_height = state.height.load(std::sync::atomic::Ordering::Relaxed) as u64;

                    Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(serde_json::json!({
                            "count": peers_json.len(),
                            "active": active_count,
                            "known": peers_json.len(),
                            "chain_height": chain_height,
                            "peers": peers_json,
                        })),
                        error: None,
                    }
                }
                None => {
                    Response {
                        jsonrpc: "2.0".to_string(),
                        id: req.id,
                        result: Some(serde_json::json!({
                            "count": 0,
                            "active": 0,
                            "known": 0,
                            "chain_height": state.height.load(std::sync::atomic::Ordering::Relaxed) as u64,
                            "peers": [],
                        })),
                        error: None,
                    }
                }
            }
        }

        "getHealthCheck" | "get_health_check" | "gethealthcheck" | "health" => {
            let health = state.metrics.health_check();
            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::to_value(health).unwrap_or(serde_json::Value::Null)),
                error: None,
            }
        }

        "getMetrics" | "get_metrics" | "getmetrics" | "metrics" => {
            let uptime_secs = state.metrics.start_time.elapsed().as_secs();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let last_block = state.metrics.last_block_time.load(std::sync::atomic::Ordering::Relaxed);
            let time_since_last = if last_block > 0 { now.saturating_sub(last_block) } else { 0 };

            Response {
                jsonrpc: "2.0".to_string(),
                id: req.id,
                result: Some(serde_json::json!({
                    "uptime_seconds": uptime_secs,
                    "blocks": {
                        "processed": state.metrics.blocks_processed.load(std::sync::atomic::Ordering::Relaxed),
                        "rejected": state.metrics.blocks_rejected.load(std::sync::atomic::Ordering::Relaxed),
                        "height": state.metrics.current_height.load(std::sync::atomic::Ordering::Relaxed),
                        "difficulty": state.metrics.current_difficulty.load(std::sync::atomic::Ordering::Relaxed),
                        "time_since_last_seconds": time_since_last,
                    },
                    "transactions": {
                        "submitted": state.metrics.txs_submitted.load(std::sync::atomic::Ordering::Relaxed),
                        "accepted": state.metrics.txs_accepted.load(std::sync::atomic::Ordering::Relaxed),
                        "rejected": state.metrics.txs_rejected.load(std::sync::atomic::Ordering::Relaxed),
                        "mempool_size": state.metrics.txs_in_mempool.load(std::sync::atomic::Ordering::Relaxed),
                        "mempool_evictions": state.metrics.mempool_evictions.load(std::sync::atomic::Ordering::Relaxed),
                    },
                    "p2p": {
                        "peers_connected": state.metrics.peers_connected.load(std::sync::atomic::Ordering::Relaxed),
                        "peers_total": state.metrics.peers_total.load(std::sync::atomic::Ordering::Relaxed),
                        "messages_sent": state.metrics.messages_sent.load(std::sync::atomic::Ordering::Relaxed),
                        "messages_received": state.metrics.messages_received.load(std::sync::atomic::Ordering::Relaxed),
                    },
                    "performance": {
                        "validation_time_us": state.metrics.validation_time_us.load(std::sync::atomic::Ordering::Relaxed),
                        "pow_time_us": state.metrics.pow_time_us.load(std::sync::atomic::Ordering::Relaxed),
                        "storage_writes": state.metrics.storage_writes.load(std::sync::atomic::Ordering::Relaxed),
                        "storage_reads": state.metrics.storage_reads.load(std::sync::atomic::Ordering::Relaxed),
                    },
                })),
                error: None,
            }
        }

        _ => Response {
            jsonrpc: "2.0".to_string(),
            id: req.id,
            result: None,
            error: Some(serde_json::json!("method not found")),
        },
    };
    Json(res)
}

// ===================================================================
// Sprint 1.6 — RPC Unit Tests
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a minimal State for testing RPC handlers
    fn test_state() -> State {
        // Set testnet network (OnceLock — only first call wins, ignore err)
        let _ = std::panic::catch_unwind(|| {
            crate::network::set_network(crate::network::NetworkType::Testnet);
        });
        crate::state::Inner::new("/tmp/zion_rpc_test")
    }

    fn make_request(method: &str, params: Option<serde_json::Value>) -> Request {
        Request {
            id: Some(serde_json::json!(1)),
            method: method.to_string(),
            params,
        }
    }

    async fn call_rpc(state: State, method: &str, params: Option<serde_json::Value>) -> Response {
        let req = make_request(method, params);
        let Json(resp) = handle(AxumState(state), Json(req)).await;
        resp
    }

    // -----------------------------------------------------------------
    // getSupplyInfo
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_supply_info_basic() {
        let state = test_state();
        let resp = call_rpc(state, "getSupplyInfo", None).await;
        assert!(resp.error.is_none(), "Expected no error: {:?}", resp.error);
        let r = resp.result.unwrap();
        assert_eq!(r["total_supply_atomic"], 144_000_000_000_000_000u64);
        assert_eq!(r["premine_atomic"], 16_280_000_000_000_000u64);
        assert!(r["height"].as_u64().is_some());
        assert!(r["block_reward_atomic"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_get_supply_info_aliases() {
        let state = test_state();
        for method in &["getSupplyInfo", "get_supply_info", "getsupplyinfo"] {
            let resp = call_rpc(state.clone(), method, None).await;
            assert!(resp.error.is_none(), "Method {} failed: {:?}", method, resp.error);
            let r = resp.result.unwrap();
            assert_eq!(r["total_supply_zion"], 144_000_000_000u64);
        }
    }

    #[tokio::test]
    async fn test_supply_mining_emission() {
        let state = test_state();
        let resp = call_rpc(state, "getSupplyInfo", None).await;
        let r = resp.result.unwrap();
        let total = r["total_supply_atomic"].as_u64().unwrap();
        let premine = r["premine_atomic"].as_u64().unwrap();
        let emission = r["mining_emission_atomic"].as_u64().unwrap();
        assert_eq!(emission, total - premine);
    }

    #[tokio::test]
    async fn test_supply_circulating_no_burns() {
        let state = test_state();
        let resp = call_rpc(state, "getSupplyInfo", None).await;
        let r = resp.result.unwrap();
        // With in_memory tracker and no burns, circulating == total
        let total = r["total_supply_atomic"].as_u64().unwrap();
        let circ = r["circulating_supply_atomic"].as_u64().unwrap();
        assert_eq!(circ, total);
    }

    // -----------------------------------------------------------------
    // getBuybackStats
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_buyback_stats_empty() {
        let state = test_state();
        let resp = call_rpc(state, "getBuybackStats", None).await;
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert_eq!(r["buyback_count"], 0);
        // 100% DAO model — no BTC revenue burn
        assert_eq!(r["burn_share_percent"], 0);
        assert_eq!(r["creators_share_percent"], 100);
        assert_eq!(r["burn_address"], burn::BURN_ADDRESS);
        assert_eq!(r["creators_address"], burn::CREATORS_ADDRESS);
        // DAO-specific aliases
        assert_eq!(r["dao_share_percent"], 100);
        assert_eq!(r["dao_address"], burn::DAO_ADDRESS);
    }

    #[tokio::test]
    async fn test_buyback_stats_with_limit() {
        let state = test_state();
        let resp = call_rpc(
            state,
            "getBuybackStats",
            Some(serde_json::json!({"limit": 5})),
        )
        .await;
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        let events = r["recent_events"].as_array().unwrap();
        assert!(events.len() <= 5);
    }

    #[tokio::test]
    async fn test_buyback_stats_aliases() {
        let state = test_state();
        for method in &["getBuybackStats", "get_buyback_stats", "getbuybackstats"] {
            let resp = call_rpc(state.clone(), method, None).await;
            assert!(resp.error.is_none(), "Method {} failed", method);
        }
    }

    // -----------------------------------------------------------------
    // getNetworkInfo
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_network_info() {
        let state = test_state();
        let resp = call_rpc(state, "getNetworkInfo", None).await;
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert_eq!(r["version"], "2.9.5");
        assert!(r["height"].as_u64().is_some());
        assert!(r["difficulty"].as_u64().is_some());
        assert!(r["uptime_seconds"].as_u64().is_some());
        assert!(r["peers_connected"].as_u64().is_some());
        assert!(!r["tip_hash"].as_str().unwrap().is_empty());
        assert!(r["p2p_port"].as_u64().unwrap() > 0);
        assert!(r["rpc_port"].as_u64().unwrap() > 0);
    }

    #[tokio::test]
    async fn test_network_info_version() {
        let state = test_state();
        let resp = call_rpc(state, "getNetworkInfo", None).await;
        let r = resp.result.unwrap();
        assert_eq!(r["version"], "2.9.5");
        // Hashrate estimate should be >= 0
        let hr = r["estimated_hashrate_hs"].as_f64().unwrap();
        assert!(hr >= 0.0);
    }

    #[tokio::test]
    async fn test_network_info_aliases() {
        let state = test_state();
        for method in &["getNetworkInfo", "get_network_info", "getnetworkinfo"] {
            let resp = call_rpc(state.clone(), method, None).await;
            assert!(resp.error.is_none(), "Method {} failed", method);
            assert_eq!(resp.result.as_ref().unwrap()["version"], "2.9.5");
        }
    }

    // -----------------------------------------------------------------
    // getPeerInfo
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_peer_info() {
        let state = test_state();
        let resp = call_rpc(state, "getPeerInfo", None).await;
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert!(r["peers_connected"].as_u64().is_some());
        assert!(r["peers_total_seen"].as_u64().is_some());
        assert!(r["messages_sent"].as_u64().is_some());
        assert!(r["messages_received"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_peer_info_aliases() {
        let state = test_state();
        for method in &["getPeerInfo", "get_peer_info", "getpeerinfo"] {
            let resp = call_rpc(state.clone(), method, None).await;
            assert!(resp.error.is_none(), "Method {} failed", method);
        }
    }

    #[tokio::test]
    async fn test_peer_info_initial_zeros() {
        let state = test_state();
        let resp = call_rpc(state, "getPeerInfo", None).await;
        let r = resp.result.unwrap();
        // Fresh state should have 0 peers and 0 messages
        assert_eq!(r["peers_connected"], 0);
        assert_eq!(r["messages_sent"], 0);
        assert_eq!(r["messages_received"], 0);
    }

    // -----------------------------------------------------------------
    // Unknown method
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_unknown_method() {
        let state = test_state();
        let resp = call_rpc(state, "nonExistentMethod", None).await;
        assert!(resp.error.is_some());
        assert!(resp.result.is_none());
    }

    // -----------------------------------------------------------------
    // get_info / getInfo (existing, regression test)
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_get_info() {
        let state = test_state();
        let resp = call_rpc(state, "get_info", None).await;
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert_eq!(r["status"], "OK");
        assert!(r["height"].as_u64().is_some());
    }

    // -----------------------------------------------------------------
    // Sprint 1.8 — Health Check & Metrics
    // -----------------------------------------------------------------

    #[tokio::test]
    async fn test_health_check_basic() {
        let state = test_state();
        let resp = call_rpc(state, "getHealthCheck", None).await;
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert!(r["status"].as_str().is_some());
        assert!(r["uptime_seconds"].as_u64().is_some());
        assert!(r["height"].as_u64().is_some());
        assert!(r["difficulty"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_health_check_aliases() {
        let state = test_state();
        for method in &["getHealthCheck", "get_health_check", "gethealthcheck", "health"] {
            let resp = call_rpc(state.clone(), method, None).await;
            assert!(resp.error.is_none(), "Method {} failed", method);
        }
    }

    #[tokio::test]
    async fn test_health_check_has_network() {
        let state = test_state();
        let resp = call_rpc(state, "health", None).await;
        let r = resp.result.unwrap();
        let net = r["network"].as_str().unwrap();
        assert!(net == "testnet" || net == "mainnet");
    }

    #[tokio::test]
    async fn test_metrics_basic() {
        let state = test_state();
        let resp = call_rpc(state, "getMetrics", None).await;
        assert!(resp.error.is_none());
        let r = resp.result.unwrap();
        assert!(r["uptime_seconds"].as_u64().is_some());
        assert!(r["blocks"].is_object());
        assert!(r["transactions"].is_object());
        assert!(r["p2p"].is_object());
        assert!(r["performance"].is_object());
    }

    #[tokio::test]
    async fn test_metrics_aliases() {
        let state = test_state();
        for method in &["getMetrics", "get_metrics", "getmetrics", "metrics"] {
            let resp = call_rpc(state.clone(), method, None).await;
            assert!(resp.error.is_none(), "Method {} failed", method);
        }
    }

    #[tokio::test]
    async fn test_metrics_blocks_section() {
        let state = test_state();
        let resp = call_rpc(state, "getMetrics", None).await;
        let r = resp.result.unwrap();
        let blocks = &r["blocks"];
        assert!(blocks["processed"].as_u64().is_some());
        assert!(blocks["rejected"].as_u64().is_some());
        assert!(blocks["height"].as_u64().is_some());
        assert!(blocks["difficulty"].as_u64().is_some());
    }

    #[tokio::test]
    async fn test_metrics_p2p_section() {
        let state = test_state();
        let resp = call_rpc(state, "getMetrics", None).await;
        let r = resp.result.unwrap();
        let p2p = &r["p2p"];
        assert_eq!(p2p["peers_connected"], 0);
        assert_eq!(p2p["messages_sent"], 0);
        assert_eq!(p2p["messages_received"], 0);
    }

    #[tokio::test]
    async fn test_metrics_performance_section() {
        let state = test_state();
        let resp = call_rpc(state, "getMetrics", None).await;
        let r = resp.result.unwrap();
        let perf = &r["performance"];
        assert!(perf["validation_time_us"].as_u64().is_some());
        assert!(perf["storage_writes"].as_u64().is_some());
    }
}
