pub mod stratum;
pub mod shares;
pub mod metrics;
pub mod config;
pub mod session;
pub mod vardiff;
pub mod jobs;
pub mod pplns;
pub mod payout;
pub mod blockchain;

// CH v3 Revenue Orchestration — L1 Phase 1
pub mod revenue_proxy;
pub mod pool_external_miner;
pub mod profit_switcher;
pub mod buyback;
pub mod stream_scheduler;

// NOTE: The following modules remain post-mainnet:
// - consciousness (XP/levels → moved to pool-level off-chain or OASIS game)
// - ncl (Neural Consciousness Layer → post-mainnet)
// - algorithms (handled by cosmic-harmony crate)
