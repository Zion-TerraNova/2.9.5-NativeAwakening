/// Blockchain communication module — Clean L1
/// Handles RPC calls to ZION Core and reward calculations

pub mod rpc_client;
pub mod reward_calculator;
pub mod template_manager;

pub use rpc_client::ZionRPCClient;
pub use reward_calculator::RewardCalculator;
pub use template_manager::{BlockTemplate, BlockTemplateManager};
