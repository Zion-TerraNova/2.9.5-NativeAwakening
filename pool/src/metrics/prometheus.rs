use prometheus::{Encoder, IntCounter, IntGauge, IntCounterVec, IntGaugeVec, Opts, TextEncoder};
use prometheus::core::Collector;
use std::sync::OnceLock;

static ACCEPTED: OnceLock<IntCounter> = OnceLock::new();
static REJECTED: OnceLock<IntCounter> = OnceLock::new();
static BLOCKS_FOUND: OnceLock<IntCounter> = OnceLock::new();
static VARDIFF_RETARGETS: OnceLock<IntCounter> = OnceLock::new();
static JOB_BROADCASTS: OnceLock<IntCounter> = OnceLock::new();

static RPC_REQUESTS: OnceLock<IntCounter> = OnceLock::new();
static RPC_ERRORS: OnceLock<IntCounter> = OnceLock::new();
static TEMPLATE_UPDATES: OnceLock<IntCounter> = OnceLock::new();
static TEMPLATE_FETCH_ERRORS: OnceLock<IntCounter> = OnceLock::new();
static BLOCK_SUBMIT_ATTEMPTS: OnceLock<IntCounter> = OnceLock::new();
static BLOCK_SUBMIT_REJECTED: OnceLock<IntCounter> = OnceLock::new();

static REDIS_ERRORS: OnceLock<IntCounter> = OnceLock::new();
static PAYOUTS_QUEUED: OnceLock<IntCounter> = OnceLock::new();
static PAYOUTS_PAID: OnceLock<IntCounter> = OnceLock::new();

static NCL_REGISTERED: OnceLock<IntCounter> = OnceLock::new();
static NCL_TASKS_CREATED: OnceLock<IntCounter> = OnceLock::new();
static NCL_TASKS_SUBMITTED: OnceLock<IntCounter> = OnceLock::new();
static NCL_TASKS_ACCEPTED: OnceLock<IntCounter> = OnceLock::new();
static NCL_TASKS_REJECTED: OnceLock<IntCounter> = OnceLock::new();

static ACTIVE_CONNECTIONS: OnceLock<IntGauge> = OnceLock::new();
static TEMPLATE_HEIGHT: OnceLock<IntGauge> = OnceLock::new();

static REDIS_UP: OnceLock<IntGauge> = OnceLock::new();
static PPLNS_WINDOW_SIZE: OnceLock<IntGauge> = OnceLock::new();

static PAYOUT_PENDING_ATOMIC: OnceLock<IntGauge> = OnceLock::new();
static PAYOUT_QUEUE_LENGTH: OnceLock<IntGauge> = OnceLock::new();

// ── Per-miner labeled metrics ──────────────────────────────────────
static MINER_HASHRATE: OnceLock<IntGaugeVec> = OnceLock::new();
static MINER_SHARES: OnceLock<IntCounterVec> = OnceLock::new();
static MINER_BLOCKS_FOUND: OnceLock<IntCounterVec> = OnceLock::new();
static MINER_PENDING_BALANCE: OnceLock<IntGaugeVec> = OnceLock::new();
static MINER_PAID_TOTAL: OnceLock<IntGaugeVec> = OnceLock::new();
static MINER_CONNECTIONS: OnceLock<IntGaugeVec> = OnceLock::new();

fn accepted() -> &'static IntCounter {
    ACCEPTED.get_or_init(|| IntCounter::new("shares_accepted_total", "Total accepted shares").unwrap())
}

fn rejected() -> &'static IntCounter {
    REJECTED.get_or_init(|| IntCounter::new("shares_rejected_total", "Total rejected shares").unwrap())
}

fn blocks_found() -> &'static IntCounter {
    BLOCKS_FOUND.get_or_init(|| IntCounter::new("blocks_found_total", "Total blocks accepted by core").unwrap())
}

fn vardiff_retargets() -> &'static IntCounter {
    VARDIFF_RETARGETS.get_or_init(|| {
        IntCounter::new(
            "vardiff_retargets_total",
            "Total VarDiff retarget events",
        )
        .unwrap()
    })
}

fn job_broadcasts() -> &'static IntCounter {
    JOB_BROADCASTS.get_or_init(|| {
        IntCounter::new(
            "job_broadcasts_total",
            "Total mining.notify broadcasts sent",
        )
        .unwrap()
    })
}

fn rpc_requests() -> &'static IntCounter {
    RPC_REQUESTS
        .get_or_init(|| IntCounter::new("rpc_requests_total", "Total RPC requests to core").unwrap())
}

fn rpc_errors() -> &'static IntCounter {
    RPC_ERRORS.get_or_init(|| IntCounter::new("rpc_errors_total", "Total RPC errors").unwrap())
}

fn template_updates() -> &'static IntCounter {
    TEMPLATE_UPDATES
        .get_or_init(|| IntCounter::new("block_template_updates_total", "Total template updates").unwrap())
}

fn template_fetch_errors() -> &'static IntCounter {
    TEMPLATE_FETCH_ERRORS.get_or_init(|| {
        IntCounter::new(
            "block_template_fetch_errors_total",
            "Total template fetch errors",
        )
        .unwrap()
    })
}

fn block_submit_attempts() -> &'static IntCounter {
    BLOCK_SUBMIT_ATTEMPTS.get_or_init(|| {
        IntCounter::new(
            "block_submit_attempts_total",
            "Total block submit attempts (block candidates)",
        )
        .unwrap()
    })
}

fn block_submit_rejected() -> &'static IntCounter {
    BLOCK_SUBMIT_REJECTED.get_or_init(|| {
        IntCounter::new(
            "block_submit_rejected_total",
            "Total block submits rejected by core",
        )
        .unwrap()
    })
}

fn active_connections() -> &'static IntGauge {
    ACTIVE_CONNECTIONS.get_or_init(|| IntGauge::new("stratum_active_connections", "Active Stratum connections").unwrap())
}

fn template_height() -> &'static IntGauge {
    TEMPLATE_HEIGHT.get_or_init(|| IntGauge::new("block_template_height", "Current block template height").unwrap())
}

fn redis_up() -> &'static IntGauge {
    REDIS_UP.get_or_init(|| IntGauge::new("redis_up", "Redis reachable (1/0)").unwrap())
}

fn pplns_window_size() -> &'static IntGauge {
    PPLNS_WINDOW_SIZE
        .get_or_init(|| IntGauge::new("pplns_window_size", "Current PPLNS window size").unwrap())
}

fn payout_pending_atomic() -> &'static IntGauge {
    PAYOUT_PENDING_ATOMIC.get_or_init(|| {
        IntGauge::new(
            "payout_pending_atomic",
            "Sum of pending payouts (atomic units), tracked by pool",
        )
        .unwrap()
    })
}

fn payout_queue_length() -> &'static IntGauge {
    PAYOUT_QUEUE_LENGTH.get_or_init(|| {
        IntGauge::new(
            "payout_queue_length",
            "Total payout queue length (items), tracked by pool",
        )
        .unwrap()
    })
}

fn redis_errors() -> &'static IntCounter {
    REDIS_ERRORS
        .get_or_init(|| IntCounter::new("redis_errors_total", "Total Redis operation errors").unwrap())
}

fn payouts_queued() -> &'static IntCounter {
    PAYOUTS_QUEUED.get_or_init(|| IntCounter::new("payouts_queued_total", "Total payouts queued").unwrap())
}

fn payouts_paid() -> &'static IntCounter {
    PAYOUTS_PAID.get_or_init(|| IntCounter::new("payouts_paid_total", "Total payouts marked paid").unwrap())
}

fn ncl_registered() -> &'static IntCounter {
    NCL_REGISTERED.get_or_init(|| {
        IntCounter::new(
            "ncl_registered_total",
            "Total NCL register calls accepted",
        )
        .unwrap()
    })
}

fn ncl_tasks_created() -> &'static IntCounter {
    NCL_TASKS_CREATED.get_or_init(|| {
        IntCounter::new("ncl_tasks_created_total", "Total NCL tasks created").unwrap()
    })
}

fn ncl_tasks_submitted() -> &'static IntCounter {
    NCL_TASKS_SUBMITTED.get_or_init(|| {
        IntCounter::new(
            "ncl_tasks_submitted_total",
            "Total NCL task submissions received",
        )
        .unwrap()
    })
}

fn ncl_tasks_accepted() -> &'static IntCounter {
    NCL_TASKS_ACCEPTED.get_or_init(|| {
        IntCounter::new(
            "ncl_tasks_accepted_total",
            "Total NCL task submissions accepted",
        )
        .unwrap()
    })
}

fn ncl_tasks_rejected() -> &'static IntCounter {
    NCL_TASKS_REJECTED.get_or_init(|| {
        IntCounter::new(
            "ncl_tasks_rejected_total",
            "Total NCL task submissions rejected",
        )
        .unwrap()
    })
}

pub fn inc_accepted() {
    accepted().inc();
}

pub fn inc_rejected() {
    rejected().inc();
}

pub fn inc_blocks_found() {
    blocks_found().inc();
}

pub fn inc_vardiff_retarget() {
    vardiff_retargets().inc();
}

pub fn inc_job_broadcasts() {
    job_broadcasts().inc();
}

pub fn inc_rpc_requests() {
    rpc_requests().inc();
}

pub fn inc_rpc_errors() {
    rpc_errors().inc();
}

pub fn inc_template_updates() {
    template_updates().inc();
}

pub fn inc_template_fetch_errors() {
    template_fetch_errors().inc();
}

pub fn inc_block_submit_attempts() {
    block_submit_attempts().inc();
}

pub fn inc_block_submit_rejected() {
    block_submit_rejected().inc();
}

pub fn inc_connections() {
    active_connections().inc();
}

pub fn dec_connections() {
    active_connections().dec();
}

pub fn set_template_height(height: u64) {
    template_height().set(height as i64);
}

pub fn set_redis_up(up: bool) {
    redis_up().set(if up { 1 } else { 0 });
}

pub fn set_pplns_window_size(size: usize) {
    pplns_window_size().set(size as i64);
}

pub fn inc_redis_errors() {
    redis_errors().inc();
}

pub fn inc_payouts_queued_by(n: u64) {
    payouts_queued().inc_by(n);
}

pub fn inc_payouts_paid() {
    payouts_paid().inc();
}

pub fn inc_ncl_registered() {
    ncl_registered().inc();
}

pub fn inc_ncl_tasks_created() {
    ncl_tasks_created().inc();
}

pub fn inc_ncl_tasks_submitted() {
    ncl_tasks_submitted().inc();
}

pub fn inc_ncl_tasks_accepted() {
    ncl_tasks_accepted().inc();
}

pub fn inc_ncl_tasks_rejected() {
    ncl_tasks_rejected().inc();
}

pub fn add_payout_pending_atomic(amount: u64) {
    payout_pending_atomic().add(amount as i64);
}

pub fn sub_payout_pending_atomic(amount: u64) {
    payout_pending_atomic().sub(amount as i64);
}

pub fn inc_payout_queue_len() {
    payout_queue_length().inc();
}

pub fn dec_payout_queue_len() {
    payout_queue_length().dec();
}

// ── Per-miner labeled metric accessors ──────────────────────────────
fn miner_hashrate() -> &'static IntGaugeVec {
    MINER_HASHRATE.get_or_init(|| {
        IntGaugeVec::new(
            Opts::new("miner_hashrate", "Current miner hashrate (H/s)"),
            &["address"],
        ).unwrap()
    })
}

fn miner_shares() -> &'static IntCounterVec {
    MINER_SHARES.get_or_init(|| {
        IntCounterVec::new(
            Opts::new("miner_shares_total", "Total shares per miner"),
            &["address", "status"],
        ).unwrap()
    })
}

fn miner_blocks_found_vec() -> &'static IntCounterVec {
    MINER_BLOCKS_FOUND.get_or_init(|| {
        IntCounterVec::new(
            Opts::new("miner_blocks_found_total", "Blocks found per miner"),
            &["address"],
        ).unwrap()
    })
}

fn miner_pending_balance() -> &'static IntGaugeVec {
    MINER_PENDING_BALANCE.get_or_init(|| {
        IntGaugeVec::new(
            Opts::new("miner_pending_balance_atomic", "Pending balance per miner (atomic units)"),
            &["address"],
        ).unwrap()
    })
}

fn miner_paid_total() -> &'static IntGaugeVec {
    MINER_PAID_TOTAL.get_or_init(|| {
        IntGaugeVec::new(
            Opts::new("miner_paid_total_atomic", "Total paid per miner (atomic units)"),
            &["address"],
        ).unwrap()
    })
}

fn miner_connections() -> &'static IntGaugeVec {
    MINER_CONNECTIONS.get_or_init(|| {
        IntGaugeVec::new(
            Opts::new("miner_connections_active", "Active connections per miner"),
            &["address"],
        ).unwrap()
    })
}

// ── Per-miner public helpers ────────────────────────────────────────
pub fn set_miner_hashrate(address: &str, hashrate: u64) {
    miner_hashrate().with_label_values(&[address]).set(hashrate as i64);
}

pub fn inc_miner_share(address: &str, valid: bool) {
    let status = if valid { "valid" } else { "invalid" };
    miner_shares().with_label_values(&[address, status]).inc();
}

pub fn inc_miner_blocks(address: &str) {
    miner_blocks_found_vec().with_label_values(&[address]).inc();
}

pub fn set_miner_pending(address: &str, amount: i64) {
    miner_pending_balance().with_label_values(&[address]).set(amount);
}

pub fn set_miner_paid(address: &str, amount: i64) {
    miner_paid_total().with_label_values(&[address]).set(amount);
}

pub fn inc_miner_connections(address: &str) {
    miner_connections().with_label_values(&[address]).inc();
}

pub fn dec_miner_connections(address: &str) {
    miner_connections().with_label_values(&[address]).dec();
}

/// Remove label set for inactive miner (cardinality control)
pub fn remove_miner(address: &str) {
    let _ = miner_hashrate().remove_label_values(&[address]);
    let _ = miner_pending_balance().remove_label_values(&[address]);
    let _ = miner_paid_total().remove_label_values(&[address]);
    let _ = miner_connections().remove_label_values(&[address]);
}

pub fn render() -> String {
    let enc = TextEncoder::new();
    let mut mfs = Vec::new();

    mfs.extend(accepted().collect());
    mfs.extend(rejected().collect());
    mfs.extend(blocks_found().collect());
    mfs.extend(vardiff_retargets().collect());
    mfs.extend(job_broadcasts().collect());
    mfs.extend(rpc_requests().collect());
    mfs.extend(rpc_errors().collect());
    mfs.extend(template_updates().collect());
    mfs.extend(template_fetch_errors().collect());
    mfs.extend(block_submit_attempts().collect());
    mfs.extend(block_submit_rejected().collect());
    mfs.extend(redis_errors().collect());
    mfs.extend(payouts_queued().collect());
    mfs.extend(payouts_paid().collect());
    mfs.extend(active_connections().collect());
    mfs.extend(template_height().collect());
    mfs.extend(redis_up().collect());
    mfs.extend(pplns_window_size().collect());
    mfs.extend(payout_pending_atomic().collect());
    mfs.extend(payout_queue_length().collect());

    mfs.extend(ncl_registered().collect());
    mfs.extend(ncl_tasks_created().collect());
    mfs.extend(ncl_tasks_submitted().collect());
    mfs.extend(ncl_tasks_accepted().collect());
    mfs.extend(ncl_tasks_rejected().collect());

    // Per-miner labeled metrics
    mfs.extend(miner_hashrate().collect());
    mfs.extend(miner_shares().collect());
    mfs.extend(miner_blocks_found_vec().collect());
    mfs.extend(miner_pending_balance().collect());
    mfs.extend(miner_paid_total().collect());
    mfs.extend(miner_connections().collect());

    let mut buf = Vec::new();
    let _ = enc.encode(&mfs, &mut buf);
    String::from_utf8_lossy(&buf).to_string()
}
