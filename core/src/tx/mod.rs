use serde::{Deserialize, Serialize};
use crate::crypto::{hash, keys, to_hex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxInput {
    pub prev_tx_hash: String,
    pub output_index: u32,
    pub signature: String, // Hex encoded 64-byte Ed25519 signature
    pub public_key: String, // Hex encoded 32-byte Ed25519 public key
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxOutput {
    pub amount: u64,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String, // Hash of the tx
    pub version: u32,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub fee: u64,
    pub timestamp: u64,
}

impl Transaction {
    pub fn new() -> Self {
        Self {
            id: String::new(),
            version: 1,
            inputs: vec![],
            outputs: vec![],
            fee: 0,
            timestamp: 0,
        }
    }

    pub fn calculate_hash(&self) -> String {
        let mut data = Vec::new();
        data.extend_from_slice(&self.version.to_le_bytes());
        for input in &self.inputs {
            data.extend_from_slice(input.prev_tx_hash.as_bytes());
            data.extend_from_slice(&input.output_index.to_le_bytes());
            // Exclude signature from ID for now (simplified SegWit-style or just Mutable ID)
            // If we want ID to be immutable once signed, we must hash what is signed (inputs+outputs etc)
            // But if we hash the signature, the ID changes when we sign.
            // Standard Bitcoin: ID = Hash(SignedTx). This causes malleability.
            // Zion V1: Simple ID = Hash(Fields without signature).
            data.extend_from_slice(input.public_key.as_bytes());
        }
        for output in &self.outputs {
            data.extend_from_slice(&output.amount.to_le_bytes());
            data.extend_from_slice(output.address.as_bytes());
        }
        data.extend_from_slice(&self.fee.to_le_bytes());
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        
        to_hex(&hash::blake(&data))
    }

    pub fn verify_signatures(&self) -> bool {
        // The message being signed is the Transaction Hash (ID).
        // Since ID excludes signatures, it is safe to calculate it.
        // However, if we simply use self.id, we rely on it being correct.
        // Better to re-calculate.
        let msg_hash_hex = self.calculate_hash();
        if self.id != msg_hash_hex {
            return false;
        }

        let msg_bytes = match keys::from_hex(&msg_hash_hex) {
            Some(b) => b,
            None => return false,
        };

        for input in &self.inputs {
            let pk_bytes = match keys::from_hex(&input.public_key) {
                Some(b) => b,
                None => return false,
            };
            let sig_bytes = match keys::from_hex(&input.signature) {
                Some(b) => b,
                None => return false,
            };

            if !keys::verify(&pk_bytes, &msg_bytes, &sig_bytes) {
                return false;
            }
        }
        true
    }
}

