# Cosmic Harmony v3 - Native Multi-Chain Mining Implementation

**Verze:** 1.0  
**Datum:** 19. ledna 2026  
**Status:** 📋 PLÁN IMPLEMENTACE  
**Autoři:** ZION Core Team

---

## Executive Summary

Tento dokument definuje plán pro **nativní implementaci všech 12 algoritmů** v Cosmic Harmony v3 (CH v3) Multi-Algorithm Engine. Cílem je plně funkční multi-chain mining bez závislosti na externích minerech.

**Klíčové cíle:**
- ✅ Nativní Python/Rust implementace pro všechny podporované algoritmy
- ✅ Skutečné PoW výpočty (ne jen hash forwarding)
- ✅ External Job Receiver pro přijímání práce z cílových poolů
- ✅ Validní share submity s `submit_accepted` na všech podporovaných coinech
- ✅ Dynamické profit-switching mezi algoritmy

---

## 1. Současný Stav (Baseline)

### 1.1 Co máme

| Komponenta | Soubor | Status |
|------------|--------|--------|
| Algorithm Module Library | `src/pool/ch3_pool_controller.py` | ✅ Definice 12 algo |
| Profitability Router | `src/pool/ch3_pool_controller.py` | ✅ CoinGecko API |
| Multi-Chain Submitter | `src/pool/ch3_hash_submitter.py` | ✅ Stratum klient |
| Revenue Settings | `src/pool/ch3_revenue_settings.py` | ✅ 5 streamů |
| Pool Integration | `src/pool/zion_pool_v2_9.py` | ✅ CH3 submitter |
| Config | `config/ch3_mining_config.yaml` | ✅ YAML |

### 1.2 Implementované Hashovací Algoritmy

| Algoritmus | Soubor | Implementace | Validní pro |
|------------|--------|--------------|-------------|
| **Cosmic Harmony** | `zion/mining/cosmic_harmony_wrapper.py` | ✅ Native C++ | ZION |
| **RandomX** | `src/core/algorithms.py` | ⚠️ SHA3 fallback | — (ne XMR) |
| **Yescrypt** | `src/core/algorithms.py` | ✅ Native + fallback | YTN |
| **Autolykos v2** | `src/core/algorithms.py` | ⚠️ Blake2b fallback | — (ne ERG) |
| KawPow | — | ❌ CHYBÍ | — |
| Ethash | — | ❌ CHYBÍ | — |
| KHeavyHash | — | ❌ CHYBÍ | — |
| Blake3 | — | ❌ CHYBÍ | — |
| Equihash | — | ❌ CHYBÍ | — |
| ProgPow | — | ❌ CHYBÍ | — |
| Argon2d | — | ❌ CHYBÍ | — |

### 1.3 Kritické Mezery

```
┌─────────────────────────────────────────────────────────────────────┐
│                    SOUČASNÝ PROBLÉM                                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│   CHv3 Pipeline:                                                     │
│   Input → Keccak → SHA3 → GoldenMatrix → Fusion → ZION ✅           │
│              │        │                                              │
│              ▼        ▼                                              │
│          [ETC Pool] [NXS Pool]                                       │
│              │        │                                              │
│              ▼        ▼                                              │
│          ❌ REJECT  ❌ REJECT                                        │
│          "Stale"   "Invalid"                                         │
│                                                                      │
│   DŮVOD: Posíláme CHv3 intermediate hash, NE validní Ethash/SHA3    │
│          work pro cílový blockchain!                                 │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 2. Cílová Architektura

### 2.1 High-Level Design

```
┌─────────────────────────────────────────────────────────────────────┐
│              CH v3 NATIVE MULTI-CHAIN MINING ENGINE                  │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                    EXTERNAL JOB RECEIVER                       │  │
│  │   Connects to: ETC, ERG, RVN, KAS, ALPH, ZEC, XMR pools       │  │
│  │   Receives: mining.notify jobs for each algorithm              │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │               NATIVE ALGORITHM LIBRARY                         │  │
│  │                                                                │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐             │  │
│  │  │ Ethash  │ │ KawPow  │ │Autolykos│ │KHeavyH  │  GPU        │  │
│  │  │ (ETC)   │ │ (RVN)   │ │  (ERG)  │ │ (KAS)   │  Algos      │  │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘             │  │
│  │                                                                │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐             │  │
│  │  │ Blake3  │ │Equihash │ │ ProgPow │ │ Keccak  │  Mixed      │  │
│  │  │ (ALPH)  │ │ (ZEC)   │ │ (VEIL)  │ │ (native)│             │  │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘             │  │
│  │                                                                │  │
│  │  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐             │  │
│  │  │ RandomX │ │Yescrypt │ │ Argon2d │ │ SHA3    │  CPU        │  │
│  │  │ (XMR)   │ │ (YTN)   │ │ (DYN)   │ │ (native)│  Algos      │  │
│  │  └─────────┘ └─────────┘ └─────────┘ └─────────┘             │  │
│  │                                                                │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                    WORK DISPATCHER                             │  │
│  │   Routes jobs to appropriate hasher based on:                  │  │
│  │   - Available hardware (GPU/CPU)                               │  │
│  │   - Current profitability                                      │  │
│  │   - Pool difficulty requirements                               │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                 MULTI-CHAIN SUBMITTER                          │  │
│  │   Submits VALID shares to each external pool                   │  │
│  │   Logs: ch3_external_pool_submit_accepted coin=XXX             │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                              │                                       │
│                              ▼                                       │
│  ┌───────────────────────────────────────────────────────────────┐  │
│  │                    COSMIC FUSION                               │  │
│  │   Final step: All work contributes to ZION blockchain          │  │
│  │   Output: Valid ZION block + multi-chain revenue               │  │
│  └───────────────────────────────────────────────────────────────┘  │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Data Flow

```
              External Pools                    ZION Pool
              ═════════════                    ══════════
                   │                               │
    ┌──────────────┼──────────────┐               │
    │              │              │               │
    ▼              ▼              ▼               │
┌───────┐    ┌───────┐    ┌───────┐              │
│  ETC  │    │  RVN  │    │  ERG  │              │
│ Pool  │    │ Pool  │    │ Pool  │              │
└───┬───┘    └───┬───┘    └───┬───┘              │
    │            │            │                   │
    │ notify     │ notify     │ notify            │
    ▼            ▼            ▼                   │
┌─────────────────────────────────────────────────┼───┐
│           EXTERNAL JOB RECEIVER                 │   │
│                                                 │   │
│  job_queue = {                                  │   │
│    "ETC": EthashJob(header, seed, target),     │   │
│    "RVN": KawPowJob(header, seed, height),     │   │
│    "ERG": AutolykosJob(msg, pk, target),       │   │
│  }                                              │   │
└───────────────────────────┬─────────────────────┘   │
                            │                         │
                            ▼                         │
┌─────────────────────────────────────────────────────┼───┐
│              NATIVE ALGORITHM WORKERS               │   │
│                                                     │   │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐ │   │
│  │ Ethash      │  │ KawPow      │  │ Autolykos   │ │   │
│  │ Worker      │  │ Worker      │  │ Worker      │ │   │
│  │             │  │             │  │             │ │   │
│  │ hash(job)   │  │ hash(job)   │  │ hash(job)   │ │   │
│  │    ↓        │  │    ↓        │  │    ↓        │ │   │
│  │ if valid:   │  │ if valid:   │  │ if valid:   │ │   │
│  │  submit()   │  │  submit()   │  │  submit()   │ │   │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘ │   │
│         │                │                │        │   │
└─────────┼────────────────┼────────────────┼────────┘   │
          │                │                │            │
          ▼                ▼                ▼            │
    ┌───────────────────────────────────────────────────┐│
    │            MULTI-CHAIN SUBMITTER                  ││
    │                                                   ││
    │  submit_share(coin="ETC", nonce=X, hash=Y)       ││
    │  submit_share(coin="RVN", nonce=X, mixhash=Y)    ││
    │  submit_share(coin="ERG", nonce=X, d=Y)          ││
    │                                                   ││
    │  → ch3_external_pool_submit_accepted coin=ETC    ││
    │  → ch3_external_pool_submit_accepted coin=RVN    ││
    │  → ch3_external_pool_submit_accepted coin=ERG    ││
    └───────────────────────────────────────────────────┘│
                                                         │
                            ┌────────────────────────────┘
                            │
                            ▼
              ┌─────────────────────────┐
              │     COSMIC FUSION       │
              │                         │
              │  Combines all work →    │
              │  → ZION Block Reward    │
              │  → Multi-chain Revenue  │
              └─────────────────────────┘
```

---

## 3. Implementační Plán

### 3.1 Fáze 1: Core Algorithm Library (2-3 týdny)

**Cíl:** Implementovat nativní hashery pro všechny algoritmy.

#### 3.1.1 GPU Algoritmy

| Algoritmus | Target | Implementace | Závislosti | Priorita |
|------------|--------|--------------|------------|----------|
| **Ethash** | ETC | Python + C ext | `pyethash` nebo vlastní | 🔴 P1 |
| **KawPow** | RVN | Python + OpenCL | `pyopencl`, DAG | 🔴 P1 |
| **Autolykos v2** | ERG | Python + NumPy | Blake2b, table gen | 🔴 P1 |
| **KHeavyHash** | KAS | Python + C ext | SHA3, matrix mult | 🟡 P2 |
| **Blake3** | ALPH | Python | `blake3` lib | 🟡 P2 |
| **Equihash** | ZEC | Python + C | Equihash lib | 🟢 P3 |
| **ProgPow** | VEIL | Python + OpenCL | ProgPow impl | 🟢 P3 |

#### 3.1.2 CPU Algoritmy

| Algoritmus | Target | Implementace | Závislosti | Priorita |
|------------|--------|--------------|------------|----------|
| **RandomX** | XMR | C wrapper | `librandomx` | 🟡 P2 |
| **Yescrypt** | YTN | ✅ Existuje | — | ✅ Done |
| **Argon2d** | DYN | Python | `argon2-cffi` | 🟢 P3 |

#### 3.1.3 Native Algoritmy (v CH v3)

| Algoritmus | Status | Poznámka |
|------------|--------|----------|
| **Keccak-256** | ✅ Built-in | `hashlib.sha3_256` (keccak variant) |
| **SHA3-512** | ✅ Built-in | `hashlib.sha3_512` |
| **Golden Matrix** | ✅ Existuje | ZION-specific transform |
| **Cosmic Fusion** | ✅ Existuje | Native C++ wrapper |

#### 3.1.4 Struktura Souborů

```
src/core/algorithms/
├── __init__.py              # Registry + lazy loading
├── base.py                  # Abstract base class
├── cosmic_harmony.py        # ✅ Existuje
├── ethash.py                # 🆕 NEW
├── kawpow.py                # 🆕 NEW
├── autolykos_v2.py          # 🆕 NEW (upgrade)
├── kheavyhash.py            # 🆕 NEW
├── blake3_algo.py           # 🆕 NEW
├── equihash.py              # 🆕 NEW
├── progpow.py               # 🆕 NEW
├── randomx.py               # 🆕 NEW (upgrade)
├── yescrypt.py              # ✅ Existuje
├── argon2d.py               # 🆕 NEW
└── native/                  # C/C++ extensions
    ├── ethash_core.c
    ├── kawpow_kernel.cl
    ├── autolykos_table.c
    └── kheavy_matrix.c
```

---

### 3.2 Fáze 2: External Job Receiver (1-2 týdny)

**Cíl:** Přijímat `mining.notify` joby z externích poolů.

#### 3.2.1 Architektura

```python
# src/pool/ch3_job_receiver.py

class ExternalJobReceiver:
    """Receives and manages jobs from external mining pools."""
    
    def __init__(self):
        self.connections: Dict[str, PoolConnection] = {}
        self.job_queues: Dict[str, asyncio.Queue] = {}
        self.current_jobs: Dict[str, MiningJob] = {}
    
    async def connect_pool(self, coin: str, host: str, port: int, wallet: str):
        """Connect to external pool and start receiving jobs."""
        conn = await self._create_connection(coin, host, port, wallet)
        self.connections[coin] = conn
        asyncio.create_task(self._job_listener(coin, conn))
    
    async def _job_listener(self, coin: str, conn: PoolConnection):
        """Listen for mining.notify messages."""
        while conn.connected:
            msg = await conn.read_message()
            if msg.get("method") == "mining.notify":
                job = self._parse_job(coin, msg["params"])
                self.current_jobs[coin] = job
                await self.job_queues[coin].put(job)
                logger.info("ch3_external_job_received", coin=coin, job_id=job.job_id)
    
    async def get_job(self, coin: str) -> Optional[MiningJob]:
        """Get current job for coin."""
        return self.current_jobs.get(coin)
```

#### 3.2.2 Job Formáty

| Coin | Protocol | Job Fields |
|------|----------|------------|
| ETC | Stratum (ETH) | `job_id, seed_hash, header_hash, clean` |
| RVN | Stratum (KawPow) | `job_id, header_hash, seed_hash, target, height, clean` |
| ERG | Stratum (Autolykos) | `job_id, msg, b, pk, target, height` |
| KAS | Stratum | `job_id, header, timestamp, target` |
| XMR | Stratum (Monero) | `job_id, blob, target, height, seed_hash` |

---

### 3.3 Fáze 3: Work Dispatcher (1 týden)

**Cíl:** Inteligentně rozdělovat práci mezi algoritmy.

```python
# src/pool/ch3_work_dispatcher.py

class WorkDispatcher:
    """Dispatches work to algorithm workers based on profitability."""
    
    def __init__(self, job_receiver: ExternalJobReceiver, 
                 profitability_router: ProfitabilityRouter):
        self.job_receiver = job_receiver
        self.profitability = profitability_router
        self.workers: Dict[str, AlgorithmWorker] = {}
        self.allocation: Dict[str, float] = {}  # coin -> % of hashpower
    
    async def update_allocation(self):
        """Update hashpower allocation based on profitability."""
        profits = await self.profitability.get_all_profits()
        
        # Sort by profit, allocate proportionally
        total_profit = sum(p for p in profits.values() if p > 0)
        if total_profit == 0:
            return
        
        for coin, profit in profits.items():
            if profit > 0:
                self.allocation[coin] = profit / total_profit
            else:
                self.allocation[coin] = 0
        
        logger.info("ch3_allocation_updated", allocation=self.allocation)
    
    async def dispatch_work(self, hardware: str = "gpu"):
        """Dispatch work to appropriate workers."""
        for coin, pct in self.allocation.items():
            if pct > 0:
                job = await self.job_receiver.get_job(coin)
                if job:
                    worker = self.workers.get(coin)
                    if worker:
                        asyncio.create_task(worker.mine(job, allocation=pct))
```

---

### 3.4 Fáze 4: Integration & Testing (1-2 týdny)

**Cíl:** Integrace do ZION Pool v2.9 a end-to-end testy.

#### 3.4.1 Pool Integration

```python
# src/pool/zion_pool_v2_9.py (update)

async def _start_ch3_multichain(self):
    """Start CH v3 multi-chain mining system."""
    
    # 1. Initialize job receiver
    self.job_receiver = ExternalJobReceiver()
    
    # 2. Connect to all configured external pools
    for coin, config in self.ch3_config["coins"].items():
        if config.get("enabled"):
            await self.job_receiver.connect_pool(
                coin=coin,
                host=config["pool_host"],
                port=config["pool_port"],
                wallet=config["wallet"]
            )
    
    # 3. Initialize algorithm workers
    self.algo_workers = {
        "ETC": EthashWorker(),
        "RVN": KawPowWorker(),
        "ERG": AutolykosWorker(),
        # ...
    }
    
    # 4. Start work dispatcher
    self.dispatcher = WorkDispatcher(self.job_receiver, self.profitability)
    asyncio.create_task(self.dispatcher.run())
    
    logger.info("ch3_multichain_started", coins=list(self.algo_workers.keys()))
```

#### 3.4.2 Test Matrix

| Test | Popis | Validace |
|------|-------|----------|
| `test_ethash_valid_share` | Ethash hasher produkuje validní share | ETC pool accepts |
| `test_kawpow_valid_share` | KawPow hasher produkuje validní share | RVN pool accepts |
| `test_autolykos_valid_share` | Autolykos hasher produkuje validní share | ERG pool accepts |
| `test_job_receiver_etc` | Job receiver parsuje ETC notify | Job fields correct |
| `test_job_receiver_rvn` | Job receiver parsuje RVN notify | Job fields correct |
| `test_profit_switching` | Dispatcher přepíná podle profitu | Allocation changes |
| `test_e2e_multichain` | Full pipeline: receive → hash → submit | All coins accepted |

---

## 4. Detailní Algoritmus Specifikace

### 4.1 Ethash (ETC)

```python
# src/core/algorithms/ethash.py

class EthashAlgorithm(BaseAlgorithm):
    """Native Ethash implementation for Ethereum Classic."""
    
    NAME = "ethash"
    TARGET_COINS = ["ETC"]
    HARDWARE = "GPU"
    
    def __init__(self):
        self.cache_size = 0
        self.dataset_size = 0
        self.cache = None
        self.dataset = None
    
    def generate_cache(self, epoch: int) -> bytes:
        """Generate Ethash cache for epoch."""
        seed = self._get_seed_hash(epoch)
        cache_size = self._get_cache_size(epoch)
        
        # Sequentially produce the initial dataset
        cache = [hashlib.sha3_512(seed).digest()]
        for i in range(1, cache_size // 64):
            cache.append(hashlib.sha3_512(cache[-1]).digest())
        
        # Use RandMemoHash to improve cache
        for _ in range(3):  # CACHE_ROUNDS
            for i in range(len(cache)):
                v = int.from_bytes(cache[i][:4], 'little') % len(cache)
                cache[i] = hashlib.sha3_512(
                    bytes(a ^ b for a, b in zip(cache[(i-1) % len(cache)], cache[v]))
                ).digest()
        
        return b''.join(cache)
    
    def hash(self, header_hash: bytes, nonce: int, cache: bytes) -> Tuple[bytes, bytes]:
        """Compute Ethash hash (light evaluation)."""
        # ... full Ethash implementation
        pass
    
    def verify(self, header_hash: bytes, nonce: int, mix_hash: bytes, 
               target: int, cache: bytes) -> bool:
        """Verify Ethash solution."""
        computed_mix, computed_hash = self.hash(header_hash, nonce, cache)
        if computed_mix != mix_hash:
            return False
        return int.from_bytes(computed_hash, 'big') < target
```

### 4.2 KawPow (RVN)

```python
# src/core/algorithms/kawpow.py

class KawPowAlgorithm(BaseAlgorithm):
    """Native KawPow implementation for Ravencoin."""
    
    NAME = "kawpow"
    TARGET_COINS = ["RVN", "CLORE"]
    HARDWARE = "GPU"
    
    PROGPOW_PERIOD = 3  # blocks
    PROGPOW_LANES = 16
    PROGPOW_REGS = 32
    PROGPOW_DAG_LOADS = 4
    PROGPOW_CACHE_BYTES = 16 * 1024
    PROGPOW_CNT_DAG = 64
    PROGPOW_CNT_CACHE = 12
    PROGPOW_CNT_MATH = 20
    
    def __init__(self):
        self.dag = None
        self.dag_epoch = -1
    
    def keccak_f800(self, state: List[int]) -> List[int]:
        """Keccak-f[800] permutation."""
        # 22 rounds of Keccak permutation on 25 32-bit words
        pass
    
    def progpow_init(self, block_number: int) -> Tuple[List, List]:
        """Initialize ProgPoW mix and sequence."""
        period = block_number // self.PROGPOW_PERIOD
        # ... initialization logic
        pass
    
    def progpow_loop(self, seed: int, mix: List[List[int]], 
                     dag: bytes, dag_words: int) -> List[List[int]]:
        """Main ProgPoW loop."""
        # ... loop implementation with random math sequences
        pass
    
    def hash(self, header_hash: bytes, nonce: int, 
             block_number: int, dag: bytes) -> Tuple[bytes, bytes]:
        """Compute KawPow hash."""
        # Initialize
        seed = self.keccak_f800([
            int.from_bytes(header_hash[i:i+4], 'little') 
            for i in range(0, 32, 4)
        ] + [nonce & 0xFFFFFFFF, nonce >> 32] + [0] * 17)
        
        # ProgPoW mix
        mix = self.progpow_init(block_number)
        mix = self.progpow_loop(seed, mix, dag, len(dag) // 4)
        
        # Final hash
        mix_hash = self._compress_mix(mix)
        final_hash = self.keccak_f800(seed[:8] + mix_hash)
        
        return bytes(mix_hash), bytes(final_hash)
```

### 4.3 Autolykos v2 (ERG)

```python
# src/core/algorithms/autolykos_v2.py

class AutolykosV2Algorithm(BaseAlgorithm):
    """Native Autolykos v2 implementation for Ergo."""
    
    NAME = "autolykos_v2"
    TARGET_COINS = ["ERG"]
    HARDWARE = "GPU"
    
    N = 2**26  # Table size
    K = 32     # Number of elements to sum
    
    def __init__(self):
        self.table = None
        self.table_height = -1
    
    def generate_table(self, height: int) -> np.ndarray:
        """Generate Autolykos lookup table."""
        # Seed from height
        seed = self._height_to_seed(height)
        
        # Generate N elements using Blake2b
        table = np.zeros(self.N, dtype=np.uint64)
        for i in range(self.N):
            h = hashlib.blake2b(seed + i.to_bytes(4, 'little'), digest_size=8)
            table[i] = int.from_bytes(h.digest(), 'little')
        
        return table
    
    def gen_indexes(self, msg: bytes, nonce: int, height: int) -> List[int]:
        """Generate K indexes from message and nonce."""
        # Blake2b256(msg || nonce || height)
        h = hashlib.blake2b(msg + nonce.to_bytes(8, 'little') + 
                           height.to_bytes(4, 'little'), digest_size=32)
        seed = h.digest()
        
        indexes = []
        for i in range(self.K):
            idx_hash = hashlib.blake2b(seed + i.to_bytes(1, 'little'), digest_size=4)
            indexes.append(int.from_bytes(idx_hash.digest(), 'little') % self.N)
        
        return indexes
    
    def hash(self, msg: bytes, nonce: int, height: int, 
             pk: bytes, table: np.ndarray) -> bytes:
        """Compute Autolykos v2 hash."""
        # Generate indexes
        indexes = self.gen_indexes(msg, nonce, height)
        
        # Sum table elements at indexes
        total = sum(table[idx] for idx in indexes)
        
        # Final hash: Blake2b256(pk || msg || nonce || sum)
        final = hashlib.blake2b(
            pk + msg + nonce.to_bytes(8, 'little') + total.to_bytes(32, 'little'),
            digest_size=32
        )
        
        return final.digest()
    
    def verify(self, msg: bytes, nonce: int, height: int,
               pk: bytes, d: bytes, target: int) -> bool:
        """Verify Autolykos v2 solution."""
        # Regenerate table if needed
        if self.table_height != height // 1024:
            self.table = self.generate_table(height)
            self.table_height = height // 1024
        
        computed = self.hash(msg, nonce, height, pk, self.table)
        return int.from_bytes(computed, 'big') < target
```

### 4.4 KHeavyHash (KAS)

```python
# src/core/algorithms/kheavyhash.py

class KHeavyHashAlgorithm(BaseAlgorithm):
    """Native kHeavyHash implementation for Kaspa."""
    
    NAME = "kheavyhash"
    TARGET_COINS = ["KAS"]
    HARDWARE = "GPU"
    
    MATRIX_SIZE = 64
    
    def __init__(self):
        self.matrix = self._generate_matrix()
    
    def _generate_matrix(self) -> np.ndarray:
        """Generate the 64x64 matrix for kHeavyHash."""
        # Deterministic matrix generation
        matrix = np.zeros((self.MATRIX_SIZE, self.MATRIX_SIZE), dtype=np.uint64)
        seed = hashlib.sha3_256(b"KHeavyHash").digest()
        
        for i in range(self.MATRIX_SIZE):
            for j in range(self.MATRIX_SIZE):
                h = hashlib.sha3_256(seed + bytes([i, j]))
                matrix[i, j] = int.from_bytes(h.digest()[:8], 'little')
        
        return matrix
    
    def heavy_hash(self, data: bytes) -> bytes:
        """Compute kHeavyHash."""
        # SHA3-256 pre-hash
        pre_hash = hashlib.sha3_256(data).digest()
        
        # Convert to 64-element vector
        vec = np.frombuffer(pre_hash + pre_hash, dtype=np.uint64)[:self.MATRIX_SIZE]
        
        # Matrix multiplication
        result = np.dot(self.matrix.astype(np.uint64), vec.astype(np.uint64))
        result = result % (2**64)
        
        # SHA3-256 post-hash
        return hashlib.sha3_256(result.tobytes()).digest()
    
    def hash(self, header: bytes, nonce: int) -> bytes:
        """Compute kHeavyHash for mining."""
        data = header + nonce.to_bytes(8, 'little')
        return self.heavy_hash(data)
```

---

## 5. Revenue Projekce (Post-Implementation)

### 5.1 S Nativní Implementací

| Stream | Coin | Daily Revenue (1000 miners) | Status |
|--------|------|----------------------------|--------|
| ZION | ZION | Base rewards | ✅ Active |
| Ethash | ETC | ~$500 | 🆕 NEW |
| KawPow | RVN | ~$800 | 🆕 NEW |
| Autolykos | ERG | ~$1,200 | 🆕 NEW |
| KHeavyHash | KAS | ~$2,000 | 🆕 NEW |
| Blake3 | ALPH | ~$1,000 | 🆕 NEW |
| RandomX | XMR | ~$300 | 🆕 NEW |
| **TOTAL EXTRA** | | **~$5,800/day** | |
| **Monthly** | | **~$174,000** | |

### 5.2 Profit Switching Scénář

```
┌─────────────────────────────────────────────────────────────────────┐
│                    DYNAMIC PROFIT ALLOCATION                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Čas 00:00 - Profitability check:                                   │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │ KAS: $2.50/day   ERG: $1.20/day   RVN: $0.80/day   ETC: $0.50  │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                         │                                            │
│                         ▼                                            │
│  Allocation: KAS 50% | ERG 24% | RVN 16% | ETC 10%                  │
│                                                                      │
│  ═══════════════════════════════════════════════════════════════    │
│                                                                      │
│  Čas 06:00 - Price spike na RVN:                                    │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │ KAS: $2.50/day   ERG: $1.20/day   RVN: $3.00/day   ETC: $0.50  │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                         │                                            │
│                         ▼                                            │
│  Allocation: RVN 42% | KAS 35% | ERG 17% | ETC 6%                   │
│                                                                      │
│  📊 Automatic rebalancing maximizes revenue 24/7                    │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 6. Časová Osa

```
┌─────────────────────────────────────────────────────────────────────┐
│                      IMPLEMENTATION TIMELINE                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  FÁZE 1: Core Algorithms (20. ledna - 10. února 2026)               │
│  ═══════════════════════════════════════════════════                │
│  Week 1: Ethash + KawPow base implementation                        │
│  Week 2: Autolykos v2 + KHeavyHash                                  │
│  Week 3: Blake3 + RandomX native wrapper                            │
│                                                                      │
│  FÁZE 2: Job Receiver (10. - 24. února 2026)                        │
│  ═══════════════════════════════════════════════════                │
│  Week 4: External pool connections + job parsing                    │
│  Week 5: Multi-coin job queue management                            │
│                                                                      │
│  FÁZE 3: Work Dispatcher (24. února - 3. března 2026)               │
│  ═══════════════════════════════════════════════════                │
│  Week 6: Dispatcher + profitability integration                     │
│                                                                      │
│  FÁZE 4: Integration & Testing (3. - 17. března 2026)               │
│  ═══════════════════════════════════════════════════                │
│  Week 7: Pool integration + unit tests                              │
│  Week 8: E2E tests + testnet deployment                             │
│                                                                      │
│  🎯 TARGET: Multi-chain mining LIVE by 17. března 2026              │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

---

## 7. Rizika a Mitigace

| Riziko | Dopad | Pravděpodobnost | Mitigace |
|--------|-------|-----------------|----------|
| GPU memory nedostatek pro DAG | ETC/RVN nefunkční | Střední | Detekce + fallback na CPU algo |
| External pool protocol změny | Submity selhávají | Nízká | Verzování + monitoring |
| Profit API nedostupnost | Špatná alokace | Střední | Cache + fallback fixní split |
| Hash rate příliš nízký | Nevyplatí se | Střední | Optimalizace + GPU kernely |
| Pool ban za invalid shares | Ztráta revenue | Vysoká během dev | Testnet + postupné rollout |

---

## 8. Metriky Úspěchu

| Metrika | Target | Měření |
|---------|--------|--------|
| **Valid share rate** | >95% | `accepted / (accepted + rejected)` |
| **Multi-chain revenue** | >$100K/měsíc | Suma payoutů z externích poolů |
| **Algorithm coverage** | 100% (12/12) | Počet funkčních algo |
| **Uptime** | >99.5% | Monitoring |
| **Profit optimization** | >90% optimal | Backtest vs. optimal allocation |

---

## 9. Závěr

Nativní implementace CH v3 Multi-Algorithm Engine je klíčová pro realizaci vize ZION jako "univerální mining platformy". Po dokončení všech 4 fází bude ZION Pool schopen:

1. ✅ Přijímat práci z 10+ externích blockchainů
2. ✅ Počítat validní PoW hashe pro každý algoritmus nativně
3. ✅ Submitovat accepted shares a generovat reálný revenue
4. ✅ Dynamicky přepínat mezi algoritmy podle profitability
5. ✅ Vše kombinovat do Cosmic Fusion pro ZION blockchain

**Odhadovaný extra revenue: ~$174,000/měsíc při 1000 minerech.**

---

**Dokument vytvořen:** 19. ledna 2026  
**Další aktualizace:** Po dokončení Fáze 1  
**Kontakt:** ZION Core Team

---

*"Cosmic Harmony v3 - Where every hash contributes to multiple blockchains."* 🌟
