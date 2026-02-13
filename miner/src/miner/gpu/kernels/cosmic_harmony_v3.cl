// Cosmic Harmony v3 - OpenCL Kernel
// Pipeline: Keccak256 -> SHA3-512 -> GoldenMatrix -> CosmicFusion

#pragma OPENCL EXTENSION cl_khr_int64_base_atomics : enable
#pragma OPENCL EXTENSION cl_khr_int64_extended_atomics : enable

// ============================================================================
// Constants
// ============================================================================

__constant ulong PHI_POWERS[16] = {
    0x9E3779B97F4A7C15UL,
    0xC6EF3720A5F82D14UL,
    0x93C467E37DB0C7A4UL,
    0xD76AA478E8C7B756UL,
    0xB7E15162E8F85F94UL,
    0x8AED2A6ABF715880UL,
    0xA8EDA8A6C43E3EF5UL,
    0xC5D2460186F7233CUL,
    0xE6546B64A8E3F7BCUL,
    0xF7E294D5C7F82A8DUL,
    0xD8A4E5F6C9B7A8E3UL,
    0xB9C5D6E7F8A9B0C1UL,
    0xCAD6E7F8091A2B3CUL,
    0xDBE7F8091A2B3C4DUL,
    0xECF8091A2B3C4D5EUL,
    0xFD091A2B3C4D5E6FUL,
};

__constant ulong KECCAK_RC[24] = {
    0x0000000000000001UL, 0x0000000000008082UL, 0x800000000000808AUL,
    0x8000000080008000UL, 0x000000000000808BUL, 0x0000000080000001UL,
    0x8000000080008081UL, 0x8000000000008009UL, 0x000000000000008AUL,
    0x0000000000000088UL, 0x0000000080008009UL, 0x000000008000000AUL,
    0x000000008000808BUL, 0x800000000000008BUL, 0x8000000000008089UL,
    0x8000000000008003UL, 0x8000000000008002UL, 0x8000000000000080UL,
    0x000000000000800AUL, 0x800000008000000AUL, 0x8000000080008081UL,
    0x8000000000008080UL, 0x0000000080000001UL, 0x8000000080008008UL,
};

__constant int KECCAK_PILN[24] = {
    10, 7, 11, 17, 18, 3, 5, 16, 8, 21, 24, 4,
    15, 23, 19, 13, 12, 2, 20, 14, 22, 9, 6, 1
};

__constant int KECCAK_ROTC[24] = {
    1, 3, 6, 10, 15, 21, 28, 36, 45, 55, 2, 14,
    27, 41, 56, 8, 25, 43, 62, 18, 39, 61, 20, 44
};

// ============================================================================
// Helper Functions
// ============================================================================

inline ulong rotl64(ulong x, int n) {
    return (x << n) | (x >> (64 - n)); // OpenCL rotate built-in exists but explicit is clearer
}

// ============================================================================
// Keccak-f[1600]
// ============================================================================

void keccak_f1600(ulong *state) {
    ulong bc[5];
    ulong t;
    
    #pragma unroll
    for (int round = 0; round < 24; round++) {
        // Theta
        for (int i = 0; i < 5; i++) {
            bc[i] = state[i] ^ state[i + 5] ^ state[i + 10] ^ state[i + 15] ^ state[i + 20];
        }
        
        for (int i = 0; i < 5; i++) {
            t = bc[(i + 4) % 5] ^ rotl64(bc[(i + 1) % 5], 1);
            for (int j = 0; j < 25; j += 5) {
                state[j + i] ^= t;
            }
        }
        
        // Rho and Pi
        t = state[1];
        for (int i = 0; i < 24; i++) {
            int j = KECCAK_PILN[i];
            bc[0] = state[j];
            state[j] = rotl64(t, KECCAK_ROTC[i]);
            t = bc[0];
        }
        
        // Chi
        for (int j = 0; j < 25; j += 5) {
            for (int i = 0; i < 5; i++) {
                bc[i] = state[j + i];
            }
            for (int i = 0; i < 5; i++) {
                state[j + i] ^= (~bc[(i + 1) % 5]) & bc[(i + 2) % 5];
            }
        }
        
        // Iota
        state[0] ^= KECCAK_RC[round];
    }
}

// ============================================================================
// Algorithms
// ============================================================================

void keccak256(const uchar* input, int input_len, ulong* output_words) {
    ulong state[25] = {0};
    
    // Absorb
    for (int i = 0; i < input_len/8; i++) {
       ulong word = 0;
       for (int b=0; b<8; b++) word |= ((ulong)input[i*8+b]) << (b*8);
       state[i] ^= word;
    }
    
    int word_idx = input_len / 8;
    int byte_idx = input_len % 8;
    
    ulong pad_word = 0;
    pad_word |= ((ulong)0x01) << (byte_idx * 8);
    state[word_idx] ^= pad_word;
    
    // 0x80 at byte 135 (Word 16, byte 7)
    state[16] ^= 0x8000000000000000UL;
    
    keccak_f1600(state);
    
    for (int i = 0; i < 4; i++) {
        output_words[i] = state[i];
    }
}

void sha3_512(const ulong* input_words, ulong* output_words) {
    ulong state[25] = {0};
    
    for (int i = 0; i < 4; i++) {
        state[i] ^= input_words[i];
    }
    
    // SHA3 Padding 0x06...0x80
    state[4] ^= 0x06;
    state[8] ^= 0x8000000000000000UL;
    
    keccak_f1600(state);
    
    for (int i = 0; i < 8; i++) {
        output_words[i] = state[i];
    }
}

void golden_matrix(ulong* state) {
    for (int i = 0; i < 8; i++) {
        ulong phi = PHI_POWERS[i % 16];
        ulong neighbor = state[(i + 1) % 8];
        state[i] = state[i] ^ (neighbor * phi);
        state[i] = rotl64(state[i], (i * 7) % 64);
    }
    
    for (int round = 0; round < 4; round++) {
        ulong temp[8]; 
        for(int k=0; k<8; k++) temp[k] = state[k];
        
        for (int i = 0; i < 8; i++) {
            state[i] ^= rotl64(state[(i + 3) % 8], 17);
            state[i] += state[(i + 5) % 8];
        }
    }
}

void cosmic_fusion(ulong* state) {
    for (int round = 0; round < 8; round++) {
        ulong phi = PHI_POWERS[round];
        
        for (int i = 0; i < 8; i++) {
            state[i] += state[(i + 1) % 8];
            state[i] = rotl64(state[i], 13);
            state[i] ^= phi;
            state[i] ^= (state[(i + 4) % 8] >> 7);
            state[(i + 2) % 8] += state[i];
        }
    }
}

// ============================================================================
// Main Kernel
// ============================================================================

__kernel void cosmic_harmony_v3_mine(
    __global const uchar* header,  
    const uint header_len,
    const ulong start_nonce,
    const ulong target_difficulty, 
    __global ulong* results,   
    __global uint* result_count
) {
    uint tid = get_global_id(0);
    ulong nonce = start_nonce + tid;
    
    uchar local_input[144]; 
    
    for (int i = 0; i < header_len; i++) {
        local_input[i] = header[i];
    }
    
    for (int i = 0; i < 8; i++) {
        local_input[header_len + i] = (nonce >> (i * 8)) & 0xFF;
    }
    int total_len = header_len + 8;
    
    // Stage 1
    ulong stage1[4];
    keccak256(local_input, total_len, stage1);
    
    // Stage 2
    ulong stage2[8];
    sha3_512(stage1, stage2);
    
    // Stage 3
    golden_matrix(stage2);
    
    // Stage 4
    cosmic_fusion(stage2);
    
    // Check
    ulong final_hash_high = stage2[3] ^ stage2[7]; // Matching logic approximately. 
    // Wait, Metal logic was: final_state[i] = stage2[i] ^ stage2[i+4];
    // And verification on final_state[3].
    
    // Correct check:
    ulong final_word_3 = stage2[3] ^ stage2[7];
    
    if (final_word_3 <= target_difficulty) {
        if (atomic_xchg(result_count, 1) == 0) {
            results[0] = 1;
            results[1] = nonce;
        }
    }
}
