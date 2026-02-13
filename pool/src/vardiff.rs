use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub struct VarDiffConfig {
    /// Desired average time between accepted shares.
    pub target_share_time: Duration,
    /// How often to retarget.
    pub retarget_time: Duration,
    /// Ignore small fluctuations; only retarget if deviation exceeds this fraction.
    pub variance: f64,
    pub min_difficulty: u64,
    pub max_difficulty: u64,
}

impl Default for VarDiffConfig {
    fn default() -> Self {
        Self {
            target_share_time: Duration::from_secs(15),
            retarget_time: Duration::from_secs(30),
            variance: 0.25,
            min_difficulty: 1000,
            max_difficulty: 10_000_000_000,
        }
    }
}

impl VarDiffConfig {
    /// Optional env overrides (useful for tests / tuning):
    /// - ZION_VARDIFF_TARGET_SHARE_SECS
    /// - ZION_VARDIFF_RETARGET_SECS
    /// - ZION_VARDIFF_VARIANCE
    /// - ZION_VARDIFF_MIN_DIFFICULTY
    /// - ZION_VARDIFF_MAX_DIFFICULTY
    pub fn from_env_or_default() -> Self {
        let mut cfg = Self::default();

        if let Ok(v) = std::env::var("ZION_VARDIFF_TARGET_SHARE_SECS") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.target_share_time = Duration::from_secs(n);
            }
        }
        if let Ok(v) = std::env::var("ZION_VARDIFF_RETARGET_SECS") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.retarget_time = Duration::from_secs(n);
            }
        }
        if let Ok(v) = std::env::var("ZION_VARDIFF_VARIANCE") {
            if let Ok(n) = v.parse::<f64>() {
                if n.is_finite() && n >= 0.0 {
                    cfg.variance = n;
                }
            }
        }
        if let Ok(v) = std::env::var("ZION_VARDIFF_MIN_DIFFICULTY") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.min_difficulty = n.max(1);
            }
        }
        if let Ok(v) = std::env::var("ZION_VARDIFF_MAX_DIFFICULTY") {
            if let Ok(n) = v.parse::<u64>() {
                cfg.max_difficulty = n.max(cfg.min_difficulty);
            }
        }

        cfg
    }
}

#[derive(Debug, Clone)]
pub struct VarDiffState {
    cfg: VarDiffConfig,
    last_retarget: Instant,
    accepted_since: u64,
}

impl VarDiffState {
    pub fn new(cfg: Option<VarDiffConfig>) -> Self {
        Self {
            cfg: cfg.unwrap_or_else(VarDiffConfig::from_env_or_default),
            last_retarget: Instant::now(),
            accepted_since: 0,
        }
    }

    /// Record a share and optionally retarget difficulty.
    ///
    /// Returns `Some(new_difficulty)` if a retarget occurred and difficulty changed.
    pub fn on_share(
        &mut self,
        now: Instant,
        accepted: bool,
        current_difficulty: u64,
    ) -> Option<u64> {
        if accepted {
            self.accepted_since = self.accepted_since.saturating_add(1);
        }

        let elapsed = now.saturating_duration_since(self.last_retarget);
        if elapsed < self.cfg.retarget_time {
            return None;
        }

        // If no accepted shares, reset window and keep current difficulty.
        if self.accepted_since == 0 {
            self.last_retarget = now;
            return None;
        }

        let elapsed_secs = elapsed.as_secs_f64().max(0.000_001);
        let avg_share_time = elapsed_secs / (self.accepted_since as f64);
        let target = self.cfg.target_share_time.as_secs_f64().max(0.000_001);

        let ratio = target / avg_share_time;
        let lower = 1.0 - self.cfg.variance;
        let upper = 1.0 + self.cfg.variance;

        self.last_retarget = now;
        self.accepted_since = 0;

        if ratio >= lower && ratio <= upper {
            return None;
        }

        let cur = (current_difficulty.max(1)) as f64;
        let mut next = (cur * ratio).round();
        if !next.is_finite() || next <= 0.0 {
            next = 1.0;
        }

        let next = (next as u64)
            .clamp(self.cfg.min_difficulty, self.cfg.max_difficulty)
            .max(1);

        if next == current_difficulty {
            None
        } else {
            Some(next)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vardiff_increases_difficulty_when_shares_too_fast() {
        let cfg = VarDiffConfig {
            target_share_time: Duration::from_secs(10),
            retarget_time: Duration::from_secs(10),
            variance: 0.0,
            min_difficulty: 1,
            max_difficulty: 1_000_000,
        };
        let mut st = VarDiffState::new(Some(cfg));
        let start = st.last_retarget;
        // 10 accepted shares in 10s => avg 1s, target 10s => ratio 10 => diff increases.
        for i in 0..10 {
            let _ = st.on_share(start + Duration::from_secs(i), true, 100);
        }
        let next = st.on_share(start + Duration::from_secs(10), true, 100);
        assert!(next.is_some());
        assert!(next.unwrap() > 100);
    }

    #[test]
    fn vardiff_decreases_difficulty_when_shares_too_slow() {
        let cfg = VarDiffConfig {
            target_share_time: Duration::from_secs(10),
            retarget_time: Duration::from_secs(20),
            variance: 0.0,
            min_difficulty: 1,
            max_difficulty: 1_000_000,
        };
        let mut st = VarDiffState::new(Some(cfg));
        let start = st.last_retarget;
        // 1 accepted share in 20s => avg 20s, ratio 0.5 => diff decreases
        let next = st.on_share(start + Duration::from_secs(20), true, 100);
        assert!(next.is_some());
        assert!(next.unwrap() < 100);
    }
}

