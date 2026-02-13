use clap::Parser;
use reqwest;
use serde_json::{json, Value};
use std::time::Duration;
use zion_core::algorithms::Algorithm;
use zion_core::miner::BlockTemplate;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// RPC endpoint URL
    #[arg(long, default_value = "http://127.0.0.1:8080/jsonrpc")]
    rpc_url: String,

    /// Wallet address for coinbase rewards
    #[arg(long)]
    wallet: String,

    /// Mining algorithm (cosmic_harmony, randomx, yescrypt, blake3)
    #[arg(long, default_value = "cosmic_harmony")]
    algorithm: String,

    /// Maximum iterations per mining attempt
    #[arg(long, default_value_t = 10_000_000)]
    max_iterations: u64,

    /// Polling interval in seconds
    #[arg(long, default_value_t = 5)]
    poll_interval: u64,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    
    let algorithm = Algorithm::from_str(&args.algorithm)
        .expect("Invalid algorithm. Use: cosmic_harmony, randomx, yescrypt, or blake3");
    
    println!("🚀 ZION Native Miner v2.9.5");
    println!("================================");
    println!("RPC URL: {}", args.rpc_url);
    println!("Wallet: {}", args.wallet);
    println!("Algorithm: {}", algorithm);
    println!("Max Iterations: {}", args.max_iterations);
    println!("Expected Hashrate: ~{} H/s", algorithm.baseline_hashrate());
    println!();

    let client = reqwest::Client::new();
    let mut previous_height = 0u64;

    loop {
        match get_block_template(&client, &args.rpc_url, &args.wallet).await {
            Ok(template) => {
                if template.height != previous_height {
                    println!("\n📦 New Block Template:");
                    println!("   Height: {}", template.height);
                    println!("   Difficulty: {}", template.difficulty);
                    println!("   Target: {}", template.target);
                    previous_height = template.height;
                }
                
                println!("\n⛏️  Mining block {} with {}...", template.height, algorithm);
                
                match zion_core::miner::mine_block(&template, args.max_iterations, Some(algorithm)) {
                    Some(result) => {
                        println!("✨ Block Found!");
                        println!("   Nonce: {}", result.nonce);
                        println!("   Hash: {}", result.hash);
                        println!("   Iterations: {}", result.iterations);
                        println!("   Hashrate: {:.2} kH/s", result.hashrate / 1000.0);
                        println!("   Algorithm: {}", result.algorithm);
                        
                        // Submit block
                        match submit_block(&client, &args.rpc_url, &template, result.nonce, &result.hash).await {
                            Ok(accepted) => {
                                if accepted {
                                    println!("🎉 Block ACCEPTED by node!");
                                } else {
                                    println!("❌ Block REJECTED by node");
                                }
                            }
                            Err(e) => {
                                eprintln!("⚠️  Submit error: {}", e);
                            }
                        }
                        
                        // Wait before fetching new template
                        tokio::time::sleep(Duration::from_secs(2)).await;
                    }
                    None => {
                        println!("   No solution found in {} iterations", args.max_iterations);
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ RPC Error: {}", e);
            }
        }
        
        tokio::time::sleep(Duration::from_secs(args.poll_interval)).await;
    }
}

async fn get_block_template(
    client: &reqwest::Client,
    rpc_url: &str,
    wallet: &str,
) -> Result<BlockTemplate, String> {
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBlockTemplate",
        "params": {"wallet_address": wallet}
    });
    
    let response = client
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    let json: Value = response
        .json()
        .await
        .map_err(|e| format!("Parse failed: {}", e))?;
    
    if let Some(error) = json.get("error") {
        return Err(format!("RPC error: {}", error));
    }
    
    let result = json.get("result")
        .ok_or("Missing result")?;
    
    Ok(BlockTemplate {
        height: result["height"].as_u64().unwrap_or(0),
        prev_hash: result["prev_hash"].as_str().unwrap_or("").to_string(),
        difficulty: result["difficulty"].as_u64().unwrap_or(1000),
        target: result["target"].as_str().unwrap_or("").to_string(),
        coinbase_address: wallet.to_string(),
        algorithm: result["algorithm"].as_str().map(|s| s.to_string()),
    })
}

async fn submit_block(
    client: &reqwest::Client,
    rpc_url: &str,
    template: &BlockTemplate,
    nonce: u64,
    _hash: &str,
) -> Result<bool, String> {
    let block_data = json!({
        "height": template.height,
        "prev_hash": template.prev_hash,
        "merkle_root": "0000000000000000000000000000000000000000000000000000000000000000",
        "timestamp": chrono::Utc::now().timestamp(),
        "difficulty": template.difficulty,
        "nonce": nonce,
        "transactions": []
    });
    
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "submitblock",
        "params": {"block_data": block_data.to_string()}
    });
    
    let response = client
        .post(rpc_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Submit request failed: {}", e))?;
    
    let json: Value = response
        .json()
        .await
        .map_err(|e| format!("Parse response failed: {}", e))?;
    
    if let Some(error) = json.get("error") {
        return Err(format!("Submit error: {}", error));
    }
    
    Ok(json.get("result")
        .and_then(|r| r.get("accepted"))
        .and_then(|a| a.as_bool())
        .unwrap_or(false))
}
