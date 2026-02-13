
/// Golden ratio constant (φ * 2^32)
const PHI: u32 = 0x9E3779B9;

/// SHA-256 initialization vector
const IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A,
    0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

/// Minimum scratchpad size (4 MB)
const MIN_SCRATCHPAD_SIZE: usize = 4 * 1024 * 1024;

/// Maximum scratchpad size (16 MB) - reserved for future difficulty scaling
#[allow(dead_code)]
const MAX_SCRATCHPAD_SIZE: usize = 16 * 1024 * 1024;

/// Base mixing rounds
const BASE_MIXING_ROUNDS: u32 = 12;

/// Maximum additional mixing rounds
const MAX_EXTRA_ROUNDS: u32 = 12;

/// Memory access patterns for ASIC resistance
#[derive(Clone, Copy, Debug)]
pub enum MemoryPattern {
    /// Sequential access (baseline)
    Sequential = 0,
    /// Random walk with state-dependent jumps
    RandomWalk = 1,
    /// Butterfly network pattern
    Butterfly = 2,
    /// Lattice-based access (grid pattern)
    Lattice = 3,
    /// Quantum-inspired interference pattern
    QuantumWalk = 4,
}

impl From<u64> for MemoryPattern {
    fn from(block_height: u64) -> Self {
        match block_height % 5 {
            0 => MemoryPattern::Sequential,
            1 => MemoryPattern::RandomWalk,
            2 => MemoryPattern::Butterfly,
            3 => MemoryPattern::Lattice,
            _ => MemoryPattern::QuantumWalk,
        }
    }
}

/// Dynamic parameters derived from blockchain state
#[derive(Clone, Debug)]
pub struct DynamicParams {
    /// Number of mixing rounds (12-24)
    pub mixing_rounds: u32,
    /// Scratchpad size in bytes (4-16 MB)
    pub scratchpad_size: usize,
    /// Memory access pattern
    pub memory_pattern: MemoryPattern,
    /// Rotation schedule for mixing (8 values)
    pub rotation_schedule: [u8; 8],
    /// Lattice noise parameters
    pub noise_modulus: u32,
}

impl DynamicParams {
    /// Derive parameters from previous block hash and height
    pub fn from_block_context(prev_hash: &[u8; 32], block_height: u64) -> Self {
        // Mixing rounds: 12 + (first byte % 13) = 12-24
        let mixing_rounds = BASE_MIXING_ROUNDS + (prev_hash[0] as u32 % (MAX_EXTRA_ROUNDS + 1));
        
        // Scratchpad size: 4 MB * (1 + height % 4) = 4-16 MB
        let size_multiplier = 1 + (block_height % 4) as usize;
        let scratchpad_size = MIN_SCRATCHPAD_SIZE * size_multiplier;
        
        // Memory pattern from block height
        let memory_pattern = MemoryPattern::from(block_height);
        
        // Rotation schedule from hash bytes 1-8
        let mut rotation_schedule = [0u8; 8];
        for i in 0..8 {
            rotation_schedule[i] = (prev_hash[i + 1] % 32) as u8;
        }
        
        // Noise modulus for lattice operations (prime number derived from hash)
        let noise_modulus = Self::derive_prime_modulus(&prev_hash[16..24]);
        
        Self {
            mixing_rounds,
            scratchpad_size,
            memory_pattern,
            rotation_schedule,
            noise_modulus,
        }
    }
    
    /// Derive a prime modulus from hash bytes (for lattice noise)
    fn derive_prime_modulus(bytes: &[u8]) -> u32 {
        // Small primes for efficient modular arithmetic
        const PRIMES: [u32; 16] = [
            65521, 65519, 65497, 65479, 65449, 65447, 65437, 65423,
            65419, 65413, 65407, 65393, 65381, 65371, 65357, 65353,
        ];
        let index = (bytes[0] as usize) % PRIMES.len();
        PRIMES[index]
    }
}

/// Cosmic Harmony v2 Hasher
pub struct CosmicHarmonyV2 {
    /// Hash state (8x u32)
    state: [u32; 8],
    /// Scratchpad for memory-hard operations
    scratchpad: Vec<u8>,
    /// Dynamic parameters
    params: DynamicParams,
}

impl CosmicHarmonyV2 {
    /// Create new hasher with dynamic parameters
    pub fn new(prev_hash: &[u8; 32], block_height: u64) -> Self {
        let params = DynamicParams::from_block_context(prev_hash, block_height);
        
        Self {
            state: IV,
            scratchpad: vec![0u8; params.scratchpad_size],
            params,
        }
    }
    
    /// Reset to initial state
    pub fn reset(&mut self) {
        self.state = IV;
        self.scratchpad.fill(0);
    }
    
    /// Hash input with nonce
    pub fn hash(&mut self, input: &[u8], nonce: u64) -> [u8; 32] {
        self.reset();
        
        // Phase 1: Absorb input
        self.absorb(input);
        self.mix_nonce(nonce);
        
        // Phase 2: Fill scratchpad (memory initialization)
        self.fill_scratchpad();
        
        // Phase 3: Memory-hard mixing
        self.memory_hard_mix();
        
        // Phase 4: Lattice noise injection (quantum resistance)
        self.inject_lattice_noise();
        
        // Phase 5: Golden finalization
        self.golden_finalize();
        
        // Output
        self.output()
    }
    
    /// Absorb input data into state
    fn absorb(&mut self, input: &[u8]) {
        // Convert to u32 words and XOR into state
        for (i, chunk) in input.chunks(4).take(8).enumerate() {
            let mut word = 0u32;
            for (j, &byte) in chunk.iter().enumerate() {
                word |= (byte as u32) << (j * 8);
            }
            self.state[i] ^= word;
        }
    }
    
    /// Mix nonce into state
    fn mix_nonce(&mut self, nonce: u64) {
        self.state[0] ^= nonce as u32;
        self.state[1] ^= (nonce >> 32) as u32;
        self.state[2] ^= nonce.rotate_left(17) as u32;
        self.state[3] ^= (nonce.rotate_right(13) >> 32) as u32;
    }
    
    /// Fill scratchpad with pseudo-random data derived from state
    fn fill_scratchpad(&mut self) {
        let chunk_size = 32;
        let num_chunks = self.params.scratchpad_size / chunk_size;
        
        for i in 0..num_chunks {
            // Generate chunk from state
            let chunk = self.generate_chunk(i as u32);
            
            // Write to scratchpad
            let offset = i * chunk_size;
            self.scratchpad[offset..offset + chunk_size].copy_from_slice(&chunk);
            
            // Update state with chunk feedback
            for j in 0..8 {
                self.state[j] ^= u32::from_le_bytes([
                    chunk[j * 4],
                    chunk[j * 4 + 1],
                    chunk[j * 4 + 2],
                    chunk[j * 4 + 3],
                ]);
            }
            
            // Quick mix after each chunk
            self.quick_mix();
        }
    }
    
    /// Generate a 32-byte chunk from state
    fn generate_chunk(&self, index: u32) -> [u8; 32] {
        let mut temp_state = self.state;
        
        // Mix index into state
        temp_state[0] ^= index;
        temp_state[7] ^= index.rotate_left(16);
        
        // Mini mixing rounds
        for _ in 0..4 {
            for i in 0..8 {
                let next = (i + 1) % 8;
                temp_state[i] = temp_state[i]
                    .rotate_left(5)
                    .wrapping_add(temp_state[next])
                    .wrapping_mul(PHI);
            }
        }
        
        // Output
        let mut output = [0u8; 32];
        for (i, &word) in temp_state.iter().enumerate() {
            output[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
        }
        output
    }
    
    /// Quick state mixing between scratchpad operations
    fn quick_mix(&mut self) {
        for i in 0..4 {
            self.state.swap(i, 7 - i);
        }
        for i in 0..8 {
            self.state[i] = self.state[i].rotate_left(7).wrapping_mul(PHI);
        }
    }
    
    /// Memory-hard mixing with random scratchpad access
    fn memory_hard_mix(&mut self) {
        let chunk_size = 32;
        let num_chunks = self.params.scratchpad_size / chunk_size;
        
        for round in 0..self.params.mixing_rounds {
            // Calculate read index based on pattern
            let read_idx = self.compute_access_index(round, num_chunks as u32);
            
            // Read from scratchpad - copy to avoid borrow conflict
            let offset = (read_idx as usize) * chunk_size;
            let mut chunk_copy = [0u8; 32];
            chunk_copy.copy_from_slice(&self.scratchpad[offset..offset + chunk_size]);
            
            // Mix into state with rotation schedule
            let rotation = self.params.rotation_schedule[(round % 8) as usize] as u32;
            self.mix_chunk(&chunk_copy, rotation);
            
            // Write back modified data (read-write dependency)
            let new_chunk = self.generate_chunk(round);
            let write_idx = self.compute_access_index(round + self.params.mixing_rounds, num_chunks as u32);
            let write_offset = (write_idx as usize) * chunk_size;
            self.scratchpad[write_offset..write_offset + chunk_size].copy_from_slice(&new_chunk);
        }
    }
    
    /// Compute memory access index based on pattern
    fn compute_access_index(&self, round: u32, max_chunks: u32) -> u32 {
        let state_idx = (self.state[0] ^ self.state[4]) as u64 
            + (self.state[1] ^ self.state[5]) as u64 * 0x10000;
        
        match self.params.memory_pattern {
            MemoryPattern::Sequential => {
                round % max_chunks
            }
            MemoryPattern::RandomWalk => {
                ((state_idx + round as u64 * PHI as u64) % max_chunks as u64) as u32
            }
            MemoryPattern::Butterfly => {
                // Butterfly network pattern
                let bits = (max_chunks as f64).log2() as u32;
                let stage = round % bits;
                let mask = 1u32 << stage;
                let base = (state_idx as u32) % max_chunks;
                base ^ mask
            }
            MemoryPattern::Lattice => {
                // 2D lattice access
                let dim = (max_chunks as f64).sqrt() as u32;
                let x = (state_idx as u32 + round) % dim;
                let y = ((state_idx >> 16) as u32 + round * 7) % dim;
                (y * dim + x) % max_chunks
            }
            MemoryPattern::QuantumWalk => {
                // Quantum random walk simulation
                let amplitude = self.state[round as usize % 8];
                let phase = self.state[(round as usize + 4) % 8];
                let interference = amplitude ^ phase;
                ((interference as u64 * state_idx) % max_chunks as u64) as u32
            }
        }
    }
    
    /// Mix chunk into state with rotation
    fn mix_chunk(&mut self, chunk: &[u8], rotation: u32) {
        for i in 0..8 {
            let word = u32::from_le_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
            
            self.state[i] = self.state[i]
                .rotate_left(rotation)
                .wrapping_add(word)
                .wrapping_mul(PHI);
        }
    }
    
    /// Inject lattice-based noise for quantum resistance
    fn inject_lattice_noise(&mut self) {
        let modulus = self.params.noise_modulus;
        
        for i in 0..8 {
            // Generate pseudo-random noise from state
            let noise_seed = self.state[i].wrapping_mul(PHI).wrapping_add(i as u32);
            
            // Discrete Gaussian-like noise (approximation)
            // Using central limit theorem with multiple additions
            let mut noise = 0u32;
            for j in 0..12 {
                let sample = noise_seed.rotate_left(j * 3) % modulus;
                noise = noise.wrapping_add(sample);
            }
            noise = noise / 6; // Normalize
            
            // Add noise to state
            self.state[i] = self.state[i].wrapping_add(noise % modulus);
        }
    }
    
    /// Golden ratio finalization
    fn golden_finalize(&mut self) {
        // XOR diffusion
        let mut xor_mix = 0u32;
        for &value in &self.state {
            xor_mix ^= value;
        }
        for value in &mut self.state {
            *value ^= xor_mix;
        }
        
        // Final golden multiplication
        for value in &mut self.state {
            *value = value.wrapping_mul(PHI);
        }
    }
    
    /// Output final 32-byte hash
    fn output(&self) -> [u8; 32] {
        let mut result = [0u8; 32];
        for (i, &word) in self.state.iter().enumerate() {
            result[i * 4..(i + 1) * 4].copy_from_slice(&word.to_le_bytes());
        }
        result
    }
}

/// Convenience function for mining
pub fn cosmic_hash_v2(
    input: &[u8],
    nonce: u64,
    prev_hash: &[u8; 32],
    block_height: u64,
) -> [u8; 32] {
    let mut hasher = CosmicHarmonyV2::new(prev_hash, block_height);
    hasher.hash(input, nonce)
}

/// Check if hash meets difficulty target
pub fn check_difficulty(hash: &[u8; 32], target_difficulty: u32) -> bool {
    let mut leading_zeros = 0u32;
    
    for &byte in hash.iter().rev() {
        if byte == 0 {
            leading_zeros += 8;
        } else {
            leading_zeros += byte.leading_zeros();
            break;
        }
    }
    
    leading_zeros >= target_difficulty
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_dynamic_params() {
        let prev_hash = [0u8; 32];
        let params = DynamicParams::from_block_context(&prev_hash, 0);
        
        assert!(params.mixing_rounds >= 12);
        assert!(params.mixing_rounds <= 24);
        assert_eq!(params.scratchpad_size, MIN_SCRATCHPAD_SIZE);
    }
    
    #[test]
    fn test_scratchpad_size_varies() {
        let prev_hash = [0u8; 32];
        
        let params0 = DynamicParams::from_block_context(&prev_hash, 0);
        let params1 = DynamicParams::from_block_context(&prev_hash, 1);
        let params3 = DynamicParams::from_block_context(&prev_hash, 3);
        
        assert_eq!(params0.scratchpad_size, 4 * 1024 * 1024);  // 4 MB
        assert_eq!(params1.scratchpad_size, 8 * 1024 * 1024);  // 8 MB
        assert_eq!(params3.scratchpad_size, 16 * 1024 * 1024); // 16 MB
    }
    
    #[test]
    fn test_memory_pattern_rotation() {
        for height in 0..5 {
            let pattern = MemoryPattern::from(height);
            assert_eq!(pattern as u64, height);
        }
    }
    
    #[test]
    fn test_hash_deterministic() {
        let prev_hash = [1u8; 32];
        let input = b"ZION_TEST";
        let nonce = 12345u64;
        
        let hash1 = cosmic_hash_v2(input, nonce, &prev_hash, 100);
        let hash2 = cosmic_hash_v2(input, nonce, &prev_hash, 100);
        
        assert_eq!(hash1, hash2);
    }
    
    #[test]
    fn test_nonce_changes_hash() {
        let prev_hash = [1u8; 32];
        let input = b"ZION_TEST";
        
        let hash1 = cosmic_hash_v2(input, 0, &prev_hash, 100);
        let hash2 = cosmic_hash_v2(input, 1, &prev_hash, 100);
        
        assert_ne!(hash1, hash2);
    }
    
    #[test]
    fn test_block_height_changes_params() {
        let prev_hash = [1u8; 32];
        let input = b"ZION_TEST";
        
        let hash1 = cosmic_hash_v2(input, 0, &prev_hash, 0);
        let hash2 = cosmic_hash_v2(input, 0, &prev_hash, 1);
        
        // Different block heights = different scratchpad sizes = different hashes
        assert_ne!(hash1, hash2);
    }
    
    #[test]
    fn test_difficulty_check() {
        let mut hash = [0xFFu8; 32];
        hash[31] = 0x00;
        hash[30] = 0x00;
        
        assert!(check_difficulty(&hash, 16));
        assert!(!check_difficulty(&hash, 17));
    }
}
