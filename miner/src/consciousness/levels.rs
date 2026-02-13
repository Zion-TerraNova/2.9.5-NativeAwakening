use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsciousnessLevel {
    Physical,
    Mental,
    Cosmic,
    OnTheStar,
}

impl ConsciousnessLevel {
    pub fn from_xp(xp: u64) -> Self {
        match xp {
            0..=999 => Self::Physical,
            1000..=9999 => Self::Mental,
            10000..=99999 => Self::Cosmic,
            _ => Self::OnTheStar,
        }
    }

    pub fn multiplier(&self) -> f64 {
        match self {
            Self::Physical => 1.0,
            Self::Mental => 1.1,
            Self::Cosmic => 2.0,
            Self::OnTheStar => 15.0,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Physical => "Physical",
            Self::Mental => "Mental",
            Self::Cosmic => "Cosmic",
            Self::OnTheStar => "On The Star",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Physical => "🌍",
            Self::Mental => "🧠",
            Self::Cosmic => "🌌",
            Self::OnTheStar => "⭐",
        }
    }

    pub fn next_level_xp(&self) -> Option<u64> {
        match self {
            Self::Physical => Some(1000),
            Self::Mental => Some(10000),
            Self::Cosmic => Some(100000),
            Self::OnTheStar => None, // Max level
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsciousnessProfile {
    pub miner_address: String,
    pub level: ConsciousnessLevel,
    pub xp: u64,
    pub shares_submitted: u64,
    pub blocks_found: u64,
}

impl ConsciousnessProfile {
    pub fn new(miner_address: String) -> Self {
        Self {
            miner_address,
            level: ConsciousnessLevel::Physical,
            xp: 0,
            shares_submitted: 0,
            blocks_found: 0,
        }
    }

    pub fn add_xp(&mut self, amount: u64) {
        self.xp += amount;
        self.level = ConsciousnessLevel::from_xp(self.xp);
    }

    pub fn share_submitted(&mut self) {
        self.shares_submitted += 1;
        self.add_xp(10); // 10 XP per share
    }

    pub fn block_found(&mut self) {
        self.blocks_found += 1;
        self.add_xp(1000); // 1000 XP per block
    }

    pub fn progress_to_next_level(&self) -> Option<f64> {
        if let Some(next_xp) = self.level.next_level_xp() {
            let current_level_start = match self.level {
                ConsciousnessLevel::Physical => 0,
                ConsciousnessLevel::Mental => 1000,
                ConsciousnessLevel::Cosmic => 10000,
                ConsciousnessLevel::OnTheStar => 100000,
            };
            
            let progress = (self.xp - current_level_start) as f64;
            let total = (next_xp - current_level_start) as f64;
            Some((progress / total) * 100.0)
        } else {
            None // Max level
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consciousness_levels() {
        assert_eq!(ConsciousnessLevel::from_xp(0), ConsciousnessLevel::Physical);
        assert_eq!(ConsciousnessLevel::from_xp(500), ConsciousnessLevel::Physical);
        assert_eq!(ConsciousnessLevel::from_xp(1000), ConsciousnessLevel::Mental);
        assert_eq!(ConsciousnessLevel::from_xp(10000), ConsciousnessLevel::Cosmic);
        assert_eq!(ConsciousnessLevel::from_xp(100000), ConsciousnessLevel::OnTheStar);
    }

    #[test]
    fn test_multipliers() {
        assert_eq!(ConsciousnessLevel::Physical.multiplier(), 1.0);
        assert_eq!(ConsciousnessLevel::Mental.multiplier(), 1.1);
        assert_eq!(ConsciousnessLevel::Cosmic.multiplier(), 2.0);
        assert_eq!(ConsciousnessLevel::OnTheStar.multiplier(), 15.0);
    }

    #[test]
    fn test_xp_progression() {
        let mut profile = ConsciousnessProfile::new("test_miner".to_string());
        
        assert_eq!(profile.level, ConsciousnessLevel::Physical);
        
        // Submit 100 shares (1000 XP)
        for _ in 0..100 {
            profile.share_submitted();
        }
        
        assert_eq!(profile.level, ConsciousnessLevel::Mental);
        assert_eq!(profile.xp, 1000);
    }

    #[test]
    fn test_progress_calculation() {
        let mut profile = ConsciousnessProfile::new("test_miner".to_string());
        profile.add_xp(500); // 50% to Mental level
        
        let progress = profile.progress_to_next_level().unwrap();
        assert!((progress - 50.0).abs() < 0.1);
    }
}
