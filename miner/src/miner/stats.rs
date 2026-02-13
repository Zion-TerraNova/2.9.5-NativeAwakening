use chrono::{DateTime, Utc};
use colored::*;
use std::collections::VecDeque;
use std::io::Write;
use std::time::Instant;

// ═══════════════════════════════════════════════════════════════════════
// ROLLING HASHRATE WINDOW — XMRig-style 10s / 60s / 15m averages
// ═══════════════════════════════════════════════════════════════════════

struct HashrateWindow {
    samples: VecDeque<(Instant, u64)>,
    window_secs: u64,
}

impl HashrateWindow {
    fn new(window_secs: u64) -> Self {
        Self {
            samples: VecDeque::with_capacity(256),
            window_secs,
        }
    }

    fn push(&mut self, now: Instant, hashes: u64) {
        self.samples.push_back((now, hashes));
        let cutoff = now.checked_sub(std::time::Duration::from_secs(self.window_secs + 2));
        if let Some(cutoff) = cutoff {
            while self.samples.front().map_or(false, |(t, _)| *t < cutoff) {
                self.samples.pop_front();
            }
        }
    }

    fn rate(&self) -> f64 {
        if self.samples.len() < 2 {
            return 0.0;
        }
        let first = self.samples.front().unwrap();
        let last = self.samples.back().unwrap();
        let dt = last.0.duration_since(first.0).as_secs_f64();
        if dt < 0.5 { return 0.0; }
        let total: u64 = self.samples.iter().skip(1).map(|(_, h)| h).sum();
        total as f64 / dt
    }
}

/// Per-thread hashrate snapshot
#[derive(Debug, Clone)]
pub struct ThreadSnapshot {
    pub thread_id: usize,
    pub hashrate: f64,
    pub hashes: u64,
}

// ═══════════════════════════════════════════════════════════════════════
// MINER STATS — Professional XMRig-style metrics engine
// ═══════════════════════════════════════════════════════════════════════

pub struct MinerStats {
    start_time: Instant,
    total_hashes: u64,
    gpu_hashes: u64,
    shares_accepted: u64,
    shares_rejected: u64,
    blocks_found: u64,
    last_update: DateTime<Utc>,

    // Pool metadata
    difficulty: f64,
    best_share_diff: f64,
    pool_height: u64,
    pool_latency_ms: u32,
    connection_count: u32,

    // Config metadata
    algorithm: String,
    worker: String,
    pool: String,
    cpu_threads: usize,
    gpu_name: String,

    // Rolling hashrate windows (XMRig 10s/60s/15m)
    window_10s: HashrateWindow,
    window_60s: HashrateWindow,
    window_15m: HashrateWindow,

    // Per-thread snapshots
    thread_snapshots: Vec<ThreadSnapshot>,

    // Print state
    print_count: u64,
    last_share_time: Option<Instant>,
    last_event: Option<String>,
}

impl MinerStats {
    pub fn new() -> Self {
        Self {
            start_time: Instant::now(),
            total_hashes: 0,
            gpu_hashes: 0,
            shares_accepted: 0,
            shares_rejected: 0,
            blocks_found: 0,
            last_update: Utc::now(),
            difficulty: 0.0,
            best_share_diff: 0.0,
            pool_height: 0,
            pool_latency_ms: 0,
            connection_count: 1,
            algorithm: String::new(),
            worker: String::new(),
            pool: String::new(),
            cpu_threads: 0,
            gpu_name: String::new(),
            window_10s: HashrateWindow::new(10),
            window_60s: HashrateWindow::new(60),
            window_15m: HashrateWindow::new(900),
            thread_snapshots: Vec::new(),
            print_count: 0,
            last_share_time: None,
            last_event: None,
        }
    }

    // ──── Config setters ────

    pub fn set_config(&mut self, algo: &str, worker: &str, pool: &str, threads: usize) {
        self.algorithm = algo.to_string();
        self.worker = worker.to_string();
        self.pool = pool.to_string();
        self.cpu_threads = threads;
    }

    pub fn set_gpu_name(&mut self, name: &str) {
        self.gpu_name = name.to_string();
    }

    pub fn set_difficulty(&mut self, diff: f64) {
        self.difficulty = diff;
    }

    pub fn set_pool_height(&mut self, height: u64) {
        self.pool_height = height;
    }

    pub fn set_pool_latency(&mut self, ms: u32) {
        self.pool_latency_ms = ms;
    }

    pub fn increment_connections(&mut self) {
        self.connection_count += 1;
    }

    // ──── Hash tracking ────

    pub fn add_hashes(&mut self, count: u64) {
        self.total_hashes += count;
        self.last_update = Utc::now();
        let now = Instant::now();
        self.window_10s.push(now, count);
        self.window_60s.push(now, count);
        self.window_15m.push(now, count);
    }

    pub fn add_gpu_hashes(&mut self, count: u64) {
        self.gpu_hashes += count;
        self.total_hashes += count;
        self.last_update = Utc::now();
        let now = Instant::now();
        self.window_10s.push(now, count);
        self.window_60s.push(now, count);
        self.window_15m.push(now, count);
    }

    pub fn update_thread_snapshots(&mut self, snapshots: Vec<ThreadSnapshot>) {
        self.thread_snapshots = snapshots;
    }

    // ──── Share tracking ────

    pub fn share_accepted(&mut self) {
        self.shares_accepted += 1;
        self.last_share_time = Some(Instant::now());
    }

    pub fn share_accepted_with_diff(&mut self, diff: f64) {
        self.shares_accepted += 1;
        self.last_share_time = Some(Instant::now());
        if diff > self.best_share_diff {
            self.best_share_diff = diff;
        }
    }

    pub fn share_rejected(&mut self) {
        self.shares_rejected += 1;
    }

    pub fn block_found(&mut self) {
        self.blocks_found += 1;
    }

    // ──── Hashrate accessors ────

    pub fn hashrate(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 { self.total_hashes as f64 / elapsed } else { 0.0 }
    }

    pub fn hashrate_10s(&self) -> f64 { self.window_10s.rate() }
    pub fn hashrate_60s(&self) -> f64 { self.window_60s.rate() }
    pub fn hashrate_15m(&self) -> f64 { self.window_15m.rate() }

    pub fn hashrate_gpu(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 { self.gpu_hashes as f64 / elapsed } else { 0.0 }
    }

    pub fn hashrate_cpu(&self) -> f64 {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed > 0.0 {
            let cpu_hashes = self.total_hashes.saturating_sub(self.gpu_hashes);
            cpu_hashes as f64 / elapsed
        } else { 0.0 }
    }

    // ──── Stat accessors ────

    pub fn total_hashes(&self) -> u64 { self.total_hashes }
    pub fn shares_accepted(&self) -> u64 { self.shares_accepted }
    pub fn shares_rejected(&self) -> u64 { self.shares_rejected }
    pub fn uptime_seconds(&self) -> u64 { self.start_time.elapsed().as_secs() }

    pub fn reset_shares(&mut self) {
        self.shares_accepted = 0;
        self.shares_rejected = 0;
    }

    // ──── Formatting helpers (XMRig-style) ────

    fn fmt_hashrate(h: f64) -> (String, &'static str) {
        if h >= 1e12      { (format!("{:.2}", h / 1e12), "TH/s") }
        else if h >= 1e9  { (format!("{:.2}", h / 1e9),  "GH/s") }
        else if h >= 1e6  { (format!("{:.2}", h / 1e6),  "MH/s") }
        else if h >= 1e3  { (format!("{:.2}", h / 1e3),  "kH/s") }
        else              { (format!("{:.1}", h),         "H/s")  }
    }

    fn fmt_uptime(secs: u64) -> String {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        let m = (secs % 3600) / 60;
        let s = secs % 60;
        if d > 0 { format!("{}d {:02}:{:02}:{:02}", d, h, m, s) }
        else { format!("{:02}:{:02}:{:02}", h, m, s) }
    }

    fn fmt_difficulty(d: f64) -> String {
        if d >= 1e12      { format!("{:.2}T", d / 1e12) }
        else if d >= 1e9  { format!("{:.2}G", d / 1e9) }
        else if d >= 1e6  { format!("{:.2}M", d / 1e6) }
        else if d >= 1e3  { format!("{:.2}K", d / 1e3) }
        else              { format!("{:.0}", d) }
    }

    fn fmt_total_hashes(h: u64) -> String {
        if h >= 1_000_000_000 { format!("{:.1}G", h as f64 / 1e9) }
        else if h >= 1_000_000 { format!("{:.1}M", h as f64 / 1e6) }
        else if h >= 1_000 { format!("{:.1}K", h as f64 / 1e3) }
        else { format!("{}", h) }
    }

    fn share_pct(&self) -> String {
        let total = self.shares_accepted + self.shares_rejected;
        if total == 0 { return "—".to_string(); }
        let pct = (self.shares_accepted as f64 / total as f64) * 100.0;
        format!("{:.1}%", pct)
    }

    // ═══════════════════════════════════════════════════════════
    // STATIC ANSI PANEL — SRBMiner-style in-place overwrite
    // No scrolling. Panel stays fixed. Numbers update in place.
    // ═══════════════════════════════════════════════════════════

    /// How many terminal lines the static panel occupies
    const PANEL_LINES: usize = 9;

    pub fn set_event(&mut self, msg: String) {
        self.last_event = Some(msg);
    }

    /// Render the static panel — overwrites previous panel in-place
    /// Uses ANSI escape: \x1B[{N}A = move cursor up N lines
    ///                    \x1B[2K  = erase entire line
    ///                    \r       = carriage return
    pub fn print(&mut self) {
        self.print_count += 1;

        let now = Utc::now().format("%H:%M:%S");
        let uptime = Self::fmt_uptime(self.uptime_seconds());
        let total_h = Self::fmt_total_hashes(self.total_hashes);
        let diff_str = Self::fmt_difficulty(self.difficulty);

        // Hashrate values — use same unit for consistency
        let (_, unit) = Self::fmt_hashrate(self.hashrate_10s());
        let divisor = match unit {
            "TH/s" => 1e12, "GH/s" => 1e9, "MH/s" => 1e6, "kH/s" => 1e3, _ => 1.0,
        };
        let v10 = self.hashrate_10s() / divisor;
        let v60 = self.hashrate_60s() / divisor;
        let v15 = self.hashrate_15m() / divisor;

        let gpu_str = if self.gpu_name.is_empty() {
            "—".to_string()
        } else {
            let (gv, gu) = Self::fmt_hashrate(self.hashrate_gpu());
            format!("{} {} [{}]", gv, gu, self.gpu_name)
        };

        let event_str = self.last_event.as_deref().unwrap_or("—");

        // If this is not the first print, move cursor up to overwrite
        let mut out = std::io::stdout().lock();
        if self.print_count > 1 {
            // Move cursor up PANEL_LINES lines
            let _ = write!(out, "\x1B[{}A", Self::PANEL_LINES);
        }

        // Each line: \x1B[2K = clear line, \r = start of line
        let bar = "─".repeat(64);
        let _ = writeln!(out, "\x1B[2K\r{}",
            format!("┌{}┐", bar).bright_black());
        let _ = writeln!(out, "\x1B[2K\r{}  {}   10s {} {}  60s {}  15m {}",
            "│".bright_black(),
            "SPEED".bright_white().bold(),
            format!("{:.2}", v10).bright_cyan().bold(),
            unit.white(),
            format!("{:.2}", v60).bright_cyan(),
            format!("{:.2}", v15).bright_cyan(),
        );
        let _ = writeln!(out, "\x1B[2K\r{}  {}  A: {}  R: {}  rate: {}",
            "│".bright_black(),
            "SHARES".bright_white().bold(),
            self.shares_accepted.to_string().bright_green().bold(),
            self.shares_rejected.to_string().bright_red(),
            self.share_pct().bright_white(),
        );
        let _ = writeln!(out, "\x1B[2K\r{}  {}    pool: {}  height: {}  blocks: {}",
            "│".bright_black(),
            "DIFF".bright_white().bold(),
            diff_str.bright_yellow(),
            self.pool_height.to_string().bright_white(),
            self.blocks_found.to_string().bright_cyan().bold(),
        );
        let _ = writeln!(out, "\x1B[2K\r{}  {}  {}  hashes: {}  algo: {}",
            "│".bright_black(),
            "UPTIME".bright_white().bold(),
            uptime.bright_white(),
            total_h.bright_cyan(),
            self.algorithm.bright_cyan(),
        );
        let _ = writeln!(out, "\x1B[2K\r{}  {}     cpu: {}T  gpu: {}",
            "│".bright_black(),
            "HW".bright_white().bold(),
            self.cpu_threads.to_string().bright_magenta(),
            gpu_str.bright_green(),
        );
        let _ = writeln!(out, "\x1B[2K\r{}  {}   pool: {}  worker: {}",
            "│".bright_black(),
            "NET".bright_white().bold(),
            self.pool.bright_white(),
            self.worker.bright_white(),
        );
        let _ = writeln!(out, "\x1B[2K\r{}  {}  [{}] {}",
            "│".bright_black(),
            "EVENT".bright_white().bold(),
            now.to_string().bright_black(),
            event_str.bright_green(),
        );
        let _ = writeln!(out, "\x1B[2K\r{}",
            format!("└{}┘", bar).bright_black());

        let _ = out.flush();
    }

    /// Called when share is accepted — just updates stats + event, no print
    pub fn print_accepted(&mut self) {
        self.set_event(format!(
            "accepted {}/{} (+1) diff {} ({})",
            self.shares_accepted, self.shares_rejected,
            Self::fmt_difficulty(self.difficulty),
            self.share_pct(),
        ));
    }

    /// Called when share is rejected — just updates stats + event, no print
    pub fn print_rejected(&mut self, reason: &str) {
        self.set_event(format!(
            "rejected {}/{} — {}",
            self.shares_accepted, self.shares_rejected, reason,
        ));
    }

    /// New job notification — just updates event, no print
    pub fn print_new_job(&mut self) {
        self.set_event(format!(
            "new job height {} diff {} algo {}",
            self.pool_height,
            Self::fmt_difficulty(self.difficulty),
            self.algorithm,
        ));
    }

    /// Block found celebration — event + special marker
    pub fn print_block_found(&mut self, height: u64) {
        self.set_event(format!(
            "🏆 BLOCK FOUND height {} (total: {})",
            height, self.blocks_found,
        ));
    }

    /// JSON payload for Desktop Agent stats file — enriched with all metrics
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "hashrate": self.hashrate(),
            "hashrate_10s": self.hashrate_10s(),
            "hashrate_60s": self.hashrate_60s(),
            "hashrate_15m": self.hashrate_15m(),
            "hashrate_max": self.hashrate(),
            "hashrate_cpu": self.hashrate_cpu(),
            "hashrate_gpu": self.hashrate_gpu(),
            "hashrate_window_hs": self.hashrate_10s(),
            "shares_sent": self.shares_accepted + self.shares_rejected,
            "shares_accepted": self.shares_accepted,
            "shares_rejected": self.shares_rejected,
            "blocks_found": self.blocks_found,
            "difficulty": self.difficulty,
            "best_share_diff": self.best_share_diff,
            "pool_height": self.pool_height,
            "pool_latency_ms": self.pool_latency_ms,
            "uptime_sec": self.uptime_seconds(),
            "total_hashes": self.total_hashes,
            "algorithm": &self.algorithm,
            "worker": &self.worker,
            "pool": &self.pool,
            "cpu_threads": self.cpu_threads,
            "gpu_name": if self.gpu_name.is_empty() { "none" } else { &self.gpu_name },
            "connection_count": self.connection_count,
            "version": "2.9.5",
            "threads": self.thread_snapshots.iter().map(|t| {
                serde_json::json!({
                    "id": t.thread_id,
                    "hashrate": t.hashrate,
                    "hashes": t.hashes,
                })
            }).collect::<Vec<_>>(),
        })
    }
}

impl Default for MinerStats {
    fn default() -> Self {
        Self::new()
    }
}
