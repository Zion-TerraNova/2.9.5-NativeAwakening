/// ZION Consensus — Difficulty Adjustment Algorithm
///
/// LWMA (Linearly Weighted Moving Average) as specified in MAINNET_CONSTITUTION:
///   - Target block time:    60 seconds
///   - Window size:          60 blocks
///   - Max change per block: ±25%
///   - Timestamp sanity:     clamp ±2× target (30–120s per solve time)
///   - Min difficulty:       1,000
///   - Max difficulty:       u64::MAX / 1000
///
/// Reference: Zawy's LWMA (used by Monero, Grin, LOKI, etc.)
/// https://github.com/zawy12/difficulty-algorithms/issues/3

// AUDIT-FIX P1-02: Removed dead `check()` function that always returned true.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Target block time in seconds
pub const TARGET_BLOCK_TIME: u64 = 60;

/// LWMA window size (number of previous blocks to consider)
pub const LWMA_WINDOW: u64 = 60;

/// Maximum per-block difficulty adjustment: +25%
pub const MAX_ADJUSTMENT_UP: f64 = 1.25;

/// Maximum per-block difficulty adjustment: −25%
pub const MAX_ADJUSTMENT_DOWN: f64 = 0.75;

/// Minimum allowed solve time per block (clamped)
pub const MIN_SOLVE_TIME: u64 = 30; // TARGET / 2

/// Maximum allowed solve time per block (clamped)
pub const MAX_SOLVE_TIME: u64 = 120; // TARGET × 2

/// Minimum difficulty floor
pub const MIN_DIFFICULTY: u64 = 1_000;

/// Maximum difficulty ceiling
pub const MAX_DIFFICULTY: u64 = u64::MAX / 1_000;

// ---------------------------------------------------------------------------
// Target Calculations
// ---------------------------------------------------------------------------

/// Calculate target from difficulty for full 256-bit comparison
pub fn target_from_difficulty_256(difficulty: u64) -> String {
    use num_bigint::BigUint;
    use num_traits::Num;

    let d = difficulty.max(1);
    let max = BigUint::from_str_radix(
        "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        16,
    )
    .unwrap();

    let target = max / BigUint::from(d);
    format!("{:0>64}", target.to_str_radix(16))
}

pub fn target_u64_from_difficulty(difficulty: u64) -> u64 {
    let d = difficulty.max(1);
    u64::MAX / d
}

pub fn target_u32_from_difficulty(difficulty: u64) -> u32 {
    let d = difficulty.max(1);
    let t = (u32::MAX as u64) / d;
    t.min(u32::MAX as u64) as u32
}

pub fn target_u128_from_difficulty(difficulty: u64) -> u128 {
    let d = difficulty.max(1);
    u128::MAX / (d as u128)
}

pub fn target_from_difficulty(difficulty: u64) -> String {
    let low64 = target_u64_from_difficulty(difficulty);
    format!("{}{:016x}", "0".repeat(48), low64)
}

// ---------------------------------------------------------------------------
// LWMA Difficulty Adjustment
// ---------------------------------------------------------------------------

/// Block timestamp + difficulty pair used for LWMA input.
#[derive(Debug, Clone, Copy)]
pub struct BlockInfo {
    pub timestamp: u64,
    pub difficulty: u64,
}

/// Calculate next difficulty using LWMA (Linearly Weighted Moving Average).
///
/// # Arguments
/// * `window` — Slice of the last N `BlockInfo` entries, **oldest first**.
///   The slice length should be `LWMA_WINDOW + 1` (we need N+1 timestamps
///   to derive N solve-times).  If fewer blocks exist (early chain), the
///   algorithm adapts gracefully.
///
/// # Returns
/// The difficulty for the *next* block to be mined.
///
/// # Algorithm
/// ```text
/// For i in 1..=N:
///     solve_time[i] = clamp(ts[i] − ts[i−1], MIN_SOLVE_TIME, MAX_SOLVE_TIME)
///     weight        = i                          // linear weight: recent = heavier
///     weighted_sum += solve_time[i] × weight
///     difficulty_sum += difficulty[i] × weight
///
/// avg_target = weighted_sum / (sum_of_weights)   // weighted average solve-time
/// next_diff  = difficulty_sum × TARGET / avg_target / sum_of_weights
/// next_diff  = clamp(next_diff, prev × 0.75, prev × 1.25)
/// next_diff  = clamp(next_diff, MIN_DIFFICULTY, MAX_DIFFICULTY)
/// ```
pub fn lwma_next_difficulty(window: &[BlockInfo]) -> u64 {
    // Need at least 2 entries to compute 1 solve-time
    if window.len() < 2 {
        return window
            .last()
            .map(|b| b.difficulty.max(MIN_DIFFICULTY))
            .unwrap_or(MIN_DIFFICULTY);
    }

    let n = window.len() - 1; // number of solve-time intervals

    let mut weighted_solve_sum: u128 = 0;
    let mut weighted_diff_sum: u128 = 0;
    let mut weight_sum: u128 = 0;

    for i in 1..=n {
        let raw_solve = window[i]
            .timestamp
            .saturating_sub(window[i - 1].timestamp);

        // Clamp solve time to [MIN_SOLVE_TIME, MAX_SOLVE_TIME]
        let solve_time = raw_solve.max(MIN_SOLVE_TIME).min(MAX_SOLVE_TIME);

        let weight = i as u128; // linear weight: 1, 2, 3, …, N
        weighted_solve_sum += solve_time as u128 * weight;
        weighted_diff_sum += window[i].difficulty as u128 * weight;
        weight_sum += weight;
    }

    if weight_sum == 0 || weighted_solve_sum == 0 {
        return window.last().unwrap().difficulty.max(MIN_DIFFICULTY);
    }

    // next_diff = weighted_diff_sum × TARGET / weighted_solve_sum
    // (this is equivalent to: avg_diff × TARGET / avg_solve_time)
    let next_diff_128 =
        weighted_diff_sum * TARGET_BLOCK_TIME as u128 / weighted_solve_sum;

    let mut next_diff = if next_diff_128 > MAX_DIFFICULTY as u128 {
        MAX_DIFFICULTY
    } else {
        next_diff_128 as u64
    };

    // ±25% clamp relative to the most recent block's difficulty
    let prev_diff = window.last().unwrap().difficulty;
    let max_allowed = (prev_diff as f64 * MAX_ADJUSTMENT_UP) as u64;
    let min_allowed = (prev_diff as f64 * MAX_ADJUSTMENT_DOWN) as u64;
    next_diff = next_diff.min(max_allowed).max(min_allowed);

    // Global floor / ceiling
    next_diff.max(MIN_DIFFICULTY).min(MAX_DIFFICULTY)
}

/// Simple single-block fallback (kept for backward compatibility with
/// `validation.rs` during the transition period, and for dev-mode use).
///
/// In production, prefer `lwma_next_difficulty()` with a full window.
pub fn calculate_next_difficulty(
    current_difficulty: u64,
    actual_time_secs: u64,
    target_time_secs: u64,
) -> u64 {
    if actual_time_secs == 0 {
        return current_difficulty;
    }

    let ratio = target_time_secs as f64 / actual_time_secs as f64;
    let clamped = ratio.max(MAX_ADJUSTMENT_DOWN).min(MAX_ADJUSTMENT_UP);

    let new_diff = (current_difficulty as f64 * clamped) as u64;
    new_diff.max(MIN_DIFFICULTY).min(MAX_DIFFICULTY)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- Target tests (unchanged) ---

    #[test]
    fn test_target_from_difficulty() {
        let t1 = target_from_difficulty(1);
        assert!(t1.starts_with("0"));

        let t_low = target_u64_from_difficulty(100);
        let t_high = target_u64_from_difficulty(1000);
        assert!(t_high < t_low);
    }

    #[test]
    fn test_target_256_bits() {
        let t = target_from_difficulty_256(1000);
        assert_eq!(t.len(), 64);

        let t1 = target_from_difficulty_256(1);
        let t2 = target_from_difficulty_256(2);
        assert!(t1 > t2);
    }

    // --- LWMA tests ---

    /// Helper: build a window of N+1 blocks with given difficulty and
    /// constant solve time.
    fn make_window(n: usize, base_diff: u64, solve_time: u64) -> Vec<BlockInfo> {
        let mut window = Vec::with_capacity(n + 1);
        for i in 0..=n {
            window.push(BlockInfo {
                timestamp: 1_000_000 + (i as u64) * solve_time,
                difficulty: base_diff,
            });
        }
        window
    }

    #[test]
    fn test_lwma_perfect_timing() {
        // All blocks at exactly TARGET_BLOCK_TIME → difficulty should stay ~same
        let window = make_window(60, 10_000, TARGET_BLOCK_TIME);
        let next = lwma_next_difficulty(&window);
        assert_eq!(next, 10_000, "Perfect timing should keep difficulty unchanged");
    }

    #[test]
    fn test_lwma_blocks_too_fast() {
        // Blocks coming every 30s (half target) → difficulty should increase
        let window = make_window(60, 10_000, 30);
        let next = lwma_next_difficulty(&window);
        // Clamped to +25% max
        assert_eq!(next, 12_500, "Fast blocks should increase diff by 25% (clamped)");
    }

    #[test]
    fn test_lwma_blocks_too_slow() {
        // Blocks coming every 120s (double target) → difficulty should decrease
        let window = make_window(60, 10_000, 120);
        let next = lwma_next_difficulty(&window);
        // Clamped to −25% max
        assert_eq!(next, 7_500, "Slow blocks should decrease diff by 25% (clamped)");
    }

    #[test]
    fn test_lwma_clamp_up() {
        // Extremely fast blocks (1s) but clamped by MAX_SOLVE_TIME → still +25% max
        let window = make_window(60, 10_000, 1);
        let next = lwma_next_difficulty(&window);
        // Solve times clamped to MIN_SOLVE_TIME=30s, so 60/30 = 2× but ±25% clamp
        assert_eq!(next, 12_500);
    }

    #[test]
    fn test_lwma_clamp_down() {
        // Extremely slow blocks (600s) but clamped
        let window = make_window(60, 10_000, 600);
        let next = lwma_next_difficulty(&window);
        // Solve times clamped to MAX_SOLVE_TIME=120s, 60/120 = 0.5× but ±25% clamp
        assert_eq!(next, 7_500);
    }

    #[test]
    fn test_lwma_minimum_difficulty() {
        // Very low difficulty should floor at MIN_DIFFICULTY
        let window = make_window(60, MIN_DIFFICULTY, 120);
        let next = lwma_next_difficulty(&window);
        assert!(next >= MIN_DIFFICULTY);
    }

    #[test]
    fn test_lwma_short_window() {
        // Only 2 blocks (1 solve time) — should still work
        let window = vec![
            BlockInfo { timestamp: 1000, difficulty: 5_000 },
            BlockInfo { timestamp: 1060, difficulty: 5_000 },
        ];
        let next = lwma_next_difficulty(&window);
        assert_eq!(next, 5_000, "2-block window with perfect time");
    }

    #[test]
    fn test_lwma_single_block() {
        // Only 1 block — return its difficulty
        let window = vec![BlockInfo { timestamp: 1000, difficulty: 8_000 }];
        let next = lwma_next_difficulty(&window);
        assert_eq!(next, 8_000);
    }

    #[test]
    fn test_lwma_empty_window() {
        let next = lwma_next_difficulty(&[]);
        assert_eq!(next, MIN_DIFFICULTY);
    }

    #[test]
    fn test_lwma_recent_blocks_weighted_more() {
        // First 50 blocks at 60s, last 10 blocks at 30s
        // Recent blocks are weighted more → difficulty should increase
        let mut window = Vec::new();
        let mut ts = 1_000_000u64;
        // Block 0 (anchor)
        window.push(BlockInfo { timestamp: ts, difficulty: 10_000 });

        // Blocks 1-50: 60s solve time (perfect)
        for _ in 1..=50 {
            ts += 60;
            window.push(BlockInfo { timestamp: ts, difficulty: 10_000 });
        }
        // Blocks 51-60: 30s solve time (fast)
        for _ in 51..=60 {
            ts += 30;
            window.push(BlockInfo { timestamp: ts, difficulty: 10_000 });
        }

        let next = lwma_next_difficulty(&window);
        // Should increase — recent fast blocks outweigh older normal blocks
        assert!(next > 10_000, "Recent fast blocks should increase difficulty: {}", next);
    }

    #[test]
    fn test_lwma_stability_simulation() {
        // Simulate 200 blocks with slight variance, verify convergence
        let mut blocks: Vec<BlockInfo> = vec![
            BlockInfo { timestamp: 1_000_000, difficulty: 10_000 }
        ];

        // Simulate blocks with alternating fast/slow times
        let solve_times = [55u64, 65, 58, 62, 50, 70, 57, 63, 59, 61];
        let mut ts = 1_000_000u64;

        for i in 0..200 {
            let st = solve_times[i % solve_times.len()];
            ts += st;

            // Get window (last 61 blocks)
            let start = if blocks.len() > LWMA_WINDOW as usize {
                blocks.len() - LWMA_WINDOW as usize - 1
            } else {
                0
            };
            let window = &blocks[start..];
            let diff = lwma_next_difficulty(window);

            blocks.push(BlockInfo { timestamp: ts, difficulty: diff });
        }

        // After 200 blocks, difficulty should be within reasonable range
        let final_diff = blocks.last().unwrap().difficulty;
        assert!(final_diff >= 5_000 && final_diff <= 20_000,
            "After 200 varied blocks, difficulty {} should stabilize near 10k", final_diff);
    }

    #[test]
    fn test_lwma_no_overflow() {
        // High difficulty should not overflow
        let window = make_window(60, MAX_DIFFICULTY / 2, 30);
        let next = lwma_next_difficulty(&window);
        assert!(next > 0 && next <= MAX_DIFFICULTY);
    }

    #[test]
    fn test_lwma_deterministic() {
        // Same input → same output
        let window = make_window(60, 10_000, 45);
        let r1 = lwma_next_difficulty(&window);
        let r2 = lwma_next_difficulty(&window);
        let r3 = lwma_next_difficulty(&window);
        assert_eq!(r1, r2);
        assert_eq!(r2, r3);
    }

    // --- Legacy single-block DAA tests ---

    #[test]
    fn test_simple_daa_compatibility() {
        // Fast blocks → increase (clamped to 25%)
        let d = calculate_next_difficulty(10_000, 30, 60);
        assert_eq!(d, 12_500); // was 20_000 with old 4× clamp

        // Slow blocks → decrease (clamped to 25%)
        let d = calculate_next_difficulty(10_000, 120, 60);
        assert_eq!(d, 7_500); // was 5_000 with old 4× clamp

        // Perfect timing → no change
        let d = calculate_next_difficulty(10_000, 60, 60);
        assert_eq!(d, 10_000);
    }

    #[test]
    fn test_simple_daa_zero_time() {
        let d = calculate_next_difficulty(10_000, 0, 60);
        assert_eq!(d, 10_000);
    }

    #[test]
    fn test_simple_daa_floor() {
        let d = calculate_next_difficulty(MIN_DIFFICULTY, 120, 60);
        assert!(d >= MIN_DIFFICULTY);
    }
}
