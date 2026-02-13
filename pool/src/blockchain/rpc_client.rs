/// ZION RPC Client - Communication with blockchain
/// 
/// Rust implementation of Python's ZionRPCClient with circuit breaker pattern

use anyhow::{anyhow, Result};
use hyper::{body::Buf, Method, Request};
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use http_body_util::{BodyExt, Full};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use zion_core::blockchain::block::Block;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::metrics::prometheus as metrics;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

#[derive(Debug, Clone)]
struct CircuitBreaker {
    failures: u32,
    last_failure: Option<Instant>,
    is_open: bool,
    max_failures: u32,
    reset_timeout: Duration,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            failures: 0,
            last_failure: None,
            is_open: false,
            max_failures: 5,
            reset_timeout: Duration::from_secs(60),
        }
    }

    fn record_failure(&mut self) {
        self.failures += 1;
        self.last_failure = Some(Instant::now());
        
        if self.failures >= self.max_failures {
            self.is_open = true;
            tracing::error!(
                "🔌 Circuit Breaker TRIPPED after {} failures! Pausing RPC for {}s",
                self.failures,
                self.reset_timeout.as_secs()
            );
        }
    }

    fn record_success(&mut self) {
        if self.failures > 0 {
            self.failures = 0;
            tracing::info!("✅ Circuit Breaker: Reset (successful call)");
        }
    }

    fn check(&mut self) -> Result<()> {
        if self.is_open {
            if let Some(last_fail) = self.last_failure {
                if last_fail.elapsed() > self.reset_timeout {
                    tracing::info!("🔌 Circuit Breaker: Resetting (half-open state)");
                    self.is_open = false;
                    self.failures = 0;
                } else {
                    return Err(anyhow!("RPC Circuit Breaker is OPEN"));
                }
            }
        }
        Ok(())
    }
}

pub struct ZionRPCClient {
    base_url: String,
    _host: String,
    _port: u16,
    timeout: Duration,
    _rpc_user: Option<String>,
    _rpc_password: Option<String>,
    client: Client<HttpConnector, Full<Bytes>>,
    circuit_breaker: Arc<RwLock<CircuitBreaker>>,
}

impl ZionRPCClient {
    pub fn new(
        host: String,
        port: u16,
        timeout: Option<Duration>,
        rpc_user: Option<String>,
        rpc_password: Option<String>,
        rpc_path: Option<String>,
    ) -> Self {
        let rpc_path = rpc_path.unwrap_or_else(|| "/jsonrpc".to_string());
        let base_url = format!("http://{}:{}{}", host, port, rpc_path);
        
        let client = Client::builder(hyper_util::rt::TokioExecutor::new())
            .build_http();

        tracing::info!("ZionRPCClient initialized: {}", base_url);

        Self {
            base_url,
            _host: host.clone(),
            _port: port,
            timeout: timeout.unwrap_or(Duration::from_secs(30)),
            _rpc_user: rpc_user,
            _rpc_password: rpc_password,
            client,
            circuit_breaker: Arc::new(RwLock::new(CircuitBreaker::new())),
        }
    }

    /// Make RPC call to blockchain
    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
            metrics::inc_rpc_requests();

            let res: Result<Value> = async {
        // Check Circuit Breaker
        {
            let mut breaker = self.circuit_breaker.write().await;
            breaker.check()?;
        }

        let payload = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: method.to_string(),
            params,
        };

        let body_bytes = serde_json::to_vec(&payload)?;
        let body = Full::new(Bytes::from(body_bytes));

        let req = Request::builder()
            .method(Method::POST)
            .uri(&self.base_url)
            .header("Content-Type", "application/json")
            .body(body)?;

        // Execute request with timeout
        let response = tokio::time::timeout(
            self.timeout,
            self.client.request(req)
        ).await
            .map_err(|_| anyhow!("RPC request timeout"))?
            .map_err(|e| anyhow!("RPC connection failed: {}", e))?;

        // Check HTTP status
        let status = response.status();
        if !status.is_success() {
            let mut breaker = self.circuit_breaker.write().await;
            breaker.record_failure();
            return Err(anyhow!("RPC HTTP error: {}", status));
        }

        // Parse response body
        let body = response.into_body();
        let body_bytes = body.collect().await?.to_bytes();
        let rpc_response: RpcResponse = serde_json::from_reader(body_bytes.reader())?;

        // Check for JSON-RPC level error
        if let Some(error) = rpc_response.error {
            tracing::error!("RPC error: {:?}", error);
            // Don't trip circuit breaker on application errors
            return Err(anyhow!("RPC error: {:?}", error));
        }

        // Success - reset failures
        {
            let mut breaker = self.circuit_breaker.write().await;
            breaker.record_success();
        }

            Ok(rpc_response.result.unwrap_or(Value::Null))
            }
            .await;

            if res.is_err() {
                metrics::inc_rpc_errors();
            }

            res
    }

    /// Get new block template
    pub async fn get_block_template(&self, wallet_address: &str) -> Result<Value> {
        // Support both legacy snake_case and JSON-RPC CamelCase.
        // Some cores ignore params; others expect a wallet_address.
        match self
            .call("get_block_template", json!({ "wallet_address": wallet_address }))
            .await
        {
            Ok(v) => Ok(v),
            Err(_) => self.call("getBlockTemplate", json!({})).await,
        }
    }

    /// Get wallet balance
    pub async fn get_balance(&self, address: &str) -> Result<Value> {
        self.call("getbalance", json!({ "address": address })).await
    }

    /// Send transaction
    pub async fn send_transaction(
        &self,
        from_addr: &str,
        to_addr: &str,
        amount: f64,
        purpose: Option<&str>,
    ) -> Result<Value> {
        let params = json!([from_addr, to_addr, amount, purpose.unwrap_or("")]);
        self.call("sendtransaction", params).await
    }

    /// Get transaction by ID
    pub async fn get_transaction(&self, tx_id: &str) -> Result<Value> {
        self.call("gettransaction", json!([tx_id])).await
    }

    /// Get UTXOs for an address (for wallet UTXO selection)
    pub async fn get_utxos(&self, address: &str, limit: usize, offset: usize) -> Result<Value> {
        self.call("getUtxos", json!({
            "address": address,
            "limit": limit,
            "offset": offset,
        })).await
    }

    /// Submit a fully signed Transaction object to mempool
    /// This is the secure path: pool builds + signs TX locally
    pub async fn submit_signed_transaction(&self, tx: &zion_core::tx::Transaction) -> Result<Value> {
        let tx_json = serde_json::to_value(tx)?;
        self.call("submitTransaction", json!([tx_json])).await
    }

    /// Get block by height
    pub async fn get_block(&self, height: u64) -> Result<Value> {
        self.call("getblock", json!([height])).await
    }

    /// Submit mined block
    /// 
    /// For RandomX, the pool may include a miner-provided PoW hash (XMRig-style
    /// `result`) so the blockchain can validate difficulty without needing a
    /// local RandomX dataset in constrained environments.
    pub async fn submit_block(
        &self,
        block_data: &str,
        result_hash: Option<&str>,
        algorithm: Option<&str>,
        wallet_address: Option<&str>,
    ) -> Result<bool> {
        let mut params = json!({ "block": block_data });
        
        if let Some(result) = result_hash {
            params["result"] = json!(result);
        }
        if let Some(algo) = algorithm {
            params["algorithm"] = json!(algo);
        }
        if let Some(wallet) = wallet_address {
            params["wallet_address"] = json!(wallet);
        }

        // Support both legacy snake_case and JSON-RPC CamelCase.
        let result = match self.call("submitblock", params.clone()).await {
            Ok(v) => v,
            Err(_) => self.call("submitBlock", params).await?,
        };

        let accepted = if let Some(b) = result.as_bool() {
            b
        } else if let Some(obj) = result.as_object() {
            obj.get("accepted")
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    obj.get("status")
                        .and_then(|s| s.as_str())
                        .map(|s| s.eq_ignore_ascii_case("accepted"))
                })
                .unwrap_or(false)
        } else {
            false
        };

        if !accepted {
            tracing::error!("submitblock rejected: {:?}", result);
        }

        Ok(accepted)
    }

    /// Submit block using template blob + nonce (preferred mode for pool)
    /// 
    /// This sends params as array: [blob_hex, nonce_u64, wallet_address]
    /// Core will reconstruct block from template blob + nonce
    pub async fn submit_block_with_nonce(
        &self,
        blob_hex: &str,
        nonce: u64,
        wallet_address: &str,
    ) -> Result<bool> {
        // Send as array: [blob, nonce, wallet]
        let params = json!([blob_hex, nonce, wallet_address]);

        // Support both legacy snake_case and JSON-RPC CamelCase.
        let result = match self.call("submitblock", params.clone()).await {
            Ok(v) => v,
            Err(_) => self.call("submitBlock", params).await?,
        };

        let accepted = if let Some(b) = result.as_bool() {
            b
        } else if let Some(obj) = result.as_object() {
            obj.get("accepted")
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    obj.get("status")
                        .and_then(|s| s.as_str())
                        .map(|s| s.eq_ignore_ascii_case("accepted") || s.eq_ignore_ascii_case("ok"))
                })
                .unwrap_or(false)
        } else {
            false
        };

        if !accepted {
            tracing::error!("submitblock (blob+nonce) rejected: {:?}", result);
        }

        Ok(accepted)
    }
    /// Submit mined block as a structured Block object (preferred)
    pub async fn submit_block_object(&self, block: &Block) -> Result<bool> {
        let params = serde_json::to_value(block)?;

        // Support both legacy snake_case and JSON-RPC CamelCase.
        let result = match self.call("submitblock", params.clone()).await {
            Ok(v) => v,
            Err(_) => self.call("submitBlock", params).await?,
        };

        let accepted = if let Some(b) = result.as_bool() {
            b
        } else if let Some(obj) = result.as_object() {
            obj.get("accepted")
                .and_then(|v| v.as_bool())
                .or_else(|| {
                    obj.get("status")
                        .and_then(|s| s.as_str())
                        .map(|s| s.eq_ignore_ascii_case("accepted"))
                })
                .unwrap_or(false)
        } else {
            false
        };

        if !accepted {
            tracing::error!("submitblock rejected: {:?}", result);
        }

        Ok(accepted)
    }

    /// Health check
    pub async fn health_check(&self) -> Result<bool> {
        match self.call("get_info", json!({})).await {
            Ok(_) => Ok(true),
            Err(e) => {
                tracing::warn!("Health check failed: {}", e);
                Ok(false)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker() {
        let mut breaker = CircuitBreaker::new();
        
        // Should be closed initially
        assert!(!breaker.is_open);
        
        // Record failures
        for _ in 0..4 {
            breaker.record_failure();
            assert!(!breaker.is_open);
        }
        
        // 5th failure should trip
        breaker.record_failure();
        assert!(breaker.is_open);
        assert!(breaker.check().is_err());
    }

    #[tokio::test]
    async fn test_rpc_client_creation() {
        let client = ZionRPCClient::new(
            "127.0.0.1".to_string(),
            18081,
            None,
            None,
            None,
            None,
        );
        
        assert_eq!(client.base_url, "http://127.0.0.1:18081/jsonrpc");
    }
}
