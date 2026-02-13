/// Stratum protocol message types and serialization
/// 
/// Supports both XMRig and Stratum JSON-RPC protocols

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Legacy simple types (kept for compatibility)
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

pub fn ok(id: Option<Value>, result: Value) -> Response {
    Response { id, result: Some(result), error: None }
}

pub fn err(id: Option<Value>, message: &str) -> Response {
    Response { id, result: None, error: Some(Value::String(message.to_string())) }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitParams {
    pub job_id: Value,
    pub nonce: Value,
    pub result: Value,
}

// Enhanced protocol types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumRequest {
    /// JSON-RPC version (usually "2.0")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,

    /// Request ID
    pub id: Value,

    /// Method name
    pub method: String,

    /// Method parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumResponse {
    /// JSON-RPC version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,

    /// Request ID
    pub id: Value,

    /// Result (if success)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,

    /// Error (if failure)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<StratumError>,
}

impl StratumResponse {
    /// Create success response
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Create error response
    pub fn error(id: Value, error: StratumError) -> Self {
        Self {
            jsonrpc: Some("2.0".to_string()),
            id,
            result: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StratumError {
    /// Error code
    pub code: i32,

    /// Error message
    pub message: String,

    /// Additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl StratumError {
    /// Standard error codes
    pub const UNKNOWN: i32 = -1;
    pub const INVALID_METHOD: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    /// Job not found
    pub const JOB_NOT_FOUND: i32 = 21;

    /// Invalid share
    pub const LOW_DIFFICULTY: i32 = 23;
    pub const DUPLICATE_SHARE: i32 = 22;

    /// Not authorized
    pub const UNAUTHORIZED: i32 = 24;

    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self::new(Self::INVALID_PARAMS, message)
    }

    pub fn job_not_found() -> Self {
        Self::new(Self::JOB_NOT_FOUND, "Job not found")
    }

    pub fn low_difficulty() -> Self {
        Self::new(Self::LOW_DIFFICULTY, "Share difficulty too low")
    }

    pub fn unauthorized() -> Self {
        Self::new(Self::UNAUTHORIZED, "Unauthorized worker")
    }
}

/// XMRig job format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XMRigJob {
    /// Block blob (hex)
    pub blob: String,

    /// Job ID
    pub job_id: String,

    /// Target difficulty (hex)
    pub target: String,

    /// Block height
    pub height: u64,

    /// Algorithm
    pub algo: String,

    /// Seed hash (RandomX)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed_hash: Option<String>,
}

/// Share submission data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareSubmission {
    /// Job ID
    pub job_id: String,

    /// Nonce
    pub nonce: String,

    /// Result/hash
    pub result: String,

    /// Worker name (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
}
