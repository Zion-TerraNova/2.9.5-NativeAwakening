/// HTTP endpoints for metrics and health checks
/// 
/// Provides:
/// - GET /metrics - Prometheus metrics
/// - GET /health - Health check
/// - GET /readiness - Readiness probe (K8s)
/// - GET /liveness - Liveness probe (K8s)

use axum::{
    extract::State as AxumState,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use super::Metrics;

/// Create metrics router
pub fn metrics_router(metrics: Arc<Metrics>) -> Router {
    Router::new()
        .route("/metrics", get(prometheus_metrics))
        .route("/health", get(health_check))
        .route("/readiness", get(readiness_check))
        .route("/liveness", get(liveness_check))
        .with_state(metrics)
}

/// Prometheus metrics endpoint
/// GET /metrics
async fn prometheus_metrics(
    AxumState(metrics): AxumState<Arc<Metrics>>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        [("Content-Type", "text/plain; version=0.0.4")],
        metrics.prometheus_export(),
    )
}

/// Health check endpoint
/// GET /health
async fn health_check(
    AxumState(metrics): AxumState<Arc<Metrics>>,
) -> impl IntoResponse {
    let health = metrics.health_check();
    let status = match health.status.as_str() {
        "healthy" => StatusCode::OK,
        "degraded" => StatusCode::OK, // Still operational
        "unhealthy" => StatusCode::SERVICE_UNAVAILABLE,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    
    (status, Json(health))
}

/// Readiness check (K8s)
/// Returns 200 if node is ready to accept traffic
/// GET /readiness
async fn readiness_check(
    AxumState(metrics): AxumState<Arc<Metrics>>,
) -> impl IntoResponse {
    let health = metrics.health_check();
    
    // Ready if:
    // - Has peers
    // - Recent block (< 15 min) — testnet can have longer gaps
    // - Mempool not full
    let is_ready = health.peers_connected > 0
        && health.time_since_last_block < 900
        && health.mempool_size < 50_000;
    
    if is_ready {
        (StatusCode::OK, "ready")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "not ready")
    }
}

/// Liveness check (K8s)
/// Returns 200 if node is alive (not deadlocked/crashed)
/// GET /liveness
async fn liveness_check(
    AxumState(metrics): AxumState<Arc<Metrics>>,
) -> impl IntoResponse {
    // Simple check: if we can read metrics, node is alive
    let _ = metrics.current_height.load(std::sync::atomic::Ordering::Relaxed);
    (StatusCode::OK, "alive")
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_metrics_router_creation() {
        let metrics = Metrics::new();
        let _router = metrics_router(metrics);
        // Router created successfully
    }
}
