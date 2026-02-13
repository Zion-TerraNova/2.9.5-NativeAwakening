/// Mining shares module
/// 
/// Handles share validation, duplicate detection, and persistence
/// Mirrors Python src/pool/mining/share_validator.py implementation

pub mod validator;
pub mod storage;
pub mod processor;

pub use validator::{ShareResult, ShareValidator, SubmittedShare, Algorithm};
pub use storage::{RedisStorage, StoredShare, MinerStats, BlockFound};
pub use processor::{ProcessedShareOutcome, ShareProcessor};

#[cfg(test)]
mod tests {}

