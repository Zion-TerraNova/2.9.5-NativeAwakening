# 💰 Kapitola 4: Ekonomický Model

> *"Matematika nelže. Supply je 144B a hotovo."*

---

## 4.1 Základní parametry (z kódu)

Tyto hodnoty jsou **immutable** — zakódované v genesis bloku a protokolu:

| Parametr | Hodnota | Zdroj |
|----------|---------|-------|
| **Total Supply** | 144,000,000,000 ZION | Genesis block |
| **Block Time** | 60 sekund | Consensus rules |
| **Mining Duration** | 45 let (2025-2070) | Protocol spec |
| **Blocks per Year** | 525,600 | 60s × 60min × 24h × 365d |
| **Total Blocks** | 23,652,000 | 45 × 525,600 |

---

## 4.2 Token Distribuce

### Genesis Allocation (16.28B ZION)

```
Genesis Block Distribution:
┌────────────────────────────────────────────────────────────┐
│ TOTAL SUPPLY: 144,000,000,000 ZION (144B)                  │
├────────────────────────────────────────────────────────────┤
│                                                            │
│ ┌──────────────────────────────────────────────────────┐   │
│ │ MINING EMISSION: 127,720,000,000 ZION (88.69%)       │   │
│ │ → Distribuováno těžbou během 45 let                  │   │
│ └──────────────────────────────────────────────────────┘   │
│                                                            │
│ ┌──────────────────────────────────────────────────────┐   │
│ │ GENESIS PREMINE: 16,280,000,000 ZION (11.31%)        │   │
│ │ → Alokováno v genesis bloku                          │   │
│ └──────────────────────────────────────────────────────┘   │
│                                                            │
└────────────────────────────────────────────────────────────┘
```

### Premine Breakdown (16.28B)

| Alokace | ZION | % z Premine | % z Total | Účel |
|---------|------|-------------|-----------|------|
| **ZION OASIS + Winners Golden Egg/Xp** | 8,250,000,000 | 50.7% | 5.73% | OASIS rewards + Golden Egg/Xp |
| **DAO Treasury** | 4,000,000,000 | 24.6% | 2.78% | Komunitní governance |
| **Infrastructure** | 2,500,000,000 | 15.4% | 1.74% | Servery, vývoj, audit |
| **Humanitarian Fund** | 1,530,000,000 | 9.4% | 1.06% | Tithe iniciální alokace |

**Poznámka:** Presale alokace (500M ZION) byla **zrušena** v lednu 2026. Tyto tokeny zůstávají v DAO Treasury.

---

## 4.3 Block Reward Výpočet

### Matematický důkaz

```python
# Vstupní hodnoty (immutable):
TOTAL_SUPPLY = 144_000_000_000        # ZION
GENESIS_PREMINE = 16_280_000_000      # ZION
MINING_EMISSION = TOTAL_SUPPLY - GENESIS_PREMINE
                = 127_720_000_000      # ZION

# Mining parametry:
MINING_YEARS = 45                      # 2025-2070
BLOCKS_PER_YEAR = 525_600
TOTAL_BLOCKS = MINING_YEARS * BLOCKS_PER_YEAR
             = 23_652_000              # bloků

# Base Block Reward:
BASE_BLOCK_REWARD = MINING_EMISSION / TOTAL_BLOCKS
                  = 127_720_000_000 / 23_652_000
                  = 5,400.067 ZION     # per block ✅
```

### Ověření

```
5,400.067 × 23,652,000 = 127,720,384,400 ZION
+ Genesis premine:        16,280,000,000 ZION
────────────────────────────────────────────────
= Total:                 144,000,384,400 ZION

Zaokrouhlovací chyba: 384,400 ZION (0.00027% z total supply)
✅ PŘIJATELNÉ
```

---

## 4.4 Reward System (MainNet)

### Dva režimy

ZION má **dva režimy** podle časového období:

#### Režim 1: Consciousness Period (2025-2035)

```
Block Reward = BASE_REWARD + CONSCIOUSNESS_BONUS

Kde:
- BASE_REWARD = 5,400.067 ZION (z mining emission)
- CONSCIOUSNESS_BONUS = 1,569.63 ZION × multiplier (z premine pool)
```

**Consciousness Bonus:**
```python
CONSCIOUSNESS_POOL = 8_250_000_000    # ZION (OASIS + Winners Golden Egg/Xp pool)
CONSCIOUSNESS_YEARS = 10              # 2025-2035
CONSCIOUSNESS_BLOCKS = 10 * 525_600 = 5_256_000

CONSCIOUSNESS_BONUS_BASE = CONSCIOUSNESS_POOL / CONSCIOUSNESS_BLOCKS
                         = 8_250_000_000 / 5_256_000
                         = 1,569.63 ZION per block
```

**Výsledná odměna (Consciousness Period):**

| Miner Type | Base | Bonus | Multiplier | Total |
|------------|------|-------|------------|-------|
| Non-whitelisted | 5,400.07 | 0 | N/A | 5,400.07 ZION |
| Whitelisted L1 | 5,400.07 | 1,569.63 | 1.0× | 6,969.70 ZION |
| Whitelisted L5 | 5,400.07 | 7,848.15 | 5.0× | 13,248.22 ZION |
| Whitelisted L9 | 5,400.07 | 15,696.30 | 10.0× | 21,096.37 ZION |

#### Režim 2: Post-Consciousness (2036-2070)

```
Block Reward = BASE_REWARD only

- BASE_REWARD = 5,400.067 ZION
- CONSCIOUSNESS_BONUS = 0 ZION (pool vyčerpán)
```

Všichni minéři dostávají stejnou odměnu: **5,400.067 ZION per block**.

---

## 4.5 TestNet Režim (Aktuální)

Pro TestNet používáme **zjednodušený model**:

```python
# TestNet configuration (src/pool/blockchain/reward_calculator.py)
if TESTNET_MODE:
    BASE_BLOCK_REWARD = Decimal("50")      # 50 ZION per block
    CONSCIOUSNESS_BONUS_BASE = Decimal("0") # No bonus
    HUMANITARIAN_TITHE = Decimal("0.00")   # No tithe
```

| Parametr | TestNet | MainNet |
|----------|---------|---------|
| Block Reward | 50 ZION | 5,400.067 ZION |
| Consciousness Bonus | 0 | 1,569.63 × level |
| Humanitarian Tithe | 0% | 10% |
| Pool Fee | 1% | 1% |

---

## 4.6 Distribuce odměn

### Fee Structure

```
Block Reward Distribution:
┌─────────────────────────────────────────────────────────┐
│ Total Block Reward: 100%                                │
├─────────────────────────────────────────────────────────┤
│                                                         │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ Humanitarian Tithe: 10%                             │ │
│ │ → ZION_CHILDREN_FUTURE_FUND                         │ │
│ └─────────────────────────────────────────────────────┘ │
│                                                         │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ Pool Fee: 1%                                        │ │
│ │ → Pool operator                                     │ │
│ └─────────────────────────────────────────────────────┘ │
│                                                         │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ Miner Share: 89%                                    │ │
│ │ → PPLNS distribution                                │ │
│ └─────────────────────────────────────────────────────┘ │
│                                                         │
└─────────────────────────────────────────────────────────┘
```

### Příklad výpočtu (MainNet, Whitelisted L1)

```
Total Reward: 6,969.70 ZION

Distribuce:
- Humanitarian Tithe (10%): 696.97 ZION
- Pool Fee (1%):             69.70 ZION
- Miner Share (89%):      6,203.03 ZION

PPLNS (miner má 25% shares):
- Miner Payout: 6,203.03 × 0.25 = 1,550.76 ZION
```

---

## 4.7 Emission Schedule

### Roční emise

```
Mining Emission per Year:
= BLOCKS_PER_YEAR × BASE_BLOCK_REWARD
= 525,600 × 5,400.067
= 2,838,275,215 ZION (~2.84B per year)
```

### Kumulativní supply

| Rok | Mining Emission | Cumulative | % of Total |
|-----|-----------------|------------|------------|
| 2025 | 2.84B | 2.84B + 16.28B = 19.12B | 13.3% |
| 2030 | 2.84B | 14.2B + 16.28B = 30.48B | 21.2% |
| 2035 | 2.84B | 28.4B + 16.28B = 44.68B | 31.0% |
| 2040 | 2.84B | 42.6B + 16.28B = 58.88B | 40.9% |
| 2050 | 2.84B | 71.0B + 16.28B = 87.28B | 60.6% |
| 2070 | 2.84B | 127.72B + 16.28B = 144B | 100% |

### Vizualizace

```
Supply Growth (144B total):
2025 ████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 13%
2030 ██████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 21%
2035 ███████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 31%
2040 ████████████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 41%
2050 ██████████████████████████████░░░░░░░░░░░░░░░░░░░░ 61%
2060 ████████████████████████████████████████░░░░░░░░░░ 80%
2070 ██████████████████████████████████████████████████ 100%
```

---

## 4.8 Inflace vs. Deflace

### ZION má **předvídatelnou inflaci**

```
Roční inflační míra (% cirkulující supply):

Rok 2025: 2.84B / 19.12B = 14.9%
Rok 2030: 2.84B / 30.48B = 9.3%
Rok 2035: 2.84B / 44.68B = 6.4%
Rok 2040: 2.84B / 58.88B = 4.8%
Rok 2050: 2.84B / 87.28B = 3.3%
Rok 2070: 2.84B / 144B   = 2.0% (konečná emise)
```

### Žádný halving

Na rozdíl od Bitcoinu, ZION **nemá halving**. Block reward je konstantní:

| Vlastnost | Bitcoin | ZION |
|-----------|---------|------|
| Halving | Každé 4 roky | Žádný |
| Block Reward | Klesá (6.25→3.125→...) | Konstantní (5,400 ZION) |
| Final Supply | ~21M (2140) | 144B (2070) |
| Předvídatelnost | Skoková změna | Lineární |

**Proč žádný halving?**
- Předvídatelnost pro minéry (žádné šoky)
- Stabilní security budget
- Jednodušší ekonomické plánování

---

## 4.9 Whitelist System

### ZION OASIS + Winners Golden Egg/Xp (5 adres)

Pro MainNet existuje **whitelist** 5 OASIS + Golden Egg adres, které dostávají consciousness bonus:

```python
# Z premine.py
OASIS_GOLDEN_EGG = [
    "ZION_SACRED_B0FA7E2A234D8C2F08545F02295C98",
    "ZION_QUANTUM_89D80B129682D41AD76DAE3F90C3E2",
    "ZION_COSMIC_397B032D6E2D3156F6F709E8179D36",
    "ZION_ENLIGHTENED_004A5DBD12FDCAACEDCB5384DDC035",
    "ZION_TRANSCENDENT_6BD30CB1835013503A8167D9CD86E0",
]
```

### Proč whitelist?

1. **Early adopter incentive**: Odměna za podporu projektu od začátku
2. **Security budget**: Zajištění dostatečného hashrate v early phase
3. **Time-limited**: Pouze 10 let (2025-2035), pak rovné podmínky
4. **Transparentní**: Adresy jsou veřejné, auditovatelné

### Po roce 2035

Whitelist **přestává platit**. Všichni minéři dostávají stejnou odměnu (5,400 ZION base).

---

## 4.10 Srovnání s jinými projekty

| Metrika | Bitcoin | Ethereum | Monero | **ZION** |
|---------|---------|----------|--------|----------|
| Total Supply | 21M | ∞ (EIP-1559) | ∞ | **144B** |
| Block Time | 10 min | 12 sec | 2 min | **60 sec** |
| Block Reward | 3.125 BTC | ~2 ETH | ~0.6 XMR | **5,400 ZION** |
| Halving | Yes | No | Tail emission | **No** |
| Premine | 0% | ~72M ETH | 0% | **11.31%** |
| Mining End | ~2140 | N/A | Never | **2070** |

---

## 4.11 Rizika a mitigace

### Známá rizika

| Riziko | Popis | Mitigace |
|--------|-------|----------|
| **Nízký hashrate** | Nedostatek minerů | Consciousness bonus incentive |
| **Inflace** | 2.84B ZION/rok | Utility (DAO, NCL, fees) |
| **Whitelist centralizace** | 5 adres má bonus | Pouze 10 let, pak fair |
| **Premine kritika** | 11.31% v genesis | Transparentní, auditovatelné |

### Co NEZARUČUJEME

- ❌ Cenu tokenu
- ❌ Listing na burze
- ❌ ROI pro minéry
- ❌ Stabilitu kurzu

### Co ZARUČUJEME

- ✅ Total supply = 144B (immutable)
- ✅ Block reward = 5,400 ZION (immutable)
- ✅ Transparentní premine (on-chain audit)
- ✅ Open-source kód (MIT licence)

---

## 4.12 Kód reference

Všechny ekonomické parametry jsou definovány v:

```
src/pool/blockchain/reward_calculator.py
├── BASE_BLOCK_REWARD = 5,400.067 ZION
├── CONSCIOUSNESS_BONUS_BASE = 1,569.63 ZION
├── HUMANITARIAN_TITHE = 10%
├── POOL_FEE = 1%
├── CONSCIOUSNESS_START_YEAR = 2025
├── CONSCIOUSNESS_END_YEAR = 2035
└── MINING_END_YEAR = 2070
```

**Audit:** Kód je open-source na [GitHub](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening).

---

**Pokračování:** [Kapitola 5 — Fair Launch & Distribuce](05_FAIR_LAUNCH.md)

---

*"In code we trust. 144B ZION. Not one satoshi more."*  
**— ZION Economic Manifesto**
