use std::net::SocketAddr;
use std::time::Duration;
use anyhow::Result;
use tokio::net::TcpStream;
use tokio::time::timeout;

/// Hardcoded seed nodes for bootstrapping
pub const SEED_NODES: &[&str] = &[
    // ZION Foundation nodes
    // "seed-eu.zionterranova.com:8334",  // EU node (configure via DNS)
    // "seed-de.zionterranova.com:8334",  // DE node (configure via DNS)
    "seed1.zionterranova.com:8334",
    "seed2.zionterranova.com:8334",
    "seed3.zionterranova.com:8334",
    
    // Community nodes (to be added)
    // "community1.zion.network:8334",
];

/// Discover reachable seed nodes
pub async fn discover_seeds() -> Vec<String> {
    let mut reachable = Vec::new();
    
    for seed in SEED_NODES {
        // Try to resolve and check connectivity
        match try_connect_seed(seed).await {
            Ok(true) => {
                println!("[P2P] Seed node reachable: {}", seed);
                reachable.push(seed.to_string());
            }
            Ok(false) => {
                println!("[P2P] Seed node unreachable: {}", seed);
            }
            Err(e) => {
                println!("[P2P] Seed check failed for {}: {}", seed, e);
            }
        }
    }
    
    if reachable.is_empty() {
        println!("[P2P] WARNING: No seed nodes reachable!");
    } else {
        println!("[P2P] Discovered {} reachable seeds", reachable.len());
    }
    
    reachable
}

/// Try to connect to a seed node (handles both IP:port and hostname:port)
async fn try_connect_seed(seed: &str) -> Result<bool> {
    // Try to connect with 3s timeout
    match timeout(Duration::from_secs(3), TcpStream::connect(seed)).await {
        Ok(Ok(_stream)) => Ok(true),
        Ok(Err(_)) => Ok(false),
        Err(_) => Ok(false), // Timeout
    }
}

/// Resolve DNS seed nodes (for future DNS-based discovery)
pub async fn resolve_dns_seeds(domain: &str) -> Result<Vec<SocketAddr>> {
    use tokio::net::lookup_host;
    
    let mut addrs = Vec::new();
    
    // Example: seed.zionterranova.com returns multiple A records
    match lookup_host(domain).await {
        Ok(iter) => {
            for addr in iter {
                addrs.push(addr);
            }
            println!("[P2P] Resolved {} addresses from {}", addrs.len(), domain);
        }
        Err(e) => {
            println!("[P2P] DNS lookup failed for {}: {}", domain, e);
        }
    }
    
    Ok(addrs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_seed_discovery() {
        // This will actually try to connect (integration test)
        let seeds = discover_seeds().await;
        // Don't fail test if no seeds reachable (network dependent)
        println!("Discovered {} seeds: {:?}", seeds.len(), seeds);
    }

    #[test]
    fn test_seed_constants() {
        assert!(!SEED_NODES.is_empty(), "Must have at least one seed");
        // Seeds can be hostnames or IP addresses, both are valid
        for seed in SEED_NODES {
            assert!(seed.contains(':'), "Seed must have port: {}", seed);
        }
    }
}
