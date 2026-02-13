# Cosmic Harmony V3 Fork — Vysvětlení

## Co je to fork?

Fork v blockchainu = změna pravidel validace bloků od určité výšky. Starší bloky se validují starými pravidly, nové bloky novými.

## Proč fork?

ZION měl původní hashovací algoritmus **Cosmic Harmony v1** (jednoduchý, 4-byte u32 porovnání). Pool a miner ale používají vylepšený **Cosmic Harmony v3** (plný 32-byte hash z `zion-cosmic-harmony-v3` crate). Tyto dva algoritmy produkují **úplně jiný hash** ze stejných dat.

## Problém

```
Bloky 0-7: Uloženy v blockchainu s hashem z CosmicHarmony v1
Blok 8+:   Pool a miner hashují pomocí CosmicHarmony v3
```

Pokud by core validoval blok 8 starým algoritmem → hash nesedí → "Insufficient PoW".
Pokud by core validoval staré bloky 0-7 novým algoritmem → hash nesedí → "Invalid prev_hash".

## Řešení: `ZION_CH_V3_FORK_HEIGHT=8`

Env proměnná `ZION_CH_V3_FORK_HEIGHT` říká core nodu:

| Výška bloku | Hashovací algoritmus | Validace PoW |
|-------------|---------------------|--------------|
| 0–7         | Cosmic Harmony **v1** (u32) | Starý způsob |
| 8+          | Cosmic Harmony **v3** (full 32-byte) | Nový způsob |

### Kde se to projevuje v kódu

**`zion-native/core/src/blockchain/block.rs`** — `BlockHeader::calculate_hash()`:
```rust
static CH_V3_FORK_HEIGHT: Lazy<u64> = Lazy::new(|| {
    std::env::var("ZION_CH_V3_FORK_HEIGHT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10)  // default = 10
});

// V calculate_hash():
if self.height >= *CH_V3_FORK_HEIGHT {
    // Nový CHv3 algoritmus (pool-kompatibilní)
    cosmic_harmony_v3(&blob, nonce)
} else {
    // Starý CH v1 algoritmus
    cosmic_harmony::hash(&data, nonce, height)
}
```

## Nasazení na servery

Každý core node potřebuje:
```bash
docker run ... -e ZION_CH_V3_FORK_HEIGHT=8 ... zion-core:2.9.5
```

## Důležité

- **Fork height musí být stejný na VŠECH nodech** (jinak desync)
- **Nesmí se měnit po nasazení** (bloky by se invalidovaly)
- **Pro mainnet**: Nastavit na výšku, kde se provede upgrade (např. 100000)
- **Pro testnet**: Nastaveno na `8` (bloky 0-7 jsou genesis + premine)

## Aktuální stav (6.2.2026)

| Server | Fork Height | Status |
|--------|------------|--------|
| Helsinki ([SEED-EU-IP]) | 8 ✅ | Funguje, height=8 |
| USA ([SEED-US-IP]) | ❌ Starý image | Potřebuje nový build |
| Singapore ([SEED-SG-IP]) | ❌ Starý image | Potřebuje nový build |

USA a Singapore mají starší Docker image (`zion-core:2.9.5-amd64`), který **nemá CHv3 fork kód vůbec**. Potřebují nový build ze zdrojového kódu.
