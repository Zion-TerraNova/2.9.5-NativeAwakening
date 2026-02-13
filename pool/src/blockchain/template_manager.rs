/// Block Template Manager - Fetch and manage block templates from ZION Core
/// 
/// Periodically fetches new block templates and notifies miners

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time;

use super::rpc_client::ZionRPCClient;
use crate::metrics::prometheus as metrics;

/// Callback type for template changes
pub type TemplateChangeCallback = Arc<dyn Fn(BlockTemplate) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockTemplate {
    pub version: u32,
    pub height: u64,
    pub difficulty: u64,
    pub prev_hash: String,
    pub target: String,
    #[serde(default)]
    pub target_u32: Option<String>,
    #[serde(default)]
    pub target_u128: Option<String>,
    pub reward_atomic: u64,
    pub timestamp: u64,
    pub blob: Option<String>,
}

impl BlockTemplate {
    /// Parse from RPC response
    pub fn from_rpc_response(value: &serde_json::Value) -> Result<Self> {
        Ok(Self {
            version: value.get("version")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32,
            height: value.get("height")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            difficulty: value.get("difficulty")
                .and_then(|v| v.as_u64())
                .unwrap_or(10000),
            prev_hash: value.get("prev_hash")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            target: value.get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            target_u32: value
                .get("target_u32")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            target_u128: value
                .get("target_u128")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            reward_atomic: value.get("reward_atomic")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            blob: value.get("blob")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        })
    }
}

pub struct BlockTemplateManager {
    rpc_client: Arc<ZionRPCClient>,
    pool_wallet: String,
    update_interval: Duration,
    current_template: Arc<RwLock<Option<BlockTemplate>>>,
    on_change: Option<TemplateChangeCallback>,
}

impl BlockTemplateManager {
    /// Create new BlockTemplateManager
    pub fn new(
        rpc_client: Arc<ZionRPCClient>,
        pool_wallet: String,
        update_interval: Option<Duration>,
    ) -> Self {
        let interval = update_interval.unwrap_or(Duration::from_secs(10));

        tracing::info!(
            "BlockTemplateManager: wallet={}, interval={}s",
            pool_wallet,
            interval.as_secs()
        );

        Self {
            rpc_client,
            pool_wallet,
            update_interval: interval,
            current_template: Arc::new(RwLock::new(None)),
            on_change: None,
        }
    }

    /// Register callback for template changes
    pub fn on_template_change<F>(&mut self, callback: F)
    where
        F: Fn(BlockTemplate) + Send + Sync + 'static,
    {
        self.on_change = Some(Arc::new(callback));
    }

    /// Start template update loop
    pub async fn start(&self) {
        let rpc_client = self.rpc_client.clone();
        let pool_wallet = self.pool_wallet.clone();
        let current_template = self.current_template.clone();
        let update_interval = self.update_interval;
        let on_change = self.on_change.clone();

        tokio::spawn(async move {
            let mut interval = time::interval(update_interval);
            
            loop {
                interval.tick().await;

                match Self::fetch_template(&rpc_client, &pool_wallet).await {
                    Ok(template) => {
                        let height = template.height;
                        let mut current = current_template.write().await;
                        
                        // Check if template changed
                        let changed = current.as_ref()
                            .map(|t| t.height != height || t.prev_hash != template.prev_hash)
                            .unwrap_or(true);

                        if changed {
                            tracing::info!(
                                "📋 New block template: height={}, difficulty={}, prev_hash={}",
                                height,
                                template.difficulty,
                                &template.prev_hash[..16]
                            );
                            metrics::set_template_height(height);
                            metrics::inc_template_updates();
                            *current = Some(template.clone());

                            // Trigger callback if registered
                            if let Some(callback) = &on_change {
                                callback(template);
                            }
                        }
                    }
                    Err(e) => {
                        metrics::inc_template_fetch_errors();
                        tracing::error!("Failed to fetch block template: {}", e);
                    }
                }
            }
        });
    }

    /// Fetch new block template from RPC
    async fn fetch_template(
        rpc_client: &ZionRPCClient,
        pool_wallet: &str,
    ) -> Result<BlockTemplate> {
        let response = rpc_client.get_block_template(pool_wallet).await?;
        BlockTemplate::from_rpc_response(&response)
    }

    /// Get current block template
    pub async fn get_template(&self) -> Option<BlockTemplate> {
        self.current_template.read().await.clone()
    }

    /// Force update template immediately
    pub async fn force_update(&self) -> Result<BlockTemplate> {
        let template = Self::fetch_template(&self.rpc_client, &self.pool_wallet).await?;
        
        let mut current = self.current_template.write().await;
        *current = Some(template.clone());

        metrics::set_template_height(template.height);
        
        tracing::info!(
            "📋 Forced template update: height={}, difficulty={}",
            template.height,
            template.difficulty
        );

        Ok(template)
    }

    /// Check if template is stale (older than 2x update interval)
    pub async fn is_stale(&self) -> bool {
        let template = self.current_template.read().await;
        
        match template.as_ref() {
            None => true,
            Some(t) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                
                let age = now.saturating_sub(t.timestamp);
                age > (self.update_interval.as_secs() * 2)
            }
        }
    }

    /// Get current height
    pub async fn get_height(&self) -> Option<u64> {
        self.current_template.read().await.as_ref().map(|t| t.height)
    }

    /// Get current difficulty
    pub async fn get_difficulty(&self) -> Option<u64> {
        self.current_template.read().await.as_ref().map(|t| t.difficulty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_template_parsing() {
        let json = serde_json::json!({
            "version": 1,
            "height": 12345,
            "difficulty": 100000,
            "prev_hash": "abc123",
            "target": "0000ffff",
            "reward_atomic": 50000000000u64,
        });

        let template = BlockTemplate::from_rpc_response(&json).unwrap();
        
        assert_eq!(template.version, 1);
        assert_eq!(template.height, 12345);
        assert_eq!(template.difficulty, 100000);
        assert_eq!(template.prev_hash, "abc123");
        assert_eq!(template.target, "0000ffff");
        assert_eq!(template.reward_atomic, 50000000000);
    }

    #[tokio::test]
    async fn test_template_manager_creation() {
        let rpc_client = Arc::new(ZionRPCClient::new(
            "127.0.0.1".to_string(),
            18081,
            None,
            None,
            None,
            None,
        ));

        let manager = BlockTemplateManager::new(
            rpc_client,
            "ZION_TEST_WALLET".to_string(),
            Some(Duration::from_secs(5)),
        );

        assert_eq!(manager.pool_wallet, "ZION_TEST_WALLET");
        assert_eq!(manager.update_interval, Duration::from_secs(5));
        
        // Initially no template
        assert!(manager.get_template().await.is_none());
    }
}
