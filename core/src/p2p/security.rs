use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Rate limiter for P2P connections
#[derive(Clone)]
pub struct RateLimiter {
    // Track connection attempts per IP
    attempts: Arc<Mutex<HashMap<IpAddr, Vec<u64>>>>,
    // Maximum connections per IP (reserved for future concurrent connection limiting)
    #[allow(dead_code)]
    max_connections_per_ip: usize,
    // Time window in seconds
    window_secs: u64,
    // Maximum connection rate (attempts per window)
    max_rate: usize,
}

impl RateLimiter {
    pub fn new(max_connections: usize, window_secs: u64, max_rate: usize) -> Self {
        Self {
            attempts: Arc::new(Mutex::new(HashMap::new())),
            max_connections_per_ip: max_connections,
            window_secs,
            max_rate,
        }
    }

    /// Check if connection from IP is allowed
    pub fn allow_connection(&self, ip: IpAddr) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut attempts = self.attempts.lock().unwrap();
        
        // Get or create attempt history for this IP
        let history = attempts.entry(ip).or_insert_with(Vec::new);
        
        // Remove old attempts outside window
        history.retain(|&timestamp| now - timestamp < self.window_secs);
        
        // Check rate limit
        if history.len() >= self.max_rate {
            println!("[P2P Security] Rate limit exceeded for {}: {} attempts in {}s", 
                     ip, history.len(), self.window_secs);
            return false;
        }
        
        // Record this attempt
        history.push(now);
        true
    }

    /// Cleanup old entries periodically
    pub fn cleanup(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut attempts = self.attempts.lock().unwrap();
        attempts.retain(|_, history| {
            history.retain(|&timestamp| now - timestamp < self.window_secs);
            !history.is_empty()
        });
    }

    /// Get current stats
    pub fn stats(&self) -> (usize, usize) {
        let attempts = self.attempts.lock().unwrap();
        let total_ips = attempts.len();
        let total_attempts: usize = attempts.values().map(|v| v.len()).sum();
        (total_ips, total_attempts)
    }
}

/// Blacklist manager for malicious IPs
#[derive(Clone)]
pub struct Blacklist {
    // Permanently banned IPs
    permanent: Arc<Mutex<Vec<IpAddr>>>,
    // Temporarily banned IPs with expiry timestamp
    temporary: Arc<Mutex<HashMap<IpAddr, u64>>>,
}

impl Blacklist {
    pub fn new() -> Self {
        Self {
            permanent: Arc::new(Mutex::new(Vec::new())),
            temporary: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if IP is blacklisted
    pub fn is_blacklisted(&self, ip: &IpAddr) -> bool {
        // Check permanent ban
        {
            let permanent = self.permanent.lock().unwrap();
            if permanent.contains(ip) {
                return true;
            }
        }

        // Check temporary ban
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let temporary = self.temporary.lock().unwrap();
        if let Some(&expiry) = temporary.get(ip) {
            if now < expiry {
                return true;
            }
        }

        false
    }

    /// Add IP to permanent blacklist
    pub fn ban_permanent(&self, ip: IpAddr) {
        let mut permanent = self.permanent.lock().unwrap();
        if !permanent.contains(&ip) {
            permanent.push(ip);
            println!("[P2P Security] Permanently banned {}", ip);
        }
    }

    /// Add IP to temporary blacklist (duration in seconds)
    pub fn ban_temporary(&self, ip: IpAddr, duration_secs: u64) {
        let expiry = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() + duration_secs;

        let mut temporary = self.temporary.lock().unwrap();
        temporary.insert(ip, expiry);
        println!("[P2P Security] Temporarily banned {} for {}s", ip, duration_secs);
    }

    /// Remove IP from blacklist
    pub fn unban(&self, ip: &IpAddr) {
        {
            let mut permanent = self.permanent.lock().unwrap();
            permanent.retain(|&banned_ip| banned_ip != *ip);
        }
        {
            let mut temporary = self.temporary.lock().unwrap();
            temporary.remove(ip);
        }
        println!("[P2P Security] Unbanned {}", ip);
    }

    /// Cleanup expired temporary bans
    pub fn cleanup(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut temporary = self.temporary.lock().unwrap();
        temporary.retain(|ip, &mut expiry| {
            let keep = now < expiry;
            if !keep {
                println!("[P2P Security] Temporary ban expired for {}", ip);
            }
            keep
        });
    }

    /// Get blacklist stats
    pub fn stats(&self) -> (usize, usize) {
        let permanent = self.permanent.lock().unwrap();
        let temporary = self.temporary.lock().unwrap();
        (permanent.len(), temporary.len())
    }
}

/// Connection limiter (max total connections)
pub struct ConnectionLimiter {
    max_connections: usize,
}

impl ConnectionLimiter {
    pub fn new(max_connections: usize) -> Self {
        Self { max_connections }
    }

    pub fn allow_connection(&self, current_count: usize) -> bool {
        if current_count >= self.max_connections {
            println!("[P2P Security] Connection limit reached: {}/{}", 
                     current_count, self.max_connections);
            false
        } else {
            true
        }
    }
}

// ---------------------------------------------------------------------------
// Sprint 1.7 — Per-Peer Message Rate Limiter
// ---------------------------------------------------------------------------

/// Per-peer message rate limiter.
///
/// Tracks message counts per peer within a sliding window.
/// Prevents flood attacks at the message level (complementary to
/// the connection-level `RateLimiter`).
///
/// Misbehavior score accumulates across windows; peers that
/// repeatedly exceed the threshold get escalating ban times.
#[derive(Clone)]
pub struct MessageRateLimiter {
    /// Peer IP → (timestamps of recent messages, misbehavior_score)
    peers: Arc<Mutex<HashMap<IpAddr, (Vec<u64>, u32)>>>,
    /// Max messages per peer per window
    max_messages: usize,
    /// Sliding window duration in seconds
    window_secs: u64,
    /// Misbehavior score threshold for auto-ban
    ban_threshold: u32,
}

impl MessageRateLimiter {
    pub fn new(max_messages: usize, window_secs: u64, ban_threshold: u32) -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
            max_messages,
            window_secs,
            ban_threshold,
        }
    }

    /// Check if a message from this peer is allowed.
    ///
    /// Returns `Ok(())` if allowed, `Err(score)` with current
    /// misbehavior score if rate exceeded.
    pub fn allow_message(&self, ip: IpAddr) -> Result<(), u32> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut peers = self.peers.lock().unwrap();
        let entry = peers.entry(ip).or_insert_with(|| (Vec::new(), 0));

        // Prune old timestamps
        entry.0.retain(|&ts| now.saturating_sub(ts) < self.window_secs);

        if entry.0.len() >= self.max_messages {
            entry.1 += 1; // Increment misbehavior score
            Err(entry.1)
        } else {
            entry.0.push(now);
            Ok(())
        }
    }

    /// Check if a peer has exceeded the ban threshold.
    pub fn should_ban(&self, ip: &IpAddr) -> bool {
        let peers = self.peers.lock().unwrap();
        if let Some((_timestamps, score)) = peers.get(ip) {
            *score >= self.ban_threshold
        } else {
            false
        }
    }

    /// Get the escalating ban duration based on misbehavior score.
    ///
    /// AUDIT-FIX P1-10: Hardened ban durations for mainnet safety.
    /// Score 1-2: 300s (5 min), Score 3-5: 1800s (30 min), Score 6+: 7200s (2 hours)
    pub fn ban_duration_secs(&self, ip: &IpAddr) -> u64 {
        let peers = self.peers.lock().unwrap();
        let score = peers.get(ip).map(|(_, s)| *s).unwrap_or(0);
        match score {
            0..=2 => 300,
            3..=5 => 1800,
            _ => 7200,
        }
    }

    /// Reset misbehavior score for a peer (e.g. after successful unban).
    pub fn reset_score(&self, ip: &IpAddr) {
        let mut peers = self.peers.lock().unwrap();
        if let Some(entry) = peers.get_mut(ip) {
            entry.1 = 0;
        }
    }

    /// Cleanup stale peer entries (no messages in 2× window).
    pub fn cleanup(&self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let cutoff = self.window_secs * 2;
        let mut peers = self.peers.lock().unwrap();
        peers.retain(|_, (timestamps, _)| {
            timestamps.retain(|&ts| now.saturating_sub(ts) < cutoff);
            !timestamps.is_empty()
        });
    }

    /// Get stats: (tracked_peers, total_messages, total_misbehavior_score)
    pub fn stats(&self) -> (usize, usize, u32) {
        let peers = self.peers.lock().unwrap();
        let tracked = peers.len();
        let total_msgs: usize = peers.values().map(|(ts, _)| ts.len()).sum();
        let total_score: u32 = peers.values().map(|(_, s)| *s).sum();
        (tracked, total_msgs, total_score)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_rate_limiter_allows_under_limit() {
        let limiter = RateLimiter::new(10, 60, 5);
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        // Should allow first 5 attempts
        for _ in 0..5 {
            assert!(limiter.allow_connection(ip));
        }

        // Should deny 6th attempt
        assert!(!limiter.allow_connection(ip));
    }

    #[test]
    fn test_blacklist_permanent() {
        let blacklist = Blacklist::new();
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));

        assert!(!blacklist.is_blacklisted(&ip));
        
        blacklist.ban_permanent(ip);
        assert!(blacklist.is_blacklisted(&ip));
        
        blacklist.unban(&ip);
        assert!(!blacklist.is_blacklisted(&ip));
    }

    #[test]
    fn test_blacklist_temporary() {
        let blacklist = Blacklist::new();
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

        blacklist.ban_temporary(ip, 2); // 2 second ban
        assert!(blacklist.is_blacklisted(&ip));
        
        // After cleanup (if time passed), should be unbanned
        // Note: This test might be flaky with timing
    }

    #[test]
    fn test_connection_limiter() {
        let limiter = ConnectionLimiter::new(10);

        assert!(limiter.allow_connection(5));
        assert!(limiter.allow_connection(9));
        assert!(!limiter.allow_connection(10));
        assert!(!limiter.allow_connection(15));
    }

    #[test]
    fn test_rate_limiter_stats() {
        let limiter = RateLimiter::new(10, 60, 5);
        let ip1 = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2));

        limiter.allow_connection(ip1);
        limiter.allow_connection(ip1);
        limiter.allow_connection(ip2);

        let (ips, attempts) = limiter.stats();
        assert_eq!(ips, 2);
        assert_eq!(attempts, 3);
    }

    // -----------------------------------------------------------------
    // Sprint 1.7 — MessageRateLimiter Tests
    // -----------------------------------------------------------------

    #[test]
    fn test_msg_rate_limiter_allows_under_limit() {
        let limiter = MessageRateLimiter::new(100, 60, 3);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

        for _ in 0..100 {
            assert!(limiter.allow_message(ip).is_ok());
        }
    }

    #[test]
    fn test_msg_rate_limiter_blocks_over_limit() {
        let limiter = MessageRateLimiter::new(5, 60, 3);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        // First 5 allowed
        for _ in 0..5 {
            assert!(limiter.allow_message(ip).is_ok());
        }
        // 6th denied
        let result = limiter.allow_message(ip);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), 1); // first misbehavior
    }

    #[test]
    fn test_msg_rate_limiter_misbehavior_score_accumulates() {
        let limiter = MessageRateLimiter::new(2, 60, 3);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));

        // Fill window
        limiter.allow_message(ip).unwrap();
        limiter.allow_message(ip).unwrap();

        // Each excess message increments score
        assert_eq!(limiter.allow_message(ip).unwrap_err(), 1);
        assert_eq!(limiter.allow_message(ip).unwrap_err(), 2);
        assert_eq!(limiter.allow_message(ip).unwrap_err(), 3);

        assert!(limiter.should_ban(&ip));
    }

    #[test]
    fn test_msg_rate_limiter_should_ban_threshold() {
        let limiter = MessageRateLimiter::new(1, 60, 2);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 4));

        limiter.allow_message(ip).unwrap();
        assert!(!limiter.should_ban(&ip));

        let _ = limiter.allow_message(ip); // score=1
        assert!(!limiter.should_ban(&ip));

        let _ = limiter.allow_message(ip); // score=2
        assert!(limiter.should_ban(&ip));
    }

    #[test]
    fn test_msg_rate_limiter_ban_duration_escalation() {
        let limiter = MessageRateLimiter::new(1, 60, 2);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5));

        // Score 0 → 300s (AUDIT-FIX P1-10: hardened)
        assert_eq!(limiter.ban_duration_secs(&ip), 300);

        // Fill + overshoot to score 1
        limiter.allow_message(ip).unwrap();
        let _ = limiter.allow_message(ip); // score 1
        assert_eq!(limiter.ban_duration_secs(&ip), 300);

        // score 3 → 1800s
        let _ = limiter.allow_message(ip); // score 2
        let _ = limiter.allow_message(ip); // score 3
        assert_eq!(limiter.ban_duration_secs(&ip), 1800);

        // score 6+ → 7200s
        let _ = limiter.allow_message(ip); // 4
        let _ = limiter.allow_message(ip); // 5
        let _ = limiter.allow_message(ip); // 6
        assert_eq!(limiter.ban_duration_secs(&ip), 7200);
    }

    #[test]
    fn test_msg_rate_limiter_reset_score() {
        let limiter = MessageRateLimiter::new(1, 60, 2);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 6));

        limiter.allow_message(ip).unwrap();
        let _ = limiter.allow_message(ip); // score 1
        let _ = limiter.allow_message(ip); // score 2
        assert!(limiter.should_ban(&ip));

        limiter.reset_score(&ip);
        assert!(!limiter.should_ban(&ip));
    }

    #[test]
    fn test_msg_rate_limiter_multiple_peers_independent() {
        let limiter = MessageRateLimiter::new(2, 60, 3);
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 8));

        // Fill ip1
        limiter.allow_message(ip1).unwrap();
        limiter.allow_message(ip1).unwrap();
        assert!(limiter.allow_message(ip1).is_err()); // ip1 exceeded

        // ip2 should still be fine
        assert!(limiter.allow_message(ip2).is_ok());
        assert!(limiter.allow_message(ip2).is_ok());
    }

    #[test]
    fn test_msg_rate_limiter_stats() {
        let limiter = MessageRateLimiter::new(10, 60, 3);
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 9));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 10));

        limiter.allow_message(ip1).unwrap();
        limiter.allow_message(ip1).unwrap();
        limiter.allow_message(ip2).unwrap();

        let (peers, msgs, score) = limiter.stats();
        assert_eq!(peers, 2);
        assert_eq!(msgs, 3);
        assert_eq!(score, 0);
    }

    // -----------------------------------------------------------------
    // Cross-component tests
    // -----------------------------------------------------------------

    #[test]
    fn test_blacklist_stats() {
        let bl = Blacklist::new();
        let ip1 = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let ip2 = IpAddr::V4(Ipv4Addr::new(5, 6, 7, 8));

        let (perm, temp) = bl.stats();
        assert_eq!(perm, 0);
        assert_eq!(temp, 0);

        bl.ban_permanent(ip1);
        bl.ban_temporary(ip2, 600);

        let (perm, temp) = bl.stats();
        assert_eq!(perm, 1);
        assert_eq!(temp, 1);
    }

    #[test]
    fn test_permanent_ban_survives_cleanup() {
        let bl = Blacklist::new();
        let ip = IpAddr::V4(Ipv4Addr::new(9, 8, 7, 6));

        bl.ban_permanent(ip);
        bl.cleanup();
        assert!(bl.is_blacklisted(&ip));
    }

    #[test]
    fn test_rate_limiter_different_ips_independent() {
        let limiter = RateLimiter::new(10, 60, 2);
        let ip1 = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(2, 2, 2, 2));

        // Fill ip1
        limiter.allow_connection(ip1);
        limiter.allow_connection(ip1);
        assert!(!limiter.allow_connection(ip1)); // blocked

        // ip2 should be independent
        assert!(limiter.allow_connection(ip2));
        assert!(limiter.allow_connection(ip2));
        assert!(!limiter.allow_connection(ip2)); // blocked
    }

    #[test]
    fn test_connection_limiter_boundary() {
        let limiter = ConnectionLimiter::new(1);
        assert!(limiter.allow_connection(0));
        assert!(!limiter.allow_connection(1));
    }

    #[test]
    fn test_msg_rate_limiter_unknown_peer_not_banned() {
        let limiter = MessageRateLimiter::new(5, 60, 3);
        let unknown = IpAddr::V4(Ipv4Addr::new(99, 99, 99, 99));
        assert!(!limiter.should_ban(&unknown));
        assert_eq!(limiter.ban_duration_secs(&unknown), 300); // default (AUDIT-FIX P1-10)
    }
}
