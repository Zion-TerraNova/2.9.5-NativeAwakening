/// ZION Revenue & DAO System (M6)
///
/// Two economic mechanisms:
///
/// 1. **Fee burning** — All transaction fees are destroyed (already in fee.rs).
///    The coinbase reward does NOT include fees; they vanish from supply.
///
/// 2. **BTC Revenue → 100% DAO** — BTC revenue from external mining (ETC, RVN,
///    XMR via 2miners / MoneroOcean) goes entirely to the DAO treasury address.
///    No burn, no split. Every satoshi earned strengthens the ZION ecosystem.
///
///    DAO funds are used for: development, infrastructure, marketing,
///    liquidity, ZION OASIS + Winners Golden Egg/Xp, humanitarian fund, and team.
///
/// Design:
///   - A burn address (`BURN_ADDRESS`) exists for fee burning (L1 consensus).
///   - A DAO address (`DAO_ADDRESS`) receives 100% of BTC/external revenue.
///   - `BuybackTracker` records each revenue event with DAO allocation.
///   - Storage persistence via JSON file (`buyback_ledger.json`).
///   - RPC / API exposes cumulative stats at `/api/buyback/stats`.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Canonical burn address — provably unspendable.
///
/// This address is constructed so that nobody holds the private key.
/// Any UTXO sent here is permanently removed from circulating supply.
/// Used ONLY for L1 fee burning (not for BTC revenue).
pub const BURN_ADDRESS: &str = "zion1burn0000000000000000000000000000000dead";

/// DAO Treasury address — receives 100% of external mining BTC revenue.
///
/// Controlled by ZION DAO multisig. Used for development, infrastructure,
/// marketing, liquidity, ZION OASIS + Winners Golden Egg/Xp, humanitarian fund, and team.
pub const DAO_ADDRESS: &str = "zion1dao00000000000000000000000000000treasury";

/// Percentage of BTC revenue allocated to burn (deflation).
/// Set to 0 — all revenue goes to DAO. Every BTC counts.
pub const BURN_SHARE_PERCENT: u64 = 0;

/// Percentage of BTC revenue allocated to DAO treasury.
/// Set to 100 — all external mining revenue strengthens the ecosystem.
pub const DAO_SHARE_PERCENT: u64 = 100;

// Legacy aliases for backward compatibility
pub const CREATORS_ADDRESS: &str = "zion1dao00000000000000000000000000000treasury";
pub const CREATORS_SHARE_PERCENT: u64 = 100;

/// Validate whether an address is the canonical burn address.
pub fn is_burn_address(address: &str) -> bool {
    address == BURN_ADDRESS
}

/// Validate whether an address is the DAO treasury address.
pub fn is_dao_address(address: &str) -> bool {
    address == DAO_ADDRESS
}

/// Legacy alias
pub fn is_creators_address(address: &str) -> bool {
    is_dao_address(address)
}

/// Calculate revenue split — 100% to DAO, 0% burn.
///
/// Returns `(burn_amount, dao_amount)` in atomic units.
/// burn_amount is always 0 (no BTC revenue is burned).
pub fn calculate_revenue_split(total_zion_atomic: u64) -> (u64, u64) {
    (0, total_zion_atomic) // 100% to DAO
}

/// Calculate the BTC revenue split (satoshis) — 100% to DAO.
///
/// Returns `(burn_btc_sats, dao_btc_sats)`.
pub fn calculate_btc_revenue_split(total_btc_sats: u64) -> (u64, u64) {
    (0, total_btc_sats) // 100% to DAO
}

// ---------------------------------------------------------------------------
// Revenue Events
// ---------------------------------------------------------------------------

/// A single revenue event: BTC from external mining → 100% DAO treasury.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuybackEvent {
    /// Unique event ID (sequential)
    pub id: u64,
    /// Unix timestamp of the event
    pub timestamp: u64,
    /// Total BTC revenue (satoshis)
    pub btc_amount_sats: u64,
    /// BTC allocated to burn (always 0)
    pub btc_burn_sats: u64,
    /// BTC allocated to DAO treasury (= btc_amount_sats)
    pub btc_creators_sats: u64,
    /// ZION burned (always 0 for BTC revenue events)
    pub zion_burned_atomic: u64,
    /// ZION equivalent sent to DAO (atomic units)
    pub zion_creators_rent_atomic: u64,
    /// Price per ZION in BTC (satoshis per ZION atomic unit) — informational
    pub price_sats_per_zion: f64,
    /// On-chain TX hash (burn — empty string, no burn)
    pub burn_tx_hash: String,
    /// On-chain TX hash proving DAO payment
    pub creators_tx_hash: String,
    /// Source of BTC revenue (e.g. "2miners-ETC", "moneroocean-XMR")
    pub source: String,
    /// Optional notes
    pub notes: String,
}

/// Cumulative revenue statistics — 100% DAO model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuybackStats {
    /// Total BTC revenue across all events (satoshis)
    pub total_btc_revenue_sats: u64,
    /// Total BTC allocated to burn (always 0 for revenue)
    pub total_btc_burn_sats: u64,
    /// Total BTC allocated to DAO (= total_btc_revenue_sats)
    pub total_btc_creators_sats: u64,
    /// Total ZION burned via buybacks (always 0)
    pub total_zion_burned_atomic: u64,
    /// Total ZION sent to DAO (atomic units)
    pub total_zion_creators_rent_atomic: u64,
    /// Total ZION burned via L1 fee burning (tracked separately)
    pub total_fees_burned_atomic: u64,
    /// Combined burn total (only fees — no BTC revenue burn)
    pub combined_burn_atomic: u64,
    /// Number of revenue events
    pub buyback_count: u64,
    /// Timestamp of last event
    pub last_buyback_timestamp: u64,
    /// Effective circulating supply = TOTAL_SUPPLY - fee_burns
    pub circulating_supply_atomic: u64,
    /// Deflation rate (%) = fee_burns / TOTAL_SUPPLY × 100
    pub deflation_rate_percent: f64,
    /// Revenue split: burn share (%) — always 0
    pub burn_share_percent: u64,
    /// Revenue split: DAO share (%) — always 100
    pub creators_share_percent: u64,
    /// DAO treasury address
    pub creators_address: String,
}

// ---------------------------------------------------------------------------
// Buyback Tracker
// ---------------------------------------------------------------------------

/// Thread-safe revenue ledger.
///
/// Tracks all BTC revenue events and provides cumulative statistics.
/// Persisted to `buyback_ledger.json` for durability across restarts.
#[derive(Clone)]
pub struct BuybackTracker {
    events: Arc<RwLock<Vec<BuybackEvent>>>,
    fees_burned: Arc<RwLock<u64>>,
    ledger_path: String,
}

impl BuybackTracker {
    /// Create a new tracker, loading existing events from disk if available.
    pub fn new(data_dir: &str) -> Self {
        let ledger_path = format!("{}/buyback_ledger.json", data_dir);
        let events = Self::load_from_disk(&ledger_path);
        Self {
            events: Arc::new(RwLock::new(events)),
            fees_burned: Arc::new(RwLock::new(0)),
            ledger_path,
        }
    }

    /// Create an in-memory tracker (for tests).
    pub fn in_memory() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            fees_burned: Arc::new(RwLock::new(0)),
            ledger_path: String::new(),
        }
    }

    /// Record a new revenue event (100% to DAO).
    ///
    /// `creators_tx_hash` (DAO payment TX) is required as proof.
    /// `burn_tx_hash` is ignored (no burn).
    pub fn record_buyback(&self, event: BuybackEvent) -> Result<u64, String> {
        if event.zion_creators_rent_atomic == 0 {
            return Err("DAO amount must be > 0".to_string());
        }
        if event.creators_tx_hash.is_empty() {
            return Err("DAO TX hash is required".to_string());
        }

        let mut events = self.events.write().unwrap();

        // Check for duplicate TX hashes
        if events.iter().any(|e| e.creators_tx_hash == event.creators_tx_hash) {
            return Err(format!(
                "Duplicate DAO TX hash: {}",
                event.creators_tx_hash
            ));
        }

        let id = events.len() as u64 + 1;
        let mut event = event;
        event.id = id;
        // Enforce: no burn in BTC revenue — 100% DAO
        event.btc_burn_sats = 0;
        event.btc_creators_sats = event.btc_amount_sats;
        event.zion_burned_atomic = 0;
        events.push(event);

        // Persist to disk
        drop(events);
        self.save_to_disk();

        Ok(id)
    }

    /// Record fees burned in a block (called by state.process_block).
    /// Note: This is L1 fee burning, NOT BTC revenue burn.
    pub fn add_fees_burned(&self, amount_atomic: u64) {
        let mut fees = self.fees_burned.write().unwrap();
        *fees = fees.saturating_add(amount_atomic);
    }

    /// Get cumulative statistics (100% DAO model).
    pub fn get_stats(&self) -> BuybackStats {
        let events = self.events.read().unwrap();
        let fees_burned = *self.fees_burned.read().unwrap();

        let total_btc_revenue: u64 = events.iter().map(|e| e.btc_amount_sats).sum();
        let total_zion_dao: u64 = events.iter().map(|e| e.zion_creators_rent_atomic).sum();
        // combined_burn = ONLY fee burns (no BTC revenue is burned)
        let combined = fees_burned;

        let total_supply = crate::blockchain::premine::TOTAL_SUPPLY;
        let circulating = total_supply.saturating_sub(combined);
        let deflation = if total_supply > 0 {
            (combined as f64 / total_supply as f64) * 100.0
        } else {
            0.0
        };

        BuybackStats {
            total_btc_revenue_sats: total_btc_revenue,
            total_btc_burn_sats: 0,
            total_btc_creators_sats: total_btc_revenue,
            total_zion_burned_atomic: 0,
            total_zion_creators_rent_atomic: total_zion_dao,
            total_fees_burned_atomic: fees_burned,
            combined_burn_atomic: combined,
            buyback_count: events.len() as u64,
            last_buyback_timestamp: events.last().map(|e| e.timestamp).unwrap_or(0),
            circulating_supply_atomic: circulating,
            deflation_rate_percent: deflation,
            burn_share_percent: BURN_SHARE_PERCENT,
            creators_share_percent: DAO_SHARE_PERCENT,
            creators_address: DAO_ADDRESS.to_string(),
        }
    }

    /// Get all revenue events.
    pub fn get_events(&self) -> Vec<BuybackEvent> {
        self.events.read().unwrap().clone()
    }

    /// Get the last N events.
    pub fn get_recent_events(&self, n: usize) -> Vec<BuybackEvent> {
        let events = self.events.read().unwrap();
        let start = events.len().saturating_sub(n);
        events[start..].to_vec()
    }

    // -- Persistence --

    fn load_from_disk(path: &str) -> Vec<BuybackEvent> {
        if path.is_empty() {
            return Vec::new();
        }
        match std::fs::read_to_string(path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    fn save_to_disk(&self) {
        if self.ledger_path.is_empty() {
            return; // In-memory mode
        }
        let events = self.events.read().unwrap();
        if let Ok(json) = serde_json::to_string_pretty(&*events) {
            let _ = std::fs::write(&self.ledger_path, json);
        }
    }
}

// ---------------------------------------------------------------------------
// Burn Verification (for L1 fee burning only)
// ---------------------------------------------------------------------------

/// Verify that a transaction sends funds to the burn address.
///
/// Returns the total amount burned (sum of outputs to BURN_ADDRESS), or 0
/// if no outputs go to the burn address.
pub fn verify_burn_tx(tx: &crate::tx::Transaction) -> u64 {
    tx.outputs
        .iter()
        .filter(|o| is_burn_address(&o.address))
        .map(|o| o.amount)
        .sum()
}

/// Format ZION amount from atomic units to human-readable string.
pub fn format_zion(atomic: u64) -> String {
    let whole = atomic / 1_000_000;
    let frac = atomic % 1_000_000;
    if frac == 0 {
        format!("{} ZION", whole)
    } else {
        format!("{}.{:06} ZION", whole, frac)
    }
}

/// Format BTC amount from satoshis to human-readable string.
pub fn format_btc(sats: u64) -> String {
    let whole = sats / 100_000_000;
    let frac = sats % 100_000_000;
    format!("{}.{:08} BTC", whole, frac)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tx::{Transaction, TxOutput};

    /// Helper: create a revenue event (100% DAO).
    fn make_event(
        btc_total: u64,
        zion_dao: u64,
        dao_hash: &str,
    ) -> BuybackEvent {
        BuybackEvent {
            id: 0,
            timestamp: 1700000000,
            btc_amount_sats: btc_total,
            btc_burn_sats: 0,
            btc_creators_sats: btc_total,
            zion_burned_atomic: 0,
            zion_creators_rent_atomic: zion_dao,
            price_sats_per_zion: 0.01,
            burn_tx_hash: String::new(),
            creators_tx_hash: dao_hash.to_string(),
            source: "test".to_string(),
            notes: "".to_string(),
        }
    }

    #[test]
    fn test_burn_address_constant() {
        assert_eq!(BURN_ADDRESS, "zion1burn0000000000000000000000000000000dead");
        assert!(BURN_ADDRESS.starts_with("zion1"));
    }

    #[test]
    fn test_dao_address_constant() {
        assert_eq!(DAO_ADDRESS, "zion1dao00000000000000000000000000000treasury");
        assert!(DAO_ADDRESS.starts_with("zion1"));
    }

    #[test]
    fn test_is_burn_address() {
        assert!(is_burn_address(BURN_ADDRESS));
        assert!(!is_burn_address("zion1someotheraddress"));
        assert!(!is_burn_address(""));
        assert!(!is_burn_address("zion1burn"));
    }

    #[test]
    fn test_is_dao_address() {
        assert!(is_dao_address(DAO_ADDRESS));
        assert!(!is_dao_address(BURN_ADDRESS));
        assert!(!is_dao_address("zion1someuser"));
    }

    #[test]
    fn test_revenue_split_100_dao() {
        let (burn, dao) = calculate_revenue_split(1_000_000);
        assert_eq!(burn, 0);
        assert_eq!(dao, 1_000_000);

        let (burn, dao) = calculate_revenue_split(1_000_001);
        assert_eq!(burn, 0);
        assert_eq!(dao, 1_000_001);
    }

    #[test]
    fn test_btc_revenue_split() {
        let (burn_btc, dao_btc) = calculate_btc_revenue_split(10_000_000);
        assert_eq!(burn_btc, 0);
        assert_eq!(dao_btc, 10_000_000);
    }

    #[test]
    fn test_split_constants_sum_100() {
        assert_eq!(BURN_SHARE_PERCENT + DAO_SHARE_PERCENT, 100);
    }

    #[test]
    fn test_split_zero_amount() {
        let (burn, dao) = calculate_revenue_split(0);
        assert_eq!(burn, 0);
        assert_eq!(dao, 0);
    }

    #[test]
    fn test_tracker_record_and_stats() {
        let tracker = BuybackTracker::in_memory();

        let event = make_event(
            10_000_000,
            1_000_000_000_000,
            "dao_abc123",
        );

        let id = tracker.record_buyback(event).unwrap();
        assert_eq!(id, 1);

        let stats = tracker.get_stats();
        assert_eq!(stats.buyback_count, 1);
        assert_eq!(stats.total_btc_revenue_sats, 10_000_000);
        assert_eq!(stats.total_btc_burn_sats, 0);
        assert_eq!(stats.total_btc_creators_sats, 10_000_000);
        assert_eq!(stats.total_zion_burned_atomic, 0);
        assert_eq!(stats.total_zion_creators_rent_atomic, 1_000_000_000_000);
        assert_eq!(stats.burn_share_percent, 0);
        assert_eq!(stats.creators_share_percent, 100);
        assert_eq!(stats.creators_address, DAO_ADDRESS);
    }

    #[test]
    fn test_tracker_duplicate_dao_tx_hash_rejected() {
        let tracker = BuybackTracker::in_memory();

        let event1 = make_event(5_000_000, 250_000_000_000, "same_dao_hash");
        let event2 = make_event(3_000_000, 150_000_000_000, "same_dao_hash");

        assert!(tracker.record_buyback(event1).is_ok());
        let result = tracker.record_buyback(event2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn test_tracker_zero_dao_amount_rejected() {
        let tracker = BuybackTracker::in_memory();
        let event = make_event(1_000_000, 0, "h1");
        assert!(tracker.record_buyback(event).is_err());
    }

    #[test]
    fn test_tracker_empty_tx_hash_rejected() {
        let tracker = BuybackTracker::in_memory();
        let event = make_event(1_000_000, 100_000, "");
        assert!(tracker.record_buyback(event).is_err());
    }

    #[test]
    fn test_tracker_cumulative_stats() {
        let tracker = BuybackTracker::in_memory();

        for i in 1..=5u64 {
            let event = BuybackEvent {
                id: 0,
                timestamp: 1700000000 + i * 1000,
                btc_amount_sats: 1_000_000 * i,
                btc_burn_sats: 0,
                btc_creators_sats: 1_000_000 * i,
                zion_burned_atomic: 0,
                zion_creators_rent_atomic: 100_000_000_000 * i,
                price_sats_per_zion: 0.01,
                burn_tx_hash: String::new(),
                creators_tx_hash: format!("dao_{}", i),
                source: "test".to_string(),
                notes: "".to_string(),
            };
            tracker.record_buyback(event).unwrap();
        }

        let stats = tracker.get_stats();
        assert_eq!(stats.buyback_count, 5);
        assert_eq!(stats.total_btc_revenue_sats, 15_000_000);
        assert_eq!(stats.total_btc_burn_sats, 0);
        assert_eq!(stats.total_btc_creators_sats, 15_000_000);
        assert_eq!(stats.total_zion_burned_atomic, 0);
        assert_eq!(stats.total_zion_creators_rent_atomic, 1_500_000_000_000);
        assert_eq!(stats.last_buyback_timestamp, 1700005000);
    }

    #[test]
    fn test_fees_burned_tracking() {
        let tracker = BuybackTracker::in_memory();

        tracker.add_fees_burned(5_000);
        tracker.add_fees_burned(3_000);

        let stats = tracker.get_stats();
        assert_eq!(stats.total_fees_burned_atomic, 8_000);
        assert_eq!(stats.combined_burn_atomic, 8_000);
    }

    #[test]
    fn test_combined_burn_is_only_fees() {
        let tracker = BuybackTracker::in_memory();

        let event = make_event(10_000_000, 1_000_000_000_000, "dao1");
        tracker.record_buyback(event).unwrap();
        tracker.add_fees_burned(50_000);

        let stats = tracker.get_stats();
        assert_eq!(stats.total_zion_burned_atomic, 0);
        assert_eq!(stats.total_zion_creators_rent_atomic, 1_000_000_000_000);
        assert_eq!(stats.total_fees_burned_atomic, 50_000);
        assert_eq!(stats.combined_burn_atomic, 50_000);
    }

    #[test]
    fn test_circulating_supply() {
        let tracker = BuybackTracker::in_memory();

        let event = make_event(100_000_000, 500_000_000_000_000, "big_dao_event");
        tracker.record_buyback(event).unwrap();
        tracker.add_fees_burned(1_000_000);

        let stats = tracker.get_stats();
        assert_eq!(
            stats.circulating_supply_atomic,
            crate::blockchain::premine::TOTAL_SUPPLY - 1_000_000
        );
    }

    #[test]
    fn test_deflation_rate_from_fees_only() {
        let tracker = BuybackTracker::in_memory();

        let one_percent = crate::blockchain::premine::TOTAL_SUPPLY / 100;
        tracker.add_fees_burned(one_percent);

        let stats = tracker.get_stats();
        assert!(
            (stats.deflation_rate_percent - 1.0).abs() < 0.01,
            "Expected ~1.0%, got {}",
            stats.deflation_rate_percent
        );
    }

    #[test]
    fn test_verify_burn_tx() {
        let tx = Transaction {
            id: "test_burn_tx".to_string(),
            version: 1,
            inputs: vec![],
            outputs: vec![
                TxOutput {
                    amount: 500_000_000,
                    address: "zion1someuser".to_string(),
                },
                TxOutput {
                    amount: 1_000_000_000,
                    address: BURN_ADDRESS.to_string(),
                },
                TxOutput {
                    amount: 200_000_000,
                    address: BURN_ADDRESS.to_string(),
                },
            ],
            fee: 1_000,
            timestamp: 100,
        };

        let burned = verify_burn_tx(&tx);
        assert_eq!(burned, 1_200_000_000);
    }

    #[test]
    fn test_verify_burn_tx_no_burn() {
        let tx = Transaction {
            id: "normal_tx".to_string(),
            version: 1,
            inputs: vec![],
            outputs: vec![
                TxOutput {
                    amount: 1_000_000,
                    address: "zion1user".to_string(),
                },
            ],
            fee: 1_000,
            timestamp: 100,
        };

        assert_eq!(verify_burn_tx(&tx), 0);
    }

    #[test]
    fn test_get_recent_events() {
        let tracker = BuybackTracker::in_memory();

        for i in 1..=10u64 {
            let mut event = make_event(1_000_000, 50_000_000, &format!("dao_{}", i));
            event.timestamp = 1700000000 + i;
            tracker.record_buyback(event).unwrap();
        }

        let recent = tracker.get_recent_events(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].creators_tx_hash, "dao_8");
        assert_eq!(recent[2].creators_tx_hash, "dao_10");
    }

    #[test]
    fn test_format_zion() {
        assert_eq!(format_zion(1_000_000), "1 ZION");
        assert_eq!(format_zion(5_400_067_000), "5400.067000 ZION");
        assert_eq!(format_zion(0), "0 ZION");
        assert_eq!(format_zion(500_000), "0.500000 ZION");
    }

    #[test]
    fn test_format_btc() {
        assert_eq!(format_btc(100_000_000), "1.00000000 BTC");
        assert_eq!(format_btc(10_000_000), "0.10000000 BTC");
        assert_eq!(format_btc(1), "0.00000001 BTC");
    }

    #[test]
    fn test_revenue_split_large_amounts() {
        let total = 1_000_000_000_000_000u64;
        let (burn, dao) = calculate_revenue_split(total);
        assert_eq!(burn, 0);
        assert_eq!(dao, total);
    }

    #[test]
    fn test_stats_include_split_config() {
        let tracker = BuybackTracker::in_memory();
        let stats = tracker.get_stats();

        assert_eq!(stats.burn_share_percent, 0);
        assert_eq!(stats.creators_share_percent, 100);
        assert_eq!(stats.creators_address, DAO_ADDRESS);
        assert_eq!(stats.buyback_count, 0);
        assert_eq!(stats.total_btc_revenue_sats, 0);
    }

    #[test]
    fn test_record_enforces_no_burn() {
        let tracker = BuybackTracker::in_memory();

        let event = BuybackEvent {
            id: 0,
            timestamp: 1700000000,
            btc_amount_sats: 10_000_000,
            btc_burn_sats: 5_000_000,
            btc_creators_sats: 5_000_000,
            zion_burned_atomic: 500_000,
            zion_creators_rent_atomic: 500_000,
            price_sats_per_zion: 0.01,
            burn_tx_hash: "should_be_ignored".to_string(),
            creators_tx_hash: "dao_enforced".to_string(),
            source: "test".to_string(),
            notes: "".to_string(),
        };

        tracker.record_buyback(event).unwrap();
        let events = tracker.get_events();
        assert_eq!(events[0].btc_burn_sats, 0);
        assert_eq!(events[0].btc_creators_sats, 10_000_000);
        assert_eq!(events[0].zion_burned_atomic, 0);
    }
}