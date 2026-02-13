/// Blake3 algorithm wrapper
/// 
/// Simple fallback algorithm for testing and compatibility

pub fn blake3_hash(input: &[u8]) -> [u8; 32] {
    blake3::hash(input).into()
}

/// Blake3 with nonce mixed into input
pub fn blake3_hash_with_nonce(input: &[u8], nonce: u32) -> [u8; 32] {
    let mut data = input.to_vec();
    data.extend_from_slice(&nonce.to_le_bytes());
    blake3_hash(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_deterministic() {
        let input = b"test data";
        let hash1 = blake3_hash(input);
        let hash2 = blake3_hash(input);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_blake3_with_nonce() {
        let input = b"test";
        let hash1 = blake3_hash_with_nonce(input, 0);
        let hash2 = blake3_hash_with_nonce(input, 1);
        assert_ne!(hash1, hash2);
    }
}
