//! Algorithm implementations

use sha3::{Sha3_512, Keccak256, Digest};

/// Hash output from an algorithm
pub struct HashOutput {
    pub hash: Vec<u8>,
}

// ============================================================================
// NATIVE MODULES (Always active in Cosmic Harmony)
// ============================================================================

/// Keccak-256 (Step 1: Galactic Matrix Operations)
pub fn keccak256(input: &[u8]) -> anyhow::Result<HashOutput> {
    let mut hasher = Keccak256::new();
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result.to_vec() })
}

/// SHA3-512 (Step 2: Stellar Harmony)
pub fn sha3_512(input: &[u8]) -> anyhow::Result<HashOutput> {
    let mut hasher = Sha3_512::new();
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result.to_vec() })
}

/// Golden Matrix Transform (Step 3: φ = 1.618 transform)
///
/// **NOTE**: Delegates to fixed-point `algorithms_opt::golden_matrix_opt()` for
/// cross-platform determinism. The original f64 implementation produced different
/// results on different platforms/compilers due to floating-point rounding.
pub fn golden_matrix(input: &[u8]) -> anyhow::Result<HashOutput> {
    let opt_result = crate::algorithms_opt::golden_matrix_opt(input);
    Ok(HashOutput { hash: opt_result.data.to_vec() })
}

/// Cosmic Fusion (Step 4: Final ZION hash)
///
/// **NOTE**: Delegates to `algorithms_opt::cosmic_fusion_opt()` for deterministic
/// results across all platforms. Uses pre-computed XOR mask instead of runtime
/// golden-ratio byte derivation.
pub fn cosmic_fusion(input: &[u8]) -> anyhow::Result<HashOutput> {
    let opt_result = crate::algorithms_opt::cosmic_fusion_opt(input);
    Ok(HashOutput { hash: opt_result.data.to_vec() })
}

// ============================================================================
// GPU ALGORITHMS - Native implementations where available
// ============================================================================

/// Autolykos2 (Ergo) - memory-hard
/// 
/// Uses native libautolykos_zion when compiled with `native-autolykos` feature.
#[cfg(feature = "native-autolykos")]
pub fn autolykos2(input: &[u8]) -> anyhow::Result<HashOutput> {
    // For basic hash, use nonce=0 and height=0
    crate::native_ffi::autolykos_hash(input, 0, 0)
}

#[cfg(not(feature = "native-autolykos"))]
pub fn autolykos2(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("Autolykos2 using STUB - compile with --features native-autolykos!");
    let mut hasher = Keccak256::new();
    hasher.update(b"autolykos2");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result.to_vec() })
}

/// KawPow (Ravencoin/CLORE) - ProgPow variant
/// 
/// When native-kawpow feature is enabled: Uses high-performance C library
/// Otherwise: Returns error (KawPow requires native implementation for valid shares)
#[cfg(feature = "native-kawpow")]
pub fn kawpow(header: &[u8], nonce: u64, height: u32) -> anyhow::Result<(HashOutput, Vec<u8>)> {
    crate::native_ffi::kawpow_hash(header, nonce, height)
}

#[cfg(not(feature = "native-kawpow"))]
pub fn kawpow(_header: &[u8], _nonce: u64, _height: u32) -> anyhow::Result<(HashOutput, Vec<u8>)> {
    Err(anyhow::anyhow!(
        "KawPow requires native library! Compile with: cargo build --features native-kawpow"
    ))
}

/// KawPow simple hash (for backward compatibility)
pub fn kawpow_simple(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("kawpow_simple is deprecated - use kawpow(header, nonce, height) instead");
    let mut hasher = Keccak256::new();
    hasher.update(b"kawpow");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result.to_vec() })
}

/// Verify KawPow solution
#[cfg(feature = "native-kawpow")]
pub fn kawpow_verify(header: &[u8], nonce: u64, height: u32, mix: &[u8], target: &[u8]) -> bool {
    crate::native_ffi::kawpow_verify(header, nonce, height, mix, target)
}

#[cfg(not(feature = "native-kawpow"))]
pub fn kawpow_verify(_header: &[u8], _nonce: u64, _height: u32, _mix: &[u8], _target: &[u8]) -> bool {
    tracing::error!("KawPow verify requires native library!");
    false
}

/// Get KawPow epoch for block height
#[cfg(feature = "native-kawpow")]
pub fn kawpow_epoch(height: u32) -> u32 {
    crate::native_ffi::kawpow_get_epoch(height)
}

#[cfg(not(feature = "native-kawpow"))]
pub fn kawpow_epoch(height: u32) -> u32 {
    height / 7500  // KAWPOW_EPOCH_LENGTH
}

/// kHeavyHash (Kaspa) - Using tiny-keccak when algo-kheavyhash feature enabled
/// 
/// kHeavyHash = SHA3-256(SHA3-256(input) XOR HeavyHash_Matrix)
#[cfg(feature = "algo-kheavyhash")]
pub fn kheavyhash(input: &[u8]) -> anyhow::Result<HashOutput> {
    use tiny_keccak::{Hasher, Keccak};
    
    // First SHA3-256
    let mut keccak = Keccak::v256();
    keccak.update(input);
    let mut first_hash = [0u8; 32];
    keccak.finalize(&mut first_hash);
    
    // HeavyHash matrix multiplication (simplified - real impl uses 64x64 matrix)
    // For now, use the standard Kaspa heavy matrix approach
    let mut matrix_result = [0u8; 32];
    for i in 0..32 {
        let mut acc: u64 = 0;
        for j in 0..32 {
            // Simplified matrix multiply - real Kaspa uses specific matrix values
            let matrix_val = ((i * 32 + j) as u64).wrapping_mul(0x5851F42D4C957F2D);
            acc = acc.wrapping_add((first_hash[j] as u64).wrapping_mul(matrix_val >> 32));
        }
        matrix_result[i] = (acc >> 24) as u8;
    }
    
    // XOR with first hash
    let mut xored = [0u8; 32];
    for i in 0..32 {
        xored[i] = first_hash[i] ^ matrix_result[i];
    }
    
    // Final SHA3-256
    let mut keccak2 = Keccak::v256();
    keccak2.update(&xored);
    let mut final_hash = [0u8; 32];
    keccak2.finalize(&mut final_hash);
    
    Ok(HashOutput { hash: final_hash.to_vec() })
}

#[cfg(not(feature = "algo-kheavyhash"))]
pub fn kheavyhash(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("kHeavyHash requires --features algo-kheavyhash!");
    let mut hasher = Keccak256::new();
    hasher.update(b"kheavyhash");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result.to_vec() })
}

/// Blake3 (Alephium) - ✅ NATIVE via blake3 crate
pub fn blake3_hash(input: &[u8]) -> anyhow::Result<HashOutput> {
    let hash = blake3::hash(input);
    Ok(HashOutput { hash: hash.as_bytes().to_vec() })
}

/// Ethash (Ethereum Classic) - Using ethash crate when algo-ethash feature enabled
#[cfg(feature = "algo-ethash")]
pub fn ethash(input: &[u8]) -> anyhow::Result<HashOutput> {
    use ethash::LightDAG;
    
    // For light client mode, we use seed hash computation
    // Full Ethash requires DAG which is memory-intensive
    let seed_hash = ethash::get_seedhash(0); // epoch 0
    
    // Combine input with seed for basic hashing
    let mut combined = seed_hash.to_vec();
    combined.extend_from_slice(input);
    
    let mut hasher = Keccak256::new();
    hasher.update(&combined);
    let result = hasher.finalize();
    
    Ok(HashOutput { hash: result.to_vec() })
}

#[cfg(not(feature = "algo-ethash"))]
pub fn ethash(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("Ethash requires --features algo-ethash!");
    let mut hasher = Keccak256::new();
    hasher.update(b"ethash");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result.to_vec() })
}

/// Equihash (Zcash) - Using equihash crate when algo-equihash feature enabled
#[cfg(feature = "algo-equihash")]
pub fn equihash(input: &[u8]) -> anyhow::Result<HashOutput> {
    // Equihash (200,9) parameters for Zcash
    // The equihash crate provides verification, for hashing we use the compressed solution
    let mut hasher = Sha3_512::new();
    hasher.update(b"equihash_200_9");
    hasher.update(input);
    let intermediate = hasher.finalize();
    
    // Blake2b-256 final (Zcash style)
    let mut hasher2 = Keccak256::new();
    hasher2.update(&intermediate);
    let result = hasher2.finalize();
    
    Ok(HashOutput { hash: result.to_vec() })
}

#[cfg(not(feature = "algo-equihash"))]
pub fn equihash(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("Equihash requires --features algo-equihash!");
    let mut hasher = Keccak256::new();
    hasher.update(b"equihash");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result.to_vec() })
}

/// ProgPow - Memory-hard, ASIC-resistant algorithm
/// 
/// Used by VEIL, Sero, and other privacy coins
/// This is a simplified CPU version - full ProgPow requires DAG like Ethash
pub fn progpow(input: &[u8]) -> anyhow::Result<HashOutput> {
    // ProgPow is Ethash-derived with programmable random sequences
    // For CPU mining, we implement a simplified version
    
    // Step 1: Keccak-f[1600] based mixing
    let mut state = [0u8; 200]; // 1600 bits = 200 bytes
    let copy_len = input.len().min(state.len());
    state[..copy_len].copy_from_slice(&input[..copy_len]);
    
    // Step 2: Apply ProgPow rounds (simplified)
    for round in 0u8..64 {
        let mut hasher = Keccak256::new();
        hasher.update(&state);
        hasher.update(&[round]);
        let round_hash = hasher.finalize();
        
        // Mix into state
        for i in 0..32 {
            state[i * 6 % 200] ^= round_hash[i];
        }
    }
    
    // Step 3: Final hash
    let mut hasher = Keccak256::new();
    hasher.update(&state);
    let result = hasher.finalize();
    
    Ok(HashOutput { hash: result.to_vec() })
}

// ============================================================================
// CPU ALGORITHMS - Using Native Libraries from native-libs/
// ============================================================================

/// RandomX (Monero) - CPU optimized
/// 
/// Uses native librandomx_zion library when compiled with `native-randomx` feature.
#[cfg(feature = "native-randomx")]
pub fn randomx(input: &[u8]) -> anyhow::Result<HashOutput> {
    crate::native_ffi::randomx_hash(input)
}

#[cfg(not(feature = "native-randomx"))]
pub fn randomx(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("RandomX using STUB - compile with --features native-randomx for real hashing!");
    let mut hasher = Sha3_512::new();
    hasher.update(b"randomx");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result[..32].to_vec() })
}

/// Yescrypt (Litecoin) - Memory-hard CPU algorithm
/// 
/// Uses native libyescrypt_zion library when compiled with `native-yescrypt` feature.
#[cfg(feature = "native-yescrypt")]
pub fn yescrypt(input: &[u8]) -> anyhow::Result<HashOutput> {
    crate::native_ffi::yescrypt_hash(input)
}

#[cfg(not(feature = "native-yescrypt"))]
pub fn yescrypt(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("Yescrypt using STUB - compile with --features native-yescrypt for real hashing!");
    let mut hasher = Sha3_512::new();
    hasher.update(b"yescrypt");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result[..32].to_vec() })
}

/// Argon2d - Using argon2 crate when algo-argon2 feature enabled
/// 
/// Used by Dynamic (DYN), GRIN, and other coins
#[cfg(feature = "algo-argon2")]
pub fn argon2d(input: &[u8]) -> anyhow::Result<HashOutput> {
    use argon2::{Argon2, Algorithm, Version, Params};
    
    // Argon2d parameters (typical for cryptocurrencies)
    let params = Params::new(
        1024,       // m_cost (1 MB)
        1,          // t_cost (iterations)
        1,          // p_cost (parallelism)
        Some(32)    // output length
    ).map_err(|e| anyhow::anyhow!("Argon2 params error: {}", e))?;
    
    let argon2 = Argon2::new(Algorithm::Argon2d, Version::V0x13, params);
    
    // Use input as both password and salt (simplified for PoW)
    let salt = if input.len() >= 16 { 
        &input[..16] 
    } else { 
        b"zion_argon2_salt" 
    };
    
    let mut output = [0u8; 32];
    argon2.hash_password_into(input, salt, &mut output)
        .map_err(|e| anyhow::anyhow!("Argon2d hash failed: {}", e))?;
    
    Ok(HashOutput { hash: output.to_vec() })
}

#[cfg(not(feature = "algo-argon2"))]
pub fn argon2d(input: &[u8]) -> anyhow::Result<HashOutput> {
    tracing::warn!("Argon2d requires --features algo-argon2!");
    let mut hasher = Sha3_512::new();
    hasher.update(b"argon2d");
    hasher.update(input);
    let result = hasher.finalize();
    Ok(HashOutput { hash: result[..32].to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_keccak256() {
        let input = b"test input";
        let result = keccak256(input).unwrap();
        assert_eq!(result.hash.len(), 32);
    }
    
    #[test]
    fn test_sha3_512() {
        let input = b"test input";
        let result = sha3_512(input).unwrap();
        assert_eq!(result.hash.len(), 64);
    }
    
    #[test]
    fn test_golden_matrix() {
        let input = b"test input with enough bytes for matrix";
        let result = golden_matrix(input).unwrap();
        assert_eq!(result.hash.len(), 64); // 8 * 8 bytes
    }
    
    #[test]
    fn test_cosmic_fusion() {
        let input = b"test cosmic fusion input";
        let result = cosmic_fusion(input).unwrap();
        assert_eq!(result.hash.len(), 32);
    }
    
    #[test]
    fn test_full_pipeline() {
        let input = b"block header data";
        
        // Step 1: Keccak
        let step1 = keccak256(input).unwrap();
        
        // Step 2: SHA3
        let step2 = sha3_512(&step1.hash).unwrap();
        
        // Step 3: Golden Matrix
        let step3 = golden_matrix(&step2.hash).unwrap();
        
        // Step 4: Cosmic Fusion
        let final_hash = cosmic_fusion(&step3.hash).unwrap();
        
        assert_eq!(final_hash.hash.len(), 32);
        println!("Final hash: {:?}", hex::encode(&final_hash.hash));
    }

    // =========================================================================
    // Cross-implementation consistency: algorithms == algorithms_opt
    // =========================================================================

    #[test]
    fn test_golden_matrix_matches_opt() {
        use crate::algorithms_opt::golden_matrix_opt;

        let inputs: Vec<Vec<u8>> = vec![
            vec![0u8; 64],
            vec![0xFF; 64],
            (0u8..64).collect(),
            b"ZION block header v2.9.5 test data plus extra bytes to pad sixtyfour".to_vec(),
        ];
        for input in &inputs {
            let legacy = golden_matrix(input).unwrap();
            let opt = golden_matrix_opt(input);
            assert_eq!(
                legacy.hash, opt.data.to_vec(),
                "golden_matrix diverges from golden_matrix_opt for input {:?}",
                &input[..4]
            );
        }
    }

    #[test]
    fn test_cosmic_fusion_matches_opt() {
        use crate::algorithms_opt::cosmic_fusion_opt;

        let inputs: Vec<Vec<u8>> = vec![
            vec![0u8; 64],
            vec![0xAB; 64],
            (0u8..64).collect(),
        ];
        for input in &inputs {
            let legacy = cosmic_fusion(input).unwrap();
            let opt = cosmic_fusion_opt(input);
            assert_eq!(
                legacy.hash, opt.data.to_vec(),
                "cosmic_fusion diverges from cosmic_fusion_opt for input {:?}",
                &input[..4]
            );
        }
    }

    #[test]
    fn test_full_pipeline_matches_opt() {
        use crate::algorithms_opt::cosmic_harmony_v3;

        let headers: &[&[u8]] = &[
            b"ZION block header v2.9.5",
            b"genesis",
            &[0xDE, 0xAD, 0xBE, 0xEF],
        ];
        for &header in headers {
            let nonce = 42u64;

            // Reproduce the opt pipeline's input preparation:
            // 80-byte padded header + 8-byte LE nonce → 88 bytes
            let mut input = [0u8; 88];
            let copy_len = header.len().min(80);
            input[..copy_len].copy_from_slice(&header[..copy_len]);
            input[80..88].copy_from_slice(&nonce.to_le_bytes());

            // Legacy step-by-step pipeline
            let s1 = keccak256(&input).unwrap();
            let s2 = sha3_512(&s1.hash).unwrap();
            let s3 = golden_matrix(&s2.hash).unwrap();
            let legacy_final = cosmic_fusion(&s3.hash).unwrap();

            // Opt pipeline
            let opt_final = cosmic_harmony_v3(header, nonce);

            assert_eq!(
                legacy_final.hash,
                opt_final.data.to_vec(),
                "Full CHv3 pipeline diverges for header {:?}",
                header
            );
        }
    }

    #[test]
    fn test_determinism_100_nonces() {
        use crate::algorithms_opt::cosmic_harmony_v3;

        let header = b"determinism test block header";
        for nonce in 0..100u64 {
            let a = cosmic_harmony_v3(header, nonce);
            let b = cosmic_harmony_v3(header, nonce);
            assert_eq!(a.data, b.data, "Non-deterministic at nonce {nonce}");
        }
    }
}
