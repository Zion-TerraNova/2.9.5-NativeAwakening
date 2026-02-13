/// Network type identification for ZION blockchain.
///
/// Prevents testnet and mainnet nodes from connecting to each other.
/// Network magic is included in P2P handshake for peer validation.

use std::fmt;
use std::sync::OnceLock;

/// Global network type — set once at startup, read everywhere.
static NETWORK: OnceLock<NetworkType> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkType {
    Testnet,
    Mainnet,
}

impl NetworkType {
    /// Human-readable network name (used in health/stats/logs).
    pub fn name(&self) -> &'static str {
        match self {
            NetworkType::Testnet => "testnet",
            NetworkType::Mainnet => "mainnet",
        }
    }

    /// Magic bytes prefix for P2P handshake validation.
    /// Prevents cross-network peer connections.
    pub fn magic(&self) -> &'static str {
        match self {
            NetworkType::Testnet => "ZION-TESTNET-V1",
            NetworkType::Mainnet => "ZION-MAINNET-V1",
        }
    }

    /// Default P2P port for each network.
    pub fn default_p2p_port(&self) -> u16 {
        match self {
            NetworkType::Testnet => 8334,
            NetworkType::Mainnet => 8333,
        }
    }

    /// Default RPC port for each network.
    pub fn default_rpc_port(&self) -> u16 {
        match self {
            NetworkType::Testnet => 8444,
            NetworkType::Mainnet => 8443,
        }
    }

    /// Genesis block timestamp.
    /// Must be identical on all nodes to produce the same genesis hash.
    /// - Testnet: Feb 8, 2026 12:00:00 UTC (TestNet launch)
    /// - Mainnet: Jan 1, 2024 00:00:00 UTC (immutable)
    pub fn genesis_timestamp(&self) -> u64 {
        match self {
            NetworkType::Testnet => 1770552000,  // Feb 8, 2026 12:00:00 UTC
            NetworkType::Mainnet => 1704067200,  // Jan 1, 2024 00:00:00 UTC
        }
    }

    /// Parse from CLI string.
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s.to_lowercase().as_str() {
            "testnet" | "test" => Ok(NetworkType::Testnet),
            "mainnet" | "main" => Ok(NetworkType::Mainnet),
            _ => Err(format!("Unknown network '{}'. Use 'testnet' or 'mainnet'.", s)),
        }
    }
}

impl fmt::Display for NetworkType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Set the global network type (call once at startup).
pub fn set_network(net: NetworkType) {
    NETWORK.set(net).expect("Network type already set");
}

/// Get the global network type.
pub fn get_network() -> NetworkType {
    *NETWORK.get().unwrap_or(&NetworkType::Testnet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_type_names() {
        assert_eq!(NetworkType::Testnet.name(), "testnet");
        assert_eq!(NetworkType::Mainnet.name(), "mainnet");
    }

    #[test]
    fn test_network_magic() {
        assert_ne!(NetworkType::Testnet.magic(), NetworkType::Mainnet.magic());
    }

    #[test]
    fn test_parse_network() {
        assert_eq!(NetworkType::from_str("testnet").unwrap(), NetworkType::Testnet);
        assert_eq!(NetworkType::from_str("MAINNET").unwrap(), NetworkType::Mainnet);
        assert_eq!(NetworkType::from_str("test").unwrap(), NetworkType::Testnet);
        assert!(NetworkType::from_str("invalid").is_err());
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", NetworkType::Testnet), "testnet");
        assert_eq!(format!("{}", NetworkType::Mainnet), "mainnet");
    }
}
