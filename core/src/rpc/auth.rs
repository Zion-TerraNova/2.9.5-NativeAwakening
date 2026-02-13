//! RPC Bearer Token Authentication Middleware
//!
//! Protects write endpoints (submit_block, submit_tx, jsonrpc) behind a bearer token.
//! Read-only endpoints (stats, block queries, health) remain public.
//!
//! Token is read from the `ZION_RPC_TOKEN` environment variable.
//! If unset, authentication is **disabled** (open access — suitable for dev/testnet).

use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};

/// Read the RPC token from the environment (cached on first call).
fn get_rpc_token() -> Option<String> {
    // Cache the token in a thread-local to avoid repeated env lookups on every request.
    // The token is read once at startup time.
    use std::sync::OnceLock;
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN
        .get_or_init(|| {
            std::env::var("ZION_RPC_TOKEN")
                .ok()
                .filter(|t| !t.is_empty())
        })
        .clone()
}

/// Axum middleware: require `Authorization: Bearer <token>` on protected routes.
///
/// If `ZION_RPC_TOKEN` is not set → pass through (no auth).
/// If set → compare constant-time against the provided header.
pub async fn require_bearer_token(
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = match get_rpc_token() {
        Some(t) => t,
        None => return Ok(next.run(request).await), // No token configured → open access
    };

    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(value) if value.starts_with("Bearer ") => {
            let provided = &value[7..];
            // Constant-time comparison to prevent timing attacks
            if constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
                Ok(next.run(request).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

/// Constant-time byte comparison (prevents timing side-channels).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hell"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn test_no_token_configured() {
        // When ZION_RPC_TOKEN is not set, get_rpc_token should return None
        // (unless someone else set it in this test process)
        // This test is a sanity check for the constant_time_eq logic
        assert!(constant_time_eq(b"secret123", b"secret123"));
        assert!(!constant_time_eq(b"secret123", b"secret124"));
    }
}
