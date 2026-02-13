# 🧠 Kapitola 8: NCL — Neural Compute Layer

> *"Mining isn't just about hashes. It's about intelligence."*

---

## 8.1 Co je NCL?

**NCL (Neural Compute Layer)** je protokol pro distribuované AI výpočty v ZION síti. Minéři mohou kromě těžby bloků provádět AI inference úlohy a získávat za ně dodatečné odměny.

```
NCL Concept:
┌─────────────────────────────────────────────────────────────┐
│                                                             │
│  Tradiční mining:                                           │
│  └── 100% hashování → Block rewards                        │
│                                                             │
│  ZION NCL Mining:                                           │
│  └── 70% hashování → Block rewards                         │
│  └── 30% AI inference → NCL rewards                        │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Proč NCL?

1. **Využití idle resources:** GPU/NPU nejsou využity 100% při mining
2. **AI democratizace:** Decentralizovaná AI inference
3. **Dodatečný příjem:** Minéři vydělávají víc
4. **Praktická utility:** ZION není jen měna, je to compute network

---

## 8.2 Architektura

### Komponenty

```
NCL Architecture:
═══════════════════════════════════════════════════════════════

┌─────────────────────────────────────────────────────────────┐
│                      NCL Protocol v1.0                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐     │
│  │   Task      │───▶│   Pool      │───▶│   Miner     │     │
│  │  Submitter  │    │  (Broker)   │    │  (Worker)   │     │
│  └─────────────┘    └─────────────┘    └─────────────┘     │
│         │                  │                   │            │
│         │                  ▼                   ▼            │
│         │           ┌─────────────┐    ┌─────────────┐     │
│         │           │ Verification│    │    NPU      │     │
│         └──────────▶│   Layer    │◀───│   Runtime   │     │
│                     └─────────────┘    └─────────────┘     │
│                                                             │
└─────────────────────────────────────────────────────────────┘

Components:
├── Task Submitter: Klient, který potřebuje AI výpočet
├── Pool (Broker): Distribuuje tasky, verifikuje výsledky
├── Miner (Worker): Provádí AI inference
├── Verification: Kontroluje správnost výsledků
└── NPU Runtime: CoreML, TensorRT, ONNX, OpenVINO
```

### Protokol verze

```rust
// Z ncl.rs
pub const NCL_PROTOCOL_VERSION: &str = "1.0";
pub const NCL_RATE_LIMIT_PER_MINUTE: u32 = 60;
pub const NCL_RATE_LIMIT_WINDOW_MS: u64 = 60_000;
```

---

## 8.3 Task Types

### Podporované úlohy

```rust
// Z ncl.rs
pub enum NclTaskType {
    HashChainingV1,       // Blake3 hash chaining (deterministický)
    Embedding,            // Text embeddings
    LlmInference,         // LLM inference
    ImageClassification,  // Klasifikace obrázků
}
```

### Rozšířené typy (ncl_integration.rs)

```rust
pub enum AITaskType {
    Embeddings,           // 0.001 ZION base
    LlmInference,         // 0.01 ZION base
    ImageClassification,  // 0.002 ZION base
    ImageGeneration,      // 0.02 ZION base
    SpeechToText,         // 0.005 ZION base
    CodeAnalysis,         // 0.003 ZION base
    ModelTraining,        // 0.1 ZION base
}
```

### Base Rewards

| Task Type | Base Reward | Popis |
|-----------|-------------|-------|
| Embeddings | 0.001 ZION | Text → vector embedding |
| LLM Inference | 0.01 ZION | Chat completion |
| Image Classification | 0.002 ZION | Rozpoznání objektů |
| Image Generation | 0.02 ZION | Stable Diffusion atd. |
| Speech to Text | 0.005 ZION | Whisper transkripce |
| Code Analysis | 0.003 ZION | Analýza kódu |
| Model Training | 0.1 ZION | Fine-tuning |

---

## 8.4 NPU Runtime Detection

### Automatická detekce

```rust
// Z ncl_integration.rs
pub enum NPURuntime {
    CoreML,     // Apple Silicon (M1/M2/M3)
    TensorRT,   // NVIDIA GPU
    OpenVINO,   // Intel CPU/GPU
    ONNX,       // Generic fallback
}

impl NPURuntime {
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        {
            #[cfg(target_arch = "aarch64")]
            return NPURuntime::CoreML;  // Apple Silicon
        }
        
        // Check for NVIDIA GPU...
        // Fallback to ONNX
        NPURuntime::ONNX
    }
}
```

### Podporované platformy

```
NPU Support Matrix:
═══════════════════════════════════════════════════════════════

Platform          │ Runtime    │ Performance │ Status
──────────────────┼────────────┼─────────────┼────────────
Apple M1/M2/M3    │ CoreML     │ ★★★★★      │ ✅ Native
NVIDIA RTX        │ TensorRT   │ ★★★★★      │ ✅ Native
Intel Arc/CPU     │ OpenVINO   │ ★★★★☆      │ ✅ Supported
AMD ROCm          │ ONNX       │ ★★★☆☆      │ ⚠️ Limited
Generic CPU       │ ONNX       │ ★★☆☆☆      │ ✅ Fallback

═══════════════════════════════════════════════════════════════
```

---

## 8.5 Time Scheduling

### Mining vs NCL alokace

```rust
// Z ncl_integration.rs
pub struct NCLScheduler {
    mining_allocation: f64,  // Default: 0.70 (70% mining)
    min_mining: f64,         // 0.50 (minimum 50%)
    max_mining: f64,         // 0.90 (maximum 90%)
}
```

### Dynamická alokace

```
Time Allocation (default 70/30):
═══════════════════════════════════════════════════════════════

  Mining (70%)                 NCL (30%)
  ██████████████████████████   ██████████
  
  │ Hash computation │         │ AI inference │
  │ Block solving    │         │ Embeddings   │
  │ Share submission │         │ LLM tasks    │
  
  Priority: Mining > NCL
  (NCL se pozastaví, pokud je potřeba více hashrate)

═══════════════════════════════════════════════════════════════
```

### Příklad použití

```rust
let scheduler = NCLScheduler::new(0.70);  // 70% mining

// Check if we should do NPU work
if scheduler.should_do_npu_work() {
    // Process AI task
    let result = process_ncl_task(&task);
    scheduler.record_npu_time(execution_time);
} else {
    // Continue mining
    mine_next_nonce();
    scheduler.record_mining_time(execution_time);
}
```

---

## 8.6 Reward Calculation

### Consciousness Multiplier

NCL rewards jsou násobeny **consciousness level**:

```rust
pub enum ConsciousnessLevel {
    Physical,   // 1.0x
    Emotional,  // 1.05x
    Mental,     // 1.1x
    Spiritual,  // 1.25x
    Cosmic,     // 1.5x
    OnTheStar,  // 2.0x
}
```

### Výpočet odměny

```rust
pub fn calculate_reward(
    task_type: AITaskType,
    consciousness: ConsciousnessLevel,
    execution_time_ms: u64,
    success: bool,
) -> f64 {
    let base_reward = task_type.base_reward();
    
    // Apply consciousness multiplier
    let mut reward = base_reward * consciousness.multiplier();
    
    // Failure penalty
    if !success {
        reward *= 0.1;  // Only 10% for failed tasks
    }
    
    // Efficiency bonus (up to 20% extra)
    let efficiency = calculate_efficiency();
    reward *= 1.0 + efficiency * 0.2;
    
    reward
}
```

### Příklad kalkulace

```
NCL Reward Example:
═══════════════════════════════════════════════════════════════

Task: LLM Inference (0.01 ZION base)
Consciousness: Cosmic (1.5x)
Execution: 150ms (good efficiency)
Success: Yes

Calculation:
├── Base:        0.01 ZION
├── × Consciousness: 1.5x = 0.015 ZION
├── × Efficiency:    1.15x = 0.01725 ZION
└── Final:       0.01725 ZION

Per hour (1000 tasks):
└── 17.25 ZION/hour bonus

═══════════════════════════════════════════════════════════════
```

---

## 8.7 Task Contract

### Struktura tasku

```rust
// Z ncl.rs
pub struct NclTask {
    pub version: String,        // "1.0"
    pub task_id: String,        // UUID
    pub task_type: String,      // "llm_inference"
    pub payload: Value,         // Task-specific data
    pub deadline_ms: u64,       // Absolute deadline
    pub verification: NclVerification,
    pub reward: Option<NclReward>,
    pub retry_policy: NclRetryPolicy,
}
```

### Verification

```rust
pub struct NclVerification {
    pub method: String,     // "blake3_chain", "model_hash"
    pub seed: String,       // Seed for deterministic verification
    pub expected: Option<String>,  // Expected result (for deterministic)
    pub rounds: Option<u32>,       // Hash chaining rounds
}
```

### Retry Policy

```rust
pub struct NclRetryPolicy {
    pub max_retries: u32,       // Default: 3
    pub retry_delay_ms: u64,    // Default: 5000ms
    pub allow_reassignment: bool,  // Allow different miner
}
```

---

## 8.8 Hash Chaining (Deterministic Verification)

### Jak to funguje

Pro **deterministické ověření** používáme Blake3 hash chaining:

```rust
// Hash Chaining v1
pub fn verify_hash_chain(seed: &str, rounds: u32, expected: &str) -> bool {
    let mut hash = blake3::hash(seed.as_bytes());
    
    for _ in 0..rounds {
        hash = blake3::hash(hash.as_bytes());
    }
    
    hash.to_hex().as_str() == expected
}
```

### Proč hash chaining?

1. **Deterministické:** Stejný seed + rounds = stejný výsledek
2. **Rychlá verifikace:** Pool může ověřit bez GPU
3. **Proof-of-Work:** Miner skutečně provedl výpočet
4. **Není falšovatelné:** Nelze předpovědět výsledek bez výpočtu

---

## 8.9 Integration s Minerem

### NCL Integration struct

```rust
pub struct NCLIntegration {
    pub miner_address: String,
    pub consciousness: ConsciousnessLevel,
    pub runtime: NPURuntime,
    pub scheduler: NCLScheduler,
    pub calculator: NCLBonusCalculator,
    
    // Stats
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub total_earnings: f64,
    pub earnings_by_type: HashMap<AITaskType, f64>,
}
```

### Inicializace

```rust
let ncl = NCLIntegration::new(
    miner_address.clone(),
    consciousness_level,  // 1-6
    mining_allocation,    // 0.70 (70%)
);

println!("NCL Runtime: {}", ncl.runtime.as_str());
println!("NPU Allocation: {}%", ncl.scheduler.npu_allocation() * 100.0);
```

---

## 8.10 API Endpoints

### Pool API

| Endpoint | Method | Popis |
|----------|--------|-------|
| `/api/v1/ncl/status` | GET | NCL layer status |
| `/api/v1/ncl/task` | POST | Submit task |
| `/api/v1/ncl/result/{task_id}` | GET | Get task result |
| `/api/v1/ncl/leaderboard` | GET | Top NCL workers |
| `/api/v1/ncl/stats/{address}` | GET | Worker stats |

### Příklad: Submit Task

```bash
curl -X POST https://pool.zionterranova.com/api/v1/ncl/task \
  -H "Content-Type: application/json" \
  -d '{
    "version": "1.0",
    "task_type": "llm_inference",
    "payload": {
      "model": "llama-7b",
      "prompt": "What is ZION?",
      "max_tokens": 100
    },
    "deadline_ms": 1706554800000,
    "reward": {
      "zion": 0.01
    }
  }'
```

---

## 8.11 Consciousness + NCL Bonus

### Double Multiplier

NCL odměny jsou ovlivněny **consciousness level**:

```
NCL Earnings by Consciousness Level:
═══════════════════════════════════════════════════════════════

Level      │ Mining Bonus │ NCL Multiplier │ Combined Effect
───────────┼──────────────┼────────────────┼─────────────────
Physical   │ 1.0x         │ 1.0x           │ Standard
Emotional  │ 1.05x        │ 1.05x          │ +5%
Mental     │ 1.1x         │ 1.1x           │ +10%
Spiritual  │ 1.25x        │ 1.25x          │ +25%
Cosmic     │ 1.5x         │ 1.5x           │ +50%
OnTheStar  │ 2.0x         │ 2.0x           │ +100%

═══════════════════════════════════════════════════════════════
```

### Příklad: 1 hodina těžby + NCL

```
Hourly Earnings (Cosmic Level, 1.5x):
═══════════════════════════════════════════════════════════════

Mining (70% time):
├── Shares submitted: 1,000
├── Block rewards (share): ~3.5 ZION
└── Consciousness bonus: ×1.5 = 5.25 ZION

NCL (30% time):
├── Tasks completed: 500
├── Average reward: 0.005 ZION
├── Total: 2.5 ZION
└── Consciousness bonus: ×1.5 = 3.75 ZION

TOTAL: 5.25 + 3.75 = 9.0 ZION/hour

═══════════════════════════════════════════════════════════════
```

---

## 8.12 Use Cases

### Kdo používá NCL?

| Use Case | Task Type | Volume |
|----------|-----------|--------|
| **AI Startups** | LLM Inference | High |
| **Researchers** | Embeddings | Medium |
| **Content Creators** | Image Generation | Medium |
| **Developers** | Code Analysis | Low |
| **Podcasters** | Speech to Text | Medium |

### Příklad: AI Startup

```
Scenario: AI startup potřebuje inference pro chatbot
─────────────────────────────────────────────────────

Requirement: 10,000 LLM requests/day
Traditional cost: $0.01/request × 10,000 = $100/day

ZION NCL:
├── Cost: 0.01 ZION × 10,000 = 100 ZION/day
├── At $0.001/ZION: $0.10/day
└── Savings: 99.9%

Benefit: Decentralized, censorship-resistant, cheap
```

---

## 8.13 Roadmap

### NCL Evolution

```
NCL Development Roadmap:
═══════════════════════════════════════════════════════════════

Q1 2026: Foundation
├── ✅ Protocol specification v1.0
├── ✅ Hash chaining verification
├── ⏳ Basic task types (embeddings, classification)
└── ⏳ Pool integration

Q2 2026: Expansion
├── 📅 LLM inference support
├── 📅 Image generation tasks
├── 📅 NPU optimizations (CoreML, TensorRT)
└── 📅 Task marketplace

Q3 2026: Maturity
├── 📅 Model training tasks
├── 📅 Federated learning
├── 📅 Enterprise API
└── 📅 SLA guarantees

Q4 2026+: Scale
├── 🔮 Multi-model routing
├── 🔮 Cross-chain integration
├── 🔮 AGI tasks (future)
└── 🔮 Consciousness-weighted AI

═══════════════════════════════════════════════════════════════
```

---

## 8.14 Technické omezení

### Aktuální stav (TestNet)

| Feature | Status | ETA |
|---------|--------|-----|
| Hash chaining | ✅ Working | Live |
| Embeddings | ⏳ Testing | Q1 2026 |
| LLM inference | 📅 Planned | Q2 2026 |
| Image tasks | 📅 Planned | Q2 2026 |
| Model training | 🔮 Future | Q3 2026+ |

### Známé limitace

```
Current Limitations:
═══════════════════════════════════════════════════════════════

⚠️ Non-deterministic verification
   - LLM outputs nelze deterministicky ověřit
   - Řešení: Sampling + reputation system

⚠️ Model distribution
   - Velké modely (7B+) je těžké distribuovat
   - Řešení: IPFS + chunked download

⚠️ Latency requirements
   - Real-time inference vyžaduje <100ms
   - Řešení: Geographical routing

⚠️ GPU memory
   - Některé modely vyžadují >8GB VRAM
   - Řešení: Quantization, model splitting

═══════════════════════════════════════════════════════════════
```

---

## 8.15 Shrnutí

```
NCL — NEURAL COMPUTE LAYER:
═══════════════════════════════════════════════════════════════

✅ AI INFERENCE          - LLM, embeddings, classification
✅ ADDITIONAL REVENUE    - Minéři vydělávají víc
✅ CONSCIOUSNESS BONUS   - Vyšší level = vyšší rewards
✅ NPU SUPPORT           - CoreML, TensorRT, ONNX
✅ TIME SCHEDULING       - 70/30 mining/NCL split
✅ VERIFICATION          - Blake3 hash chaining

═══════════════════════════════════════════════════════════════
```

### Kód reference

```
2.9.5/zion-native/pool/src/ncl.rs        # 688 LOC - Pool NCL
2.9.5/zion-cosmic-harmony-v3/src/ncl_integration.rs  # 552 LOC - Miner NCL

Key structures:
├── NclTask        - Task contract
├── NCLScheduler   - Time allocation
├── NCLBonusCalculator - Reward calculation
└── NPURuntime     - Platform detection
```

---

**Pokračování:** [Kapitola 9 — Roadmap 2026-2027](09_ROADMAP.md)

---

*"Mining the future, one inference at a time."*  
**— ZION NCL Manifesto**
