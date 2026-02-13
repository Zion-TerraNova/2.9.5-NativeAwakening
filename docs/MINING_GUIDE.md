# ⛏️ ZION Miner — Kompletní průvodce těžbou (Mining Guide)

> **Verze:** v2.9.5 Native Awakening  
> **Poslední aktualizace:** Únor 2026

---

## 📋 Obsah (Table of Contents)

1. [Co je ZION mining?](#-co-je-zion-mining)
2. [Systémové požadavky](#-systémové-požadavky)
3. [Stažení mineru](#-stažení-mineru)
4. [Instalace krok za krokem](#-instalace-krok-za-krokem)
   - [Linux (x86_64)](#linux-x86_64--intel--amd)
   - [Linux (ARM64)](#linux-arm64--raspberry-pi--oracle-cloud)
   - [macOS (Apple Silicon)](#macos-apple-silicon--m1--m2--m3--m4)
5. [Konfigurace a spuštění](#-konfigurace-a-spuštění)
6. [Podporované algoritmy](#-podporované-algoritmy)
7. [Pokročilá konfigurace](#-pokročilá-konfigurace)
8. [Řešení problémů](#-řešení-problémů)
9. [FAQ](#-faq)
10. [Kompletní návod od 0 — Laik](#-kompletní-návod-od-0--laik)
11. [Kompletní návod — Profi](#-kompletní-návod--profi-node--wallet--miner)

---

## 🌟 Co je ZION mining?

ZION TerraNova je **proof-of-work (PoW) blockchain** — to znamená, že vaše CPU (procesor) počítá matematické úlohy a za každý nalezený blok dostanete odměnu v mincích **ZION**.

Těžba (mining) je:
- ✅ **Spravedlivá** — Cosmic Harmony v3 algoritmus rotuje mezi různými PoW algoritmy, takže žádný typ hardware nemá trvalou výhodu
- ✅ **Ekologická** — Dynamická obtížnost snižuje zbytečné plýtvání energií
- ✅ **Decentralizovaná** — Může těžit kdokoli s běžným počítačem

**Odměna za blok:** Blok je nalezen přibližně každých 60 sekund. 10 % z odměny automaticky směřuje na humanitární projekty (Humanitarian Tithe).

---

## 💻 Systémové požadavky

### Minimum

| Komponenta | Požadavek |
|-----------|-----------|
| **OS** | Linux (x86_64 nebo ARM64) / macOS (Apple Silicon) |
| **CPU** | 2+ jádra |
| **RAM** | 2 GB |
| **Disk** | 100 MB volného místa |
| **Síť** | Stabilní internetové připojení |

### Doporučeno

| Komponenta | Doporučeno |
|-----------|-----------|
| **CPU** | 4+ jader, moderní procesor (AMD Ryzen, Intel 12th+, Apple M1+) |
| **RAM** | 4+ GB |
| **Síť** | Nízká latence (< 100 ms k node) |

> 💡 **Tip:** ZION miner je optimalizován pro CPU těžbu. GPU zatím není vyžadováno.

---

## 📥 Stažení mineru

### Možnost A: GitHub Release (doporučeno)

Přejděte na stránku **Releases**:

👉 **[github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases)**

Stáhněte soubor pro váš systém:

| Soubor | Systém |
|--------|--------|
| `zion-miner-linux-x86_64` | Linux — Intel / AMD (většina serverů a PC) |
| `zion-miner-linux-arm64` | Linux — ARM64 (Raspberry Pi 4/5, Oracle Cloud, AWS Graviton) |
| `zion-miner-macos-arm64` | macOS — Apple Silicon (M1, M2, M3, M4) |

### Možnost B: Přímý odkaz (wget/curl)

```bash
# Linux x86_64 (Intel/AMD)
wget https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-linux-x86_64

# Linux ARM64
wget https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-linux-arm64

# macOS Apple Silicon
curl -LO https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-macos-arm64
```

---

## 🔧 Instalace krok za krokem

### Linux x86_64 — Intel / AMD

```bash
# 1. Stáhněte binárku
wget https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-linux-x86_64

# 2. Nastavte práva ke spuštění
chmod +x zion-miner-linux-x86_64

# 3. Přesuňte do systémové cesty (volitelné)
sudo mv zion-miner-linux-x86_64 /usr/local/bin/zion-miner

# 4. Ověřte instalaci
zion-miner --version
# Výstup: zion-core 2.9.5
```

### Linux ARM64 — Raspberry Pi / Oracle Cloud

```bash
# 1. Stáhněte binárku
wget https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-linux-arm64

# 2. Nastavte práva ke spuštění
chmod +x zion-miner-linux-arm64

# 3. Přesuňte do systémové cesty (volitelné)
sudo mv zion-miner-linux-arm64 /usr/local/bin/zion-miner

# 4. Ověřte instalaci
zion-miner --version
# Výstup: zion-core 2.9.5
```

### macOS Apple Silicon — M1 / M2 / M3 / M4

```bash
# 1. Stáhněte binárku
curl -LO https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-macos-arm64

# 2. Nastavte práva ke spuštění
chmod +x zion-miner-macos-arm64

# 3. Odblokujte v macOS (Gatekeeper)
#    macOS může blokovat neznámé binárky — toto je bezpečné:
xattr -d com.apple.quarantine zion-miner-macos-arm64

# 4. Přesuňte do systémové cesty (volitelné)
sudo mv zion-miner-macos-arm64 /usr/local/bin/zion-miner

# 5. Ověřte instalaci
zion-miner --version
# Výstup: zion-core 2.9.5
```

> ⚠️ **macOS uživatelé:** Pokud se zobrazí hlášení *"cannot be opened because the developer cannot be verified"*, otevřete **System Settings → Privacy & Security** a klikněte na **"Allow Anyway"** / **"Přesto povolit"**.

---

## 🚀 Konfigurace a spuštění

### Základní příkaz — Pool mining (doporučeno)

```bash
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet VAŠE_ZION_ADRESA
```

To je vše! Miner se připojí na veřejný pool a začne těžit s algoritmem **Cosmic Harmony**.

### Příklad s vlákny + algoritmem

```bash
zion-miner \
  --pool stratum+tcp://pool.zionterranova.com:3333 \
  --wallet zion1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh \
  --threads 4 \
  --algorithm cosmic_harmony
```

### Všechny dostupné parametry

| Parametr | Popis | Výchozí hodnota |
|----------|-------|-----------------|
| `--pool` / `-p` | **[POVINNÝ]** URL poolu (stratum+tcp://host:port) | — |
| `--wallet` / `-w` | **[POVINNÝ]** Vaše ZION adresa | — |
| `--algorithm` / `-a` | Algoritmus těžby | `cosmic_harmony` |
| `--threads` / `-t` | Počet CPU vláken (0 = auto) | `0` |
| `--gpu` | Zapnutí GPU režimu | vypnuto |
| `--ncl` | Neural Compute Layer bonus | vypnuto |
| `--help` / `-h` | Zobrazí nápovědu | — |
| `--version` / `-V` | Zobrazí verzi | — |

---

## 🔮 Podporované algoritmy

ZION Cosmic Harmony v3 automaticky rotuje mezi algoritmy:

| Algoritmus | Typ | Popis |
|-----------|-----|-------|
| **cosmic_harmony** | Multi-PoW | 🌟 Výchozí — automatická rotace (doporučeno) |
| **randomx** | CPU-friendly | Vhodný pro CPU, odolný vůči ASIC |
| **yescrypt** | CPU-friendly | Paměťově náročný, vhodný pro CPU |
| **blake3** | Rychlý hash | Velmi rychlý, nízká spotřeba |

### Volba algoritmu

```bash
# Doporučeno — nechte Cosmic Harmony rozhodnout
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet VAŠE_ADRESA --algorithm cosmic_harmony

# Specifický algoritmus (pro pokročilé)
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet VAŠE_ADRESA --algorithm randomx
```

> 💡 **Doporučení:** Ponechte výchozí `cosmic_harmony`. Systém automaticky vybere optimální algoritmus.

---

## ⚙️ Pokročilá konfigurace

### Spuštění na pozadí (systemd — Linux)

Vytvořte soubor `/etc/systemd/system/zion-miner.service`:

```ini
[Unit]
Description=ZION TerraNova Miner
After=network.target

[Service]
Type=simple
User=zionminer
ExecStart=/usr/local/bin/zion-miner \
  --pool stratum+tcp://pool.zionterranova.com:3333 \
  --wallet VAŠE_ZION_ADRESA \
  --threads 0
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Aktivace:

```bash
# Vytvořte uživatele (bezpečnější než root)
sudo useradd -r -s /bin/false zionminer

# Aktivujte a spusťte
sudo systemctl daemon-reload
sudo systemctl enable zion-miner
sudo systemctl start zion-miner

# Kontrola stavu
sudo systemctl status zion-miner

# Zobrazení logů
sudo journalctl -u zion-miner -f
```

### Spuštění přes screen/tmux

```bash
# S tmux
tmux new -s miner
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet VAŠE_ADRESA
# Ctrl+B, pak D pro odpojení

# S screen
screen -S miner
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet VAŠE_ADRESA
# Ctrl+A, pak D pro odpojení
```

---

## 🔍 Řešení problémů

### "Permission denied"

```bash
chmod +x zion-miner-linux-x86_64
```

### "cannot be opened because the developer cannot be verified" (macOS)

```bash
xattr -d com.apple.quarantine zion-miner-macos-arm64
```

Nebo: **System Settings → Privacy & Security → Allow Anyway**

### "Connection refused" / "Pool error"

Miner se nemůže připojit k poolu. Zkontrolujte:

1. Máte správnou adresu poolu? (`pool.zionterranova.com:3333`)
2. Je firewall otevřený pro odchozí TCP na portu 3333?
3. Máte internetové připojení?

```bash
# Test TCP připojení k poolu
nc -zv pool.zionterranova.com 3333

# Pokud nefunguje, zkuste přes IP
zion-miner --pool stratum+tcp://77.42.31.72:3333 --wallet VAŠE_ADRESA
```

### "GLIBC not found" (starší Linux)

Binárka vyžaduje moderní Linux. Pokud vidíte chybu s GLIBC:
- Aktualizujte systém: `sudo apt update && sudo apt upgrade`
- Nebo použijte novější distribuci (Ubuntu 22.04+, Debian 12+)

### Nízký hashrate

- Ujistěte se, že neběží jiné náročné procesy
- Zkontrolujte teplotu CPU (`sensors` na Linuxu)
- Zkuste jiný algoritmus: `--algorithm randomx`

---

## ❓ FAQ

### Potřebuji vlastní ZION node?

**Ne.** Stačí se připojit na veřejný pool `pool.zionterranova.com:3333`. Pool se stará o komunikaci s blockchainem za vás. Vlastní node je potřeba pouze pokud chcete provozovat vlastní pool nebo solo mining.

### Kolik vydělám?

Záleží na výkonu vašeho CPU a aktuální obtížnosti sítě. Blok je nalezen každých ~60 sekund. Vaše šance na nalezení bloku roste s vaším hashratu vůči celkové síti.

### Je to bezpečné?

Ano. Miner pouze počítá hashe a komunikuje s poolem. Nepotřebuje přístup k vašemu privátnímu klíči — pouze veřejnou wallet adresu.

### Mohu těžit na Raspberry Pi?

Ano! Stáhněte verzi `zion-miner-linux-arm64`. Raspberry Pi 4/5 zvládne těžbu, ale hashrate bude nižší než u výkonných serverů.

### Kde získám ZION wallet adresu?

Navštivte [zionterranova.com](https://zionterranova.com) nebo se zeptejte na [Discordu](https://discord.gg/zion-terranova).

---

## 🧭 Kompletní návod od 0 — Laik

### 1) Co stáhnout

Z release stáhni 3 věci pro svůj OS:
- `zion-wallet-*` (vytvoření adresy)
- `zion-miner-*` (těžba)
- `zion-node-*` (volitelné, pokud chceš vlastní node)

### 2) Vytvoření wallet adresy

```bash
zion-wallet gen-mnemonic --out my-wallet.json --print
```

Ulož si bezpečně:
- 24 slov (mnemonic)
- soubor `my-wallet.json`

### 3) Spuštění těžby (nejjednodušší)

```bash
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet TVOJE_ZION_ADRESA
```

### 4) Kontrola zůstatku

```bash
zion-wallet balance --address TVOJE_ZION_ADRESA --node https://node.zionterranova.com
```

### 5) Odeslání transakce

```bash
zion-wallet send --wallet my-wallet.json --to zion1PRIJEMCE --amount 10 --node https://node.zionterranova.com
```

---

## 🛠️ Kompletní návod — Profi (Node + Wallet + Miner)

### A) Spuštění vlastního node

```bash
./zion-node-linux-x86_64 \
  --network mainnet \
  --rpc-port 8444 \
  --p2p-port 8334 \
  --data-dir ./data/zion-core-v1
```

### B) Health check node

```bash
curl -s http://127.0.0.1:8444/jsonrpc \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"get_info","params":{},"id":1}'
```

### C) Wallet operations

```bash
# wallet info
zion-wallet info --wallet my-wallet.json

# sign / verify
zion-wallet sign --wallet my-wallet.json --message-hex deadbeef
zion-wallet verify --public-key-hex PUBKEY_HEX --message-hex deadbeef --signature-hex SIG_HEX
```

### D) Miner proti vlastní infrastruktuře

```bash
# doporučeno: veřejný pool
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet TVOJE_ZION_ADRESA --threads 0
```

### E) systemd služby (node + miner)

Node service (`/etc/systemd/system/zion-node.service`):

```ini
[Unit]
Description=ZION Core Node
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/zion-node-linux-x86_64 --network mainnet --rpc-port 8444 --p2p-port 8334 --data-dir /var/lib/zion
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
```

Miner service (`/etc/systemd/system/zion-miner.service`):

```ini
[Unit]
Description=ZION Miner
After=network.target zion-node.service

[Service]
Type=simple
ExecStart=/usr/local/bin/zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet TVOJE_ZION_ADRESA --threads 0
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Aktivace:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now zion-node zion-miner
sudo systemctl status zion-node zion-miner
```

---

## 🌐 Komunita

- **Website:** [zionterranova.com](https://zionterranova.com)
- **Discord:** [discord.gg/zion-terranova](https://discord.gg/zion-terranova)
- **GitHub:** [github.com/Zion-TerraNova](https://github.com/Zion-TerraNova)

---

**Happy Mining! ⛏️✨**  
*ZION TerraNova — Built with ❤️ by the Community*
