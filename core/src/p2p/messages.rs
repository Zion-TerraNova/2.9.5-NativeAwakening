use serde::{Deserialize, Serialize};

/// Default block batch limit when not specified by peer.
fn default_block_limit() -> u32 {
    10
}

/// IBD batch limit — larger batches for initial sync.
fn default_ibd_limit() -> u32 {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    Handshake { version: u32, agent: String, height: u64, #[serde(default)] network: String, #[serde(default)] nonce: u64 },
    HandshakeAck { version: u32, height: u64, #[serde(default)] nonce: u64 },
    
    // Gossip
    NewBlock { height: u64, hash: String },
    NewTx { id: String },
    
    // Sync Blocks
    GetBlocks { from_height: u64, #[serde(default = "default_block_limit")] limit: u32 },
    Blocks { blocks: Vec<crate::blockchain::block::Block> },

    // IBD (Initial Block Download) — large batch sync
    GetBlocksIBD { from_height: u64, #[serde(default = "default_ibd_limit")] limit: u32 },
    BlocksIBD { blocks: Vec<crate::blockchain::block::Block>, remaining: u64 },
    
    // Sync Txs
    GetTx { id: String },
    Tx { transaction: crate::tx::Transaction },

    GetTip,
    Tip { height: u64, hash: String },
}
