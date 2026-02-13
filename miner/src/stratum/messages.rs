use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

impl StratumRequest {
    /// Create login request
    pub fn login(id: u64, wallet: &str, worker: &str, pass: &str, algorithm: &str) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            method: "login".to_string(),
            params: serde_json::json!({
                "login": wallet,
                "pass": pass,
                "rigid": worker,
                "agent": "zion-universal-miner/2.9.5",
                "algo": algorithm
            }),
        }
    }

    /// Create subscribe request (Stratum)
    pub fn subscribe(id: u64) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            method: "mining.subscribe".to_string(),
            params: serde_json::json!([]),
        }
    }

    /// Create authorize request (Stratum)
    pub fn authorize(id: u64, username: &str, password: &str) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            method: "mining.authorize".to_string(),
            params: serde_json::json!([username, password]),
        }
    }

    /// Create submit request
    pub fn submit(id: u64, session_id: &str, job_id: &str, nonce: u32, result: &str) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            method: "submit".to_string(),
            params: serde_json::json!({
                "id": session_id,
                "job_id": job_id,
                "nonce": format!("{:08x}", nonce),
                "result": result
            }),
        }
    }

    /// Create submit request (Stratum)
    pub fn submit_stratum(id: u64, worker: &str, job_id: &str, nonce_hex: &str, result: &str) -> Self {
        // Submit with result hash for CH v3 revenue stream forwarding.
        // Pool extracts result from params[5] and forwards it to external pools
        // (MoneroOcean/CryptoNote requires the result hash for share validation).
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            method: "mining.submit".to_string(),
            params: serde_json::json!([worker, job_id, "00", "00000000", nonce_hex, result]),
        }
    }

    /// Create keepalive request
    pub fn keepalive(id: u64, session_id: &str) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            method: "keepalived".to_string(),
            params: serde_json::json!({
                "id": session_id
            }),
        }
    }

    /// Create getjob request (XMRig protocol)
    pub fn getjob(id: u64) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            method: "getjob".to_string(),
            params: serde_json::json!({}),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumResponse {
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<StratumError>,
    pub method: Option<String>,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub job_id: String,
    pub blob: String,
    pub target: String,
    pub height: u64,
    pub seed_hash: Option<String>,
    pub algo: Option<String>,
    /// Coin being mined (e.g., "ZION", "ERG", "ETC") — set by StreamScheduler v2
    #[serde(default)]
    pub coin: Option<String>,
    #[serde(default)]
    pub cosmic_state0_endian: Option<String>,
}
