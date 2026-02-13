//! 🧠 NCL (Neural Compute Layer) Client for ZION Miner
//! 
//! Handles AI task fetching, NPU scheduling, and bonus submission.
//! Implements the 5th revenue stream from CH v3.
//! 
//! ## Protocol Version
//! This client supports NCL Protocol v1.0

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// NCL Protocol Version (must match pool)
pub const NCL_PROTOCOL_VERSION: &str = "1.0";

fn normalize_hex(s: &str) -> String {
    s.trim().trim_start_matches("0x").to_lowercase()
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Supported NCL Task Types (mirrors pool enum)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NclTaskType {
    /// Blake3 hash chaining (deterministic, CPU-verifiable)
    HashChainingV1,
    /// Text embedding inference
    #[serde(rename = "embedding")]
    Embedding,
    /// LLM inference
    #[serde(rename = "llm_inference")]
    LlmInference,
    /// Image classification
    #[serde(rename = "image_classification")]
    ImageClassification,
}

impl NclTaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NclTaskType::HashChainingV1 => "hash_chaining_v1",
            NclTaskType::Embedding => "embedding",
            NclTaskType::LlmInference => "llm_inference",
            NclTaskType::ImageClassification => "image_classification",
        }
    }
    
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "hash_chaining_v1" => Some(NclTaskType::HashChainingV1),
            "embedding" | "embeddings" => Some(NclTaskType::Embedding),
            "llm_inference" => Some(NclTaskType::LlmInference),
            "image_classification" => Some(NclTaskType::ImageClassification),
            _ => None,
        }
    }
    
    /// Check if task type is deterministically verifiable
    pub fn is_deterministic(&self) -> bool {
        matches!(self, NclTaskType::HashChainingV1)
    }
    
    /// Get default time budget for this task type (ms)
    pub fn default_budget_ms(&self) -> u64 {
        match self {
            NclTaskType::HashChainingV1 => 5000,
            NclTaskType::Embedding => 50,
            NclTaskType::LlmInference => 500,
            NclTaskType::ImageClassification => 100,
        }
    }
}

/// NPU Runtime Type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpuType {
    /// CPU fallback (ONNX Runtime)
    Cpu,
    /// Apple Neural Engine (Core ML)
    CoreMl,
    /// NVIDIA TensorRT
    TensorRt,
    /// Intel OpenVINO
    OpenVino,
    /// AMD ROCm
    Rocm,
    /// Generic ONNX Runtime
    Onnx,
}

impl NpuType {
    pub fn as_str(&self) -> &'static str {
        match self {
            NpuType::Cpu => "cpu",
            NpuType::CoreMl => "coreml",
            NpuType::TensorRt => "tensorrt",
            NpuType::OpenVino => "openvino",
            NpuType::Rocm => "rocm",
            NpuType::Onnx => "onnx",
        }
    }
    
    /// Detect best available NPU on this system
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            // macOS: prefer Core ML (Apple Neural Engine)
            return NpuType::CoreMl;
        }

        #[cfg(target_os = "linux")]
        {
            // Linux: check for NVIDIA GPU
            if std::process::Command::new("nvidia-smi").output().is_ok() {
                return NpuType::TensorRt;
            }
            // TODO: Detect Intel NPU, AMD XDNA, etc.
            return NpuType::Cpu;
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            return NpuType::Cpu;
        }
    }
    
    /// Estimated TFLOPS for this NPU type
    pub fn estimated_tflops(&self) -> f32 {
        match self {
            NpuType::Cpu => 0.5,       // ~500 GFLOPS typical
            NpuType::CoreMl => 11.0,   // Apple M1/M2/M3 Neural Engine
            NpuType::TensorRt => 40.0, // RTX 3080 typical
            NpuType::OpenVino => 5.0,  // Intel iGPU
            NpuType::Rocm => 25.0,     // AMD RX 6800
            NpuType::Onnx => 0.5,      // CPU fallback
        }
    }
}

/// Retry policy (mirrors pool)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NclRetryPolicy {
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,
    #[serde(default)]
    pub allow_reassignment: bool,
}

fn default_max_retries() -> u32 { 3 }
fn default_retry_delay_ms() -> u64 { 5000 }

impl Default for NclRetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: default_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
            allow_reassignment: true,
        }
    }
}

/// NCL Reward configuration
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NclReward {
    #[serde(default)]
    pub zion: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_hashrate: Option<f64>,
}

/// NCL Verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NclVerification {
    pub method: String,
    pub seed: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rounds: Option<u32>,
}

/// AI Task from pool (NCL Contract v1.0)
/// 
/// This struct matches the pool's NclTask contract.
/// All required fields must be present for task validation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NCLTask {
    /// Protocol version (must match NCL_PROTOCOL_VERSION)
    #[serde(default = "default_version")]
    pub version: String,
    
    /// Unique task ID (UUID format)
    pub task_id: String,
    
    /// Task type (validated against NclTaskType enum)
    pub task_type: String,
    
    /// Task payload (JSON) - contains rounds, seed, etc.
    #[serde(default)]
    pub payload: serde_json::Value,
    
    /// Absolute deadline (Unix timestamp ms)
    #[serde(default)]
    pub deadline_ms: u64,
    
    /// Creation timestamp
    #[serde(default = "current_timestamp_ms")]
    pub created_at: u64,
    
    /// Reward configuration
    #[serde(default)]
    pub reward: NclReward,
    
    /// Verification configuration
    #[serde(default)]
    pub verification: Option<NclVerification>,
    
    /// Retry policy
    #[serde(default)]
    pub retry_policy: NclRetryPolicy,
    
    // Legacy fields for backward compatibility
    /// Input data (base64 or JSON) - legacy
    #[serde(default)]
    pub input_data: String,
    /// Model to use - legacy
    #[serde(default)]
    pub model: String,
    /// Maximum compute time in milliseconds - legacy
    #[serde(default)]
    pub max_time_ms: u64,
    /// Reward multiplier - legacy
    #[serde(default = "default_reward_multiplier")]
    pub reward_multiplier: f64,
}

fn default_version() -> String { NCL_PROTOCOL_VERSION.to_string() }
fn default_reward_multiplier() -> f64 { 1.0 }

impl NCLTask {
    /// Validate task contract
    pub fn validate(&self) -> Result<(), String> {
        // Version check (warn but don't fail for minor mismatches)
        if !self.version.starts_with("1.") {
            warn!("NCL version mismatch: {} (expected {})", self.version, NCL_PROTOCOL_VERSION);
        }
        
        // Task type validation
        if NclTaskType::from_str(&self.task_type).is_none() {
            return Err(format!("Unknown task_type: {}", self.task_type));
        }
        
        Ok(())
    }
    
    /// Check if task has expired
    pub fn is_expired(&self) -> bool {
        if self.deadline_ms == 0 {
            return false; // No deadline set
        }
        current_timestamp_ms() > self.deadline_ms
    }
    
    /// Get remaining time in milliseconds
    pub fn remaining_ms(&self) -> Option<u64> {
        if self.deadline_ms == 0 {
            return None;
        }
        let now = current_timestamp_ms();
        if self.deadline_ms > now {
            Some(self.deadline_ms - now)
        } else {
            None
        }
    }
    
    /// Get task type as enum
    pub fn task_type_enum(&self) -> Option<NclTaskType> {
        NclTaskType::from_str(&self.task_type)
    }
    
    /// Get effective time budget (from payload, verification, or legacy field)
    pub fn effective_max_time_ms(&self) -> u64 {
        // Try deadline first
        if let Some(remaining) = self.remaining_ms() {
            return remaining.min(30_000); // Cap at 30s
        }
        
        // Fallback to legacy max_time_ms
        if self.max_time_ms > 0 {
            return self.max_time_ms;
        }
        
        // Default based on task type
        self.task_type_enum()
            .map(|t| t.default_budget_ms())
            .unwrap_or(5000)
    }
}

/// NCL Task Result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NCLTaskResult {
    pub task_id: String,
    pub result_hash: String,
    pub compute_time_ms: u64,
    pub success: bool,
}

/// NCL Client Configuration
#[derive(Debug, Clone)]
pub struct NCLConfig {
    /// Enable NCL (AI bonus)
    pub enabled: bool,
    /// Time allocation for AI (0.0 - 0.5)
    pub allocation: f32,
    /// NPU type to use
    pub npu_type: NpuType,
    /// Minimum task interval (ms)
    pub min_task_interval_ms: u64,
}

impl Default for NCLConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            allocation: 0.4,  // 40% time for AI (Targeting 20% Revenue)
            npu_type: NpuType::detect(),
            min_task_interval_ms: 1000,
        }
    }
}

/// NCL Client - handles communication with pool for AI tasks
pub struct NCLClient {
    config: NCLConfig,
    /// Session ID from stratum
    session_id: Arc<RwLock<Option<String>>>,
    /// Current pending task
    current_task: Arc<RwLock<Option<NCLTask>>>,
    /// Total NCL bonus earned
    total_bonus: Arc<RwLock<f64>>,
    /// Tasks completed
    tasks_completed: Arc<RwLock<u64>>,
    /// Last task fetch time
    last_fetch: Arc<RwLock<Instant>>,
    /// Registered with pool
    registered: Arc<RwLock<bool>>,
}

impl NCLClient {
    pub fn new(config: NCLConfig) -> Self {
        Self {
            config,
            session_id: Arc::new(RwLock::new(None)),
            current_task: Arc::new(RwLock::new(None)),
            total_bonus: Arc::new(RwLock::new(0.0)),
            tasks_completed: Arc::new(RwLock::new(0)),
            last_fetch: Arc::new(RwLock::new(Instant::now())),
            registered: Arc::new(RwLock::new(false)),
        }
    }
    
    /// Set session ID (from stratum login)
    pub async fn set_session_id(&self, session_id: String) {
        *self.session_id.write().await = Some(session_id);
    }
    
    /// Check if NCL is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }
    
    /// Get current allocation
    pub fn allocation(&self) -> f32 {
        self.config.allocation
    }
    
    /// Get NPU type
    pub fn npu_type(&self) -> NpuType {
        self.config.npu_type
    }
    
    /// Build NCL register message for stratum
    pub fn build_register_message(&self, id: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "jsonrpc": "2.0",
            "method": "ncl.register",
            "params": {
                "version": NCL_PROTOCOL_VERSION,
                "npu_type": self.config.npu_type.as_str(),
                "npu_tflops": self.config.npu_type.estimated_tflops(),
                "allocation": self.config.allocation,
                "supported_task_types": [
                    "hash_chaining_v1",
                    "embedding",
                    "llm_inference",
                    "image_classification"
                ]
            }
        })
    }
    
    /// Build NCL status request message
    pub fn build_status_message(&self, id: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "jsonrpc": "2.0",
            "method": "ncl.status",
            "params": {}
        })
    }
    
    /// Build NCL task request message
    pub fn build_get_task_message(&self, id: u64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "jsonrpc": "2.0",
            "method": "ncl.get_task",
            "params": {}
        })
    }
    
    /// Build NCL submit message
    pub fn build_submit_message(&self, id: u64, result: &NCLTaskResult, base_reward: f64) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "jsonrpc": "2.0",
            "method": "ncl.submit",
            "params": {
                "version": NCL_PROTOCOL_VERSION,
                "task_id": result.task_id,
                // Pool v1 expects `result`, but older clients used `result_hash`.
                "result": result.result_hash,
                "result_hash": result.result_hash,
                "compute_time_ms": result.compute_time_ms,
                "base_reward": base_reward
            }
        })
    }

    /// Build submit message for pool v1 hash_chaining task.
    pub fn build_submit_hash_chain_message(&self, id: u64, task_id: &str, result_hex: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "jsonrpc": "2.0",
            "method": "ncl.submit",
            "params": {
                "version": NCL_PROTOCOL_VERSION,
                "task_id": task_id,
                "result": result_hex,
                "result_hash": result_hex
            }
        })
    }

    /// Compute the pool v1 deterministic verification: blake3(seed) repeated `rounds`.
    pub async fn compute_blake3_chain(&self, seed_hex: &str, rounds: u32) -> Result<String> {
        let seed_hex = normalize_hex(seed_hex);
        let seed_bytes = hex::decode(seed_hex)?;
        if seed_bytes.len() != 32 {
            anyhow::bail!("seed must be 32 bytes (hex)");
        }

        let mut seed = [0u8; 32];
        seed.copy_from_slice(&seed_bytes);

        let out = tokio::task::spawn_blocking(move || {
            let mut state = seed;
            for _ in 0..rounds {
                let h = blake3::hash(&state);
                state.copy_from_slice(h.as_bytes());
                std::hint::black_box(state);
            }
            hex::encode(state)
        })
        .await?;

        Ok(out)
    }

    pub fn min_task_interval_ms(&self) -> u64 {
        self.config.min_task_interval_ms
    }
    
    /// Process a received task
    pub async fn set_task(&self, task: NCLTask) {
        info!("📥 Received NCL task: {} ({})", task.task_id, task.task_type);
        *self.current_task.write().await = Some(task);
    }
    
    /// Get current task if any
    pub async fn get_current_task(&self) -> Option<NCLTask> {
        self.current_task.read().await.clone()
    }
    
    /// Clear current task
    pub async fn clear_task(&self) {
        *self.current_task.write().await = None;
    }
    
    /// Execute AI task.
    ///
    /// Current implementation uses a real CPU-bound compute loop (blake3 chaining)
    /// executed via `spawn_blocking` to avoid blocking the async runtime.
    ///
    /// For non-CPU NPU types we currently fall back to CPU compute (backend wiring
    /// for CoreML/TensorRT/OpenVINO/etc can be added later behind feature flags).
    pub async fn execute_task(&self, task: &NCLTask) -> Result<NCLTaskResult> {
        let start = Instant::now();

        info!(
            "🧠 Executing NCL task: {} (type: {}, model: {})",
            task.task_id,
            task.task_type,
            task.model
        );

        if task.max_time_ms == 0 {
            return Ok(NCLTaskResult {
                task_id: task.task_id.clone(),
                result_hash: String::new(),
                compute_time_ms: 0,
                success: false,
            });
        }

        // Deterministic mode (verifiable):
        // If input_data is a JSON string containing {"mode":"deterministic","iterations":N},
        // we execute exactly N rounds of the chaining algorithm.
        let deterministic_iterations: Option<u64> = serde_json::from_str::<serde_json::Value>(&task.input_data)
            .ok()
            .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(|m| m.to_string()).zip(v.get("iterations").and_then(|i| i.as_u64())))
            .and_then(|(mode, iters)| if mode == "deterministic" { Some(iters) } else { None });

        // Time-budget mode (best-effort compute), capped by task.max_time_ms.
        let default_budget_ms: u64 = match task.task_type.as_str() {
            "embeddings" => 50,
            "llm_inference" => 500,
            "image_classification" => 100,
            _ => 100,
        };
        let budget_ms = task.max_time_ms.min(default_budget_ms).max(1);

        let npu_type = self.config.npu_type;
        if npu_type != NpuType::Cpu && npu_type != NpuType::Onnx {
            warn!(
                "NCL backend {:?} not wired yet; falling back to CPU compute",
                npu_type
            );
        }

        let task_id = task.task_id.clone();
        let task_type = task.task_type.clone();
        let input_data = task.input_data.clone();
        let model = task.model.clone();

        let result_hash = tokio::task::spawn_blocking(move || {
            // Deterministic seed derived from task fields.
            let mut state = blake3::hash(format!("{}:{}:{}", task_id, task_type, model).as_bytes());

            if let Some(iters) = deterministic_iterations {
                let iters = iters.min(5_000_000).max(1);
                for counter in 0..iters {
                    let mut hasher = blake3::Hasher::new();
                    hasher.update(state.as_bytes());
                    hasher.update(input_data.as_bytes());
                    hasher.update(model.as_bytes());
                    hasher.update(&counter.to_le_bytes());
                    state = hasher.finalize();
                    std::hint::black_box(state);
                }
                return hex::encode(state.as_bytes());
            }

            // Time-budget loop.
            let deadline = Instant::now() + Duration::from_millis(budget_ms);
            let mut counter: u64 = 0;
            while Instant::now() < deadline {
                let mut hasher = blake3::Hasher::new();
                hasher.update(state.as_bytes());
                hasher.update(input_data.as_bytes());
                hasher.update(model.as_bytes());
                hasher.update(&counter.to_le_bytes());
                state = hasher.finalize();
                counter = counter.wrapping_add(1);
                std::hint::black_box(state);
            }

            hex::encode(state.as_bytes())
        })
        .await?;

        let elapsed_ms = start.elapsed().as_millis() as u64;
        Ok(NCLTaskResult {
            task_id: task.task_id.clone(),
            result_hash,
            compute_time_ms: elapsed_ms,
            success: true,
        })
    }
    
    /// Record bonus received
    pub async fn record_bonus(&self, bonus: f64) {
        *self.total_bonus.write().await += bonus;
        *self.tasks_completed.write().await += 1;
    }
    
    /// Get statistics
    pub async fn get_stats(&self) -> NCLStats {
        NCLStats {
            enabled: self.config.enabled,
            npu_type: self.config.npu_type,
            allocation: self.config.allocation,
            total_bonus: *self.total_bonus.read().await,
            tasks_completed: *self.tasks_completed.read().await,
            registered: *self.registered.read().await,
        }
    }
    
    /// Mark as registered with pool
    pub async fn set_registered(&self, registered: bool) {
        *self.registered.write().await = registered;
    }
    
    /// Check if time for next AI cycle (based on allocation)
    pub fn should_do_ai_cycle(&self, mining_cycle_count: u64) -> bool {
        if !self.config.enabled {
            return false;
        }
        
        // allocation 0.3 = every ~3rd cycle is AI
        let ai_frequency = (1.0 / self.config.allocation) as u64;
        mining_cycle_count % ai_frequency == 0
    }
}

/// NCL Statistics
#[derive(Debug, Clone, Serialize)]
pub struct NCLStats {
    pub enabled: bool,
    pub npu_type: NpuType,
    pub allocation: f32,
    pub total_bonus: f64,
    pub tasks_completed: u64,
    pub registered: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_npu_detection() {
        let npu = NpuType::detect();
        assert!(npu.estimated_tflops() > 0.0);
    }
    
    #[test]
    fn test_ncl_client_creation() {
        let client = NCLClient::new(NCLConfig::default());
        assert!(client.is_enabled());
        assert_eq!(client.allocation(), 0.4); // 40% time for AI (20% Revenue target)
    }
    
    #[test]
    fn test_register_message() {
        let client = NCLClient::new(NCLConfig::default());
        let msg = client.build_register_message(1);
        assert_eq!(msg["method"], "ncl.register");
    }
    
    #[tokio::test]
    async fn test_task_execution() {
        let client = NCLClient::new(NCLConfig::default());
        let task = NCLTask {
            version: NCL_PROTOCOL_VERSION.to_string(),
            task_id: "test-123".to_string(),
            task_type: "embedding".to_string(),
            payload: serde_json::json!({}),
            deadline_ms: 0,
            created_at: current_timestamp_ms(),
            reward: NclReward::default(),
            verification: None,
            retry_policy: NclRetryPolicy::default(),
            // Legacy fields
            input_data: "test input".to_string(),
            model: "test-model".to_string(),
            max_time_ms: 25,
            reward_multiplier: 1.0,
        };
        
        let result = client.execute_task(&task).await.unwrap();
        assert!(result.success);
        assert_eq!(result.task_id, "test-123");
        assert!(!result.result_hash.is_empty());
        assert!(result.compute_time_ms > 0);
    }
}
