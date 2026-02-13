//! Configuration management for ZION miner
//!
//! Supports JSON config files and environment variables.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::fs;

/// Miner configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Pool configuration
    pub pool: PoolConfig,
    
    /// Mining configuration
    pub mining: MiningConfig,
    
    /// Hardware configuration
    pub hardware: HardwareConfig,
    
    /// Logging configuration
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Primary pool URL
    pub url: String,
    
    /// Backup pool URLs
    #[serde(default)]
    pub backup_urls: Vec<String>,
    
    /// Wallet address
    pub wallet: String,
    
    /// Worker name
    #[serde(default = "default_worker_name")]
    pub worker: String,
    
    /// Reconnection attempts
    #[serde(default = "default_reconnect_attempts")]
    pub reconnect_attempts: u32,
    
    /// Reconnection delay (seconds)
    #[serde(default = "default_reconnect_delay")]
    pub reconnect_delay_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MiningConfig {
    /// Mining algorithm
    #[serde(default = "default_algorithm")]
    pub algorithm: String,
    
    /// Enable auto-algorithm switching
    #[serde(default)]
    pub auto_switch: bool,
    
    /// Difficulty target (optional, pool overrides)
    pub difficulty: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareConfig {
    /// Number of CPU threads (0 = auto)
    #[serde(default)]
    pub cpu_threads: usize,
    
    /// Enable GPU mining
    #[serde(default)]
    pub gpu_enabled: bool,
    
    /// GPU device IDs
    #[serde(default)]
    pub gpu_devices: Vec<usize>,
    
    /// GPU intensity (0-30)
    #[serde(default = "default_gpu_intensity")]
    pub gpu_intensity: u8,
    
    /// CPU affinity (core IDs)
    #[serde(default)]
    pub cpu_affinity: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,
    
    /// Disable colored output
    #[serde(default)]
    pub no_color: bool,
    
    /// Quiet mode
    #[serde(default)]
    pub quiet: bool,
    
    /// Log to file
    pub log_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            pool: PoolConfig {
                url: "stratum+tcp://pool.zionterranova.com:3333".to_string(),
                backup_urls: vec![],
                wallet: String::new(),
                worker: default_worker_name(),
                reconnect_attempts: default_reconnect_attempts(),
                reconnect_delay_secs: default_reconnect_delay(),
            },
            mining: MiningConfig {
                algorithm: default_algorithm(),
                auto_switch: false,
                difficulty: None,
            },
            hardware: HardwareConfig {
                cpu_threads: 0,
                gpu_enabled: false,
                gpu_devices: vec![],
                gpu_intensity: default_gpu_intensity(),
                cpu_affinity: vec![],
            },
            logging: LoggingConfig {
                level: default_log_level(),
                no_color: false,
                quiet: false,
                log_file: None,
            },
        }
    }
}

impl Config {
    /// Load config from file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path.as_ref())
            .context("Failed to read config file")?;
        
        let config: Config = serde_json::from_str(&content)
            .context("Failed to parse config JSON")?;
        
        Ok(config)
    }
    
    /// Save config to file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;
        
        fs::write(path.as_ref(), json)
            .context("Failed to write config file")?;
        
        Ok(())
    }
    
    /// Get default config path
    pub fn default_path() -> Result<PathBuf> {
        let home = dirs::home_dir()
            .context("Could not determine home directory")?;
        
        let config_dir = home.join(".zion");
        fs::create_dir_all(&config_dir)?;
        
        Ok(config_dir.join("miner-config.json"))
    }
    
    /// Load config from default location
    pub fn load_default() -> Result<Self> {
        let path = Self::default_path()?;
        
        if path.exists() {
            Self::from_file(path)
        } else {
            Ok(Self::default())
        }
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Check wallet address
        if self.pool.wallet.is_empty() {
            anyhow::bail!("Wallet address is required");
        }
        
        if !self.pool.wallet.starts_with("ZION") {
            anyhow::bail!("Invalid wallet address format (must start with 'ZION')");
        }
        
        // Check pool URL
        if !self.pool.url.starts_with("stratum+tcp://") {
            anyhow::bail!("Pool URL must start with 'stratum+tcp://'");
        }
        
        // Check GPU intensity
        if self.hardware.gpu_intensity > 30 {
            anyhow::bail!("GPU intensity must be between 0 and 30");
        }
        
        // Check algorithm
        let valid_algos = ["cosmic_harmony", "cosmic_harmony_v2", "randomx", "yescrypt", "blake3"];
        if !valid_algos.contains(&self.mining.algorithm.as_str()) {
            anyhow::bail!("Invalid algorithm: {}. Valid: {:?}", self.mining.algorithm, valid_algos);
        }
        
        Ok(())
    }
}

// Default value functions
fn default_worker_name() -> String {
    hostname::get()
        .unwrap_or_else(|_| "unknown".into())
        .to_string_lossy()
        .into_owned()
}

fn default_reconnect_attempts() -> u32 {
    5
}

fn default_reconnect_delay() -> u64 {
    10
}

fn default_algorithm() -> String {
    "cosmic_harmony".to_string()
}

fn default_gpu_intensity() -> u8 {
    18
}

fn default_log_level() -> String {
    "info".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.mining.algorithm, "cosmic_harmony");
        assert_eq!(config.hardware.cpu_threads, 0);
        assert!(!config.hardware.gpu_enabled);
    }
    
    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        
        assert_eq!(config.mining.algorithm, deserialized.mining.algorithm);
    }
    
    #[test]
    fn test_config_validation() {
        let mut config = Config::default();
        
        // Should fail - no wallet
        assert!(config.validate().is_err());
        
        // Should fail - invalid wallet format
        config.pool.wallet = "invalid".to_string();
        assert!(config.validate().is_err());
        
        // Should succeed
        config.pool.wallet = "ZION_test_address_12345".to_string();
        assert!(config.validate().is_ok());
    }
}
