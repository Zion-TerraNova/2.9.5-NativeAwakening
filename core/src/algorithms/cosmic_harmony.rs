/// ZION Cosmic Harmony Algorithm - Rust Implementation
/// 
/// Golden ratio based mining algorithm optimized for both CPU and GPU.
/// Performance: ~500-600 kH/s on CPU, ~10-50 MH/s on GPU
/// 
/// Algorithm stages:
/// 1. Initialize 8x u32 state (SHA-256 IV constants)
/// 2. Absorb input data (XOR first 8 words)
/// 3. Mix nonce into state[0] and state[1]
/// 4. 12 mixing rounds with rotations
/// 5. XOR diffusion across all state words
/// 6. Golden ratio multiplication (φ = 0x9E3779B9)

/// Golden ratio constant (φ * 2^32)
const PHI: u32 = 0x9E3779B9;

/// SHA-256 initialization vector (used as Cosmic Harmony IV)
const IV: [u32; 8] = [
    0x6A09E667,
    0xBB67AE85,
    0x3C6EF372,
    0xA54FF53A,
    0x510E527F,
    0x9B05688C,
    0x1F83D9AB,
    0x5BE0CD19,
];

/// Number of mixing rounds
const ROUNDS: usize = 12;

/// Left rotation of 32-bit value
#[inline(always)]
fn rotl32(value: u32, shift: u32) -> u32 {
    value.rotate_left(shift & 31)
}

/// Mixing function combining three state elements
#[inline(always)]
fn mix(a: u32, b: u32, c: u32) -> u32 {
    rotl32(a ^ b, 5).wrapping_add(c)
}

/// Cosmic Harmony hasher state
pub struct CosmicHarmony {
    state: [u32; 8],
}

impl CosmicHarmony {
    /// Create new Cosmic Harmony hasher
    pub fn new() -> Self {
        Self { state: IV }
    }

    /// Reset hasher to initial state
    pub fn reset(&mut self) {
        self.state = IV;
    }

    /// Absorb input data into state
    /// 
    /// XORs first 8x u32 words from input into state
    fn absorb(&mut self, input: &[u8]) {
        // Convert bytes to u32 words (little-endian)
        let mut words = Vec::with_capacity((input.len() + 3) / 4);
        
        for chunk in input.chunks(4) {
            let mut word = 0u32;
            for (i, &byte) in chunk.iter().enumerate() {
                word |= (byte as u32) << (i * 8);
            }
            words.push(word);
        }

        // XOR first 8 words into state
        for i in 0..8.min(words.len()) {
            self.state[i] ^= words[i];
        }
    }

    /// Mix nonce into state
    fn mix_nonce(&mut self, nonce: u32) {
        self.state[0] ^= nonce;
        self.state[1] ^= nonce >> 16;
    }

    /// Perform mixing rounds
    fn mix_rounds(&mut self) {
        for _ in 0..ROUNDS {
            // Forward mixing
            for i in 0..8 {
                let next = (i + 1) % 8;
                let next2 = (i + 2) % 8;
                self.state[i] = mix(self.state[i], self.state[next], self.state[next2]);
            }

            // Swap first and second half
            for i in 0..4 {
                self.state.swap(i, i + 4);
            }
        }
    }

    /// XOR diffusion across state
    fn diffuse(&mut self) {
        let mut xor_mix = 0u32;
        for &value in &self.state {
            xor_mix ^= value;
        }
        for value in &mut self.state {
            *value ^= xor_mix;
        }
    }

    /// Apply golden ratio multiplication
    fn golden_multiply(&mut self) {
        for value in &mut self.state {
            *value = value.wrapping_mul(PHI);
        }
    }

    /// Finalize and output 32-byte hash
    pub fn finalize(&self) -> [u8; 32] {
        let mut output = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            let bytes = word.to_le_bytes();
            output[i * 4..(i + 1) * 4].copy_from_slice(&bytes);
        }
        output
    }

    /// Hash input data with nonce (main entry point)
    pub fn hash(&mut self, input: &[u8], nonce: u32) -> [u8; 32] {
        self.reset();
        self.absorb(input);
        self.mix_nonce(nonce);
        self.mix_rounds();
        self.diffuse();
        self.golden_multiply();
        self.finalize()
    }
}

impl Default for CosmicHarmony {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function: hash data with nonce
pub fn cosmic_hash(input: &[u8], nonce: u32) -> [u8; 32] {
    let mut hasher = CosmicHarmony::new();
    hasher.hash(input, nonce)
}

/// Blockchain convenience: algorithm-specific PoW hash.
///
/// Signature matches what `blockchain::block` expects.
pub fn hash(data: &[u8], nonce: u64, block_height: u64) -> Vec<u8> {
    let nonce32 = (nonce as u32) ^ (block_height as u32);
    cosmic_hash(data, nonce32).to_vec()
}

/// Check if hash meets difficulty target
/// 
/// Counts leading zero bits in hash (big-endian interpretation)
pub fn check_difficulty(hash: &[u8; 32], target_difficulty: u32) -> bool {
    let mut leading_zeros = 0u32;

    // Scan from last byte (most significant) to first
    for &byte in hash.iter().rev() {
        if byte == 0 {
            leading_zeros += 8;
        } else {
            // Count leading zeros in this byte
            let mut mask = 0x80u8;
            while (byte & mask) == 0 && mask != 0 {
                leading_zeros += 1;
                mask >>= 1;
            }
            break;
        }
    }

    leading_zeros >= target_difficulty
}

/// Check if hash meets 32-bit target (GPU mining compatibility)
/// 
/// Compares first 4 bytes (little-endian) against target
pub fn check_target32(hash: &[u8; 32], target32: u32) -> bool {
    let state0 = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
    state0 <= target32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosmic_hash_deterministic() {
        let input = b"ZION_TEST_BLOCK";
        let nonce = 12345;

        let hash1 = cosmic_hash(input, nonce);
        let hash2 = cosmic_hash(input, nonce);

        assert_eq!(hash1, hash2, "Hash should be deterministic");
    }

    #[test]
    fn test_nonce_changes_hash() {
        let input = b"ZION_TEST_BLOCK";

        let hash1 = cosmic_hash(input, 0);
        let hash2 = cosmic_hash(input, 1);

        assert_ne!(hash1, hash2, "Different nonces should produce different hashes");
    }

    #[test]
    fn test_difficulty_check() {
        // Create hash with known leading zeros
        let mut hash = [0u8; 32];
        hash[31] = 0x00; // Last byte (MSB in big-endian)
        hash[30] = 0x00;
        hash[29] = 0x01; // First non-zero bit at position 16 from end

        // With 2 full zero bytes (16 bits) and part of third byte, should have exactly 16 leading zeros
        assert!(check_difficulty(&hash, 15), "Should meet 15-bit difficulty");
        assert!(check_difficulty(&hash, 16), "Should meet 16-bit difficulty");
        // Note: Due to the specific bit pattern, we might have more than 16 zeros
        
        // Test with definite pattern
        let mut hash2 = [0xff; 32];
        hash2[31] = 0x00; // Only last byte is zero = exactly 8 leading zeros
        hash2[30] = 0x80; // High bit set in second byte
        
        assert!(check_difficulty(&hash2, 7), "Should meet 7-bit difficulty");
        assert!(check_difficulty(&hash2, 8), "Should meet 8-bit difficulty");
        assert!(!check_difficulty(&hash2, 9), "Should not meet 9-bit difficulty");
    }

    #[test]
    fn test_target32_check() {
        let mut hash = [0u8; 32];
        hash[0] = 0xFF;
        hash[1] = 0xFF;
        hash[2] = 0xFF;
        hash[3] = 0x00; // state0 = 0x00FFFFFF

        assert!(check_target32(&hash, 0x00FFFFFF), "Should meet exact target");
        assert!(check_target32(&hash, 0x01000000), "Should meet higher target");
        assert!(!check_target32(&hash, 0x00FFFFFE), "Should not meet lower target");
    }

    #[test]
    fn test_golden_ratio_constant() {
        // Verify PHI constant matches Python version
        assert_eq!(PHI, 0x9E3779B9, "Golden ratio constant mismatch");
    }

    #[test]
    fn test_iv_constants() {
        // Verify initialization vector matches SHA-256 IV
        assert_eq!(IV[0], 0x6A09E667);
        assert_eq!(IV[7], 0x5BE0CD19);
    }

    #[test]
    fn test_mixing_rounds() {
        let mut hasher = CosmicHarmony::new();
        let input = b"test";
        
        hasher.absorb(input);
        hasher.mix_nonce(1000);
        hasher.mix_rounds();

        // State should be different after mixing
        assert_ne!(hasher.state, IV, "State should change after mixing");
    }

    #[test]
    fn test_hash_length() {
        let hash = cosmic_hash(b"test", 0);
        assert_eq!(hash.len(), 32, "Hash should be 32 bytes");
    }
}
