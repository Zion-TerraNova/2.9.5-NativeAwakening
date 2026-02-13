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

### Základní příkaz — Solo mining

```bash
zion-miner --wallet VAŠE_ZION_ADRESA
```

To je vše! Miner se připojí k výchozímu RPC endpointu a začne těžit s algoritmem **Cosmic Harmony**.

### Příklad s vlastním RPC

```bash
zion-miner \
  --wallet zion1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh \
  --rpc-url http://node.zionterranova.com:8080/jsonrpc
```

### Všechny dostupné parametry

| Parametr | Popis | Výchozí hodnota |
|----------|-------|-----------------|
| `--wallet` | **[POVINNÝ]** Vaše ZION adresa pro odměny | — |
| `--rpc-url` | URL adresa ZION node RPC | `http://127.0.0.1:8080/jsonrpc` |
| `--algorithm` | Algoritmus těžby | `cosmic_harmony` |
| `--max-iterations` | Maximum iterací na pokus | `10000000` |
| `--poll-interval` | Interval dotazování v sekundách | `5` |
| `--help` | Zobrazí nápovědu | — |
| `--version` | Zobrazí verzi | — |

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
zion-miner --wallet VAŠE_ADRESA --algorithm cosmic_harmony

# Specifický algoritmus (pro pokročilé)
zion-miner --wallet VAŠE_ADRESA --algorithm randomx
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
  --wallet VAŠE_ZION_ADRESA \
  --rpc-url http://127.0.0.1:8080/jsonrpc
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
zion-miner --wallet VAŠE_ADRESA
# Ctrl+B, pak D pro odpojení

# S screen
screen -S miner
zion-miner --wallet VAŠE_ADRESA
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

### "Connection refused" / "RPC error"

Miner se nemůže připojit k ZION node. Zkontrolujte:

1. Je ZION node spuštěný? (`curl http://127.0.0.1:8080/jsonrpc`)
2. Používáte správnou `--rpc-url`?
3. Je firewall otevřený na portu 8080?

```bash
# Test připojení k veřejnému node
zion-miner --wallet VAŠE_ADRESA --rpc-url http://node.zionterranova.com:8080/jsonrpc
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

**Ne nutně.** Pro začátek můžete použít veřejný RPC endpoint. Pro lepší výkon a decentralizaci doporučujeme spustit vlastní node.

### Kolik vydělám?

Záleží na výkonu vašeho CPU a aktuální obtížnosti sítě. Blok je nalezen každých ~60 sekund. Vaše šance na nalezení bloku roste s vaším hashratu vůči celkové síti.

### Je to bezpečné?

Ano. Miner pouze počítá hashe a komunikuje s ZION node přes RPC. Nepotřebuje přístup k vašemu privátnímu klíči — pouze veřejnou wallet adresu.

### Mohu těžit na Raspberry Pi?

Ano! Stáhněte verzi `zion-miner-linux-arm64`. Raspberry Pi 4/5 zvládne těžbu, ale hashrate bude nižší než u výkonných serverů.

### Kde získám ZION wallet adresu?

Navštivte [zionterranova.com](https://zionterranova.com) nebo se zeptejte na [Discordu](https://discord.gg/zion-terranova).

---

## 🌐 Komunita

- **Website:** [zionterranova.com](https://zionterranova.com)
- **Discord:** [discord.gg/zion-terranova](https://discord.gg/zion-terranova)
- **GitHub:** [github.com/Zion-TerraNova](https://github.com/Zion-TerraNova)

---

**Happy Mining! ⛏️✨**  
*ZION TerraNova — Built with ❤️ by the Community*
