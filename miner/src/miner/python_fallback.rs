//! Python Fallback Mining Backend
//!
//! Spawns the Python CHv3 GPU miner (`zion_chv3_gpu_miner.py`) or the
//! legacy Python miner (`zion_native_miner_v2_9.py`) as a subprocess.
//!
//! This provides:
//! - Metal GPU mining on macOS when Rust Metal crate isn't compiled
//! - Pure Python fallback when no native libraries are available
//! - Legacy algorithm support (Cosmic Harmony v1/v2, RandomX, Yescrypt)
//!
//! The Python process connects to the pool independently and mines.
//! Stats are read from a JSON stats file that the Python miner writes.

use anyhow::{anyhow, Result};
use log::{info, warn, error};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

/// Python fallback miner configuration
#[derive(Debug, Clone)]
pub struct PythonFallbackConfig {
    /// Pool URL (host:port format)
    pub pool_url: String,
    /// Wallet address
    pub wallet: String,
    /// Worker name
    pub worker: String,
    /// Mining algorithm name
    pub algorithm: String,
    /// Enable GPU in Python miner
    pub gpu: bool,
    /// Number of CPU threads
    pub threads: usize,
    /// Path to stats JSON file
    pub stats_file: PathBuf,
    /// Stats update interval (seconds)
    pub stats_interval: u64,
    /// Python miner variant to use
    pub variant: PythonMinerVariant,
    /// Explicit path to Python script (overrides auto-detection)
    pub script_path: Option<PathBuf>,
    /// Extra arguments to pass to the Python script
    pub extra_args: Vec<String>,
}

/// Which Python miner to launch
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PythonMinerVariant {
    /// CHv3 GPU miner (zion_chv3_gpu_miner.py) - Metal GPU + native C
    Chv3Gpu,
    /// Legacy native miner (zion_native_miner_v2_9.py) - all algorithms
    Legacy,
}

impl PythonMinerVariant {
    pub fn script_name(&self) -> &'static str {
        match self {
            Self::Chv3Gpu => "zion_chv3_gpu_miner.py",
            Self::Legacy => "zion_native_miner_v2_9.py",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "chv3" | "chv3_gpu" | "chv3-gpu" | "gpu" => Some(Self::Chv3Gpu),
            "legacy" | "native" | "v2.9" | "v29" => Some(Self::Legacy),
            _ => None,
        }
    }
}

/// Stats read from the Python miner's JSON stats file
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PythonMinerStats {
    pub hashrate: f64,
    #[serde(default)]
    pub hashrate_window_hs: f64,
    #[serde(default)]
    pub hashrate_cpu: f64,
    #[serde(default)]
    pub hashrate_gpu: f64,
    #[serde(default)]
    pub shares_sent: u64,
    #[serde(default)]
    pub shares_accepted: u64,
    #[serde(default)]
    pub shares_rejected: u64,
    #[serde(default)]
    pub uptime_sec: f64,
}

/// Python fallback miner — manages a subprocess
pub struct PythonFallbackMiner {
    config: PythonFallbackConfig,
    child: Arc<RwLock<Option<Child>>>,
    running: Arc<RwLock<bool>>,
}

impl PythonFallbackMiner {
    pub fn new(config: PythonFallbackConfig) -> Self {
        Self {
            config,
            child: Arc::new(RwLock::new(None)),
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Find the Python script to execute
    fn find_script(&self) -> Result<PathBuf> {
        // If explicit path given, use it
        if let Some(ref path) = self.config.script_path {
            if path.exists() {
                return Ok(path.clone());
            }
            return Err(anyhow!("Specified Python script not found: {}", path.display()));
        }

        let script_name = self.config.variant.script_name();

        // Search paths relative to the binary/workspace
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        let cwd = std::env::current_dir().ok();

        let mut search_paths: Vec<PathBuf> = Vec::new();

        // 1. Same directory as binary
        if let Some(ref dir) = exe_dir {
            search_paths.push(dir.join(script_name));
        }

        // 2. Current working directory
        if let Some(ref dir) = cwd {
            search_paths.push(dir.join(script_name));
            // 3. Workspace-relative paths
            search_paths.push(dir.join("2.9.5/zion-universal-miner").join(script_name));
        }

        // 4. Relative to this crate's source
        let crate_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        search_paths.push(crate_dir.join(script_name));
        search_paths.push(crate_dir.join("..").join(script_name));

        // 5. Legacy miner location (repo root)
        if let Some(ref dir) = cwd {
            search_paths.push(dir.join("zion_native_miner_v2_9.py"));
        }
        if let Some(ref dir) = exe_dir {
            search_paths.push(dir.join("..").join("..").join(script_name));
        }

        for path in &search_paths {
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if canonical.exists() {
                info!("📂 Found Python miner: {}", canonical.display());
                return Ok(canonical);
            }
        }

        Err(anyhow!(
            "Python miner script '{}' not found. Searched: {:?}",
            script_name,
            search_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>()
        ))
    }

    /// Find a Python 3 interpreter
    fn find_python() -> Result<String> {
        // Try python3 first, then python
        for cmd in &["python3", "python"] {
            let result = Command::new(cmd)
                .arg("--version")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();

            if let Ok(output) = result {
                if output.status.success() {
                    let version = String::from_utf8_lossy(&output.stdout);
                    let version = version.trim();
                    if version.is_empty() {
                        let version = String::from_utf8_lossy(&output.stderr).trim().to_string();
                        if version.contains("3.") {
                            return Ok(cmd.to_string());
                        }
                    } else if version.contains("3.") {
                        return Ok(cmd.to_string());
                    }
                }
            }
        }

        Err(anyhow!("Python 3 not found. Install Python 3.9+ to use Python fallback."))
    }

    /// Build command arguments for the Python miner
    fn build_args(&self, script: &Path) -> Vec<String> {
        let mut args = vec![script.to_string_lossy().to_string()];

        match self.config.variant {
            PythonMinerVariant::Chv3Gpu => {
                args.push("--pool".to_string());
                args.push(self.pool_host_port());
                args.push("--wallet".to_string());
                args.push(self.config.wallet.clone());
                args.push("--worker".to_string());
                args.push(format!("{}-py", self.config.worker));
                if self.config.gpu {
                    args.push("--gpu".to_string());
                }
                args.push("--threads".to_string());
                args.push(self.config.threads.to_string());
                args.push("--batch-size".to_string());
                args.push("10000".to_string());
            }
            PythonMinerVariant::Legacy => {
                args.push("--algorithm".to_string());
                args.push(self.config.algorithm.clone());
                args.push("--pool".to_string());
                args.push(self.pool_host_port());
                args.push("--wallet".to_string());
                args.push(self.config.wallet.clone());
                args.push("--worker".to_string());
                args.push(format!("{}-py", self.config.worker));
                args.push("--mode".to_string());
                args.push(if self.config.gpu { "gpu" } else { "cpu" }.to_string());
                args.push("--threads".to_string());
                args.push(self.config.threads.to_string());
                args.push("--stats-file".to_string());
                args.push(self.config.stats_file.to_string_lossy().to_string());
                args.push("--stats-interval".to_string());
                args.push(self.config.stats_interval.to_string());
            }
        }

        // Append extra user args
        args.extend(self.config.extra_args.iter().cloned());

        args
    }

    /// Extract host:port from pool URL
    fn pool_host_port(&self) -> String {
        self.config.pool_url
            .strip_prefix("stratum+tcp://")
            .or_else(|| self.config.pool_url.strip_prefix("tcp://"))
            .unwrap_or(&self.config.pool_url)
            .trim()
            .to_string()
    }

    /// Start the Python miner subprocess
    pub async fn start(&self) -> Result<()> {
        if *self.running.read().await {
            return Err(anyhow!("Python fallback miner already running"));
        }

        let script = self.find_script()?;
        let python = Self::find_python()?;
        let args = self.build_args(&script);

        info!("🐍 Starting Python fallback miner:");
        info!("   Script:  {}", script.display());
        info!("   Python:  {}", python);
        info!("   Variant: {:?}", self.config.variant);
        info!("   GPU:     {}", if self.config.gpu { "enabled" } else { "disabled" });
        info!("   Pool:    {}", self.pool_host_port());
        info!("   Wallet:  {}", self.config.wallet);

        let mut cmd = Command::new(&python);
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Set working directory to script's parent
        if let Some(parent) = script.parent() {
            cmd.current_dir(parent);
        }

        // Environment
        cmd.env("PYTHONUNBUFFERED", "1");

        let child = cmd.spawn().map_err(|e| {
            anyhow!(
                "Failed to spawn Python miner: {} (python={}, script={})",
                e, python, script.display()
            )
        })?;

        let pid = child.id();
        *self.child.write().await = Some(child);
        *self.running.write().await = true;

        info!("🐍 Python miner started (PID: {})", pid);

        // Spawn stdout/stderr reader tasks
        self.spawn_output_readers().await;

        Ok(())
    }

    /// Spawn tasks to read and log Python process output
    async fn spawn_output_readers(&self) {
        let child_arc = Arc::clone(&self.child);
        let running = Arc::clone(&self.running);

        tokio::task::spawn_blocking(move || {
            use std::io::{BufRead, BufReader};

            let mut guard = child_arc.blocking_write();
            let child = match guard.as_mut() {
                Some(c) => c,
                None => return,
            };

            let stdout = match child.stdout.take() {
                Some(s) => s,
                None => return,
            };
            let stderr = child.stderr.take();

            drop(guard);

            // Read stdout in this thread
            let stdout_reader = BufReader::new(stdout);
            let running_clone = Arc::clone(&running);

            let stderr_handle = stderr.map(|stderr| {
                std::thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines() {
                        match line {
                            Ok(l) => {
                                if !l.trim().is_empty() {
                                    warn!("[py-err] {}", l);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
            });

            for line in stdout_reader.lines() {
                if !*running_clone.blocking_read() {
                    break;
                }
                match line {
                    Ok(l) => {
                        if !l.trim().is_empty() {
                            info!("[py] {}", l);
                        }
                    }
                    Err(_) => break,
                }
            }

            // Process exited
            *running_clone.blocking_write() = false;

            if let Some(h) = stderr_handle {
                let _ = h.join();
            }

            // Collect exit status
            let mut guard = child_arc.blocking_write();
            if let Some(ref mut child) = *guard {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            info!("🐍 Python miner exited normally");
                        } else {
                            warn!("🐍 Python miner exited with status: {}", status);
                        }
                    }
                    Ok(None) => {
                        warn!("🐍 Python miner stdout closed but process still running");
                    }
                    Err(e) => {
                        error!("🐍 Error checking Python miner status: {}", e);
                    }
                }
            }
        });
    }

    /// Read stats from the JSON stats file (written by legacy Python miner)
    pub async fn read_stats(&self) -> Option<PythonMinerStats> {
        if !self.config.stats_file.exists() {
            return None;
        }

        match tokio::fs::read_to_string(&self.config.stats_file).await {
            Ok(content) => {
                match serde_json::from_str::<PythonMinerStats>(&content) {
                    Ok(stats) => Some(stats),
                    Err(e) => {
                        warn!("Failed to parse Python miner stats: {}", e);
                        None
                    }
                }
            }
            Err(_) => None,
        }
    }

    /// Check if the Python miner process is still alive
    pub async fn is_running(&self) -> bool {
        let mut guard = self.child.write().await;
        if let Some(ref mut child) = *guard {
            match child.try_wait() {
                Ok(None) => true,  // Still running
                Ok(Some(_)) => {
                    *self.running.write().await = false;
                    false
                }
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Stop the Python miner subprocess
    pub async fn stop(&self) {
        *self.running.write().await = false;

        let mut guard = self.child.write().await;
        if let Some(ref mut child) = *guard {
            info!("🐍 Stopping Python miner (PID: {:?})...", child.id());

            // Try graceful SIGTERM first (Unix)
            #[cfg(unix)]
            {
                unsafe {
                    libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
                }
                // Wait up to 3 seconds for graceful exit
                for _ in 0..30 {
                    if let Ok(Some(_)) = child.try_wait() {
                        info!("🐍 Python miner stopped gracefully");
                        *guard = None;
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }

            // Force kill if still alive
            match child.kill() {
                Ok(_) => info!("🐍 Python miner killed"),
                Err(e) => warn!("🐍 Failed to kill Python miner: {}", e),
            }
            let _ = child.wait();
        }
        *guard = None;
    }
}

impl Drop for PythonFallbackMiner {
    fn drop(&mut self) {
        // Best-effort sync kill on drop
        if let Ok(mut guard) = self.child.try_write() {
            if let Some(ref mut child) = *guard {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variant_from_str() {
        assert_eq!(PythonMinerVariant::from_str("chv3"), Some(PythonMinerVariant::Chv3Gpu));
        assert_eq!(PythonMinerVariant::from_str("gpu"), Some(PythonMinerVariant::Chv3Gpu));
        assert_eq!(PythonMinerVariant::from_str("legacy"), Some(PythonMinerVariant::Legacy));
        assert_eq!(PythonMinerVariant::from_str("native"), Some(PythonMinerVariant::Legacy));
        assert_eq!(PythonMinerVariant::from_str("unknown"), None);
    }

    #[test]
    fn test_pool_host_port() {
        let config = PythonFallbackConfig {
            pool_url: "stratum+tcp://pool.zionterranova.com:3333".to_string(),
            wallet: "test".to_string(),
            worker: "w".to_string(),
            algorithm: "cosmic_harmony".to_string(),
            gpu: false,
            threads: 1,
            stats_file: PathBuf::from("/tmp/stats.json"),
            stats_interval: 5,
            variant: PythonMinerVariant::Chv3Gpu,
            script_path: None,
            extra_args: vec![],
        };
        let miner = PythonFallbackMiner::new(config);
        assert_eq!(miner.pool_host_port(), "pool.zionterranova.com:3333");
    }

    #[test]
    fn test_find_python() {
        // Should find python3 on most systems
        let result = PythonFallbackMiner::find_python();
        // Don't assert success — CI may not have Python
        if let Ok(py) = result {
            assert!(py.contains("python"));
        }
    }
}
