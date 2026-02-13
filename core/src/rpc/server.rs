use axum::{middleware, Router, routing::get, routing::post};
use crate::rpc::methods;
use crate::rpc::auth;
use crate::jsonrpc;
use crate::state::State;
use crate::metrics;

pub fn build(state: State) -> Router {
    // Create metrics subrouter
    let metrics_router = metrics::endpoints::metrics_router(state.metrics.clone());

    // --- Protected routes (require ZION_RPC_TOKEN if set) ---
    let protected = Router::new()
        .route("/rpc/submit_block", post(methods::submit_block).with_state(state.clone()))
        .route("/rpc/submit_tx", post(methods::submit_tx).with_state(state.clone()))
        .route("/jsonrpc", post(jsonrpc::handle).with_state(state.clone()))
        .layer(middleware::from_fn(auth::require_bearer_token));

    // --- Public routes (read-only, no auth) ---
    let public = Router::new()
        .route("/stats", get(methods::stats).with_state(state.clone()))
        .route("/rpc/get_block_template", get(methods::get_block_template).with_state(state.clone()))
        .route("/rpc/get_premine_total", get(methods::get_premine_total))
        .route("/rpc/get_premine_summary", get(methods::get_premine_summary))
        .route("/rpc/get_premine_list", get(methods::get_premine_list))
        .route("/api/block/hash/:hash", get(methods::get_block_by_hash_rest).with_state(state.clone()))
        .route("/api/block/height/:height", get(methods::get_block_by_height_rest).with_state(state.clone()))
        .route("/api/blocks/range/:start/:end", get(methods::get_blocks_range_rest).with_state(state.clone()))
        .route("/api/tx/:txid", get(methods::get_tx_rest).with_state(state.clone()))
        .route("/api/mempool/info", get(methods::get_mempool_info_rest).with_state(state.clone()))
        .route("/api/address/:address/balance", get(methods::get_address_balance_rest).with_state(state.clone()))
        .route("/api/address/:address/utxos", get(methods::get_address_utxos_rest).with_state(state.clone()))
        .route("/api/sync/status", get(methods::get_sync_status_rest));
    
    public
        .merge(protected)
        .merge(metrics_router)
}
