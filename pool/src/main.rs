// ZION Pool v2.9.5 — L1 MainNet with CH v3 Revenue Orchestration
//
// Core: Stratum, PPLNS, Payout, Block Templates
// Revenue: revenue_proxy, profit_switcher, buyback, stream_scheduler, pool_external_miner

use zion_pool::stratum;
use zion_pool::config::Config;
use axum::{extract::{Path, Query, State}, http::{header, StatusCode}, response::IntoResponse, routing::get, Json};
use serde::Deserialize;
use serde_json::json;
use zion_pool::payout;
use std::sync::Arc;
use zion_pool::session::SessionManager;
use zion_pool::shares::{RedisStorage, ShareProcessor, ShareValidator};
use zion_pool::blockchain::{BlockTemplateManager, ZionRPCClient};
use std::time::Duration;
use zion_pool::pplns::PPLNSCalculator;
use zion_pool::metrics::prometheus as metrics;
use chrono::Utc;

// CH v3 Revenue Orchestration imports
use zion_pool::revenue_proxy::RevenueProxyManager;
use zion_pool::pool_external_miner::{PoolExternalMiner, ExternalMinerConfig};
use zion_pool::profit_switcher::ProfitSwitcher;
use zion_pool::buyback::BuybackEngine;
use zion_pool::stream_scheduler::{StreamScheduler, ScheduledJob, StreamId};

#[derive(Clone)]
struct ApiState {
    storage: Arc<RedisStorage>,
    pplns: Arc<PPLNSCalculator>,
    template_manager: Arc<BlockTemplateManager>,
    rpc_client: Arc<ZionRPCClient>,
    // CH v3 Revenue subsystems
    revenue_proxy: Arc<RevenueProxyManager>,
    external_miner: Arc<PoolExternalMiner>,
    profit_switcher: Arc<ProfitSwitcher>,
    buyback_engine: Arc<BuybackEngine>,
    stream_scheduler: Arc<StreamScheduler>,
    start_time: i64,
    min_payout: f64,
    pool_fee_percent: f64,
    humanitarian_tithe_percent: f64,
    pool_wallet: String,
    listen: String,
    api_listen: String,
}

async fn api_health(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let redis_ok = state.storage.ping().await.is_ok();
    Json(json!({"status": "ok", "redis": redis_ok}))
}

async fn api_metrics() -> impl IntoResponse {
    let body = metrics::render();
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}

async fn api_miner_stats(
    Path(addr): Path<String>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    match state.storage.get_miner_stats(&addr).await {
        Ok(stats) => Json(json!({"ok": true, "stats": stats})),
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

async fn api_miner_payouts(
    Path(addr): Path<String>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    let pending_balance = state.pplns.get_pending_balance(&addr).await;
    let pending_payouts = state.pplns.get_pending_payouts(&addr).await;
    match (pending_balance, pending_payouts) {
        (Ok(balance), Ok(payouts)) => Json(json!({
            "ok": true,
            "pending_balance": balance,
            "pending_payouts": payouts,
        })),
        (b, p) => Json(json!({
            "ok": false,
            "balance_error": b.err().map(|e| e.to_string()),
            "payouts_error": p.err().map(|e| e.to_string()),
        })),
    }
}

async fn api_recent_blocks(
    Path(count): Path<usize>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    let count = count.clamp(1, 200);
    match state.storage.get_recent_blocks(count).await {
        Ok(blocks) => Json(json!({"ok": true, "blocks": blocks})),
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

fn parse_port(addr: &str, default_port: u16) -> u16 {
    addr.split(':')
        .last()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(default_port)
}

async fn api_pool_info(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let miner_pct = 100.0 - state.pool_fee_percent - state.humanitarian_tithe_percent;
    Json(json!({
        "name": "ZION Pool",
        "version": "2.9.5-mainnet",
        "wallet": state.pool_wallet,
        "fee": state.pool_fee_percent,
        "humanitarian_tithe": state.humanitarian_tithe_percent,
        "miner_share": miner_pct,
        "reward_distribution": {
            "miners_percent": miner_pct,
            "humanitarian_tithe_percent": state.humanitarian_tithe_percent,
            "pool_fee_percent": state.pool_fee_percent,
        },
        "min_payout": state.min_payout,
        "ports": {
            "stratum": parse_port(&state.listen, 3333),
            "stats": parse_port(&state.api_listen, 8080),
        }
    }))
}

async fn api_history_pool(State(state): State<ApiState>) -> Json<serde_json::Value> {
    match state.storage.get_pool_history().await {
        Ok(history) => Json(json!(history)),
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

async fn api_miners(
    Query(params): Query<LimitParams>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(100).clamp(1, 500);
    match state.storage.get_recent_miners(limit).await {
        Ok(miners) => {
            let list: Vec<serde_json::Value> = miners
                .into_iter()
                .map(|(addr, last_share)| {
                    json!({
                        "address": addr,
                        "last_share": last_share,
                    })
                })
                .collect();
            Json(json!({
                "count": list.len(),
                "miners": list,
            }))
        }
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

async fn api_blocks(
    Query(params): Query<LimitParams>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(100).clamp(1, 500);
    match state.storage.get_recent_blocks(limit).await {
        Ok(blocks) => Json(json!({
            "count": blocks.len(),
            "blocks": blocks,
        })),
        Err(e) => Json(json!({"error": e.to_string()})),
    }
}

#[derive(Deserialize)]
struct LimitParams {
    limit: Option<usize>,
}

async fn api_payouts(
    Query(params): Query<LimitParams>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    let limit = params.limit.unwrap_or(50).clamp(1, 500);
    match state.storage.get_recent_payouts(limit).await {
        Ok(payouts) => {
            let total_paid: f64 = payouts
                .iter()
                .filter(|p| p.status == "confirmed")
                .map(|p| p.amount)
                .sum();
            let pending_count = payouts
                .iter()
                .filter(|p| p.status == "pending" || p.status == "sent")
                .count();
            Json(json!({
                "ok": true,
                "count": payouts.len(),
                "total_paid": total_paid,
                "pending_count": pending_count,
                "payouts": payouts,
            }))
        }
        Err(e) => Json(json!({"ok": false, "error": e.to_string()})),
    }
}

async fn api_stats(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let pplns_window_size = state.storage.get_pplns_window_size().await.unwrap_or(0);
    let recent_blocks = state.storage.get_recent_blocks(10).await.unwrap_or_default();
    let blocks_found = state.storage.get_blocks_count().await.unwrap_or(0);
    let (hashrate_1h, hashrate_24h) = state.storage.get_pool_hashrate().await.unwrap_or((0.0, 0.0));
    let (valid_shares, invalid_shares) = state.storage.get_global_share_counts().await.unwrap_or((0, 0));
    let active_miners = state.storage.get_active_miners(600).await.unwrap_or(0);
    let total_miners = state.storage.get_total_miners().await.unwrap_or(0);
    let (pending_total, pending_miners) = state.storage.get_pending_payout_totals().await.unwrap_or((0, 0));
    let template = state.template_manager.get_template().await;
    let height = template.as_ref().map(|t| t.height).unwrap_or(0);
    let difficulty = template.as_ref().map(|t| t.difficulty).unwrap_or(0);
    let connected = state.rpc_client.health_check().await.unwrap_or(false);
    let now = Utc::now().timestamp();
    let uptime_secs = now.saturating_sub(state.start_time);
    Json(json!({
        "ok": true,
        "pool": {
            "name": "ZION Pool",
            "version": "2.9.5",
            "uptime": state.start_time,
            "uptime_secs": uptime_secs,
            "fee": state.pool_fee_percent,
            "humanitarian_tithe": state.humanitarian_tithe_percent,
            "miner_share": 100.0 - state.pool_fee_percent - state.humanitarian_tithe_percent,
            "min_payout": state.min_payout
        },
        "blockchain": {
            "height": height,
            "difficulty": difficulty,
            "connected": connected,
        },
        "miners": {
            "active": active_miners,
            "total": total_miners,
        },
        "shares": {
            "valid": valid_shares,
            "invalid": invalid_shares,
        },
        "hashrate": {
            "pool": hashrate_1h,
            "pool_1h": hashrate_1h,
            "pool_24h": hashrate_24h,
        },
        "blocks": {
            "found": blocks_found,
            "pending": 0,
        },
        "payouts": {
            "pending_total_atomic": pending_total,
            "pending_miners": pending_miners,
        },
        "pplns_window_size": pplns_window_size,
        "recent_blocks_count": recent_blocks.len(),
    }))
}

// ─── Revenue API Handlers (CH v3) ───

/// External mining stats API endpoint
async fn api_external_mining(State(state): State<ApiState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "revenue_proxy": state.revenue_proxy.stats_json(),
        "pool_miner": state.external_miner.stats_json(),
    }))
}

/// Profit switching status API endpoint
async fn api_profit_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let stats = state.profit_switcher.stats_json().await;
    Json(json!({
        "status": "ok",
        "profit_switching": stats,
    }))
}

/// Force switch to a specific coin via API
async fn api_profit_switch(
    Path(coin): Path<String>,
    State(state): State<ApiState>,
) -> Json<serde_json::Value> {
    match state.profit_switcher.force_switch(&coin).await {
        Ok(()) => Json(json!({"status": "ok", "message": format!("Switched to {}", coin)})),
        Err(e) => Json(json!({"status": "error", "message": e})),
    }
}

/// BTC buyback monitoring status
async fn api_buyback_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let stats = state.buyback_engine.stats_json().await;
    Json(json!({
        "status": "ok",
        "buyback": stats,
    }))
}

/// CH v3 Stream Scheduler status — time-splitting across revenue streams
async fn api_scheduler_status(State(state): State<ApiState>) -> Json<serde_json::Value> {
    let stats = state.stream_scheduler.stats_json().await;
    Json(json!({
        "status": "ok",
        "scheduler": stats,
    }))
}

// ─── Main ───

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .with_thread_ids(false)
        .init();

    println!("🚀 ZION Pool v2.9.5 — L1 MainNet with CH v3 Revenue Orchestration");
    let cfg = Config::load();

    // ── CH v3 Revenue Subsystems ──
    // Architecture:
    //   - ZION miners always get CosmicHarmony jobs (never ethash/kawpow)
    //   - Pool runs xmrig INTERNALLY (server-side) to mine ETC/RVN/ERG on external pools
    //   - BTC payouts from 2miners → buyback engine → 100% DAO treasury
    //   - This is ALWAYS active — it's core L1 revenue infrastructure

    // Initialize External Revenue Proxy Manager (connects to external pools)
    let revenue_proxy = Arc::new(RevenueProxyManager::new(cfg.revenue.streams.clone()));
    {
        let proxy_handle = revenue_proxy.clone();
        tokio::spawn(async move {
            proxy_handle.start().await;
        });
    }

    // Start profit switcher (auto-switch to most profitable external coin)
    // Must be created before external miner to check CPU-only mode
    let profit_switcher = ProfitSwitcher::new(cfg.revenue.profit_switching.clone());

    // Pool-side external miner (xmrig subprocess) — DISABLED in CH3 CPU-only mode.
    //
    // CH3 Architecture: Server-side xmrig is replaced by native RandomX in the miner binary.
    // CH v3 Revenue: PoolExternalMiner spawns xmrig as a subprocess.
    //
    // CPU-only mode (ARM64): xmrig runs with 1 thread on MoneroOcean.
    //   MoneroOcean auto-selects the most profitable CPU algo (rx/0, cn/r, etc.)
    //   and pays out in XMR.  No TimeSplit needed — xmrig runs independently.
    //
    // GPU mode: xmrig runs with 2 threads + GPU mining handled by profit-switched coins.
    let cpu_only = profit_switcher.is_cpu_only();
    let miner_config = ExternalMinerConfig {
        threads: if cpu_only { 1 } else { 2 },
        ..ExternalMinerConfig::default()
    };
    let external_miner = Arc::new(PoolExternalMiner::new(
        miner_config,
        revenue_proxy.clone(),
    ));
    {
        let miner_handle = external_miner.clone();
        let mode_str = if cpu_only { "CPU-only, 1 thread" } else { "GPU, 2 threads" };
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            tracing::info!("⛏️ Starting PoolExternalMiner ({}) — xmrig → MoneroOcean", mode_str);
            miner_handle.start().await;
        });
    }
    {
        let switcher_handle = profit_switcher.clone();
        tokio::spawn(async move {
            // Give revenue proxy + miner time to connect first
            tokio::time::sleep(Duration::from_secs(15)).await;
            switcher_handle.run().await;
        });
    }

    // Start BTC buyback monitoring engine (100% DAO treasury)
    let buyback_engine = BuybackEngine::new(cfg.revenue.buyback.clone());
    {
        let buyback_handle = buyback_engine.clone();
        tokio::spawn(async move {
            // Give pools time to start earning
            tokio::time::sleep(Duration::from_secs(30)).await;
            buyback_handle.run().await;
        });
    }

    // ── Core Pool Infrastructure ──

    // Session manager
    let session_manager = Arc::new(SessionManager::new());

    // Share processing pipeline
    let pplns_window_shares = if cfg.pplns_window_shares > 0 { cfg.pplns_window_shares } else { cfg.pplns_size };
    let storage = Arc::new(RedisStorage::new(&cfg.redis_url, pplns_window_shares).expect("redis storage"));
    let validator = Arc::new(ShareValidator::new("little"));

    // Block template manager (real jobs from ZION Core JSON-RPC)
    let (rpc_host, rpc_port, rpc_path) = match cfg.core_rpc_url.parse::<hyper::Uri>() {
        Ok(uri) => {
            let host = uri.host().unwrap_or("127.0.0.1").to_string();
            let port = uri.port_u16().unwrap_or(8444);
            let path = {
                let p = uri.path();
                if p.is_empty() || p == "/" { "/jsonrpc".to_string() } else { p.to_string() }
            };
            (host, port, path)
        }
        Err(e) => {
            eprintln!("Invalid core_rpc_url '{}': {} (using defaults)", cfg.core_rpc_url, e);
            ("127.0.0.1".to_string(), 8444, "/jsonrpc".to_string())
        }
    };

    let rpc_client = Arc::new(ZionRPCClient::new(
        rpc_host, rpc_port,
        Some(Duration::from_secs(5)),
        None, None,
        Some(rpc_path),
    ));

    let share_processor = Arc::new(ShareProcessor::new(
        validator,
        storage.clone(),
        Some(rpc_client.clone()),
        cfg.pool_wallet.clone(),
        cfg.humanitarian_wallet.clone(),
        cfg.pool_fee_percent,
        cfg.humanitarian_tithe_percent,
        pplns_window_shares as u64,
    ));

    let mut template_manager = BlockTemplateManager::new(
        rpc_client.clone(),
        cfg.pool_wallet.clone(),
        Some(Duration::from_secs(cfg.notify_secs)),
    );

    // Stratum server v2
    let server = Arc::new(stratum::StratumServer::new(
        cfg.listen.split(':').next().unwrap_or("0.0.0.0").to_string(),
        cfg.listen.split(':').last().unwrap_or("3333").parse().unwrap_or(3333),
        session_manager,
        share_processor,
        Some(10_000),
    ));

    // ── CH v3 StreamScheduler — time-splits mining jobs across revenue streams ──

    let stream_scheduler = Arc::new(StreamScheduler::new(
        &cfg.revenue.streams,
        Some(revenue_proxy.clone()),
    ));
    server.set_stream_scheduler(stream_scheduler.clone());

    // Register template change callback with CH v3 StreamScheduler integration
    {
        let server_weak = Arc::downgrade(&server);
        let scheduler = stream_scheduler.clone();
        template_manager.on_template_change(move |template| {
            if let Some(server) = server_weak.upgrade() {
                let server = server.clone();
                let template_clone = template.clone();
                let scheduler = scheduler.clone();
                tokio::spawn(async move {
                    // Update StreamScheduler with new ZION job
                    let job_id = format!("h{}-{}", template_clone.height,
                        template_clone.prev_hash.chars().take(8).collect::<String>());
                    let blob = template_clone.blob.clone().unwrap_or_else(|| "0".repeat(152));
                    let zion_job = ScheduledJob {
                        stream_id: StreamId::Zion,
                        job_id: job_id.clone(),
                        blob: blob.clone(),
                        target: template_clone.target.clone(),
                        difficulty: template_clone.difficulty as f64,
                        height: template_clone.height,
                        algorithm: "cosmic_harmony".to_string(),
                        coin: "ZION".to_string(),
                        clean_jobs: true,
                        extranonce: String::new(),
                        raw_notify_params: Vec::new(),
                        seed_hash: String::new(),
                        created_at: std::time::Instant::now(),
                    };
                    scheduler.update_zion_job(zion_job).await;

                    // Also broadcast via normal path (backward compatible)
                    server.broadcast_new_job(template_clone).await;
                });
            }
        });
    }

    let template_manager = Arc::new(template_manager);
    server.set_template_manager(template_manager.clone());

    // Listen to external pool jobs and feed them into the scheduler
    {
        let scheduler = stream_scheduler.clone();
        let job_rx = revenue_proxy.subscribe_jobs();
        tokio::spawn(async move {
            scheduler.listen_external_jobs(job_rx).await;
        });
    }

    // Listen to ProfitSwitcher coin changes → update scheduler + broadcast to Revenue miners
    {
        let scheduler = stream_scheduler.clone();
        let coin_rx = profit_switcher.subscribe();
        let server_weak = Arc::downgrade(&server);
        tokio::spawn(async move {
            let scheduler_ref = scheduler.clone();
            let mut coin_rx = coin_rx;
            tracing::info!("👂 StreamScheduler: Listening for ProfitSwitcher changes");
            loop {
                if coin_rx.changed().await.is_err() {
                    tracing::warn!("StreamScheduler: ProfitSwitcher channel closed");
                    break;
                }
                let new_coin = coin_rx.borrow().clone();
                if let Some(new_job) = scheduler_ref.set_best_coin(&new_coin).await {
                    tracing::info!("📢 Profit switch → {} — broadcasting to Revenue miners", new_coin.to_uppercase());
                    if let Some(server) = server_weak.upgrade() {
                        let revenue_miners = scheduler_ref.get_revenue_miners().await;
                        if !revenue_miners.is_empty() {
                            server.broadcast_job_to_sessions(&revenue_miners, new_job).await;
                        }
                    }
                }
            }
        });
    }

    // Run the stream switching loop — time-split mode (< 3 miners) + periodic rebalance
    // NOTE: StreamScheduler internally guards ZION-only mode (zion_share >= 1.0)
    //       so connected miners NEVER get ethash/kawpow jobs — only CosmicHarmony
    //       External mining is done server-side by PoolExternalMiner (xmrig subprocess)
    {
        let scheduler = stream_scheduler.clone();
        let server_weak = Arc::downgrade(&server);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            tracing::info!("🔄 StreamScheduler v2: Switch loop started (hybrid mode)");
            loop {
                interval.tick().await;
                // Time-split mode: maybe_switch returns job when switching ZION↔Revenue
                if let Some(new_job) = scheduler.maybe_switch().await {
                    if let Some(server) = server_weak.upgrade() {
                        server.broadcast_scheduled_job(new_job).await;
                    }
                }
                // Per-miner mode: periodic rebalance (every 30s effectively)
                let changes = scheduler.rebalance().await;
                for (session_id, _group, job_opt) in changes {
                    if let (Some(server), Some(job)) = (server_weak.upgrade(), job_opt) {
                        server.broadcast_job_to_sessions(&[session_id], job).await;
                    }
                }
            }
        });
    }

    // Start template manager in background
    let template_manager_clone = template_manager.clone();
    tokio::spawn(async move {
        template_manager_clone.start().await;
    });

    // Start Stratum server
    let server_clone = server.clone();
    tokio::spawn(async move {
        if let Err(e) = server_clone.start().await {
            eprintln!("Stratum server error: {}", e);
        }
    });

    // PPLNS + Payout
    let pplns = Arc::new(PPLNSCalculator::new(storage.clone(), pplns_window_shares as u64));
    let payout_manager = payout::PayoutManager::new(
        storage.clone(), pplns.clone(), rpc_client.clone(),
        cfg.pool_wallet.clone(), cfg.min_payout,
        cfg.max_payout_per_tx, cfg.payout_interval_seconds,
        cfg.payout_batch_limit, cfg.payout_confirm_timeout_seconds,
    );
    payout_manager.start();

    // Optional: PostgreSQL payout scheduler
    if let Ok(postgres_url) = std::env::var("PAYOUT_DB_URL") {
        println!("Starting PostgreSQL payout scheduler...");
        match zion_pool::payout::scheduler::PayoutScheduler::new(
            &postgres_url, cfg.min_payout,
            Duration::from_secs(cfg.payout_interval_seconds as u64),
        ).await {
            Ok(scheduler) => {
                if let Err(e) = scheduler.init_schema().await {
                    eprintln!("Failed to initialize payout scheduler schema: {}", e);
                } else {
                    let scheduler = Arc::new(scheduler);
                    let process_enabled = std::env::var("PAYOUT_DB_PROCESS")
                        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
                        .unwrap_or(false);

                    if process_enabled {
                        let rpc = rpc_client.clone();
                        let wallet = cfg.pool_wallet.clone();
                        tokio::spawn(async move {
                            if let Err(e) = scheduler.start_rpc_payout_loop(rpc, wallet).await {
                                eprintln!("Payout scheduler (rpc) error: {:#}", e);
                            }
                        });
                        println!("✅ Payout scheduler started (RPC processing enabled)");
                    } else {
                        tokio::spawn(async move { scheduler.run().await; });
                        println!("✅ Payout scheduler started (monitor-only)");
                    }
                }
            }
            Err(e) => eprintln!("Failed to create payout scheduler: {}", e),
        }
    }

    // API state with revenue subsystems
    let api_state = ApiState {
        storage: storage.clone(),
        pplns,
        template_manager: template_manager.clone(),
        rpc_client: rpc_client.clone(),
        revenue_proxy: revenue_proxy.clone(),
        external_miner: external_miner.clone(),
        profit_switcher: profit_switcher.clone(),
        buyback_engine: buyback_engine.clone(),
        stream_scheduler: stream_scheduler.clone(),
        start_time: Utc::now().timestamp(),
        min_payout: cfg.min_payout,
        pool_fee_percent: cfg.pool_fee_percent,
        humanitarian_tithe_percent: cfg.humanitarian_tithe_percent,
        pool_wallet: cfg.pool_wallet.clone(),
        listen: cfg.listen.clone(),
        api_listen: cfg.api_listen.clone(),
    };

    // Background stats sampler
    {
        let storage = storage.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            let mut last_snapshot = 0;
            let mut last_miner_metrics = 0i64;
            loop {
                interval.tick().await;
                let redis_ok = storage.ping().await.is_ok();
                metrics::set_redis_up(redis_ok);
                if let Ok(sz) = storage.get_pplns_window_size().await {
                    metrics::set_pplns_window_size(sz);
                }
                let now = Utc::now().timestamp();
                if now - last_snapshot >= 60 {
                    if let Err(e) = storage.snapshot_pool_stats().await {
                        tracing::error!("Failed to snapshot pool stats: {}", e);
                    }
                    last_snapshot = now;
                }
                // Per-miner Prometheus metrics update (every 30s)
                if now - last_miner_metrics >= 30 {
                    if let Ok(miners) = storage.get_recent_miners(500).await {
                        for (addr, _last_share) in &miners {
                            if let Ok(stats) = storage.get_miner_stats(addr).await {
                                metrics::set_miner_hashrate(addr, stats.hashrate_1h as u64);
                                metrics::set_miner_pending(addr, stats.pending_balance as i64);
                                metrics::set_miner_paid(addr, stats.total_paid as i64);
                            }
                        }
                    }
                    last_miner_metrics = now;
                }
            }
        });
    }

    // HTTP API routes — L1 + CH v3 Revenue
    let api = axum::Router::new()
        .route("/health", get(api_health))
        .route("/metrics", get(api_metrics))
        .route("/stats", get(api_stats))
        .route("/pool", get(api_pool_info))
        .route("/miners", get(api_miners))
        .route("/blocks", get(api_blocks))
        .route("/payouts", get(api_payouts))
        .route("/api/v1/miner/:addr/stats", get(api_miner_stats))
        .route("/api/v1/miner/:addr/payouts", get(api_miner_payouts))
        .route("/api/v1/blocks/recent/:count", get(api_recent_blocks))
        .route("/api/v1/pool/history", get(api_history_pool))
        .route("/history/pool", get(api_history_pool))
        // CH v3 Revenue API endpoints
        .route("/api/v1/external/stats", get(api_external_mining))
        .route("/api/v1/profit/status", get(api_profit_status))
        .route("/api/v1/profit/switch/:coin", get(api_profit_switch))
        .route("/api/v1/buyback/status", get(api_buyback_status))
        .route("/api/v1/scheduler/status", get(api_scheduler_status))
        .with_state(api_state);

    let listener = tokio::net::TcpListener::bind(&cfg.api_listen).await.unwrap();

    // Graceful shutdown (cross-platform: ctrl_c + SIGTERM on Unix)
    let shutdown_signal = async {
        #[cfg(unix)]
        {
            let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to register SIGTERM handler");
            let ctrl_c = tokio::signal::ctrl_c();
            tokio::select! {
                _ = sigterm.recv() => tracing::info!("SIGTERM — shutting down"),
                _ = ctrl_c => tracing::info!("SIGINT — shutting down"),
            }
        }
        #[cfg(not(unix))]
        {
            // Windows: only ctrl_c is supported
            tokio::signal::ctrl_c().await.expect("Failed to register Ctrl+C handler");
            tracing::info!("Ctrl+C — shutting down");
        }
    };

    tracing::info!("📡 ZION Pool API listening on {}", cfg.api_listen);
    tracing::info!("💰 CH v3 Revenue: proxy={}, scheduler={}, profit_switch={}, buyback={}",
        cfg.revenue.enabled,
        cfg.revenue.streams.zion.enabled,
        cfg.revenue.profit_switching.enabled,
        cfg.revenue.buyback.enabled,
    );
    axum::serve(listener, api)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .unwrap();

    tracing::info!("🏁 ZION Pool shut down cleanly");
}
