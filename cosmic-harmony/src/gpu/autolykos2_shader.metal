/*
 * ZION Autolykos2 Metal GPU Compute Shader — TABLELESS Mining v4
 * 
 * Autolykos v2 algorithm for ERG (Ergo) mining on Apple Silicon (M1-M5)
 *
 * v4 OPTIMIZATIONS (based on CUDA reference: mhssamadani/Autolykos2_NV_Miner):
 * 1. FULLY UNROLLED Blake2b: All 12 rounds expanded inline (no sigma lookup)
 *    - Matches CUDA's devB2B_MIX() pattern with direct message word indexing
 *    - Eliminates constant array lookups and loop overhead per compression
 * 2. uint64 word-level operations throughout
 * 3. Direct uint64 word reads from M buffer (no byte-by-byte)
 * 4. Specialized oneblock Blake2b for small inputs (<=128B) also unrolled
 *
 * TABLELESS MODE: We compute R = Blake2b256(j||h||M) on-the-fly for each index.
 * The CUDA miner uses a prehash table (N×32B ≈ 64GB at current N) for O(1) lookup,
 * but Apple M1 has only 16GB unified memory, so table-based is impossible.
 * 
 * Total Blake2b256 per nonce: 1(nonce) + 1(e) + 1(seed) + 32(R) + 1(final) = 36
 * Each R-element hash = 65 compressions (8200B input)
 * Total compressions per nonce: ~2,180 (vs CUDA's ~2 with prehash table)
 *
 * Reference: ergoplatform/ergo AutolykosPowScheme.scala
 *            mhssamadani/Autolykos2_NV_Miner mining.cu + prehash.cu
 * Author: ZION AI Native Team
 * Version: 2.9.5-v4 (fully unrolled Blake2b)
 * Date: February 2026
 */

#include <metal_stdlib>
using namespace metal;

// ============================================================================
// Blake2b Constants — IV only, sigma is inlined in unrolled rounds
// ============================================================================

constant uint64_t BLAKE2B_IV[8] = {
    0x6a09e667f3bcc908ULL, 0xbb67ae8584caa73bULL,
    0x3c6ef372fe94f82bULL, 0xa54ff53a5f1d36f1ULL,
    0x510e527fade682d1ULL, 0x9b05688c2b3e6c1fULL,
    0x1f83d9abfb41bd6bULL, 0x5be0cd19137e2179ULL
};

// ============================================================================
// Helper: rotr64
// ============================================================================

inline uint64_t rotr64(uint64_t x, int n) {
    return (x >> n) | (x << (64 - n));
}

// ============================================================================
// Blake2b Mixing Function G — macro for maximum inlining
// Matches CUDA's devB2B_G() pattern
// ============================================================================

#define B2B_G(v, a, b, c, d, x, y) \
    v[a] = v[a] + v[b] + (x); \
    v[d] = rotr64(v[d] ^ v[a], 32); \
    v[c] = v[c] + v[d]; \
    v[b] = rotr64(v[b] ^ v[c], 24); \
    v[a] = v[a] + v[b] + (y); \
    v[d] = rotr64(v[d] ^ v[a], 16); \
    v[c] = v[c] + v[d]; \
    v[b] = rotr64(v[b] ^ v[c], 63);

// ============================================================================
// FULLY UNROLLED Blake2b-256 Compression — 12 rounds, no loops, no sigma table
// Matches CUDA's devB2B_MIX(): each G() call uses constant message word indices
// ============================================================================

void blake2b_compress(
    thread uint64_t *h,
    thread const uint64_t *m,
    uint64_t t,
    bool last
) {
    uint64_t v[16];
    
    v[0]=h[0]; v[1]=h[1]; v[2]=h[2]; v[3]=h[3];
    v[4]=h[4]; v[5]=h[5]; v[6]=h[6]; v[7]=h[7];
    v[8]=BLAKE2B_IV[0]; v[9]=BLAKE2B_IV[1]; v[10]=BLAKE2B_IV[2]; v[11]=BLAKE2B_IV[3];
    v[12]=BLAKE2B_IV[4]^t; v[13]=BLAKE2B_IV[5];
    v[14]=last?(BLAKE2B_IV[6]^0xFFFFFFFFFFFFFFFFULL):BLAKE2B_IV[6];
    v[15]=BLAKE2B_IV[7];
    
    // Round 0: sigma = {0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15}
    B2B_G(v, 0,4, 8,12, m[ 0], m[ 1]);
    B2B_G(v, 1,5, 9,13, m[ 2], m[ 3]);
    B2B_G(v, 2,6,10,14, m[ 4], m[ 5]);
    B2B_G(v, 3,7,11,15, m[ 6], m[ 7]);
    B2B_G(v, 0,5,10,15, m[ 8], m[ 9]);
    B2B_G(v, 1,6,11,12, m[10], m[11]);
    B2B_G(v, 2,7, 8,13, m[12], m[13]);
    B2B_G(v, 3,4, 9,14, m[14], m[15]);
    // Round 1: sigma = {14,10,4,8,9,15,13,6,1,12,0,2,11,7,5,3}
    B2B_G(v, 0,4, 8,12, m[14], m[10]);
    B2B_G(v, 1,5, 9,13, m[ 4], m[ 8]);
    B2B_G(v, 2,6,10,14, m[ 9], m[15]);
    B2B_G(v, 3,7,11,15, m[13], m[ 6]);
    B2B_G(v, 0,5,10,15, m[ 1], m[12]);
    B2B_G(v, 1,6,11,12, m[ 0], m[ 2]);
    B2B_G(v, 2,7, 8,13, m[11], m[ 7]);
    B2B_G(v, 3,4, 9,14, m[ 5], m[ 3]);
    // Round 2: sigma = {11,8,12,0,5,2,15,13,10,14,3,6,7,1,9,4}
    B2B_G(v, 0,4, 8,12, m[11], m[ 8]);
    B2B_G(v, 1,5, 9,13, m[12], m[ 0]);
    B2B_G(v, 2,6,10,14, m[ 5], m[ 2]);
    B2B_G(v, 3,7,11,15, m[15], m[13]);
    B2B_G(v, 0,5,10,15, m[10], m[14]);
    B2B_G(v, 1,6,11,12, m[ 3], m[ 6]);
    B2B_G(v, 2,7, 8,13, m[ 7], m[ 1]);
    B2B_G(v, 3,4, 9,14, m[ 9], m[ 4]);
    // Round 3: sigma = {7,9,3,1,13,12,11,14,2,6,5,10,4,0,15,8}
    B2B_G(v, 0,4, 8,12, m[ 7], m[ 9]);
    B2B_G(v, 1,5, 9,13, m[ 3], m[ 1]);
    B2B_G(v, 2,6,10,14, m[13], m[12]);
    B2B_G(v, 3,7,11,15, m[11], m[14]);
    B2B_G(v, 0,5,10,15, m[ 2], m[ 6]);
    B2B_G(v, 1,6,11,12, m[ 5], m[10]);
    B2B_G(v, 2,7, 8,13, m[ 4], m[ 0]);
    B2B_G(v, 3,4, 9,14, m[15], m[ 8]);
    // Round 4: sigma = {9,0,5,7,2,4,10,15,14,1,11,12,6,8,3,13}
    B2B_G(v, 0,4, 8,12, m[ 9], m[ 0]);
    B2B_G(v, 1,5, 9,13, m[ 5], m[ 7]);
    B2B_G(v, 2,6,10,14, m[ 2], m[ 4]);
    B2B_G(v, 3,7,11,15, m[10], m[15]);
    B2B_G(v, 0,5,10,15, m[14], m[ 1]);
    B2B_G(v, 1,6,11,12, m[11], m[12]);
    B2B_G(v, 2,7, 8,13, m[ 6], m[ 8]);
    B2B_G(v, 3,4, 9,14, m[ 3], m[13]);
    // Round 5: sigma = {2,12,6,10,0,11,8,3,4,13,7,5,15,14,1,9}
    B2B_G(v, 0,4, 8,12, m[ 2], m[12]);
    B2B_G(v, 1,5, 9,13, m[ 6], m[10]);
    B2B_G(v, 2,6,10,14, m[ 0], m[11]);
    B2B_G(v, 3,7,11,15, m[ 8], m[ 3]);
    B2B_G(v, 0,5,10,15, m[ 4], m[13]);
    B2B_G(v, 1,6,11,12, m[ 7], m[ 5]);
    B2B_G(v, 2,7, 8,13, m[15], m[14]);
    B2B_G(v, 3,4, 9,14, m[ 1], m[ 9]);
    // Round 6: sigma = {12,5,1,15,14,13,4,10,0,7,6,3,9,2,8,11}
    B2B_G(v, 0,4, 8,12, m[12], m[ 5]);
    B2B_G(v, 1,5, 9,13, m[ 1], m[15]);
    B2B_G(v, 2,6,10,14, m[14], m[13]);
    B2B_G(v, 3,7,11,15, m[ 4], m[10]);
    B2B_G(v, 0,5,10,15, m[ 0], m[ 7]);
    B2B_G(v, 1,6,11,12, m[ 6], m[ 3]);
    B2B_G(v, 2,7, 8,13, m[ 9], m[ 2]);
    B2B_G(v, 3,4, 9,14, m[ 8], m[11]);
    // Round 7: sigma = {13,11,7,14,12,1,3,9,5,0,15,4,8,6,2,10}
    B2B_G(v, 0,4, 8,12, m[13], m[11]);
    B2B_G(v, 1,5, 9,13, m[ 7], m[14]);
    B2B_G(v, 2,6,10,14, m[12], m[ 1]);
    B2B_G(v, 3,7,11,15, m[ 3], m[ 9]);
    B2B_G(v, 0,5,10,15, m[ 5], m[ 0]);
    B2B_G(v, 1,6,11,12, m[15], m[ 4]);
    B2B_G(v, 2,7, 8,13, m[ 8], m[ 6]);
    B2B_G(v, 3,4, 9,14, m[ 2], m[10]);
    // Round 8: sigma = {6,15,14,9,11,3,0,8,12,2,13,7,1,4,10,5}
    B2B_G(v, 0,4, 8,12, m[ 6], m[15]);
    B2B_G(v, 1,5, 9,13, m[14], m[ 9]);
    B2B_G(v, 2,6,10,14, m[11], m[ 3]);
    B2B_G(v, 3,7,11,15, m[ 0], m[ 8]);
    B2B_G(v, 0,5,10,15, m[12], m[ 2]);
    B2B_G(v, 1,6,11,12, m[13], m[ 7]);
    B2B_G(v, 2,7, 8,13, m[ 1], m[ 4]);
    B2B_G(v, 3,4, 9,14, m[10], m[ 5]);
    // Round 9: sigma = {10,2,8,4,7,6,1,5,15,11,9,14,3,12,13,0}
    B2B_G(v, 0,4, 8,12, m[10], m[ 2]);
    B2B_G(v, 1,5, 9,13, m[ 8], m[ 4]);
    B2B_G(v, 2,6,10,14, m[ 7], m[ 6]);
    B2B_G(v, 3,7,11,15, m[ 1], m[ 5]);
    B2B_G(v, 0,5,10,15, m[15], m[11]);
    B2B_G(v, 1,6,11,12, m[ 9], m[14]);
    B2B_G(v, 2,7, 8,13, m[ 3], m[12]);
    B2B_G(v, 3,4, 9,14, m[13], m[ 0]);
    // Round 10 = Round 0
    B2B_G(v, 0,4, 8,12, m[ 0], m[ 1]);
    B2B_G(v, 1,5, 9,13, m[ 2], m[ 3]);
    B2B_G(v, 2,6,10,14, m[ 4], m[ 5]);
    B2B_G(v, 3,7,11,15, m[ 6], m[ 7]);
    B2B_G(v, 0,5,10,15, m[ 8], m[ 9]);
    B2B_G(v, 1,6,11,12, m[10], m[11]);
    B2B_G(v, 2,7, 8,13, m[12], m[13]);
    B2B_G(v, 3,4, 9,14, m[14], m[15]);
    // Round 11 = Round 1
    B2B_G(v, 0,4, 8,12, m[14], m[10]);
    B2B_G(v, 1,5, 9,13, m[ 4], m[ 8]);
    B2B_G(v, 2,6,10,14, m[ 9], m[15]);
    B2B_G(v, 3,7,11,15, m[13], m[ 6]);
    B2B_G(v, 0,5,10,15, m[ 1], m[12]);
    B2B_G(v, 1,6,11,12, m[ 0], m[ 2]);
    B2B_G(v, 2,7, 8,13, m[11], m[ 7]);
    B2B_G(v, 3,4, 9,14, m[ 5], m[ 3]);
    
    h[0]^=v[0]^v[ 8]; h[1]^=v[1]^v[ 9]; h[2]^=v[2]^v[10]; h[3]^=v[3]^v[11];
    h[4]^=v[4]^v[12]; h[5]^=v[5]^v[13]; h[6]^=v[6]^v[14]; h[7]^=v[7]^v[15];
}

// ============================================================================
// Emit Blake2b-256 state to 32-byte output
// ============================================================================

inline void blake2b_emit(thread const uint64_t *h, thread uint8_t *output) {
    for (int i = 0; i < 4; i++) {
        uint64_t word = h[i];
        output[i * 8 + 0] = uint8_t(word);
        output[i * 8 + 1] = uint8_t(word >> 8);
        output[i * 8 + 2] = uint8_t(word >> 16);
        output[i * 8 + 3] = uint8_t(word >> 24);
        output[i * 8 + 4] = uint8_t(word >> 32);
        output[i * 8 + 5] = uint8_t(word >> 40);
        output[i * 8 + 6] = uint8_t(word >> 48);
        output[i * 8 + 7] = uint8_t(word >> 56);
    }
}

// ============================================================================
// Swap bytes of a big-endian uint64 to little-endian (for Blake2b message words)
// M stores values as big-endian (Java Longs.toByteArray), Blake2b needs LE.
// ============================================================================

inline uint64_t bswap64(uint64_t x) {
    x = ((x & 0x00FF00FF00FF00FFULL) << 8) | ((x >> 8) & 0x00FF00FF00FF00FFULL);
    x = ((x & 0x0000FFFF0000FFFFULL) << 16) | ((x >> 16) & 0x0000FFFF0000FFFFULL);
    return (x << 32) | (x >> 32);
}

// ============================================================================
// Load a uint64 from device M buffer as little-endian word
// M is stored big-endian, Blake2b uses LE words → byte-swap needed
// ============================================================================

inline uint64_t load_m_word_le(device const uint64_t *M_words, uint32_t word_index) {
    return bswap64(M_words[word_index]);
}

// ============================================================================
// OPTIMIZED: Blake2b-256 for R-element input pattern: j(4B) || h(4B) || M(8192B)
//
// Total input: 8200 bytes = 64 full 128B blocks + 8 bytes remainder
//
// Block 0:  prefix[8B] + M_words[0..14] (120B) = 128 bytes
// Block b (1..63): M_words[b*16-1 .. b*16+14] = 16 words = 128 bytes
// Block 64 (final): M_words[1023] (8B) + zeros = 8 bytes remainder
//
// Key optimization: NO byte-by-byte conditionals — direct uint64 word loads!
// ============================================================================

void blake2b_256_jhm(
    uint32_t j_index,
    uint32_t height,
    device const uint64_t *M_words,
    thread uint8_t *output
) {
    // Init Blake2b-256 state
    uint64_t h[8];
    for (int i = 0; i < 8; i++) h[i] = BLAKE2B_IV[i];
    h[0] ^= 0x01010020;
    
    uint64_t m_block[16];
    
    // ---- Block 0: 8B prefix + 120B from M ----
    // Prefix = j(4B BE) || h(4B BE) = 8 bytes, reinterpreted as LE uint64
    uint64_t prefix_le = uint64_t(uint8_t(j_index >> 24))
                       | (uint64_t(uint8_t(j_index >> 16)) << 8)
                       | (uint64_t(uint8_t(j_index >> 8)) << 16)
                       | (uint64_t(uint8_t(j_index)) << 24)
                       | (uint64_t(uint8_t(height >> 24)) << 32)
                       | (uint64_t(uint8_t(height >> 16)) << 40)
                       | (uint64_t(uint8_t(height >> 8)) << 48)
                       | (uint64_t(uint8_t(height)) << 56);
    
    m_block[0] = prefix_le;
    for (int w = 0; w < 15; w++) {
        m_block[w + 1] = load_m_word_le(M_words, w);
    }
    blake2b_compress(h, m_block, 128, false);
    
    // ---- Blocks 1..63: Pure M data (16 words each, zero branching) ----
    for (int b = 1; b < 64; b++) {
        int m_word_base = b * 16 - 1;
        for (int w = 0; w < 16; w++) {
            m_block[w] = load_m_word_le(M_words, m_word_base + w);
        }
        blake2b_compress(h, m_block, uint64_t(b + 1) * 128, false);
    }
    
    // ---- Block 64 (final): 8 bytes from M (word 1023), rest zero ----
    for (int i = 0; i < 16; i++) m_block[i] = 0;
    m_block[0] = load_m_word_le(M_words, 1023);
    blake2b_compress(h, m_block, 8200, true);
    
    blake2b_emit(h, output);
}

// ============================================================================
// OPTIMIZED: Blake2b-256 single-block hash for inputs <= 128 bytes
// Used for: genIndexes seed (71B), final hash (32B), header+nonce (40B)
// ============================================================================

void blake2b_256_oneblock(
    thread const uint8_t *input,
    uint32_t input_len,
    thread uint8_t *output
) {
    uint64_t h[8];
    for (int i = 0; i < 8; i++) h[i] = BLAKE2B_IV[i];
    h[0] ^= 0x01010020;
    
    uint64_t m[16];
    for (int i = 0; i < 16; i++) m[i] = 0;
    
    for (uint32_t i = 0; i < input_len; i++) {
        uint32_t word_idx = i / 8;
        uint32_t byte_idx = i % 8;
        m[word_idx] |= uint64_t(input[i]) << (byte_idx * 8);
    }
    
    blake2b_compress(h, m, uint64_t(input_len), true);
    blake2b_emit(h, output);
}

// ============================================================================
// Compute R element on-the-fly using optimized blake2b_256_jhm
// r[j] = takeRight(31, Blake2b256(j || h || M))
// ============================================================================

void compute_r_element(
    uint32_t j_index,
    uint32_t height,
    device const uint64_t *M_words,
    thread uint8_t *r_out
) {
    uint8_t hash[32];
    blake2b_256_jhm(j_index, height, M_words, hash);
    
    for (int i = 0; i < 31; i++) {
        r_out[i] = hash[i + 1];
    }
}

// ============================================================================
// genIndexes — Generate k=32 pseudorandom indexes from seed
// ============================================================================

void gen_indexes(
    thread const uint8_t *seed,
    uint32_t seed_len,
    uint32_t N,
    thread uint32_t *indexes
) {
    uint8_t hash[32];
    blake2b_256_oneblock(seed, seed_len, hash);
    
    uint8_t extended[35];
    for (int i = 0; i < 32; i++) extended[i] = hash[i];
    extended[32] = hash[0];
    extended[33] = hash[1];
    extended[34] = hash[2];
    
    for (int i = 0; i < 32; i++) {
        uint32_t val = (uint32_t(extended[i]) << 24) |
                       (uint32_t(extended[i + 1]) << 16) |
                       (uint32_t(extended[i + 2]) << 8) |
                       uint32_t(extended[i + 3]);
        indexes[i] = val % N;
    }
}

// ============================================================================
// Mining Parameters (same struct layout as Rust side)
// ============================================================================

struct AutolykosMiningParams {
    uint64_t start_nonce;
    uint32_t height;
    uint32_t N;
    uint8_t  header_hash[32];
    uint8_t  target[32];
};

struct AutolykosMiningResult {
    uint64_t found_nonce;
    uint8_t  result_hash[32];
    uint32_t found;
};

// ============================================================================
// Compare hash < target (big-endian)
// ============================================================================

bool hash_below_target(thread const uint8_t *hash, device const uint8_t *target) {
    for (int i = 0; i < 32; i++) {
        if (hash[i] < target[i]) return true;
        if (hash[i] > target[i]) return false;
    }
    return false;
}

// ============================================================================
// BigInt Addition IN-PLACE (big-endian, 32 bytes)
// result += addend — eliminates temp array copy
// ============================================================================

void add_bigint_be32_inplace(thread uint8_t *result, thread const uint8_t *addend) {
    uint32_t carry = 0;
    for (int i = 31; i >= 0; i--) {
        uint32_t sum = uint32_t(result[i]) + uint32_t(addend[i]) + carry;
        result[i] = uint8_t(sum & 0xFF);
        carry = sum >> 8;
    }
}

// ============================================================================
// Main Autolykos2 Mining Kernel — TABLELESS OPTIMIZED
// ============================================================================

kernel void autolykos2_mine(
    device const AutolykosMiningParams& params [[buffer(0)]],
    device const uint8_t* M_raw [[buffer(1)]],
    device AutolykosMiningResult& result [[buffer(2)]],
    uint32_t thread_id [[thread_position_in_grid]]
) {
    device const uint64_t *M_words = (device const uint64_t *)M_raw;
    
    uint64_t nonce = params.start_nonce + uint64_t(thread_id);
    uint32_t N = params.N;
    uint32_t height = params.height;
    
    // Step 1: i = takeRight(8, Blake2b256(m || nonce)) mod N
    uint8_t mn_input[40];
    for (int j = 0; j < 32; j++) mn_input[j] = params.header_hash[j];
    mn_input[32] = uint8_t(nonce >> 56);
    mn_input[33] = uint8_t(nonce >> 48);
    mn_input[34] = uint8_t(nonce >> 40);
    mn_input[35] = uint8_t(nonce >> 32);
    mn_input[36] = uint8_t(nonce >> 24);
    mn_input[37] = uint8_t(nonce >> 16);
    mn_input[38] = uint8_t(nonce >>  8);
    mn_input[39] = uint8_t(nonce);
    
    uint8_t hash_i[32];
    blake2b_256_oneblock(mn_input, 40, hash_i);
    
    uint64_t i_val = (uint64_t(hash_i[24]) << 56) | (uint64_t(hash_i[25]) << 48) |
                     (uint64_t(hash_i[26]) << 40) | (uint64_t(hash_i[27]) << 32) |
                     (uint64_t(hash_i[28]) << 24) | (uint64_t(hash_i[29]) << 16) |
                     (uint64_t(hash_i[30]) <<  8) | uint64_t(hash_i[31]);
    uint32_t i_idx = uint32_t(i_val % uint64_t(N));
    
    // Step 2: e = takeRight(31, Blake2b256(i || h || M))
    uint8_t e[31];
    compute_r_element(i_idx, height, M_words, e);
    
    // Step 3: seed = e || m || nonce (71 bytes)
    uint8_t gen_seed[71];
    for (int j = 0; j < 31; j++) gen_seed[j] = e[j];
    for (int j = 0; j < 32; j++) gen_seed[31 + j] = params.header_hash[j];
    gen_seed[63] = uint8_t(nonce >> 56);
    gen_seed[64] = uint8_t(nonce >> 48);
    gen_seed[65] = uint8_t(nonce >> 40);
    gen_seed[66] = uint8_t(nonce >> 32);
    gen_seed[67] = uint8_t(nonce >> 24);
    gen_seed[68] = uint8_t(nonce >> 16);
    gen_seed[69] = uint8_t(nonce >>  8);
    gen_seed[70] = uint8_t(nonce);
    
    uint32_t indexes[32];
    gen_indexes(gen_seed, 71, N, indexes);
    
    // Step 4+5: R elements + BigInt accumulation IN-PLACE
    uint8_t f[32];
    for (int j = 0; j < 32; j++) f[j] = 0;
    
    for (int k = 0; k < 32; k++) {
        uint8_t r_elem[31];
        compute_r_element(indexes[k], height, M_words, r_elem);
        
        uint8_t elem32[32];
        elem32[0] = 0;
        for (int j = 0; j < 31; j++) elem32[j + 1] = r_elem[j];
        
        add_bigint_be32_inplace(f, elem32);
    }
    
    // Step 6: hash = Blake2b256(f)
    uint8_t final_hash[32];
    blake2b_256_oneblock(f, 32, final_hash);
    
    // Step 7: Check if hash < target
    if (hash_below_target(final_hash, params.target)) {
        uint32_t expected = 0;
        if (atomic_compare_exchange_weak_explicit(
            (device atomic_uint*)&result.found,
            &expected, 1u,
            memory_order_relaxed,
            memory_order_relaxed
        )) {
            result.found_nonce = nonce;
            for (int j = 0; j < 32; j++) {
                result.result_hash[j] = final_hash[j];
            }
        }
    }
}

// ============================================================================
// Benchmark Kernel — TABLELESS OPTIMIZED
// ============================================================================

kernel void autolykos2_benchmark(
    device const AutolykosMiningParams& params [[buffer(0)]],
    device const uint8_t* M_raw [[buffer(1)]],
    device AutolykosMiningResult& result [[buffer(2)]],
    uint32_t thread_id [[thread_position_in_grid]]
) {
    device const uint64_t *M_words = (device const uint64_t *)M_raw;
    
    uint64_t nonce = params.start_nonce + uint64_t(thread_id);
    uint32_t N = params.N;
    uint32_t height = params.height;
    
    uint8_t mn_input[40];
    for (int j = 0; j < 32; j++) mn_input[j] = params.header_hash[j];
    mn_input[32] = uint8_t(nonce >> 56);
    mn_input[33] = uint8_t(nonce >> 48);
    mn_input[34] = uint8_t(nonce >> 40);
    mn_input[35] = uint8_t(nonce >> 32);
    mn_input[36] = uint8_t(nonce >> 24);
    mn_input[37] = uint8_t(nonce >> 16);
    mn_input[38] = uint8_t(nonce >>  8);
    mn_input[39] = uint8_t(nonce);
    
    uint8_t hash_i[32];
    blake2b_256_oneblock(mn_input, 40, hash_i);
    
    uint64_t i_val = (uint64_t(hash_i[24]) << 56) | (uint64_t(hash_i[25]) << 48) |
                     (uint64_t(hash_i[26]) << 40) | (uint64_t(hash_i[27]) << 32) |
                     (uint64_t(hash_i[28]) << 24) | (uint64_t(hash_i[29]) << 16) |
                     (uint64_t(hash_i[30]) <<  8) | uint64_t(hash_i[31]);
    uint32_t i_idx = uint32_t(i_val % uint64_t(N));
    
    uint8_t e[31];
    compute_r_element(i_idx, height, M_words, e);
    
    uint8_t gen_seed[71];
    for (int j = 0; j < 31; j++) gen_seed[j] = e[j];
    for (int j = 0; j < 32; j++) gen_seed[31 + j] = params.header_hash[j];
    gen_seed[63] = uint8_t(nonce >> 56);
    gen_seed[64] = uint8_t(nonce >> 48);
    gen_seed[65] = uint8_t(nonce >> 40);
    gen_seed[66] = uint8_t(nonce >> 32);
    gen_seed[67] = uint8_t(nonce >> 24);
    gen_seed[68] = uint8_t(nonce >> 16);
    gen_seed[69] = uint8_t(nonce >>  8);
    gen_seed[70] = uint8_t(nonce);
    
    uint32_t indexes[32];
    gen_indexes(gen_seed, 71, N, indexes);
    
    uint8_t f[32];
    for (int j = 0; j < 32; j++) f[j] = 0;
    
    for (int k = 0; k < 32; k++) {
        uint8_t r_elem[31];
        compute_r_element(indexes[k], height, M_words, r_elem);
        
        uint8_t elem32[32];
        elem32[0] = 0;
        for (int j = 0; j < 31; j++) elem32[j + 1] = r_elem[j];
        
        add_bigint_be32_inplace(f, elem32);
    }
    
    uint8_t final_hash[32];
    blake2b_256_oneblock(f, 32, final_hash);
    
    for (int j = 0; j < 32; j++) {
        result.result_hash[j] = final_hash[j];
    }
}
