# ⚙️ Kapitola 2: Technická Architektura

> *"Kód je poezie. Rust je její nejčistší forma."*

---

## 2.1 Přehled architektury

ZION TerraNova v2.9.5 "Native Awakening" představuje kompletní přepis do **100% nativního Rust stacku**. Tato migrace přináší:

- **50× vyšší propustnost** (1,000 → 50,000 minerů)
- **15× rychlejší validaci** (3-5ms → 0.3ms)
- **50× nižší paměťové nároky** (150KB → 3KB per miner)
- **90% snížení infrastrukturních nákladů**

```
┌─────────────────────────────────────────────────────────────┐
│                    ZION v2.9.5 Stack                        │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐  │
│  │   Miners    │  │   Miners    │  │      Miners         │  │
│  │  (CPU/GPU)  │  │  (CPU/GPU)  │  │     (CPU/GPU)       │  │
│  └──────┬──────┘  └──────┬──────┘  └──────────┬──────────┘  │
│         │                │                     │            │
│         └────────────────┼─────────────────────┘            │
│                          ▼                                  │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              MINING POOL (Rust)                       │  │
│  │  • Stratum v2 Server     • PPLNS Calculator          │  │
│  │  • VarDiff Engine        • Share Validator           │  │
│  │  • Template Manager      • NCL Protocol              │  │
│  │  • Prometheus Metrics    • Payout Scheduler          │  │
│  └───────────────────────────┬───────────────────────────┘  │
│                              │ JSON-RPC                     │
│                              ▼                              │
│  ┌───────────────────────────────────────────────────────┐  │
│  │              BLOCKCHAIN CORE (Rust)                   │  │
│  │  • Block/TX Validation   • UTXO Management           │  │
│  │  • LMDB Storage          • Mempool                   │  │
│  │  • P2P Network           • DAA (Difficulty)          │  │
│  │  • Consensus Engine      • Reorg Handler             │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

---

## 2.2 Blockchain Core

### 2.2.1 Základní parametry

| Parametr | Hodnota |
|----------|---------|
| **Block Time** | 60 sekund |
| **Block Size** | 1 MB (soft limit) |
| **TX per Block** | ~1,000 (target) |
| **Finality** | 6 bloků (~6 minut) |
| **Storage Engine** | LMDB |
| **Kódová základna** | ~6,550 LOC Rust |

### 2.2.2 Konsenzus mechanismus

ZION používá **Proof-of-Work** s vlastním algoritmem **Cosmic Harmony v3**:

```rust
// Simplified PoW validation
fn validate_block_pow(block: &Block) -> Result<bool> {
    let header_hash = block.compute_header_hash();
    let pow_hash = cosmic_harmony_v3(&header_hash, block.nonce);
    
    // Check difficulty target
    pow_hash <= block.difficulty_target
}
```

**Výhody PoW oproti PoS:**
- ✅ Skutečná decentralizace (kdokoli může těžit)
- ✅ Objektivní konsenzus (matematický důkaz)
- ✅ Odolnost proti cenzuře
- ✅ Žádné "rich get richer" efekty

### 2.2.3 UTXO Model

ZION používá **UTXO model** (jako Bitcoin) pro maximální auditovatelnost:

```
Transaction {
    inputs: [
        UTXO { tx_hash: "abc...", index: 0, amount: 100 ZION }
    ],
    outputs: [
        { address: "zion1recipient...", amount: 95 ZION },
        { address: "zion1change...", amount: 4.99 ZION }
    ],
    fee: 0.01 ZION
}
```

**Validace transakcí:**
1. ✅ UTXO existuje a není utraceno
2. ✅ Součet vstupů ≥ součet výstupů + fee
3. ✅ Digitální podpis odpovídá vlastníkovi UTXO
4. ✅ Žádné double-spend v mempoolu

### 2.2.4 Storage (LMDB)

```
LMDB Databases:
├── blocks          # Block headers + bodies
├── block_height    # height → block_hash index
├── tx_index        # tx_hash → block_hash index
├── utxo_set        # Unspent transaction outputs
└── mempool         # Pending transactions
```

**Výkon:**
- Read latency: <0.1ms
- Write throughput: 10,000+ ops/sec
- Crash recovery: ACID compliant

---

## 2.3 Mining Pool

### 2.3.1 Stratum v2 Server

Pool implementuje **Stratum v2** protokol kompatibilní s XMRig a dalšími standardními minery:

```json
// Job notification
{
    "jsonrpc": "2.0",
    "method": "job",
    "params": {
        "job_id": "abc123",
        "blob": "0707e8c4d...",
        "target": "b88d0600",
        "height": 12345,
        "seed_hash": "..."
    }
}

// Share submission
{
    "id": 1,
    "method": "submit",
    "params": {
        "id": "miner1",
        "job_id": "abc123",
        "nonce": "deadbeef",
        "result": "..."
    }
}
```

### 2.3.2 VarDiff (Variable Difficulty)

Pool automaticky upravuje obtížnost pro každého minera:

```rust
struct VarDiffConfig {
    target_time: Duration,     // 30 sekund
    variance_percent: f64,     // 30%
    min_difficulty: u64,       // 1000
    max_difficulty: u64,       // 1_000_000_000
    retarget_time: Duration,   // 120 sekund
}
```

**Algoritmus:**
1. Měří čas mezi shares
2. Pokud příliš rychlé → zvýšit difficulty
3. Pokud příliš pomalé → snížit difficulty
4. Cíl: stabilní ~30s mezi shares

### 2.3.3 PPLNS (Pay Per Last N Shares)

```
Reward Distribution:
┌─────────────────────────────────────────┐
│ Block Found: 50 ZION + 392.857 bonus    │
├─────────────────────────────────────────┤
│ Pool Fee (1%):          4.43 ZION       │
│ Humanitarian Tithe (10%): 44.29 ZION    │
│ Miners (89%):           394.14 ZION     │
└─────────────────────────────────────────┘

PPLNS Window: Last 10,000 shares
Miner A (2,500 shares): 25% → 98.54 ZION
Miner B (5,000 shares): 50% → 197.07 ZION
Miner C (2,500 shares): 25% → 98.54 ZION
```

### 2.3.4 Share Validation

**Kritická bezpečnostní funkce:** Pool VŽDY počítá hash sám:

```rust
fn validate_share(share: &Share, job: &Job) -> ShareResult {
    // 1. Reconstruct block header from job + miner nonce
    let header = reconstruct_header(job, share.nonce);
    
    // 2. Compute hash ourselves (NEVER trust miner's result)
    let computed_hash = cosmic_harmony_v3(&header);
    
    // 3. Verify against share difficulty
    if computed_hash > share_target {
        return ShareResult::Invalid("hash above target");
    }
    
    // 4. Check if block found
    if computed_hash <= network_target {
        return ShareResult::BlockFound(computed_hash);
    }
    
    ShareResult::Valid
}
```

---

## 2.4 Mining Algoritmy

### 2.4.1 Cosmic Harmony v3 (Primární)

**Vlastní ZION algoritmus** optimalizovaný pro CPU mining:

```
Cosmic Harmony v3 Pipeline:
┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│  Blake3     │ → │  Memory     │ → │  Final      │
│  Init       │    │  Hard Mix   │    │  Blake3     │
└─────────────┘    └─────────────┘    └─────────────┘
     64 bytes        4 MB scratchpad      32 bytes hash
```

**Parametry:**
- Memory: 4 MB scratchpad (ASIC-resistant)
- Iterations: 524,288
- Output: 32 bytes

**Výkon:**
| Hardware | Hashrate |
|----------|----------|
| AMD Ryzen 9 5900X (12C) | ~2.5 MH/s |
| Intel i7-12700K (12C) | ~2.0 MH/s |
| Apple M2 Pro (10C) | ~1.8 MH/s |
| Raspberry Pi 4 (4C) | ~50 kH/s |

### 2.4.2 Podporované algoritmy (Multi-chain)

ZION pool podporuje těžbu na více chainech:

| Algoritmus | Coin | Status | Nativní knihovna |
|------------|------|--------|------------------|
| **Cosmic Harmony** | ZION | ✅ E2E | `libcosmic_harmony.so` |
| **RandomX** | XMR | ⚠️ WIP | `librandomx_zion.so` |
| **Yescrypt** | LTC | ⚠️ WIP | `libyescrypt_zion.so` |
| **Ethash** | ETC | ⚠️ WIP | `libethash_zion.so` |
| **Autolykos v2** | ERG | ⚠️ WIP | `libautolykos_zion.so` |
| **KawPow** | RVN | ⚠️ WIP | `libkawpow_zion.so` |
| **kHeavyHash** | KAS | ⚠️ WIP | `libkheavyhash_zion.so` |

---

## 2.5 P2P Network

### 2.5.1 Node Discovery

```rust
// Seed nodes (hardcoded)
const SEED_NODES: &[&str] = &[
    "seed1.zionterranova.com:18444",
    "seed2.zionterranova.com:18444",
    "seed3.zionterranova.com:18444",
];

// Peer discovery flow
1. Connect to seed nodes
2. Exchange peer lists (gossip)
3. Persist known peers to JSON
4. Prefer peers with low failure rate
```

### 2.5.2 Security Hardening

```rust
struct P2PSecurity {
    // Rate limiting
    max_requests_per_minute: 10,
    
    // Connection limits
    max_connections_total: 100,
    max_connections_per_ip: 50,
    
    // Blacklist
    temporary_ban_duration: Duration::hours(1),
    permanent_ban_threshold: 10,  // misbehaviors
    
    // Message validation
    max_message_size: 10 * 1024 * 1024,  // 10 MB
    ban_on_invalid_blocks: 3,
}
```

### 2.5.3 Message Types

| Message | Direction | Purpose |
|---------|-----------|---------|
| `Handshake` | Bidirectional | Version, capabilities |
| `GetTip` | Request | Current chain tip |
| `Tip` | Response | Block hash + height |
| `GetBlocks` | Request | Request blocks by hash |
| `Blocks` | Response | Block data |
| `NewBlock` | Broadcast | Announce new block |
| `NewTx` | Broadcast | Announce new transaction |

---

## 2.6 Universal Miner

### 2.6.1 Architektura

```rust
struct UniversalMiner {
    // Connection
    stratum_client: StratumClient,
    
    // Mining
    cpu_threads: Vec<CpuWorker>,
    gpu_devices: Vec<GpuDevice>,  // WIP
    
    // NCL (optional)
    ncl_client: Option<NclClient>,
    
    // Stats
    hashrate: AtomicU64,
    shares_valid: AtomicU64,
    shares_invalid: AtomicU64,
}
```

### 2.6.2 CPU Mining Loop

```rust
fn mining_loop(job: &Job, start_nonce: u64, thread_id: usize) {
    let mut nonce = start_nonce;
    
    loop {
        // Check for new job
        if job.is_stale() { break; }
        
        // Compute hash
        let hash = cosmic_harmony_v3(&job.blob, nonce);
        
        // Check difficulty
        if hash <= job.target {
            submit_share(job.id, nonce, hash);
        }
        
        nonce += NUM_THREADS;
    }
}
```

### 2.6.3 Statistiky

```
⚡ ZION Universal Miner v2.9.5
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Algorithm:  Cosmic Harmony v3
Pool:       stratum+tcp://pool.zionterranova.com:3333
Wallet:     zion1abc...xyz
Workers:    8 CPU threads
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Hashrate:   | 1,946.98 kH/s |
Shares:     403 valid / 0 invalid
Blocks:     0 found
Uptime:     0:05:23
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

---

## 2.7 Metriky a Monitoring

### 2.7.1 Prometheus Endpoints

```
GET /metrics

# Pool metrics
stratum_active_connections 127
shares_accepted_total 45678
shares_rejected_total 23
vardiff_retargets_total 890
blocks_found_total 12

# Core metrics  
block_template_height 12345
block_template_updates_total 456
rpc_requests_total 78901

# Payout metrics
payouts_queued_total 100
payouts_paid_total 95
payout_pending_atomic 58520526000
```

### 2.7.2 HTTP API

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Node health status |
| `/stats` | GET | Pool statistics |
| `/miners` | GET | Active miners list |
| `/blocks` | GET | Found blocks |
| `/api/v1/ncl/status` | GET | NCL layer status |
| `/api/v1/ncl/leaderboard` | GET | Top NCL workers |

---

## 2.8 Build & Deploy

### 2.8.1 Requirements

```bash
# Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable

# Dependencies (Ubuntu/Debian)
apt install build-essential pkg-config libssl-dev

# Dependencies (macOS)
xcode-select --install
```

### 2.8.2 Build Commands

```bash
# Clone repository
git clone https://github.com/Zion-TerraNova/2.9.5-NativeAwakening.git
cd Zion-2.9/2.9.5

# Build entire workspace
cargo build --release --workspace

# Run tests
cargo test --workspace

# Individual components
cargo build --release -p zion-core
cargo build --release -p zion-pool
cargo build --release -p zion-universal-miner
```

### 2.8.3 Production Deployment

```bash
# Core node
./target/release/zion-core \
    --data-dir /var/lib/zion \
    --rpc-bind 0.0.0.0:8444 \
    --p2p-bind 0.0.0.0:18444

# Mining pool
./target/release/zion-pool \
    --core-rpc http://localhost:8444 \
    --stratum-bind 0.0.0.0:3333 \
    --api-bind 0.0.0.0:8080
```

---

## 2.9 Bezpečnostní model

### 2.9.1 Kryptografické primitivy

| Účel | Algoritmus | Knihovna |
|------|------------|----------|
| Hashing | Blake3 | `blake3` crate |
| Signatures | Ed25519 | `ed25519-dalek` |
| Key derivation | Argon2id | `argon2` |
| Encryption | ChaCha20-Poly1305 | `chacha20poly1305` |
| Random | CSPRNG | `rand` + `getrandom` |

### 2.9.2 Známé limitace

| Limitace | Stav | Plán |
|----------|------|------|
| P2P bez TLS | ⚠️ Plaintext | Mainnet: TLS 1.3 |
| GPU mining | ⚠️ Placeholder | Q2 2026: CUDA/OpenCL |
| External audit | ⚠️ Pending | Q2 2026: Trail of Bits |

---

**Pokračování:** [Kapitola 3 — Consciousness Mining](03_CONSCIOUSNESS_MINING.md)

---

*"Good code is its own best documentation."*  
**— Steve McConnell**
