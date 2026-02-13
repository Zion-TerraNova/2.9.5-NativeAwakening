/// Reward Calculator — Clean L1 (constant emission, no consciousness bonus)
///
/// Block reward: 5,400.067 ZION (constant for 45 years)
/// Distribution: 89% miner, 10% humanitarian tithe, 1% pool fee

use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

/// Constant block reward: 5,400.067 ZION
pub const BASE_BLOCK_REWARD: Decimal = dec!(5400.067);

/// Default pool fee: 1%
pub const DEFAULT_POOL_FEE_PERCENT: Decimal = dec!(1.0);

/// Default humanitarian tithe: 10%
pub const DEFAULT_TITHE_PERCENT: Decimal = dec!(10.0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RewardBreakdown {
    pub total_reward: String,
    pub miner_share: String,
    pub humanitarian_tithe: String,
    pub pool_fee: String,
}

pub struct RewardCalculator {
    pool_fee_percent: Decimal,
    tithe_percent: Decimal,
}

impl RewardCalculator {
    /// Create new RewardCalculator
    pub fn new(pool_fee_percent: Option<f64>, tithe_percent: Option<f64>) -> Self {
        let pool_fee = pool_fee_percent
            .map(|p| Decimal::from_f64_retain(p).unwrap_or(DEFAULT_POOL_FEE_PERCENT))
            .unwrap_or(DEFAULT_POOL_FEE_PERCENT);

        let tithe = tithe_percent
            .map(|p| Decimal::from_f64_retain(p).unwrap_or(DEFAULT_TITHE_PERCENT))
            .unwrap_or(DEFAULT_TITHE_PERCENT);

        tracing::info!(
            "RewardCalculator: pool_fee={}%, tithe={}%",
            pool_fee,
            tithe
        );

        Self {
            pool_fee_percent: pool_fee,
            tithe_percent: tithe,
        }
    }

    /// Calculate total block reward (constant — no consciousness bonus on L1)
    pub fn calculate_block_reward(&self) -> Decimal {
        BASE_BLOCK_REWARD
    }

    /// Calculate complete reward breakdown
    pub fn calculate_reward_breakdown(&self) -> RewardBreakdown {
        let total_reward = BASE_BLOCK_REWARD;

        // Humanitarian tithe: 10%
        let humanitarian_tithe = total_reward * (self.tithe_percent / dec!(100));

        // Pool fee: 1%
        let pool_fee = total_reward * (self.pool_fee_percent / dec!(100));

        // Miner gets the rest: 89%
        let miner_share = total_reward - humanitarian_tithe - pool_fee;

        RewardBreakdown {
            total_reward: total_reward.to_string(),
            miner_share: miner_share.to_string(),
            humanitarian_tithe: humanitarian_tithe.to_string(),
            pool_fee: pool_fee.to_string(),
        }
    }

    /// Calculate PPLNS payout for a miner
    pub fn calculate_pplns_payout(
        &self,
        miner_shares: u64,
        total_shares: u64,
    ) -> Result<Decimal> {
        if total_shares == 0 {
            return Ok(Decimal::ZERO);
        }

        let breakdown = self.calculate_reward_breakdown();
        let miner_share: Decimal = breakdown.miner_share.parse()?;

        let share_ratio = Decimal::from(miner_shares) / Decimal::from(total_shares);
        let payout = miner_share * share_ratio;

        Ok(payout)
    }

    /// Get pool fee percentage
    pub fn pool_fee_percent(&self) -> Decimal {
        self.pool_fee_percent
    }

    /// Get tithe percentage
    pub fn tithe_percent(&self) -> Decimal {
        self.tithe_percent
    }
}

impl Default for RewardCalculator {
    fn default() -> Self {
        Self::new(None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_reward() {
        let calc = RewardCalculator::default();
        assert_eq!(calc.calculate_block_reward(), dec!(5400.067));
    }

    #[test]
    fn test_reward_breakdown() {
        let calc = RewardCalculator::default();
        let breakdown = calc.calculate_reward_breakdown();

        let total: Decimal = breakdown.total_reward.parse().unwrap();
        let miner: Decimal = breakdown.miner_share.parse().unwrap();
        let tithe: Decimal = breakdown.humanitarian_tithe.parse().unwrap();
        let fee: Decimal = breakdown.pool_fee.parse().unwrap();

        // Must add up
        assert_eq!(total, miner + tithe + fee);
        assert_eq!(total, dec!(5400.067));

        // Verify percentages
        let tithe_pct = (tithe / total) * dec!(100);
        let fee_pct = (fee / total) * dec!(100);
        assert!((tithe_pct - dec!(10)).abs() < dec!(0.1));
        assert!((fee_pct - dec!(1)).abs() < dec!(0.1));
    }

    #[test]
    fn test_pplns_payout() {
        let calc = RewardCalculator::default();

        // Miner has 100 shares out of 1000 (10%)
        let payout = calc.calculate_pplns_payout(100, 1000).unwrap();

        let breakdown = calc.calculate_reward_breakdown();
        let miner_share: Decimal = breakdown.miner_share.parse().unwrap();
        let expected = miner_share * dec!(0.1);

        assert_eq!(payout, expected);
    }

    #[test]
    fn test_zero_shares() {
        let calc = RewardCalculator::default();
        let payout = calc.calculate_pplns_payout(0, 1000).unwrap();
        assert_eq!(payout, Decimal::ZERO);
    }

    #[test]
    fn test_zero_total_shares() {
        let calc = RewardCalculator::default();
        let payout = calc.calculate_pplns_payout(100, 0).unwrap();
        assert_eq!(payout, Decimal::ZERO);
    }
}
