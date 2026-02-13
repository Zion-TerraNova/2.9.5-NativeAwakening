# 🧘 Kapitola 3: Consciousness Mining

> *"Těžba není jen o hashrate. Je o tom, kým se stáváš."*

---

## 3.1 Co je Consciousness Mining?

**Consciousness Mining** je revoluční koncept, který transformuje těžbu kryptoměn z čistě mechanické činnosti na **cestu osobního růstu**.

Tradiční mining:
```
Hashrate → Shares → Reward
```

ZION Consciousness Mining:
```
Hashrate × Consciousness Level × Community Contribution → Enhanced Reward
```

### Proč Consciousness Mining?

| Tradiční mining | Consciousness Mining |
|-----------------|---------------------|
| Jen výkon hardware | Výkon + osobní rozvoj |
| Anonymní hash výpočty | Gamifikovaná cesta |
| Žádná komunita | Aktivní spolupráce |
| Čistě finanční motivace | Vyšší smysl + finance |
| Rich get richer | Každý může růst |

---

## 3.2 Devět úrovní vědomí

ZION rozlišuje **9 úrovní vědomí**, každá s vlastním multiplikátorem odměn:

```
┌─────────────────────────────────────────────────────────────┐
│                 CONSCIOUSNESS PYRAMID                        │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│                        ★                                    │
│                       /|\           Level 9: ON THE STAR    │
│                      / | \          Multiplier: 10.0×       │
│                     /  |  \                                 │
│                    ────────          Level 8: COSMIC        │
│                   /        \         Multiplier: 5.0×       │
│                  /          \                               │
│                 ──────────────        Level 7: TRANSCENDENT │
│                /              \       Multiplier: 3.0×      │
│               /                \                            │
│              ────────────────────     Level 6: ENLIGHTENED  │
│             /                    \    Multiplier: 2.0×      │
│            /                      \                         │
│           ──────────────────────────  Level 5: AWAKENED     │
│          /                          \ Multiplier: 1.5×      │
│         /                            \                      │
│        ────────────────────────────── Level 4: CONSCIOUS    │
│       /                              \Multiplier: 1.3×      │
│      /                                \                     │
│     ──────────────────────────────────Level 3: AWARE        │
│    /                                  \Multiplier: 1.2×     │
│   /                                    \                    │
│  ────────────────────────────────────── Level 2: MENTAL     │
│ /                                      \Multiplier: 1.1×    │
│/                                        \                   │
│══════════════════════════════════════════ Level 1: PHYSICAL │
│                                          Multiplier: 1.0×   │
└─────────────────────────────────────────────────────────────┘
```

### Detailní přehled úrovní

| Level | Název | Multiplikátor | XP požadavek | Charakteristika |
|-------|-------|---------------|--------------|-----------------|
| 1 | **PHYSICAL** | 1.0× | 0 | Začátečník, učí se základy |
| 2 | **MENTAL** | 1.1× | 1,000 | Pochopení blockchainu |
| 3 | **AWARE** | 1.2× | 5,000 | Aktivní člen komunity |
| 4 | **CONSCIOUS** | 1.3× | 15,000 | Přispívá do ekosystému |
| 5 | **AWAKENED** | 1.5× | 50,000 | Mentor pro nováčky |
| 6 | **ENLIGHTENED** | 2.0× | 150,000 | Vývojář/tvůrce obsahu |
| 7 | **TRANSCENDENT** | 3.0× | 500,000 | Vůdce komunity |
| 8 | **COSMIC** | 5.0× | 1,500,000 | Vizionář projektu |
| 9 | **ON THE STAR** | 10.0× | 5,000,000 | Mistr, duchovní učitel |

---

## 3.3 XP Systém (Experience Points)

### Jak získat XP?

| Aktivita | XP | Frekvence |
|----------|------|-----------|
| **Mining share** | 10 XP | Každý validní share |
| **Block found** | 1,000 XP | Když pool najde blok |
| **NCL task** | 50-500 XP | Podle složitosti AI tasku |
| **Community help** | 100 XP | Pomoc novému minerovi |
| **Bug report** | 500 XP | Validní security report |
| **Code contribution** | 1,000 XP | Merged PR na GitHubu |
| **Documentation** | 500 XP | Překlad, tutoriál |
| **Humanitarian donation** | 1 XP/ZION | Dobrovolný příspěvek |

### XP Decay (Rozpad zkušeností)

Aby systém odměňoval **aktivní** účastníky, XP pomalu klesá při neaktivitě:

```rust
// XP decay formula
fn calculate_xp_decay(last_activity: DateTime, current_xp: u64) -> u64 {
    let days_inactive = (now() - last_activity).days();
    
    if days_inactive < 7 {
        return current_xp;  // No decay first week
    }
    
    // 1% decay per day after 7 days, max 50% total
    let decay_percent = min((days_inactive - 7) * 1, 50);
    current_xp * (100 - decay_percent) / 100
}
```

**Praktický příklad:**
- 30 dní aktivní mining → XP roste
- 7 dní pauza → XP zůstává
- 14 dní pauza → XP kleslo o 7%
- 60+ dní pauza → XP kleslo o 50% (maximum)

---

## 3.4 Výpočet odměn

### Base Formula

```
Miner Reward = (Block Reward + Consciousness Bonus) × PPLNS Share × Level Multiplier
```

### Příklad výpočtu

**Situace:**
- Block reward: 50 ZION
- Consciousness bonus pool: 392.857 ZION
- Pool najde blok
- Miner má 25% PPLNS shares
- Miner je Level 5 (AWAKENED, 1.5×)

```
Step 1: Total block value
= 50 + 392.857 = 442.857 ZION

Step 2: After fees (pool 1% + tithe 10%)
= 442.857 × 0.89 = 394.14 ZION for miners

Step 3: Miner's PPLNS share
= 394.14 × 0.25 = 98.54 ZION (base)

Step 4: Apply consciousness multiplier
= 98.54 × 1.5 = 147.81 ZION (final)
```

**Level 5 miner vydělá o 50% více než Level 1 miner se stejným hashrate!**

### Srovnání odměn podle úrovně

| Level | Multiplikátor | Odměna za blok (25% share) |
|-------|---------------|---------------------------|
| 1 PHYSICAL | 1.0× | 98.54 ZION |
| 3 AWARE | 1.2× | 118.25 ZION |
| 5 AWAKENED | 1.5× | 147.81 ZION |
| 7 TRANSCENDENT | 3.0× | 295.62 ZION |
| 9 ON THE STAR | 10.0× | 985.40 ZION |

---

## 3.5 Consciousness Bonus Pool

### Kde se bere bonus?

Z Genesis bloku bylo alokováno **5.5 miliard ZION** do Consciousness Bonus Pool:

```
Genesis Allocation:
├── Mining Rewards:     128B ZION (89%)
├── Consciousness Pool:   5.5B ZION (3.8%)  ← Bonus source
├── DAO Treasury:         2B ZION
├── Dev/Ops:              4B ZION
└── Humanitarian:         2B ZION
```

### Distribuce bonusu

```rust
const CONSCIOUSNESS_BONUS_POOL: u64 = 5_500_000_000;  // 5.5B ZION
const BONUS_DISTRIBUTION_YEARS: u64 = 14;  // 2026-2040
const BLOCKS_PER_YEAR: u64 = 525_600;  // 60s blocks

// Per-block bonus
fn consciousness_bonus_per_block() -> Decimal {
    CONSCIOUSNESS_BONUS_POOL / (BONUS_DISTRIBUTION_YEARS * BLOCKS_PER_YEAR)
    // = 5.5B / 7,358,400 = ~747.49 ZION per block
    // Split: 392.857 ZION base + level multiplier
}
```

### Vyčerpání bonusu (2040)

Po roce 2040 bude Consciousness Bonus Pool vyčerpán. Mining bude pokračovat pouze s base reward (50 ZION), ale:

1. Consciousness levels zůstávají aktivní
2. XP systém pokračuje
3. Multiplikátory se aplikují na base reward
4. NCL bonusy nahradí část consciousness bonusu

---

## 3.6 Consciousness Challenges

### AI-Powered Výzvy

Každý týden systém generuje **Consciousness Challenges** — speciální úkoly pro XP bonus:

| Challenge Type | Popis | XP Bonus |
|---------------|-------|----------|
| **Mining Marathon** | 168h nepřetržité těžby | 5,000 XP |
| **Community Helper** | Pomozte 5 nováčkům | 2,500 XP |
| **Code Warrior** | PR merged do repo | 10,000 XP |
| **Documentation Sage** | Napište tutoriál | 3,000 XP |
| **Bug Hunter** | Nahlaste validní bug | 5,000 XP |
| **NCL Pioneer** | Dokončete 100 AI tasků | 7,500 XP |

### Příklad Challenge

```json
{
    "id": "weekly_2026_05",
    "type": "mining_marathon",
    "title": "Week 5 Mining Marathon",
    "description": "Mine continuously for 168 hours",
    "requirements": {
        "min_uptime_hours": 168,
        "min_shares": 10000,
        "no_gaps_over_minutes": 30
    },
    "reward": {
        "xp": 5000,
        "badge": "Marathon Miner 🏃"
    },
    "deadline": "2026-02-02T23:59:59Z"
}
```

---

## 3.7 Consciousness Badges

### Achievement System

Minéři mohou sbírat **badges** (odznaky) za speciální úspěchy:

| Badge | Podmínka | Bonus |
|-------|----------|-------|
| 🌱 **First Block** | Účast na prvním nalezeném bloku | +100 XP |
| ⛏️ **1K Shares** | 1,000 validních shares | +500 XP |
| 💎 **Diamond Hands** | 1 rok nepřetržité těžby | +50,000 XP |
| 🤝 **Community Pillar** | Pomohli 100 nováčkům | +25,000 XP |
| 🧠 **NCL Master** | 10,000 NCL tasků | +100,000 XP |
| 🏆 **Block Finder** | Osobně nalezený blok | +10,000 XP |
| 📚 **Lore Keeper** | Překlad whitepaperu | +15,000 XP |
| 🔧 **Core Dev** | Commit do core repo | +50,000 XP |

### Badge Display

```
┌─────────────────────────────────────────┐
│  Miner: zion1abc...xyz                  │
│  Level: 6 ENLIGHTENED (2.0×)            │
│  XP: 245,000 / 500,000                  │
│                                         │
│  Badges:                                │
│  🌱 ⛏️ 💎 🤝 🏆 📚                        │
│                                         │
│  Next milestone: TRANSCENDENT (3.0×)    │
│  Progress: ████████░░ 49%               │
└─────────────────────────────────────────┘
```

---

## 3.8 Consciousness Leaderboard

### Globální žebříček

Pool udržuje real-time leaderboard nejlepších minerů:

```
GET /api/v1/consciousness/leaderboard?limit=10

┌────┬─────────────────────┬───────┬──────────┬────────────┐
│ #  │ Miner               │ Level │ XP       │ Badges     │
├────┼─────────────────────┼───────┼──────────┼────────────┤
│ 1  │ zion1master...      │ 9 ★   │ 5,234,567│ 🌱⛏️💎🤝🧠🏆│
│ 2  │ zion1cosmic...      │ 8     │ 2,145,000│ 🌱⛏️💎🤝🏆 │
│ 3  │ zion1trans...       │ 7     │ 892,000  │ 🌱⛏️💎🤝   │
│ 4  │ zion1enlight...     │ 6     │ 345,000  │ 🌱⛏️💎     │
│ 5  │ zion1awaken...      │ 5     │ 123,000  │ 🌱⛏️       │
│ ...│ ...                 │ ...   │ ...      │ ...        │
└────┴─────────────────────┴───────┴──────────┴────────────┘
```

### Filtry leaderboardu

- **Global** — všichni minéři
- **Weekly** — nejlepší tento týden
- **Country** — podle geolokace (volitelné)
- **Pool** — podle mining poolu

---

## 3.9 Filosofie za Consciousness Mining

### Proč 9 úrovní?

Devět úrovní vědomí odpovídá mnoha spirituálním tradicím:

| Tradice | Koncept |
|---------|---------|
| **Kabala** | 9 Sefirot (bez Keter/Malkut) |
| **Buddhismus** | 9 stupňů meditace (Jhana) |
| **Dante** | 9 kruhů Ráje |
| **Čakry** | 7 hlavních + 2 transpersonální |
| **Enneagram** | 9 typů osobnosti |

### Mining jako meditace

> *"Když tvůj počítač těží, ty můžeš meditovat. Hashrate je mantrou digitálního věku."*

Consciousness Mining není jen o odměnách. Je to připomínka, že:

1. **Trpělivost je ctnost** — level up trvá měsíce
2. **Komunita je síla** — sám daleko nedojdeš
3. **Služba je odměna** — humanitarian tithe je povinný
4. **Růst je cesta** — cíl není bohatství, ale evoluce

---

## 3.10 Implementace v kódu

### Rust struktura

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsciousnessLevel {
    Physical = 1,      // 1.0×
    Mental = 2,        // 1.1×
    Aware = 3,         // 1.2×
    Conscious = 4,     // 1.3×
    Awakened = 5,      // 1.5×
    Enlightened = 6,   // 2.0×
    Transcendent = 7,  // 3.0×
    Cosmic = 8,        // 5.0×
    OnTheStar = 9,     // 10.0×
}

impl ConsciousnessLevel {
    pub fn multiplier(&self) -> Decimal {
        match self {
            Self::Physical => dec!(1.0),
            Self::Mental => dec!(1.1),
            Self::Aware => dec!(1.2),
            Self::Conscious => dec!(1.3),
            Self::Awakened => dec!(1.5),
            Self::Enlightened => dec!(2.0),
            Self::Transcendent => dec!(3.0),
            Self::Cosmic => dec!(5.0),
            Self::OnTheStar => dec!(10.0),
        }
    }
    
    pub fn xp_required(&self) -> u64 {
        match self {
            Self::Physical => 0,
            Self::Mental => 1_000,
            Self::Aware => 5_000,
            Self::Conscious => 15_000,
            Self::Awakened => 50_000,
            Self::Enlightened => 150_000,
            Self::Transcendent => 500_000,
            Self::Cosmic => 1_500_000,
            Self::OnTheStar => 5_000_000,
        }
    }
}
```

### Pool integration

```rust
pub async fn calculate_miner_reward(
    &self,
    miner_address: &str,
    pplns_share: Decimal,
    block_reward: Decimal,
    consciousness_bonus: Decimal,
) -> Result<Decimal> {
    // Get miner's consciousness level
    let level = self.get_miner_level(miner_address).await?;
    
    // Calculate base reward
    let total_block = block_reward + consciousness_bonus;
    let after_fees = total_block * dec!(0.89);  // 11% fees
    let base_reward = after_fees * pplns_share;
    
    // Apply consciousness multiplier
    let final_reward = base_reward * level.multiplier();
    
    Ok(final_reward)
}
```

---

**Pokračování:** [Kapitola 4 — Ekonomický model](04_ECONOMIC_MODEL.md)

---

*"The real mining happens within."*  
**— ZION Consciousness Manifesto**
