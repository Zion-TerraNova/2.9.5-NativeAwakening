/// Mining algorithm implementations
/// 
/// ZION supports multiple mining algorithms:
/// - **Cosmic Harmony**: ZION's native algorithm (fastest, GPU-friendly)
/// - **Cosmic Harmony v2**: Quantum-resistant, memory-hard (v3.0 preview)
/// - **RandomX**: CPU-optimized (Monero-based) ⚠️ ASIC BROKEN by Antminer X5
/// - **Yescrypt**: Memory-hard (ZCash-inspired)
/// - **Blake3**: Fallback algorithm

pub mod cosmic_harmony;
pub mod cosmic_harmony_v2;
pub mod blake3_algo;
pub mod randomx;
pub mod yescrypt;

// Small convenience shim so blockchain code can call `algorithms::blake3::hash(...)`.
pub mod blake3 {
    pub fn hash(data: &[u8]) -> [u8; 32] {
        super::blake3_algo::blake3_hash(data)
    }

    pub fn hash_with_nonce(data: &[u8], nonce: u32) -> [u8; 32] {
        super::blake3_algo::blake3_hash_with_nonce(data, nonce)
    }
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Algorithm {
    /// ZION native algorithm - fastest, golden ratio based
    CosmicHarmony,
    /// Quantum-resistant, memory-hard (v3.0 preview)
    CosmicHarmonyV2,
    /// Monero-style RandomX (CPU optimized) ⚠️ ASIC EXISTS (Antminer X5)
    RandomX,
    /// Memory-hard Yescrypt
    Yescrypt,
    /// Simple Blake3 fallback
    Blake3,
}

impl Algorithm {
    /// Parse algorithm from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "cosmic" | "cosmic_harmony" | "cosmicharmony" | "cosmic-harmony" => Some(Self::CosmicHarmony),
            "cosmic_harmony_v2" | "cosmicharmonyv2" | "cosmic-harmony-v2" => Some(Self::CosmicHarmonyV2),
            "randomx" | "random-x" | "rx/0" | "rx0" => Some(Self::RandomX),
            "yescrypt" => Some(Self::Yescrypt),
            "blake3" => Some(Self::Blake3),
            _ => None,
        }
    }

    /// Get algorithm name
    pub fn name(&self) -> &'static str {
        match self {
            Self::CosmicHarmony => "cosmic_harmony",
            Self::CosmicHarmonyV2 => "cosmic_harmony_v2",
            Self::RandomX => "randomx",
            Self::Yescrypt => "yescrypt",
            Self::Blake3 => "blake3",
        }
    }

    /// Get expected hashrate (H/s) for CPU baseline
    pub fn baseline_hashrate(&self) -> u64 {
        match self {
            Self::CosmicHarmony => 500_000,    // 500 kH/s
            Self::CosmicHarmonyV2 => 50_000,   // 50 kH/s (memory-hard)
            Self::RandomX => 600,              // 600 H/s
            Self::Yescrypt => 1_000,           // 1 kH/s
            Self::Blake3 => 5_000_000,         // 5 MH/s
        }
    }
    
    /// Check if algorithm has known ASIC hardware
    pub fn has_known_asic(&self) -> bool {
        match self {
            Self::RandomX => true,  // Antminer X5 (212 kH/s, ~$5-8K)
            _ => false,
        }
    }
    
    /// Get ASIC resistance level (0-100)
    pub fn asic_resistance_score(&self) -> u8 {
        match self {
            Self::CosmicHarmony => 75,      // No known ASIC, but simple algorithm
            Self::CosmicHarmonyV2 => 95,    // Memory-hard + dynamic params
            Self::RandomX => 20,            // ASIC exists (Antminer X5)
            Self::Yescrypt => 85,           // Memory-hard, no known ASIC
            Self::Blake3 => 10,             // Trivial to ASIC
        }
    }
    
    /// Is this algorithm quantum-resistant?
    pub fn is_quantum_resistant(&self) -> bool {
        match self {
            Self::CosmicHarmonyV2 => true,  // Lattice-based noise injection
            _ => false,
        }
    }
}

impl std::fmt::Display for Algorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl Default for Algorithm {
    fn default() -> Self {
        Self::CosmicHarmony
    }
}
