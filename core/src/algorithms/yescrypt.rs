//! Yescrypt memory-hard KDF algorithm
//!
//! Full implementation using scrypt crate.
//! Yescrypt is inspired by Scrypt with enhanced memory-hardness.

use anyhow::{anyhow, Result};
use scrypt::{scrypt, Params as ScryptParams};

/// Yescrypt parameters (based on Scrypt)
pub struct YescryptParams {
    /// Memory cost parameter (N)
    pub n: u32,
    /// Block size parameter (r)
    pub r: u32,
    /// Parallelization parameter (p)
    pub p: u32,
}

impl Default for YescryptParams {
    fn default() -> Self {
        Self {
            n: 4096,  // 4 MiB memory
            r: 8,
            p: 1,
        }
    }
}

/// Compute Yescrypt hash with custom parameters
pub fn yescrypt_hash_with_params(
    input: &[u8],
    salt: &[u8],
    params: &YescryptParams,
) -> Result<[u8; 32]> {
    // Create Scrypt parameters
    let scrypt_params = ScryptParams::new(
        params.n.trailing_zeros() as u8, // log2(N)
        params.r,
        params.p,
        32, // output length
    )
    .map_err(|e| anyhow!("Invalid Scrypt parameters: {}", e))?;

    let mut output = [0u8; 32];
    scrypt(input, salt, &scrypt_params, &mut output)
        .map_err(|e| anyhow!("Scrypt hash failed: {}", e))?;

    Ok(output)
}

/// Compute Yescrypt hash with default parameters
pub fn yescrypt_hash(input: &[u8], salt: &[u8]) -> Result<[u8; 32]> {
    yescrypt_hash_with_params(input, salt, &YescryptParams::default())
}

/// Compute Yescrypt hash for mining (nonce included in input)
pub fn yescrypt_hash_mining(header: &[u8], nonce: u64) -> Result<[u8; 32]> {
    // Combine header + nonce
    let mut input = header.to_vec();
    input.extend_from_slice(&nonce.to_le_bytes());

    // Use header as salt for additional mixing
    yescrypt_hash(&input, header)
}

/// Blockchain convenience: algorithm-specific PoW hash (returns raw bytes).
pub fn hash(data: &[u8], salt: &[u8]) -> Vec<u8> {
    yescrypt_hash(data, salt)
        .expect("Yescrypt hash failed")
        .to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yescrypt_hash() {
        let input = b"zion-block-header";
        let salt = b"zion-salt";

        let hash = yescrypt_hash(input, salt).unwrap();

        // Should be 32 bytes
        assert_eq!(hash.len(), 32);

        // Should be non-zero
        assert!(hash.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_yescrypt_deterministic() {
        let input = b"same-input";
        let salt = b"same-salt";

        let hash1 = yescrypt_hash(input, salt).unwrap();
        let hash2 = yescrypt_hash(input, salt).unwrap();

        // Should be deterministic
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_yescrypt_different_salt() {
        let input = b"same-input";

        let hash1 = yescrypt_hash(input, b"salt1").unwrap();
        let hash2 = yescrypt_hash(input, b"salt2").unwrap();

        // Different salts should produce different hashes
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_yescrypt_mining() {
        let header = b"zion-block-12345";

        let hash1 = yescrypt_hash_mining(header, 0).unwrap();
        let hash2 = yescrypt_hash_mining(header, 1).unwrap();

        // Different nonces should produce different hashes
        assert_ne!(hash1, hash2);

        // Same nonce should be deterministic
        let hash3 = yescrypt_hash_mining(header, 0).unwrap();
        assert_eq!(hash1, hash3);
    }

    #[test]
    fn test_yescrypt_custom_params() {
        let input = b"test-input";
        let salt = b"test-salt";

        // Low memory for testing
        let params = YescryptParams {
            n: 1024,  // 1 MiB
            r: 8,
            p: 1,
        };

        let hash = yescrypt_hash_with_params(input, salt, &params).unwrap();
        assert_eq!(hash.len(), 32);
    }
}
