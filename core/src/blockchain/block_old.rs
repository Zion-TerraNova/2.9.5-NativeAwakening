use serde::{Deserialize, Serialize};
use crate::crypto::{hash, to_hex};
use crate::tx::Transaction;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub version: u32,
    pub height: u64,
    pub prev_hash: String,
    pub merkle_root: String,
    pub timestamp: u64,
    pub difficulty: u64,
    pub nonce: u64,
    pub transactions: Vec<Transaction>,
}

impl Block {
    pub fn new(
        version: u32,
        height: u64,
        prev_hash: String,
        timestamp: u64,
        difficulty: u64,
        nonce: u64,
        transactions: Vec<Transaction>
    ) -> Self {
        let merkle_root = Self::calculate_merkle_root(&transactions);
        Self {
            version,
            height,
            prev_hash,
            merkle_root,
            timestamp,
            difficulty,
            nonce,
            transactions,
        }
    }

    pub fn calculate_hash(&self) -> String {
        let mut data = Vec::new();
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&self.height.to_le_bytes());
        data.extend_from_slice(self.prev_hash.as_bytes());
        data.extend_from_slice(self.merkle_root.as_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&self.difficulty.to_le_bytes());
        data.extend_from_slice(&self.nonce.to_le_bytes());
        to_hex(&hash::blake(&data))
    }

    pub fn calculate_merkle_root(txs: &[Transaction]) -> String {
        if txs.is_empty() {
             return to_hex(&[0u8; 32]);
        }
        let mut hashes: Vec<Vec<u8>> = txs.iter()
            .map(|tx| crate::crypto::keys::from_hex(&tx.id).unwrap_or(vec![0u8; 32]))
            .collect();

        while hashes.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in hashes.chunks(2) {
                let mut combined = chunk[0].clone();
                if chunk.len() > 1 {
                    combined.extend_from_slice(&chunk[1]);
                } else {
                    combined.extend_from_slice(&chunk[0]); // Duplicate last if odd
                }
                next_level.push(hash::blake(&combined).to_vec());
            }
            hashes = next_level;
        }
        to_hex(&hashes[0])
    }
}
