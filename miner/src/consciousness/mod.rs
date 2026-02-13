mod levels;

pub use levels::{ConsciousnessLevel, ConsciousnessProfile};

use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ConsciousnessTracker {
    profile: Arc<RwLock<ConsciousnessProfile>>,
}

impl ConsciousnessTracker {
    pub fn new(miner_address: String) -> Self {
        Self {
            profile: Arc::new(RwLock::new(ConsciousnessProfile::new(miner_address))),
        }
    }

    pub async fn add_xp(&self, amount: u64) {
        let mut profile = self.profile.write().await;
        profile.add_xp(amount);
    }

    pub async fn get_level(&self) -> ConsciousnessLevel {
        let profile = self.profile.read().await;
        profile.level
    }

    pub async fn get_profile(&self) -> ConsciousnessProfile {
        let profile = self.profile.read().await;
        profile.clone()
    }
}
