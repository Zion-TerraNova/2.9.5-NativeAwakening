// Allow dead code for modules that contain future/planned features
#![allow(dead_code)]

mod miner;
mod stratum;
mod consciousness;
mod telemetry;
mod config;
mod ncl;

use clap::Parser;
use colored::*;
use log::{info, warn, error};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::signal;

use miner::MinerConfig;
use miner::Algorithm;
use miner::gpu::{auto_tune, run_benchmark, print_benchmark_results, AutoTuneConfig};
use miner::python_fallback::{PythonFallbackMiner, PythonFallbackConfig, PythonMinerVariant};
use ncl::{NCLClient, NCLConfig, NpuType};

#[derive(Parser, Debug)]
#[command(
    name = "zion-universal-miner",
    version = "2.9.5",
    author = "ZION Core Team",
    about = "🌟 ZION Universal Native Miner - Multi-algorithm CPU+GPU mining",
    long_about = None
)]
struct Cli {
    /// Pool URL (stratum+tcp://host:port)
    #[arg(short, long)]
    pool: String,

    /// ZION wallet address
    #[arg(short, long)]
    wallet: String,

    /// Mining algorithm (cosmic_harmony, cosmic_harmony_v2, randomx, yescrypt, blake3)
    #[arg(short, long, default_value = "cosmic_harmony")]
    algorithm: String,

    /// Difficulty hint (e.g. 1, 8, 64). Sent as `d=` to the pool when supported.
    #[arg(long)]
    difficulty: Option<u64>,

    /// Number of CPU threads (0 = auto-detect)
    #[arg(short, long, default_value_t = 0)]
    threads: usize,

    /// Worker name (default: hostname)
    #[arg(long)]
    worker: Option<String>,

    /// Mining mode (cpu|gpu|dual). This is a compatibility flag used by the Desktop Agent.
    /// If set to gpu/dual, GPU mining will be enabled.
    #[arg(long)]
    mode: Option<String>,

    /// Enable GPU mining
    #[arg(long)]
    gpu: bool,

    /// GPU device IDs (comma-separated, e.g., "0,1")
    #[arg(long)]
    gpu_devices: Option<String>,

    /// Config file path
    #[arg(short, long)]
    config: Option<String>,

    /// Enable NCL (Neural Compute Layer) for AI bonus
    #[arg(long, default_value_t = true)]
    ncl: bool,

    /// NCL time allocation (0.0-0.5, default 0.3 = 30% AI time)
    #[arg(long, default_value_t = 0.3)]
    ncl_allocation: f32,

    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Quiet mode (minimal output)
    #[arg(short, long)]
    quiet: bool,

    /// Debug logging
    #[arg(long)]
    debug: bool,

    /// Run GPU benchmark only (no mining)
    #[arg(long)]
    benchmark: bool,

    /// Auto-tune GPU batch size
    #[arg(long)]
    auto_tune: bool,

    /// External pool mining: coin (etc, rvn, erg, kas)
    #[arg(long)]
    external_coin: Option<String>,

    /// External pool URL (default: 2miners)
    #[arg(long)]
    external_pool: Option<String>,

    /// External pool wallet (BTC for payout)
    #[arg(long)]
    external_wallet: Option<String>,

    /// Hashpower percentage for external mining (0-100)
    #[arg(long, default_value_t = 25)]
    external_percent: u8,

    /// Write miner stats JSON to this file (Desktop Agent reads this)
    #[arg(long)]
    stats_file: Option<String>,

    /// Stats file update interval in seconds
    #[arg(long, default_value_t = 5)]
    stats_interval: u64,

    /// Enable Python fallback miner (spawns Python process).
    /// Values: "chv3" (CHv3 GPU miner), "legacy" (v2.9 native miner), or "auto".
    /// Use when Rust GPU/Metal isn't available or for algorithm fallback.
    #[arg(long)]
    python_fallback: Option<String>,

    /// Path to Python miner script (auto-detected if not set)
    #[arg(long)]
    python_script: Option<String>,

    /// Extra arguments to pass to the Python miner (comma-separated)
    #[arg(long)]
    python_args: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Setup logging
    if cli.debug {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Debug)
            .init();
    } else if cli.quiet {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Warn)
            .init();
    } else {
        env_logger::Builder::from_default_env()
            .filter_level(log::LevelFilter::Info)
            .init();
    }

    // Disable colors if requested
    if cli.no_color {
        colored::control::set_override(false);
    }

    print_banner();

    // Parse algorithm
    let algorithm = Algorithm::from_str(&cli.algorithm)
        .ok_or_else(|| anyhow::anyhow!("Invalid algorithm: {}", cli.algorithm))?;

    println!("{} {}", " * ".bright_green().bold(), "ABOUT".bright_white().bold());
    println!("{}  {} {}", "   ".bright_black(), "ZION".bright_cyan().bold(), "v2.9.5 TerraNova".white());
    println!("{}  libs {}", "   ".bright_black(), "tokio/1.35  colored/2.1  clap/4.4".bright_black());
    println!();
    println!("{} {}", " * ".bright_green().bold(), "COMMANDS".bright_white().bold());
    println!("{}  {} - {} {}", "   ".bright_black(), "h".bright_magenta(), "hashrate".white(), "· show current speed".bright_black());
    println!("{}  {} - {} {}", "   ".bright_black(), "p".bright_magenta(), "pause".white(), "· pause mining".bright_black());
    println!("{}  {} - {} {}", "   ".bright_black(), "r".bright_magenta(), "resume".white(), "· resume mining".bright_black());
    println!("{}  {} - {} {}", "   ".bright_black(), "s".bright_magenta(), "status".white(), "· full status panel".bright_black());
    println!();
    println!("{} {}", " * ".bright_green().bold(), "CONFIG".bright_white().bold());
    println!("{}  {:<12} {}", "   ".bright_black(), "algorithm".bright_black(), algorithm.name().bright_cyan());
    println!("{}  {:<12} {}", "   ".bright_black(), "pool".bright_black(), cli.pool.bright_white());
    println!("{}  {:<12} {}...{}", "   ".bright_black(), "wallet".bright_black(), &cli.wallet[..8].bright_white(), &cli.wallet[cli.wallet.len().saturating_sub(6)..].bright_white());

    // Determine thread count
    let threads = if cli.threads == 0 {
        num_cpus::get()
    } else {
        cli.threads
    };
    println!("{}  {:<12} {}", "   ".bright_black(), "threads".bright_black(), threads.to_string().bright_magenta().bold());

    // Detect GPUs if enabled
    if cli.gpu {
        match miner::detect_gpus() {
            Ok(gpus) => {
                if gpus.is_empty() {
                    println!("{}  {:<12} {}", "   ".bright_black(), "gpu".bright_black(), "none detected".bright_red());
                } else {
                    for gpu in &gpus {
                        println!("{}  {:<12} {} {} {} CUs {} MB",
                            "   ".bright_black(), "gpu".bright_black(),
                            gpu.name.bright_green().bold(),
                            format!("[{:?}]", gpu.platform).bright_black(),
                            gpu.compute_units.to_string().bright_cyan(),
                            gpu.memory_mb.to_string().bright_cyan(),
                        );
                    }
                }
            }
            Err(_e) => {
                println!("{}  {:<12} {}", "   ".bright_black(), "gpu".bright_black(), "detection failed".bright_red());
            }
        }
    }

    // GPU info (support compatibility --mode)
    let mode_lower = cli.mode.as_deref().unwrap_or("").to_lowercase();
    let gpu_enabled = cli.gpu || mode_lower == "gpu" || mode_lower == "dual";

    // Auto-detect GPU availability for CH3 Revenue stream routing
    let has_gpu = miner::detect_gpu_available();
    
    if gpu_enabled {
        println!("{}  {:<12} {}", "   ".bright_black(), "gpu-mode".bright_black(), "ENABLED".bright_green().bold());
    } else if has_gpu {
        println!("{}  {:<12} {}", "   ".bright_black(), "gpu-mode".bright_black(), "available (--gpu to enable)".bright_yellow());
    } else {
        println!("{}  {:<12} {} {}", "   ".bright_black(), "gpu-mode".bright_black(), "DISABLED".bright_red(), "→ revenue XMR/RandomX".bright_black());
    }

    // NCL (Neural Compute Layer) info
    let ncl_config = if cli.ncl {
        let npu = NpuType::detect();
        println!("{}  {:<12} {} {} {:.1} TFLOPS",
            "   ".bright_black(), "ncl".bright_black(),
            "ENABLED".bright_green().bold(),
            format!("[{:?}]", npu).bright_black(),
            npu.estimated_tflops(),
        );
        println!("{}  {:<12} {}%", "   ".bright_black(), "ncl-alloc".bright_black(), ((cli.ncl_allocation * 100.0) as u32).to_string().bright_cyan());
        Some(NCLConfig {
            enabled: true,
            allocation: cli.ncl_allocation.clamp(0.0, 0.5),
            npu_type: npu,
            min_task_interval_ms: 1000,
        })
    } else {
        println!("{}  {:<12} {}", "   ".bright_black(), "ncl".bright_black(), "DISABLED".bright_red());
        None
    };

    // Worker name
    let worker = cli.worker.unwrap_or_else(|| {
        hostname::get()
            .unwrap_or_else(|_| "unknown".into())
            .to_string_lossy()
            .into_owned()
    });
    println!("{}  {:<12} {}", "   ".bright_black(), "worker".bright_black(), worker.bright_white().bold());

    // Build miner config
    let config = MinerConfig {
        pool_url: cli.pool.clone(),
        wallet_address: cli.wallet.clone(),
        worker_name: worker.clone(),
        algorithm,
        difficulty: cli.difficulty,
        cpu_threads: threads,
        gpu_enabled: gpu_enabled,
        gpu_devices: parse_gpu_devices(cli.gpu_devices.as_deref()),
        stats_file: cli.stats_file.as_deref().map(PathBuf::from),
        stats_interval_secs: cli.stats_interval.max(1),
    };

    println!();
    println!("{}", "─────────────────────────────────────────────────────────────────".bright_black());
    println!();

    // Handle benchmark/auto-tune mode
    if cli.benchmark || cli.auto_tune {
        return run_benchmark_mode(cli.benchmark, cli.auto_tune).await;
    }

    // Initialize NCL client if enabled
    let ncl_client = ncl_config.map(|cfg| Arc::new(NCLClient::new(cfg)));
    
    if let Some(ref _ncl) = ncl_client {
        log::debug!("NCL Client initialized");
    }

    // Start external pool mining if configured
    if let Some(ref ext_coin_str) = cli.external_coin {
        use crate::stratum::ethstratum::ExternalCoin;
        use crate::miner::external_pool::{ExternalPoolConfig, ExternalMiner};

        let ext_coin = ExternalCoin::from_str(ext_coin_str)
            .ok_or_else(|| anyhow::anyhow!("Unknown external coin: {}. Use: etc, rvn, erg, kas", ext_coin_str))?;

        let ext_pool = cli.external_pool.clone()
            .unwrap_or_else(|| ext_coin.default_pool_url().to_string());

        let ext_wallet = cli.external_wallet.clone()
            .unwrap_or_else(|| {
                // Default BTC wallet
                "YOUR_BTC_WALLET_ADDRESS".to_string()
            });

        println!("{}  {:<12} {} on {}", "   ".bright_black(), "external".bright_black(), ext_coin.name().bright_cyan(), ext_pool.bright_white());
        println!("{}  {:<12} {}", "   ".bright_black(), "ext-wallet".bright_black(), ext_wallet.bright_white());
        println!("{}  {:<12} {}%", "   ".bright_black(), "ext-power".bright_black(), cli.external_percent.to_string().bright_cyan());

        let ext_config = ExternalPoolConfig {
            coin: ext_coin,
            pool_url: ext_pool,
            wallet: ext_wallet,
            worker: worker.clone(),
            cpu_threads: 1,
            gpu_enabled: gpu_enabled,
            hashpower_percent: cli.external_percent,
        };

        let ext_miner = Arc::new(ExternalMiner::new(ext_config));
        let ext_miner_clone = Arc::clone(&ext_miner);
        tokio::spawn(async move {
            if let Err(e) = ext_miner_clone.start().await {
                warn!("❌ External pool mining failed: {}", e);
            }
        });
    }

    // Start miner
    // If Python fallback is requested, spawn the Python miner process
    if let Some(ref fallback_mode) = cli.python_fallback {
        let variant = match fallback_mode.to_lowercase().as_str() {
            "auto" => {
                // Auto-select: CHv3 GPU on macOS, legacy everywhere else
                if cfg!(target_os = "macos") && algorithm == Algorithm::CosmicHarmony {
                    PythonMinerVariant::Chv3Gpu
                } else {
                    PythonMinerVariant::Legacy
                }
            }
            other => PythonMinerVariant::from_str(other)
                .unwrap_or_else(|| {
                    warn!("Unknown Python fallback variant '{}', defaulting to 'chv3'", other);
                    PythonMinerVariant::Chv3Gpu
                }),
        };

        let py_stats_file = cli.stats_file.clone()
            .unwrap_or_else(|| "data/python_miner_stats.json".to_string());

        let extra_args: Vec<String> = cli.python_args.as_deref()
            .map(|s| s.split(',').map(|a| a.trim().to_string()).collect())
            .unwrap_or_default();

        let py_config = PythonFallbackConfig {
            pool_url: cli.pool.clone(),
            wallet: cli.wallet.clone(),
            worker: worker.clone(),
            algorithm: algorithm.name().to_string(),
            gpu: gpu_enabled,
            threads,
            stats_file: PathBuf::from(&py_stats_file),
            stats_interval: cli.stats_interval.max(1),
            variant,
            script_path: cli.python_script.as_deref().map(PathBuf::from),
            extra_args,
        };

        let py_miner = Arc::new(PythonFallbackMiner::new(py_config));

        info!("🐍 Python fallback: {:?} variant", variant);

        // Spawn Python miner
        let py_clone = Arc::clone(&py_miner);
        tokio::spawn(async move {
            if let Err(e) = py_clone.start().await {
                error!("❌ Python fallback miner failed: {}", e);
            }
        });

        // Spawn stats monitor for Python miner
        let py_stats = Arc::clone(&py_miner);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
            loop {
                interval.tick().await;
                if !py_stats.is_running().await {
                    warn!("🐍 Python miner process exited");
                    break;
                }
                if let Some(stats) = py_stats.read_stats().await {
                    if stats.hashrate > 0.0 {
                        let hr = if stats.hashrate > 1_000_000.0 {
                            format!("{:.2} MH/s", stats.hashrate / 1_000_000.0)
                        } else if stats.hashrate > 1_000.0 {
                            format!("{:.2} kH/s", stats.hashrate / 1_000.0)
                        } else {
                            format!("{:.0} H/s", stats.hashrate)
                        };
                        info!(
                            "🐍 Python miner: {} | accepted: {} | rejected: {}",
                            hr, stats.shares_accepted, stats.shares_rejected
                        );
                    }
                }
            }
        });

        // Graceful shutdown for Python miner
        let py_shutdown = Arc::clone(&py_miner);
        tokio::spawn(async move {
            signal::ctrl_c().await.ok();
            warn!("{}", "Shutting down Python miner...".yellow());
            py_shutdown.stop().await;
            std::process::exit(0);
        });

        // If Python fallback is the ONLY mode (no Rust mining), just wait
        info!("🐍 Python fallback miner running alongside Rust miner");
    }

    let miner = Arc::new(miner::UniversalMiner::new_with_ncl(config, ncl_client.clone())?);
    // Handle Ctrl+C gracefully
    let miner_clone = Arc::clone(&miner);
    tokio::spawn(async move {
        signal::ctrl_c().await.ok();
        println!("\n{} {} {}\n",
            format!("[{}]", chrono::Utc::now().format("%H:%M:%S")).bright_black(),
            "signal".bright_yellow(),
            "Ctrl+C — shutting down...".bright_yellow().bold(),
        );
        miner_clone.stop().await;
        std::process::exit(0);
    });

    // Run miner
    // If external mining is 100%, don't require main pool connection
    if cli.external_percent >= 100 && cli.external_coin.is_some() {
        info!("⛏️  External-only mode ({}%) — main pool connection skipped", cli.external_percent);
        info!("   Waiting for external mining to complete...");
        // Wait indefinitely — external mining runs in background tokio::spawn
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    } else {
        miner.start().await?;
    }

    Ok(())
}

/// Run GPU benchmark or auto-tune mode
async fn run_benchmark_mode(full_benchmark: bool, do_auto_tune: bool) -> anyhow::Result<()> {
    use miner::gpu::{detect_gpus, create_miner};

    info!("🔧 GPU Benchmark/Auto-tune Mode");

    let gpus = detect_gpus()?;
    if gpus.is_empty() {
        anyhow::bail!("No GPU devices found! Build with --features gpu or --features cuda");
    }

    let mut benchmarks = Vec::new();

    for gpu in &gpus {
        info!("Initializing GPU {}: {}", gpu.id, gpu.name);
        
        let mut miner = create_miner(gpu)?;
        miner.init()?;

        if full_benchmark {
            let config = AutoTuneConfig::default();
            match run_benchmark(miner.as_mut(), &config) {
                Ok(result) => benchmarks.push(result),
                Err(e) => warn!("Benchmark failed for GPU {}: {}", gpu.id, e),
            }
        } else if do_auto_tune {
            match auto_tune(miner.as_mut()) {
                Ok(optimal) => {
                    info!("✅ GPU {} optimal batch size: {}", gpu.id, optimal);
                }
                Err(e) => warn!("Auto-tune failed for GPU {}: {}", gpu.id, e),
            }
        }
    }

    if !benchmarks.is_empty() {
        print_benchmark_results(&benchmarks);
    }

    Ok(())
}

fn print_banner() {
    println!();
    println!("{}",   " ╔══════════════════════════════════════════════════════════════════╗".bright_cyan());
    println!("{}{}{}" ," ║ ".bright_cyan(), "       ZION UNIVERSAL MINER  v2.9.5  TerraNova              ".bright_white().bold(), " ║".bright_cyan());
    println!("{}{}{}" ," ║ ".bright_cyan(), "       Multi-Algorithm  ·  CPU + GPU + NCL AI               ".bright_black(), " ║".bright_cyan());
    println!("{}",   " ╠══════════════════════════════════════════════════════════════════╣".bright_cyan());
    println!("{}{}{}" ," ║ ".bright_cyan(), " Algorithms   cosmic_harmony · randomx · yescrypt · blake3   ".white(), " ║".bright_cyan());
    println!("{}{}{}" ," ║ ".bright_cyan(), " GPU Accel    Metal (macOS) · CUDA · OpenCL                  ".white(), " ║".bright_cyan());
    println!("{}{}{}" ," ║ ".bright_cyan(), " Revenue      ERG/RVN/KAS/ETC (GPU) · XMR (CPU)             ".white(), " ║".bright_cyan());
    println!("{}{}{}" ," ║ ".bright_cyan(), " NCL Bonus    Neural Compute Layer — AI task rewards         ".white(), " ║".bright_cyan());
    println!("{}",   " ╚══════════════════════════════════════════════════════════════════╝".bright_cyan());
    println!();
}

fn parse_gpu_devices(devices: Option<&str>) -> Vec<usize> {
    devices
        .map(|s| {
            s.split(',')
                .filter_map(|d| d.trim().parse::<usize>().ok())
                .collect()
        })
        .unwrap_or_else(Vec::new)
}