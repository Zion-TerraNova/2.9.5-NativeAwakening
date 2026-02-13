use std::env;
use std::fs;
use std::sync::Arc;
use serde::Deserialize;
use tokio::net::TcpStream;
use tokio::io::{AsyncWriteExt, AsyncReadExt};

// Simplified config structs to read the JSON file locally
#[derive(Deserialize, Debug)]
struct TestConfig {
    streams: TestStreams,
}

#[derive(Deserialize, Debug)]
struct TestStreams {
    etc: TestEtcStream,
}

#[derive(Deserialize, Debug)]
struct TestEtcStream {
    enabled: bool,
    pool: TestPoolBlock,
}

#[derive(Deserialize, Debug)]
struct TestPoolBlock {
    stratum: String,
    wallet: String,
    worker: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 ZION External Pool Connectivity Test");
    println!("=======================================");

    // 1. Load Config
    let config_path = "../../../config/ch3_revenue_settings.json";
    println!("📂 Loading config from: {}", config_path);
    
    let content = fs::read_to_string(config_path)
        .expect("Failed to read config file - make sure you are in zion-native/pool/ directory or path is correct");

    let cfg: TestConfig = serde_json::from_str(&content)?;
    
    if !cfg.streams.etc.enabled {
        println!("⚠️ ETC stream is disabled in config.");
        return Ok(());
    }

    let url_str = cfg.streams.etc.pool.stratum;
    println!("🔗 Target: {}", url_str);

    // Parse URL (stratum+tcp://etc.2miners.com:1010)
    let clean_url = url_str.trim_start_matches("stratum+tcp://").trim_start_matches("stratum://");
    
    println!("⏳ Connecting to {}...", clean_url);
    
    match TcpStream::connect(clean_url).await {
        Ok(mut stream) => {
            println!("✅ TCP Connection ESTABLISHED!");
            
            // Try simple Stratum handshake
            let login_msg = format!(
                "{{\"id\": 1, \"method\": \"mining.subscribe\", \"params\": [\"ZION-TestAgent/1.0\", null]}}\n"
            );
            
            println!("📤 Sending: {}", login_msg.trim());
            stream.write_all(login_msg.as_bytes()).await?;
            
            // Read response (wait up to 5s)
            let mut buf = [0u8; 1024];
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                stream.read(&mut buf)
            ).await;

            match result {
                Ok(Ok(n)) if n > 0 => {
                    let response = String::from_utf8_lossy(&buf[..n]);
                    println!("📥 Received: {}", response);
                    println!("✨ Stratum handshake successful!");
                },
                Ok(Ok(_)) => println!("⚠️ Connection closed remotely."),
                Ok(Err(e)) => println!("❌ Error reading: {}", e),
                Err(_) => println!("⏱️ Timeout waiting for response."),
            }
        },
        Err(e) => {
            println!("❌ Connection FAILED: {}", e);
            println!("   Check your internet connection or if connection is blocked.");
        }
    }

    Ok(())
}
