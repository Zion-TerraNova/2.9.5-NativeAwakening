use clap::Parser;
use zion_core::rpc;
use zion_core::p2p;
use zion_core::state::Inner as NodeState;
use zion_core::network::{self, NetworkType};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Port for JSON-RPC API
    #[arg(long, default_value_t = 8444)]
    rpc_port: u16,

    /// Port for P2P networking
    #[arg(long, default_value_t = 8334)]
    p2p_port: u16,

    /// Path to data directory
    #[arg(long, default_value = "./data/zion-core-v1")]
    data_dir: String,

    /// Initial peers to connect to (comma separated)
    #[arg(long)]
    peers: Option<String>,

    /// Network type: testnet or mainnet
    #[arg(long, default_value = "testnet")]
    network: String,
}

#[tokio::main]
async fn main() {
    let mut args = Args::parse();

    if let Ok(v) = std::env::var("ZION_RPC_PORT") {
        if let Ok(p) = v.parse::<u16>() {
            args.rpc_port = p;
        }
    }
    if let Ok(v) = std::env::var("ZION_P2P_PORT") {
        if let Ok(p) = v.parse::<u16>() {
            args.p2p_port = p;
        }
    }
    if let Ok(v) = std::env::var("ZION_DATA_DIR") {
        if !v.trim().is_empty() {
            args.data_dir = v;
        }
    }
    if args.peers.is_none() {
        if let Ok(v) = std::env::var("ZION_P2P_SEEDS") {
            if !v.trim().is_empty() {
                args.peers = Some(v);
            }
        }
    }
    
    // Parse and set network type
    let net_type = match std::env::var("ZION_NETWORK") {
        Ok(v) if !v.trim().is_empty() => NetworkType::from_str(&v).unwrap_or_else(|e| {
            eprintln!("Invalid ZION_NETWORK: {}", e);
            std::process::exit(1);
        }),
        _ => NetworkType::from_str(&args.network).unwrap_or_else(|e| {
            eprintln!("Invalid --network: {}", e);
            std::process::exit(1);
        }),
    };
    network::set_network(net_type);

    println!("Starting Zion Core V1 [{}]", net_type.name().to_uppercase());
    println!("Network: {} (magic: {})", net_type.name(), net_type.magic());
    println!("Data Dir: {}", args.data_dir);
    println!("RPC Port: {}", args.rpc_port);
    println!("P2P Port: {}", args.p2p_port);

    // Initialize State
    let state = NodeState::new(&args.data_dir);
    
    // Parse Initial Peers
    let mut initial_peers = match args.peers {
        Some(s) => s.split(',').map(|s| s.trim().to_string()).collect(),
        None => vec![],
    };

    // If no peers provided, discover seed nodes
    if initial_peers.is_empty() {
        println!("No peers specified, discovering seed nodes...");
        initial_peers = zion_core::p2p::seeds::discover_seeds().await;
    }

    // Start P2P
    let state_p2p = state.clone();
    let p2p_port = args.p2p_port;
    tokio::spawn(async move { 
        if let Err(e) = p2p::start(state_p2p, p2p_port, initial_peers).await { 
            eprintln!("P2P Error: {}", e);
        }
    });

    // Start RPC
    let app = rpc::server::build(state);
    let addr = format!("0.0.0.0:{}", args.rpc_port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    println!("ZION Core RPC listening on {}", addr);
    axum::serve(listener, app).await.unwrap();
}
