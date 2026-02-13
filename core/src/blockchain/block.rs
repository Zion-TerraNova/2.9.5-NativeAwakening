use serde::{Deserialize, Serialize};
use crate::crypto::{hash, to_hex};
use crate::tx::Transaction;
use crate::algorithms;

/// CHv3 fork height — hardcoded consensus constant.
/// CHv3 is active from genesis on all networks.
/// AUDIT-FIX P0-05: Must never be read from environment variable.
pub const CH_V3_FORK_HEIGHT: u64 = 0;

/// Mining algorithm identifier
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Algorithm {
    CosmicHarmony = 0,
    Blake3 = 1,
    RandomX = 2,
    Yescrypt = 3,
}

impl Algorithm {
    pub fn from_height(_height: u64) -> Self {
        // TESTNET: Use only Cosmic Harmony for all heights
        // TODO: Restore rotation for mainnet
        Algorithm::CosmicHarmony
        /*
        match height % 4 {
            0 => Algorithm::CosmicHarmony,
            1 => Algorithm::Blake3,
            2 => Algorithm::RandomX,
            3 => Algorithm::Yescrypt,
            _ => unreachable!(),
        }
        */
    }
    
    pub fn name(&self) -> &'static str {
        match self {
            Algorithm::CosmicHarmony => "cosmic_harmony",
            Algorithm::Blake3 => "blake3",
            Algorithm::RandomX => "randomx",
            Algorithm::Yescrypt => "yescrypt",
        }
    }
}

/// Block header (used for PoW calculation)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockHeader {
    pub version: u32,
    pub height: u64,
    pub prev_hash: String,
    pub merkle_root: String,
    pub timestamp: u64,
    pub difficulty: u64,
    pub nonce: u64,
    pub algorithm: Algorithm,
}

impl BlockHeader {
    pub fn calculate_hash(&self) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&self.version.to_le_bytes());
        data.extend_from_slice(&self.height.to_le_bytes());
        
        // Prev hash and merkle root must be padded to 64 bytes to match template blob format
        let prev_hash_bytes = self.prev_hash.as_bytes();
        let merkle_bytes = self.merkle_root.as_bytes();
        
        let mut prev_buf = [0u8; 64];
        let mut merkle_buf = [0u8; 64];
        
        let prev_len = prev_hash_bytes.len().min(64);
        let merkle_len = merkle_bytes.len().min(64);
        
        prev_buf[..prev_len].copy_from_slice(&prev_hash_bytes[..prev_len]);
        merkle_buf[..merkle_len].copy_from_slice(&merkle_bytes[..merkle_len]);
        
        data.extend_from_slice(&prev_buf);
        data.extend_from_slice(&merkle_buf);
        
        data.extend_from_slice(&self.timestamp.to_le_bytes());
        data.extend_from_slice(&self.difficulty.to_le_bytes());
        data.extend_from_slice(&self.nonce.to_le_bytes());
        
        // Use algorithm-specific hash
        match self.algorithm {
            Algorithm::CosmicHarmony => {
                // TestNet fork: move Cosmic Harmony PoW to CHv3 for blocks >= fork height.
                // Pool + native miner already mine CHv3 (see pool ShareValidator).
                if self.height >= CH_V3_FORK_HEIGHT {
                    // Recreate the canonical template blob bytes and hash it the same way the pool does:
                    // CHv3 uses the first 80 bytes of the blob as header, and appends the nonce.
                    let blob_hex = Block::build_template_blob(
                        self.version,
                        self.height,
                        &self.prev_hash,
                        &self.merkle_root,
                        self.timestamp,
                        self.difficulty,
                    );
                    let blob = hex::decode(blob_hex).unwrap_or_default();
                    let h = zion_cosmic_harmony_v3::algorithms_opt::cosmic_harmony_v3(
                        &blob,
                        self.nonce,
                    );
                    h.data.to_vec()
                } else {
                    algorithms::cosmic_harmony::hash(&data, self.nonce, self.height)
                }
            }
            Algorithm::Blake3 => algorithms::blake3::hash(&data).to_vec(),
            Algorithm::RandomX => algorithms::randomx::hash(&data, &self.height.to_le_bytes()),
            Algorithm::Yescrypt => algorithms::yescrypt::hash(&data, &self.height.to_le_bytes()),
        }
    }
    
    pub fn meets_target(&self, target: &[u8; 32]) -> bool {
        let hash = self.calculate_hash();
        let mut hash_padded = [0u8; 32];
        let copy_len = hash.len().min(32);
        hash_padded[..copy_len].copy_from_slice(&hash[..copy_len]);
        
        // Compare as big-endian (most significant bytes first)
        for i in 0..32 {
            if hash_padded[i] < target[i] {
                return true;
            } else if hash_padded[i] > target[i] {
                return false;
            }
        }
        true // Equal is valid
    }
}

/// Complete block (header + transactions)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub header: BlockHeader,
    pub transactions: Vec<Transaction>,
}

impl Block {
    /// Build template blob (header without nonce) for miners
    /// Format: version(4) + height(8) + prev_hash(64) + merkle_root(64) + timestamp(8) + difficulty(8) + algo(1) + nonce_reserved(8)
    pub fn build_template_blob(
        version: u32,
        height: u64,
        prev_hash: &str,
        merkle_root: &str,
        timestamp: u64,
        difficulty: u64,
    ) -> String {
        let mut data = Vec::new();
        
        // Header fields (all in little-endian for consistency)
        data.extend_from_slice(&version.to_le_bytes());
        data.extend_from_slice(&height.to_le_bytes());
        
        // Prev hash and merkle root as ASCII hex strings (64 chars each)
        let prev_hash_bytes = prev_hash.as_bytes();
        let merkle_bytes = merkle_root.as_bytes();
        
        // Pad to 64 bytes if needed
        let mut prev_buf = [0u8; 64];
        let mut merkle_buf = [0u8; 64];
        
        let prev_len = prev_hash_bytes.len().min(64);
        let merkle_len = merkle_bytes.len().min(64);
        
        prev_buf[..prev_len].copy_from_slice(&prev_hash_bytes[..prev_len]);
        merkle_buf[..merkle_len].copy_from_slice(&merkle_bytes[..merkle_len]);
        
        data.extend_from_slice(&prev_buf);
        data.extend_from_slice(&merkle_buf);
        
        data.extend_from_slice(&timestamp.to_le_bytes());
        data.extend_from_slice(&difficulty.to_le_bytes());
        
        // Algorithm byte
        let algo = Algorithm::from_height(height);
        data.push(algo as u8);
        
        // Nonce placeholder (8 bytes reserved for miner)
        data.extend_from_slice(&[0u8; 8]);
        
        to_hex(&data)
    }
    
    /// Parse template blob and nonce to create block header
    pub fn from_template_blob(blob_hex: &str, nonce: u64) -> Result<BlockHeader, String> {
        let blob = match hex::decode(blob_hex) {
            Ok(b) => b,
            Err(e) => return Err(format!("Invalid blob hex: {}", e)),
        };
        
        // Expected size: 4 + 8 + 64 + 64 + 8 + 8 + 1 + 8 = 165 bytes
        if blob.len() < 165 {
            return Err(format!("Blob too short: {} bytes (expected 165)", blob.len()));
        }
        
        let version = u32::from_le_bytes([blob[0], blob[1], blob[2], blob[3]]);
        let height = u64::from_le_bytes([
            blob[4], blob[5], blob[6], blob[7],
            blob[8], blob[9], blob[10], blob[11],
        ]);
        
        let prev_hash = String::from_utf8_lossy(&blob[12..76]).trim_end_matches('\0').to_string();
        let merkle_root = String::from_utf8_lossy(&blob[76..140]).trim_end_matches('\0').to_string();
        
        let timestamp = u64::from_le_bytes([
            blob[140], blob[141], blob[142], blob[143],
            blob[144], blob[145], blob[146], blob[147],
        ]);
        
        let difficulty = u64::from_le_bytes([
            blob[148], blob[149], blob[150], blob[151],
            blob[152], blob[153], blob[154], blob[155],
        ]);
        
        let algo_byte = blob[156];
        let algorithm = match algo_byte {
            0 => Algorithm::CosmicHarmony,
            1 => Algorithm::Blake3,
            2 => Algorithm::RandomX,
            3 => Algorithm::Yescrypt,
            _ => return Err(format!("Unknown algorithm byte: {}", algo_byte)),
        };
        
        Ok(BlockHeader {
            version,
            height,
            prev_hash,
            merkle_root,
            timestamp,
            difficulty,
            nonce,
            algorithm,
        })
    }
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
        let algorithm = Algorithm::from_height(height);
        
        Self {
            header: BlockHeader {
                version,
                height,
                prev_hash,
                merkle_root,
                timestamp,
                difficulty,
                nonce,
                algorithm,
            },
            transactions,
        }
    }
    
    /// The genesis message — permanently inscribed in block 0's coinbase,
    /// just as Satoshi embedded "The Times 03/Jan/2009 …" in Bitcoin's genesis.
    /// Contains ASCII art Tree of Life, ZION logo, and a dedication.
    /// Loaded at compile time from `GENESIS_MESSAGE.txt` (~4.5 KB).
    pub const GENESIS_MESSAGE: &'static str =
        include_str!("GENESIS_MESSAGE.txt");

    pub fn genesis() -> Self {
        // P1-04: Use proper genesis timestamp from network config
        // instead of 0. This ensures all nodes produce the same genesis hash.
        let genesis_ts = crate::network::get_network().genesis_timestamp();

        // Coinbase input carries the genesis message in its signature field
        // (Bitcoin-style: scriptSig of the coinbase input).
        let coinbase_input = crate::tx::TxInput {
            prev_tx_hash: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            output_index: 0xFFFFFFFF,
            signature: hex::encode(Self::GENESIS_MESSAGE.as_bytes()),
            public_key: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        };

        let genesis_tx = Transaction {
            id: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            version: 1,
            inputs: vec![coinbase_input],
            outputs: vec![],
            fee: 0,
            timestamp: genesis_ts,
        };
        
        Self::new(
            1,
            0,
            "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            genesis_ts,
            1000,
            0,
            vec![genesis_tx]
        )
    }

    pub fn calculate_hash(&self) -> String {
        to_hex(&self.header.calculate_hash())
    }
    
    pub fn calculate_hash_bytes(&self) -> Vec<u8> {
        self.header.calculate_hash()
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
    
    // Convenience accessors (backward compatibility)
    pub fn version(&self) -> u32 { self.header.version }
    pub fn height(&self) -> u64 { self.header.height }
    pub fn prev_hash(&self) -> &str { &self.header.prev_hash }
    pub fn merkle_root(&self) -> &str { &self.header.merkle_root }
    pub fn timestamp(&self) -> u64 { self.header.timestamp }
    pub fn difficulty(&self) -> u64 { self.header.difficulty }
    pub fn nonce(&self) -> u64 { self.header.nonce }
    pub fn algorithm(&self) -> Algorithm { self.header.algorithm }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_algorithm_from_height() {
        // TESTNET: algorithm rotation is currently disabled.
        assert_eq!(Algorithm::from_height(0), Algorithm::CosmicHarmony);
        assert_eq!(Algorithm::from_height(1), Algorithm::CosmicHarmony);
        assert_eq!(Algorithm::from_height(2), Algorithm::CosmicHarmony);
        assert_eq!(Algorithm::from_height(3), Algorithm::CosmicHarmony);
        assert_eq!(Algorithm::from_height(4), Algorithm::CosmicHarmony);
    }
    
    #[test]
    fn test_genesis_block() {
        let genesis = Block::genesis();
        assert_eq!(genesis.height(), 0);
        assert_eq!(genesis.version(), 1);
        assert_eq!(genesis.transactions.len(), 1);
    }
    
    #[test]
    fn test_genesis_message_in_coinbase() {
        let genesis = Block::genesis();
        let coinbase = &genesis.transactions[0];
        
        // Genesis coinbase must have exactly one input carrying the message
        assert_eq!(coinbase.inputs.len(), 1, "Genesis coinbase must have one input");
        
        let input = &coinbase.inputs[0];
        assert_eq!(input.output_index, 0xFFFFFFFF, "Coinbase output_index must be 0xFFFFFFFF");
        
        // Decode the message from hex-encoded signature field
        let msg_bytes = hex::decode(&input.signature).expect("Signature must be valid hex");
        let msg = String::from_utf8(msg_bytes).expect("Message must be valid UTF-8");
        
        assert!(msg.contains("Sarah Issobel"), "Message must mention Sarah Issobel");
        assert!(msg.contains("Maitreya Buddha"), "Message must mention Maitreya Buddha");
        assert!(msg.contains("Radha"), "Message must mention Radha");
        assert!(msg.contains("Sita"), "Message must mention Sita");
        assert!(msg.contains("Freedom Humanity"), "Message must mention Freedom Humanity");
        assert!(msg.contains("ZION is yours"), "Message must contain 'ZION is yours'");
        assert!(msg.contains("Golden Age"), "Message must mention Golden Age");
        assert!(msg.contains("Yose"), "Message must mention Yose");
        assert!(msg.contains("Hiranyagarbha"), "Message must mention Hiranyagarbha");
        assert!(msg.contains("Kalki"), "Message must mention Kalki");
        assert!(msg.contains("████"), "Message must contain ZION ASCII logo");
        
        // Verify total size is reasonable (ASCII art + message)
        assert!(msg.len() > 1000, "Genesis message must include ASCII art (got {} bytes)", msg.len());
        assert!(msg.len() < 10000, "Genesis message must be under 10KB (got {} bytes)", msg.len());
        
        // Verify it matches the constant exactly
        assert_eq!(msg, Block::GENESIS_MESSAGE);
        
        println!("✅ Genesis message: {}", msg);
    }
    
    #[test]
    fn test_genesis_hash_deterministic() {
        let g1 = Block::genesis();
        let g2 = Block::genesis();
        assert_eq!(g1.calculate_hash(), g2.calculate_hash(),
            "Genesis hash must be deterministic across calls");
    }
    
    #[test]
    fn test_merkle_root_empty() {
        let root = Block::calculate_merkle_root(&[]);
        assert_eq!(root.len(), 64); // 32 bytes hex
    }
    
    #[test]
    fn test_block_hash() {
        let genesis = Block::genesis();
        let hash = genesis.calculate_hash();
        assert_eq!(hash.len(), 64); // 32 bytes hex = 64 hex chars
    }
}
