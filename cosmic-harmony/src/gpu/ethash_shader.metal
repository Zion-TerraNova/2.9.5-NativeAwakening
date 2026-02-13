/*
 * ZION Ethash Metal GPU Compute Shader
 * 
 * Ethash algorithm for ETC mining on Apple Silicon (M1-M5)
 * Reuses keccak_f1600 from CHv3 shader.
 *
 * Ethash pipeline:
 *   1. seed = Keccak-512(header_hash[32] + nonce[8])
 *   2. mix[128] = seed[0..63] repeated (FNV init)
 *   3. 64 iterations: mix = FNV(mix, DAG[index])
 *   4. cmix[32] = FNV-compress(mix[128] → 32 bytes)
 *   5. result = Keccak-256(seed[0..63] + cmix[32])
 *   6. Check result <= target
 *
 * Author: ZION AI Native Team
 * Version: 2.9.5
 * Date: February 2026
 */

#include <metal_stdlib>
using namespace metal;

// ============================================================================
// Keccak Constants (shared with CHv3 — duplicated here for separate metallib)
// ============================================================================

constant uint64_t ETH_KECCAK_RC[24] = {
    0x0000000000000001ULL, 0x0000000000008082ULL,
    0x800000000000808AULL, 0x8000000080008000ULL,
    0x000000000000808BULL, 0x0000000080000001ULL,
    0x8000000080008081ULL, 0x8000000000008009ULL,
    0x000000000000008AULL, 0x0000000000000088ULL,
    0x0000000080008009ULL, 0x000000008000000AULL,
    0x000000008000808BULL, 0x800000000000008BULL,
    0x8000000000008089ULL, 0x8000000000008003ULL,
    0x8000000000008002ULL, 0x8000000000000080ULL,
    0x000000000000800AULL, 0x800000008000000AULL,
    0x8000000080008081ULL, 0x8000000000008080ULL,
    0x0000000080000001ULL, 0x8000000080008008ULL
};

constant int ETH_KECCAK_ROTC[24] = {
    1,  3,  6,  10, 15, 21, 28, 36,
    45, 55, 2,  14, 27, 41, 56, 8,
    25, 43, 62, 18, 39, 61, 20, 44
};

constant int ETH_KECCAK_PILN[24] = {
    10, 7,  11, 17, 18, 3,  5,  16,
    8,  21, 24, 4,  15, 23, 19, 13,
    12, 2,  20, 14, 22, 9,  6,  1
};

// ============================================================================
// Ethash Constants
// ============================================================================

constant uint32_t FNV_PRIME = 0x01000193;
constant uint32_t FNV_OFFSET_BASIS = 0x811c9dc5;
constant int ETHASH_ACCESSES = 64;       // Number of DAG accesses per hash
constant int ETHASH_MIX_BYTES = 128;     // Mix buffer size in bytes
constant int ETHASH_HASH_BYTES = 64;     // Keccak-512 output (seed)
constant int ETHASH_DATASET_PARENTS = 256;
constant int ETHASH_WORD_BYTES = 4;      // 32-bit words

// ============================================================================
// Helper
// ============================================================================

inline uint64_t eth_rotl64(uint64_t x, int n) {
    return (x << n) | (x >> (64 - n));
}

inline uint32_t fnv1(uint32_t u, uint32_t v) {
    return (u * FNV_PRIME) ^ v;
}

// ============================================================================
// Keccak-f[1600] Permutation (24 rounds)
// ============================================================================

void eth_keccak_f1600(thread uint64_t *state) {
    uint64_t bc[5];
    uint64_t t;
    
    for (int round = 0; round < 24; round++) {
        // θ step
        for (int i = 0; i < 5; i++) {
            bc[i] = state[i] ^ state[i + 5] ^ state[i + 10] ^ state[i + 15] ^ state[i + 20];
        }
        for (int i = 0; i < 5; i++) {
            t = bc[(i + 4) % 5] ^ eth_rotl64(bc[(i + 1) % 5], 1);
            for (int j = 0; j < 25; j += 5) {
                state[j + i] ^= t;
            }
        }
        
        // ρ and π steps
        t = state[1];
        for (int i = 0; i < 24; i++) {
            int j = ETH_KECCAK_PILN[i];
            bc[0] = state[j];
            state[j] = eth_rotl64(t, ETH_KECCAK_ROTC[i]);
            t = bc[0];
        }
        
        // χ step
        for (int j = 0; j < 25; j += 5) {
            for (int i = 0; i < 5; i++) {
                bc[i] = state[j + i];
            }
            for (int i = 0; i < 5; i++) {
                state[j + i] ^= (~bc[(i + 1) % 5]) & bc[(i + 2) % 5];
            }
        }
        
        // ι step
        state[0] ^= ETH_KECCAK_RC[round];
    }
}

// ============================================================================
// Keccak-512 (Ethash uses Keccak with 0x01 padding, NOT SHA3 0x06)
// Rate = 72 bytes, output = 64 bytes
// ============================================================================

void keccak512_ethash(thread const uint8_t *input, int input_len, thread uint64_t *output) {
    uint64_t state[25];
    for (int i = 0; i < 25; i++) state[i] = 0;
    
    const int rate = 72; // 1600 - 2*512 = 576 bits = 72 bytes
    int offset = 0;
    
    // Absorb full blocks
    while (offset + rate <= input_len) {
        for (int i = 0; i < rate / 8; i++) {
            uint64_t word = 0;
            for (int j = 0; j < 8; j++) {
                word |= uint64_t(input[offset + i * 8 + j]) << (j * 8);
            }
            state[i] ^= word;
        }
        eth_keccak_f1600(state);
        offset += rate;
    }
    
    // Final block with Keccak padding (0x01, NOT SHA3 0x06!)
    uint8_t block[72];
    for (int i = 0; i < rate; i++) block[i] = 0;
    int remaining = input_len - offset;
    for (int i = 0; i < remaining; i++) {
        block[i] = input[offset + i];
    }
    block[remaining] = 0x01;
    block[rate - 1] |= 0x80;
    
    for (int i = 0; i < rate / 8; i++) {
        uint64_t word = 0;
        for (int j = 0; j < 8; j++) {
            word |= uint64_t(block[i * 8 + j]) << (j * 8);
        }
        state[i] ^= word;
    }
    eth_keccak_f1600(state);
    
    // Output 64 bytes as 8 x uint64_t (LE)
    for (int i = 0; i < 8; i++) {
        output[i] = state[i];
    }
}

// ============================================================================
// Keccak-256 (Ethash Keccak with 0x01 padding)
// Rate = 136 bytes, output = 32 bytes
// ============================================================================

void keccak256_ethash(thread const uint8_t *input, int input_len, thread uint8_t *output) {
    uint64_t state[25];
    for (int i = 0; i < 25; i++) state[i] = 0;
    
    const int rate = 136; // 1600 - 2*256 = 1088 bits = 136 bytes
    int offset = 0;
    
    // Absorb full blocks
    while (offset + rate <= input_len) {
        for (int i = 0; i < rate / 8; i++) {
            uint64_t word = 0;
            for (int j = 0; j < 8; j++) {
                word |= uint64_t(input[offset + i * 8 + j]) << (j * 8);
            }
            state[i] ^= word;
        }
        eth_keccak_f1600(state);
        offset += rate;
    }
    
    // Final block with Keccak padding
    uint8_t block[136];
    for (int i = 0; i < rate; i++) block[i] = 0;
    int remaining = input_len - offset;
    for (int i = 0; i < remaining; i++) {
        block[i] = input[offset + i];
    }
    block[remaining] = 0x01;
    block[rate - 1] |= 0x80;
    
    for (int i = 0; i < rate / 8; i++) {
        uint64_t word = 0;
        for (int j = 0; j < 8; j++) {
            word |= uint64_t(block[i * 8 + j]) << (j * 8);
        }
        state[i] ^= word;
    }
    eth_keccak_f1600(state);
    
    // Squeeze 32 bytes
    for (int i = 0; i < 4; i++) {
        for (int j = 0; j < 8; j++) {
            output[i * 8 + j] = uint8_t(state[i] >> (j * 8));
        }
    }
}

// ============================================================================
// Ethash Mining Parameters — buffer(0)
// ============================================================================

struct EthashMiningParams {
    uint64_t start_nonce;       // offset 0, size 8
    uint32_t dag_num_items;     // offset 8, size 4 — number of 64-byte DAG items
    uint8_t  header_hash[32];   // offset 12, size 32 — pre-hashed header from pool
    uint8_t  target[32];        // offset 44, size 32 — boundary target (LE 256-bit)
};                              // total: 76 → padded to 80

// ============================================================================
// Ethash Mining Result — buffer(2)
// ============================================================================

struct EthashMiningResult {
    uint64_t found_nonce;       // offset 0, size 8
    uint8_t  mix_digest[32];    // offset 8, size 32 — mix hash for share submission
    uint8_t  result_hash[32];   // offset 40, size 32 — final hash for verification
    uint32_t found;             // offset 72, size 4 — atomic: 0 = not found, 1 = found
};                              // total: 76 → padded to 80

// ============================================================================
// Ethash Core: hashimoto_light equivalent on GPU
// 
// DAG is pre-generated on CPU and uploaded as buffer(1)
// Each DAG item is 64 bytes (512 bits)
// ============================================================================

kernel void ethash_mine(
    device const EthashMiningParams& params [[buffer(0)]],
    device const uint8_t* dag [[buffer(1)]],             // Full DAG (~2.4 GB for ETC)
    device EthashMiningResult& result [[buffer(2)]],
    uint32_t thread_id [[thread_position_in_grid]]
) {
    uint64_t nonce = params.start_nonce + thread_id;
    
    // ---- Step 1: Compute seed hash = Keccak-512(header_hash[32] + nonce[8]) ----
    uint8_t seed_input[40];
    for (int i = 0; i < 32; i++) seed_input[i] = params.header_hash[i];
    // Nonce in LITTLE ENDIAN
    seed_input[32] = uint8_t(nonce >>  0);
    seed_input[33] = uint8_t(nonce >>  8);
    seed_input[34] = uint8_t(nonce >> 16);
    seed_input[35] = uint8_t(nonce >> 24);
    seed_input[36] = uint8_t(nonce >> 32);
    seed_input[37] = uint8_t(nonce >> 40);
    seed_input[38] = uint8_t(nonce >> 48);
    seed_input[39] = uint8_t(nonce >> 56);
    
    // seed_hash = 64 bytes (8 x uint64_t LE)
    uint64_t seed_hash[8];
    keccak512_ethash(seed_input, 40, seed_hash);
    
    // ---- Step 2: Initialize mix (128 bytes = 32 x uint32_t) from seed ----
    // mix[0..15] = seed_hash as uint32_t LE
    // mix[16..31] = seed_hash as uint32_t LE (duplicated)
    uint32_t mix[32];
    for (int i = 0; i < 16; i++) {
        // Convert uint64_t LE pairs to uint32_t LE
        uint64_t v = seed_hash[i / 2];
        if (i % 2 == 0) {
            mix[i] = uint32_t(v & 0xFFFFFFFF);
        } else {
            mix[i] = uint32_t(v >> 32);
        }
    }
    // Duplicate
    for (int i = 0; i < 16; i++) {
        mix[16 + i] = mix[i];
    }
    
    // ---- Step 3: 64 DAG accesses with FNV mixing ----
    uint32_t dag_num_items = params.dag_num_items;
    
    // seed_hash[0] as uint32_t LE for index calculation
    uint32_t seed0 = uint32_t(seed_hash[0] & 0xFFFFFFFF);
    
    for (int access = 0; access < ETHASH_ACCESSES; access++) {
        // Calculate DAG item index
        // p = fnv(access ^ seed0, mix[access % 32]) % dag_num_items
        uint32_t p = fnv1(uint32_t(access) ^ seed0, mix[access % 32]) % dag_num_items;
        
        // Read 128 bytes from DAG (2 × 64-byte items for 128-byte mix)
        // Each DAG item is 64 bytes → we need 2 consecutive items for 128 bytes
        uint32_t dag_item_idx = p * 2; // 2 items per access for 128-byte alignment
        
        // Ensure we don't go past DAG
        // In Ethash, we actually access 128-byte pages from the full DAG
        // Index calculation: p % (dag_size / hash_bytes) 
        // where hash_bytes = 64 for keccak-512
        // For mix of 128 bytes, we actually read 128 bytes from DAG at offset
        uint32_t dag_page = p % (dag_num_items / 2); // Pages of 128 bytes
        uint64_t dag_offset = uint64_t(dag_page) * 128;
        
        // Read 32 x uint32_t from DAG (128 bytes)
        device const uint8_t* dag_ptr = dag + dag_offset;
        
        for (int w = 0; w < 32; w++) {
            uint32_t dag_word = uint32_t(dag_ptr[w * 4 + 0])
                             | (uint32_t(dag_ptr[w * 4 + 1]) << 8)
                             | (uint32_t(dag_ptr[w * 4 + 2]) << 16)
                             | (uint32_t(dag_ptr[w * 4 + 3]) << 24);
            mix[w] = fnv1(mix[w], dag_word);
        }
    }
    
    // ---- Step 4: Compress mix (128 bytes → 32 bytes) ----
    // FNV-reduce: every 4 consecutive uint32_t into 1
    uint32_t cmix[8];
    for (int i = 0; i < 8; i++) {
        cmix[i] = fnv1(fnv1(fnv1(mix[i * 4], mix[i * 4 + 1]), mix[i * 4 + 2]), mix[i * 4 + 3]);
    }
    
    // Convert cmix to bytes (LE)
    uint8_t cmix_bytes[32];
    for (int i = 0; i < 8; i++) {
        cmix_bytes[i * 4 + 0] = uint8_t(cmix[i] >>  0);
        cmix_bytes[i * 4 + 1] = uint8_t(cmix[i] >>  8);
        cmix_bytes[i * 4 + 2] = uint8_t(cmix[i] >> 16);
        cmix_bytes[i * 4 + 3] = uint8_t(cmix[i] >> 24);
    }
    
    // ---- Step 5: Final hash = Keccak-256(seed[0..64] + cmix[32]) ----
    // Construct 96-byte input: seed_hash as bytes + cmix_bytes
    uint8_t final_input[96];
    for (int i = 0; i < 8; i++) {
        for (int j = 0; j < 8; j++) {
            final_input[i * 8 + j] = uint8_t(seed_hash[i] >> (j * 8));
        }
    }
    for (int i = 0; i < 32; i++) {
        final_input[64 + i] = cmix_bytes[i];
    }
    
    uint8_t final_hash[32];
    keccak256_ethash(final_input, 96, final_hash);
    
    // ---- Step 6: Check against target (256-bit LE comparison) ----
    // Compare final_hash <= target, both as 256-bit LE numbers
    // We compare from most significant byte (byte 31) to least significant (byte 0)
    bool below_target = false;
    bool equal = true;
    
    for (int i = 31; i >= 0; i--) {
        if (equal) {
            if (final_hash[i] < params.target[i]) {
                below_target = true;
                equal = false;
            } else if (final_hash[i] > params.target[i]) {
                below_target = false;
                equal = false;
            }
        }
    }
    if (equal) below_target = true; // hash == target is also valid
    
    // ---- Store result atomically ----
    if (below_target) {
        uint32_t expected = 0;
        if (atomic_compare_exchange_weak_explicit(
            (device atomic_uint*)&result.found,
            &expected, 1u,
            memory_order_relaxed,
            memory_order_relaxed
        )) {
            result.found_nonce = nonce;
            for (int i = 0; i < 32; i++) {
                result.mix_digest[i] = cmix_bytes[i];
                result.result_hash[i] = final_hash[i];
            }
        }
    }
}

// ============================================================================
// Ethash Benchmark Kernel (easy target, write all results)
// ============================================================================

kernel void ethash_benchmark(
    device const EthashMiningParams& params [[buffer(0)]],
    device const uint8_t* dag [[buffer(1)]],
    device uint8_t* output_hashes [[buffer(2)]],           // 32 bytes per thread
    uint32_t thread_id [[thread_position_in_grid]]
) {
    uint64_t nonce = params.start_nonce + thread_id;
    
    // Same seed computation
    uint8_t seed_input[40];
    for (int i = 0; i < 32; i++) seed_input[i] = params.header_hash[i];
    seed_input[32] = uint8_t(nonce >>  0);
    seed_input[33] = uint8_t(nonce >>  8);
    seed_input[34] = uint8_t(nonce >> 16);
    seed_input[35] = uint8_t(nonce >> 24);
    seed_input[36] = uint8_t(nonce >> 32);
    seed_input[37] = uint8_t(nonce >> 40);
    seed_input[38] = uint8_t(nonce >> 48);
    seed_input[39] = uint8_t(nonce >> 56);
    
    uint64_t seed_hash[8];
    keccak512_ethash(seed_input, 40, seed_hash);
    
    // Initialize mix
    uint32_t mix[32];
    for (int i = 0; i < 16; i++) {
        uint64_t v = seed_hash[i / 2];
        mix[i] = (i % 2 == 0) ? uint32_t(v & 0xFFFFFFFF) : uint32_t(v >> 32);
    }
    for (int i = 0; i < 16; i++) mix[16 + i] = mix[i];
    
    // DAG accesses
    uint32_t dag_num_items = params.dag_num_items;
    uint32_t seed0 = uint32_t(seed_hash[0] & 0xFFFFFFFF);
    
    for (int access = 0; access < ETHASH_ACCESSES; access++) {
        uint32_t p = fnv1(uint32_t(access) ^ seed0, mix[access % 32]) % dag_num_items;
        uint32_t dag_page = p % (dag_num_items / 2);
        uint64_t dag_offset = uint64_t(dag_page) * 128;
        device const uint8_t* dag_ptr = dag + dag_offset;
        
        for (int w = 0; w < 32; w++) {
            uint32_t dag_word = uint32_t(dag_ptr[w * 4 + 0])
                             | (uint32_t(dag_ptr[w * 4 + 1]) << 8)
                             | (uint32_t(dag_ptr[w * 4 + 2]) << 16)
                             | (uint32_t(dag_ptr[w * 4 + 3]) << 24);
            mix[w] = fnv1(mix[w], dag_word);
        }
    }
    
    // Compress
    uint32_t cmix[8];
    for (int i = 0; i < 8; i++) {
        cmix[i] = fnv1(fnv1(fnv1(mix[i * 4], mix[i * 4 + 1]), mix[i * 4 + 2]), mix[i * 4 + 3]);
    }
    
    uint8_t cmix_bytes[32];
    for (int i = 0; i < 8; i++) {
        cmix_bytes[i * 4 + 0] = uint8_t(cmix[i] >>  0);
        cmix_bytes[i * 4 + 1] = uint8_t(cmix[i] >>  8);
        cmix_bytes[i * 4 + 2] = uint8_t(cmix[i] >> 16);
        cmix_bytes[i * 4 + 3] = uint8_t(cmix[i] >> 24);
    }
    
    // Final hash
    uint8_t final_input[96];
    for (int i = 0; i < 8; i++) {
        for (int j = 0; j < 8; j++) {
            final_input[i * 8 + j] = uint8_t(seed_hash[i] >> (j * 8));
        }
    }
    for (int i = 0; i < 32; i++) final_input[64 + i] = cmix_bytes[i];
    
    uint8_t final_hash[32];
    keccak256_ethash(final_input, 96, final_hash);
    
    // Write output
    device uint8_t* my_output = output_hashes + thread_id * 32;
    for (int i = 0; i < 32; i++) {
        my_output[i] = final_hash[i];
    }
}
