# ⚡ ZION v2.9.5 — Kompletní průvodce | Complete Guide

> **Verze:** v2.9.5 Native Awakening  
> **Aktualizováno:** 14. únor 2026  
> **Jazyk / Language:** Čeština + English

---

## 📋 Obsah / Table of Contents

### Laik / Beginner 🟢
1. [Co je ZION?](#-co-je-zion)
2. [Co potřebuji?](#-co-potřebuji)
3. [Quick Start — 5 minut do těžby](#-quick-start--5-minut-do-těžby)
4. [Wallet — vytvoření peněženky](#-wallet--vytvoření-peněženky-krok-za-krokem)
5. [Miner — spuštění těžby](#️-miner--spuštění-těžby)
6. [Node — spuštění vlastního uzlu](#-node--spuštění-vlastního-uzlu)
7. [Kontrola zůstatku + odeslání](#-kontrola-zůstatku--odeslání-zion)

### Profi / Professional 🔴
8. [Infrastruktura — systemd služby](#️-infrastruktura--systemd-služby)
9. [Node — pokročilá konfigurace + monitoring](#️-node--pokročilá-konfigurace--monitoring)
10. [Wallet — pokročilé operace](#-wallet--pokročilé-operace)
11. [Miner — pokročilý tuning](#-miner--pokročilý-tuning)
12. [Kompletní stack na jednom serveru](#-kompletní-stack-na-jednom-serveru)
13. [Bezpečnost a best practices](#️-bezpečnost-a-best-practices)

### Přílohy
14. [Řešení problémů / Troubleshooting](#-řešení-problémů--troubleshooting)
15. [FAQ](#-faq)
16. [CLI reference (všechny parametry)](#-cli-reference)

---

# 🟢 ČÁST 1 — LAIK (Beginner)

---

## 🌟 Co je ZION?

ZION TerraNova je **proof-of-work blockchain** — tvůj počítač řeší matematické úlohy a za každý nalezený blok dostaneš odměnu v mincích **ZION**.

- ✅ **Spravedlivý** — algoritmus Cosmic Harmony v3 rotuje mezi různými typy výpočtů, žádný hardware nemá trvalou výhodu
- ✅ **Ekologický** — dynamická obtížnost snižuje zbytečnou spotřebu
- ✅ **Decentralizovaný** — může těžit kdokoli s běžným počítačem
- ✅ **Humanitární** — 10 % z každého bloku jde na dobročinné projekty

**Blok:** každých ~60 sekund | **Algoritmus:** Cosmic Harmony v3 (RandomX + Yescrypt + Blake3)

---

## 💻 Co potřebuji?

| Věc | Minimum | Doporučeno |
|-----|---------|-----------|
| **Systém** | Windows 10/11, Linux, macOS | Cokoliv z toho |
| **Procesor** | 2 jádra | 4+ jader (AMD Ryzen, Intel 12th+, Apple M1+) |
| **RAM** | 2 GB | 4+ GB |
| **Disk** | 100 MB | 500 MB (s vlastním node) |
| **Internet** | Jakékoliv | Stabilní, nízká latence |

> 💡 **Těžit můžeš i na Raspberry Pi 4/5!**

---

## 🚀 Quick Start — 5 minut do těžby

### Krok 1: Stáhni

Jdi na **[Releases](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/tag/v2.9.5)** a stáhni **2 soubory** pro tvůj systém:

| Tvůj systém | Wallet | Miner |
|-------------|--------|-------|
| **Windows 10/11** | `zion-wallet-windows-x86_64.exe` | `zion-miner-windows-x86_64.exe` |
| **Linux (Intel/AMD)** | `zion-wallet-linux-x86_64` | `zion-miner-linux-x86_64` |
| **Linux (ARM / RPi)** | `zion-wallet-linux-arm64` | `zion-miner-linux-arm64` |
| **macOS (M1–M4)** | `zion-wallet-macos-arm64` | `zion-miner-macos-arm64` |

### Krok 2: Nainstaluj

<details>
<summary>🪟 <b>Windows 10 / 11</b></summary>

1. Stáhněte oba `.exe` soubory do složky (např. `C:\ZION\`)
2. Otevřete **PowerShell** (klikněte pravým na Start → "Terminal" / "PowerShell")
3. Přejděte do složky:
```powershell
cd C:\ZION
```
4. Ověřte:
```powershell
.\zion-wallet-windows-x86_64.exe --version
.\zion-miner-windows-x86_64.exe --version
```

> 💡 **Tip:** Můžete přejmenovat soubory pro pohodlí:
> ```powershell
> Rename-Item .\zion-wallet-windows-x86_64.exe zion-wallet.exe
> Rename-Item .\zion-miner-windows-x86_64.exe zion-miner.exe
> ```

> ⚠️ **Windows Defender:** Pokud Windows zablokuje spuštění, klikněte "Více informací" → "Přesto spustit". Binárka je bezpečná — open-source projekt bez digitálního podpisu.

</details>

<details>
<summary>🐧 <b>Linux (x86_64 / ARM64)</b></summary>

```bash
# Nahraďte ARCH podle vašeho systému: x86_64 nebo arm64
ARCH=x86_64

# Stažení
wget https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-wallet-linux-${ARCH}
wget https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-linux-${ARCH}

# Práva + přesun
chmod +x zion-wallet-linux-${ARCH} zion-miner-linux-${ARCH}
sudo mv zion-wallet-linux-${ARCH} /usr/local/bin/zion-wallet
sudo mv zion-miner-linux-${ARCH} /usr/local/bin/zion-miner

# Ověření
zion-wallet --version
zion-miner --version
```

</details>

<details>
<summary>🍎 <b>macOS (Apple Silicon — M1/M2/M3/M4)</b></summary>

```bash
# Stažení
curl -LO https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-wallet-macos-arm64
curl -LO https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5/zion-miner-macos-arm64

# Práva + odblokování Gatekeeper
chmod +x zion-wallet-macos-arm64 zion-miner-macos-arm64
xattr -d com.apple.quarantine zion-wallet-macos-arm64
xattr -d com.apple.quarantine zion-miner-macos-arm64

# Přesun
sudo mv zion-wallet-macos-arm64 /usr/local/bin/zion-wallet
sudo mv zion-miner-macos-arm64 /usr/local/bin/zion-miner

# Ověření
zion-wallet --version
zion-miner --version
```

> ⚠️ Pokud macOS hlásí *"cannot be opened because the developer cannot be verified"*: **System Settings → Privacy & Security → Allow Anyway**

</details>

### Krok 3: Vytvoř peněženku

```bash
zion-wallet gen-mnemonic --out my-wallet.json --print
```

**Windows:**
```powershell
.\zion-wallet.exe gen-mnemonic --out my-wallet.json --print
```

Zobrazí se:
```
Mnemonic (24 words): apple banana cherry ... zebra
Address: zion1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh
Wallet saved to: my-wallet.json
```

> 🔐 **DŮLEŽITÉ:** Zapiš si 24 slov na papír! Kdo má slova, má přístup k tvým mincím. Nikdy je nesdílej online!

### Krok 4: Spusť těžbu

```bash
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet zion1TVOJE_ADRESA
```

**Windows:**
```powershell
.\zion-miner.exe --pool stratum+tcp://pool.zionterranova.com:3333 --wallet zion1TVOJE_ADRESA
```

**🎉 Hotovo! Těžíš ZION!** Miner se připojí na veřejný pool a začne hledat bloky.

### Krok 5: Zkontroluj zůstatek

```bash
zion-wallet balance --address zion1TVOJE_ADRESA --node https://node.zionterranova.com
```

---

## 💰 Wallet — Vytvoření peněženky krok za krokem

### Co je wallet?

Wallet (peněženka) je klíčový pár:
- **Veřejná adresa** (`zion1...`) — jako číslo účtu, sdíleš s ostatními
- **Privátní klíč** — jako PIN, nikdy nesdílej!
- **24 slov (mnemonic)** — záloha klíče, zapsat na papír

### Nová peněženka (24 slov)

```bash
zion-wallet gen-mnemonic --out my-wallet.json --print
```

**Windows:**
```powershell
.\zion-wallet.exe gen-mnemonic --out my-wallet.json --print
```

Volitelně délka mnemonic:

```bash
# 12 slov (kratší, jednodušší)
zion-wallet gen-mnemonic --words 12 --out wallet.json --print

# 24 slov (výchozí, bezpečnější)
zion-wallet gen-mnemonic --words 24 --out wallet.json --print
```

### Obnovení ze zálohy (24 slov)

Pokud máš zálohu 24 slov z dřívějška:

```bash
zion-wallet import-mnemonic --mnemonic "apple banana cherry ... zebra" --out recovered-wallet.json
```

### Obnovení z privátního klíče

```bash
zion-wallet import-secret-key --secret-key HEX_KLÍČE --out recovered-wallet.json
```

### Zobrazení adresy z wallet souboru

```bash
zion-wallet address --wallet my-wallet.json
```

### Validace adresy

```bash
zion-wallet validate --address zion1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh
```

---

## ⛏️ Miner — Spuštění těžby

### Nejjednodušší příkaz

```bash
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet TVOJE_ADRESA
```

**Windows:**
```powershell
.\zion-miner.exe --pool stratum+tcp://pool.zionterranova.com:3333 --wallet TVOJE_ADRESA
```

Miner automaticky:
- Detekuje počet CPU jader
- Vybere optimální algoritmus (Cosmic Harmony)
- Připojí se k veřejnému poolu

### S výběrem vláken

```bash
# Použij 4 vlákna (nech zbytek pro systém)
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet TVOJE_ADRESA --threads 4
```

### Výstup mineru

Po spuštění uvidíš:
```
[INFO] ZION Miner v2.9.5 — Cosmic Harmony v3
[INFO] Connecting to pool.zionterranova.com:3333...
[INFO] Connected! Mining with 4 threads
[INFO] Algorithm: cosmic_harmony (auto-rotate)
[INFO] Hashrate: 485.29 kH/s
[INFO] Share accepted! (difficulty: 1024)
```

### Zastavení

Stiskni `Ctrl+C` pro zastavení mineru.

---

## 🌐 Node — Spuštění vlastního uzlu

> 💡 **Potřebuješ vlastní node?** Ne! Pro těžbu stačí veřejný pool. Node spouštěj pouze pokud:
> - Chceš ověřovat transakce sám
> - Plánuješ provozovat vlastní pool
> - Chceš podpořit decentralizaci sítě

### Stažení

| Tvůj systém | Soubor |
|-------------|--------|
| Windows 10/11 | `zion-node-windows-x86_64.exe` |
| Linux Intel/AMD | `zion-node-linux-x86_64` |
| Linux ARM64 | `zion-node-linux-arm64` |
| macOS M1–M4 | `zion-node-macos-arm64` |

### Instalace

<details>
<summary>🪟 <b>Windows</b></summary>

```powershell
cd C:\ZION
.\zion-node-windows-x86_64.exe --version
# Volitelně přejmenovat:
Rename-Item .\zion-node-windows-x86_64.exe zion-node.exe
```

</details>

<details>
<summary>🐧 <b>Linux</b></summary>

```bash
chmod +x zion-node-linux-x86_64
sudo mv zion-node-linux-x86_64 /usr/local/bin/zion-node
```

</details>

<details>
<summary>🍎 <b>macOS</b></summary>

```bash
chmod +x zion-node-macos-arm64
xattr -d com.apple.quarantine zion-node-macos-arm64
sudo mv zion-node-macos-arm64 /usr/local/bin/zion-node
```

</details>

### Spuštění

```bash
zion-node --network mainnet --rpc-port 8444 --p2p-port 8334 --data-dir ./data/zion
```

**Windows:**
```powershell
.\zion-node.exe --network mainnet --rpc-port 8444 --p2p-port 8334 --data-dir .\data\zion
```

### Co node dělá?

1. Připojí se k síti (P2P na portu 8334)
2. Stáhne celý blockchain (synchronizace)
3. Ověřuje všechny transakce a bloky
4. Zpřístupní JSON-RPC API na portu 8444

---

## 💸 Kontrola zůstatku + Odeslání ZION

### Kontrola zůstatku

```bash
zion-wallet balance --address zion1TVOJE_ADRESA --node https://node.zionterranova.com
```

**Windows:**
```powershell
.\zion-wallet.exe balance --address zion1TVOJE_ADRESA --node https://node.zionterranova.com
```

### Odeslání ZION

```bash
zion-wallet send \
  --wallet my-wallet.json \
  --to zion1ADRESA_PRIJEMCE \
  --amount 100 \
  --node https://node.zionterranova.com
```

**Windows:**
```powershell
.\zion-wallet.exe send --wallet my-wallet.json --to zion1ADRESA_PRIJEMCE --amount 100 --node https://node.zionterranova.com
```

> ⚠️ **Transakce je nevratná!** Vždy ověřte adresu příjemce.

---

# 🔴 ČÁST 2 — PROFI (Professional)

---

## 🏗️ Infrastruktura — systemd služby

### Node jako systemd služba

Vytvořte `/etc/systemd/system/zion-node.service`:

```ini
[Unit]
Description=ZION TerraNova Full Node
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=zion
Group=zion
ExecStart=/usr/local/bin/zion-node \
  --network mainnet \
  --rpc-port 8444 \
  --p2p-port 8334 \
  --data-dir /var/lib/zion/node
Restart=always
RestartSec=5
LimitNOFILE=65535
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

### Miner jako systemd služba

Vytvořte `/etc/systemd/system/zion-miner.service`:

```ini
[Unit]
Description=ZION TerraNova Miner
After=network-online.target zion-node.service
Wants=network-online.target

[Service]
Type=simple
User=zion
Group=zion
ExecStart=/usr/local/bin/zion-miner \
  --pool stratum+tcp://pool.zionterranova.com:3333 \
  --wallet zion1VAŠE_ADRESA \
  --threads 0 \
  --algorithm cosmic_harmony
Restart=always
RestartSec=10
Nice=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=multi-user.target
```

### Setup a aktivace

```bash
# Vytvořte systémového uživatele
sudo useradd -r -m -d /var/lib/zion -s /bin/false zion
sudo mkdir -p /var/lib/zion/node
sudo chown -R zion:zion /var/lib/zion

# Nainstalujte binárky
sudo cp zion-node-linux-x86_64 /usr/local/bin/zion-node
sudo cp zion-miner-linux-x86_64 /usr/local/bin/zion-miner
sudo chmod +x /usr/local/bin/zion-node /usr/local/bin/zion-miner

# Reload + enable + start
sudo systemctl daemon-reload
sudo systemctl enable --now zion-node
sudo systemctl enable --now zion-miner

# Kontrola
sudo systemctl status zion-node zion-miner
```

### Logy

```bash
# Sledování živě
sudo journalctl -u zion-node -f
sudo journalctl -u zion-miner -f

# Poslední hodina
sudo journalctl -u zion-node --since "1 hour ago"
```

---

## 🖥️ Node — Pokročilá konfigurace + monitoring

### Parametry

| Parametr | Popis | Výchozí |
|----------|-------|---------|
| `--network` | Síť: `mainnet` / `testnet` | `testnet` |
| `--rpc-port` | Port pro JSON-RPC API | `8444` |
| `--p2p-port` | Port pro P2P síťování | `8334` |
| `--data-dir` | Složka pro blockchain data | `./data/zion-core-v1` |
| `--peers` | Seznam počátečních peerů | auto-discovery |

### Firewall (ufw)

```bash
# P2P port (povinný)
sudo ufw allow 8334/tcp comment "ZION P2P"

# RPC port (pouze pokud chcete veřejné API)
sudo ufw allow 8444/tcp comment "ZION RPC"
```

### Nginx reverse proxy (HTTPS pro RPC)

```nginx
server {
    listen 443 ssl http2;
    server_name node.vasedomena.com;

    ssl_certificate     /etc/letsencrypt/live/node.vasedomena.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/node.vasedomena.com/privkey.pem;

    location /jsonrpc {
        proxy_pass http://127.0.0.1:8444/jsonrpc;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        limit_req zone=rpc burst=20 nodelay;
    }
}
```

### Health check skript

```bash
#!/bin/bash
# /opt/zion/health-check.sh

NODE_RPC="http://127.0.0.1:8444/jsonrpc"

HEIGHT=$(curl -sf "$NODE_RPC" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"get_info","params":{},"id":1}' \
  | jq -r '.result.height // "ERROR"')

PEERS=$(curl -sf "$NODE_RPC" \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"get_peer_info","params":{},"id":1}' \
  | jq '.result | length // 0')

echo "[$(date)] Height: $HEIGHT | Peers: $PEERS"

if [ "$HEIGHT" = "ERROR" ]; then
  echo "⚠️ NODE DOWN — restarting"
  sudo systemctl restart zion-node
fi
```

```bash
# Přidejte do cron (každých 5 min)
echo "*/5 * * * * /opt/zion/health-check.sh >> /var/log/zion-health.log 2>&1" | sudo crontab -
```

### JSON-RPC metody

| Metoda | Popis |
|--------|-------|
| `get_info` | Stav node (výška, peers, verze) |
| `get_block_template` | Šablona pro mining |
| `get_peer_info` | Seznam připojených peerů |
| `submit_block` | Odeslání nalezeného bloku |
| `get_transaction` | Detail transakce |
| `get_balance` | Zůstatek adresy |

```bash
# Příklad: stav node
curl -s http://127.0.0.1:8444/jsonrpc \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"get_info","params":{},"id":1}' | jq .
```

---

## 🔑 Wallet — Pokročilé operace

### Podepisování zpráv

```bash
# Podepsání (hex encoded message)
zion-wallet sign --wallet my-wallet.json --message-hex 48656c6c6f

# Ověření podpisu
zion-wallet verify \
  --public-key-hex VEŘEJNÝ_KLÍČ_HEX \
  --message-hex 48656c6c6f \
  --signature-hex PODPIS_HEX
```

### Info o wallet souboru

```bash
zion-wallet info --wallet my-wallet.json
```

### Záloha a obnova

```bash
# Záloha
cp my-wallet.json /media/usb-backup/zion-wallet-$(date +%Y%m%d).json

# Obnova ze slov
zion-wallet import-mnemonic \
  --mnemonic "word1 word2 word3 ... word24" \
  --out recovered-wallet.json

# Obnova z klíče
zion-wallet import-secret-key \
  --secret-key KLÍČ_HEX \
  --out recovered-wallet.json
```

### Bezpečné úložiště (Linux)

```bash
chmod 600 my-wallet.json

# Šifrovaný disk (volitelné)
sudo cryptsetup luksFormat /dev/sdb1
sudo cryptsetup open /dev/sdb1 zion-vault
sudo mkfs.ext4 /dev/mapper/zion-vault
sudo mount /dev/mapper/zion-vault /mnt/zion-vault
cp my-wallet.json /mnt/zion-vault/
```

---

## ⚡ Miner — Pokročilý tuning

### Výběr algoritmu

| Algoritmus | Nejlepší pro | Hashrate profil |
|-----------|-------------|-----------------|
| `cosmic_harmony` | 🌟 Všechny CPU | Auto-rotace, vyvážený |
| `randomx` | Moderní CPU (velká L3 cache) | Střední, ASIC-odolný |
| `yescrypt` | CPU s hodně RAM | Paměťově náročný |
| `blake3` | Jakékoliv CPU | Nejvyšší hashrate |

```bash
# Automatická rotace (doporučeno)
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ADRESA --algorithm cosmic_harmony

# Specifický algoritmus
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ADRESA --algorithm randomx
```

### GPU mining

```bash
# Metal (macOS) / CUDA / OpenCL (Linux)
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ADRESA --gpu

# GPU + Neural Compute Layer
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ADRESA --gpu --ncl
```

### Optimalizace CPU (Linux)

```bash
# 75 % jader
THREADS=$(( $(nproc) * 3 / 4 ))
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ADRESA --threads $THREADS

# Nízká priorita
nice -n 19 zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ADRESA

# CPU affinity (jádra 0-3)
taskset -c 0-3 zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ADRESA --threads 4
```

### Monitoring

```bash
# Hashrate z logů
sudo journalctl -u zion-miner -f | grep -i hash

# Teplota CPU
watch -n 5 sensors
```

---

## 🏢 Kompletní stack na jednom serveru

### Architektura

```
┌──────────────────────────────────────┐
│              SERVER                  │
│                                      │
│  ┌─────────┐  ┌─────────┐           │
│  │  NODE    │  │  MINER  │           │
│  │ :8444   ◄──┤ → pool   │           │
│  │ :8334    │  └─────────┘           │
│  └────▲─────┘                        │
│       │                              │
│  ┌────┴─────┐                        │
│  │  NGINX   │ :443 (HTTPS)           │
│  └──────────┘                        │
│                                      │
│  ┌──────────┐                        │
│  │  WALLET  │  (CLI, on-demand)      │
│  └──────────┘                        │
└──────────────────────────────────────┘
```

### One-command setup skript

```bash
#!/bin/bash
# setup-zion-full-stack.sh — Plná instalace ZION na Ubuntu 22.04+
set -euo pipefail

ARCH=$(uname -m)
case $ARCH in
  x86_64)  SUFFIX="linux-x86_64" ;;
  aarch64) SUFFIX="linux-arm64"  ;;
  *)       echo "Nepodporovaná architektura: $ARCH"; exit 1 ;;
esac

RELEASE="https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/v2.9.5"
WALLET="${1:?Použití: $0 <vaše_zion_adresa>}"

echo "=== ZION v2.9.5 Full Stack Setup ==="

# 1) Stažení
echo "[1/5] Stahování binárek..."
wget -q "${RELEASE}/zion-node-${SUFFIX}" -O /usr/local/bin/zion-node
wget -q "${RELEASE}/zion-miner-${SUFFIX}" -O /usr/local/bin/zion-miner
wget -q "${RELEASE}/zion-wallet-${SUFFIX}" -O /usr/local/bin/zion-wallet
chmod +x /usr/local/bin/zion-{node,miner,wallet}

# 2) Uživatel
echo "[2/5] Systémový uživatel..."
useradd -r -m -d /var/lib/zion -s /bin/false zion 2>/dev/null || true
mkdir -p /var/lib/zion/node && chown -R zion:zion /var/lib/zion

# 3) Node service
echo "[3/5] Node service..."
cat > /etc/systemd/system/zion-node.service <<EOF
[Unit]
Description=ZION Full Node
After=network-online.target

[Service]
Type=simple
User=zion
ExecStart=/usr/local/bin/zion-node --network mainnet --rpc-port 8444 --p2p-port 8334 --data-dir /var/lib/zion/node
Restart=always
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
EOF

# 4) Miner service
echo "[4/5] Miner service..."
cat > /etc/systemd/system/zion-miner.service <<EOF
[Unit]
Description=ZION Miner
After=network-online.target zion-node.service

[Service]
Type=simple
User=zion
ExecStart=/usr/local/bin/zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet ${WALLET} --threads 0
Restart=always
RestartSec=10
Nice=10

[Install]
WantedBy=multi-user.target
EOF

# 5) Start
echo "[5/5] Spouštění..."
systemctl daemon-reload
systemctl enable --now zion-node
sleep 3
systemctl enable --now zion-miner

echo ""
echo "=== ✅ ZION nainstalován! ==="
echo "Node:   systemctl status zion-node"
echo "Miner:  systemctl status zion-miner"
echo "Logy:   journalctl -u zion-node -f"
echo "Wallet: zion-wallet balance --address $WALLET --node http://127.0.0.1:8444"
```

```bash
# Spuštění
sudo bash setup-zion-full-stack.sh zion1VAŠE_ADRESA
```

---

## 🛡️ Bezpečnost a best practices

### Wallet

| ✅ Dělej | ❌ Nedělej |
|----------|-----------|
| Zapiš 24 slov na papír | Neukládej slova do cloudu |
| `chmod 600 wallet.json` | Neposílej wallet.json emailem |
| Zálohuj na offline USB | Nefoť mnemonic telefonem |
| Používej silné heslo na serveru | Nesdílej privátní klíč |

### Node

| ✅ Dělej | ❌ Nedělej |
|----------|-----------|
| Firewall: povolte jen 8334 (P2P) | RPC nechte za nginx s rate-limitem |
| Pravidelně aktualizujte | Neběžte jako root |
| Monitorujte disk space | Neotevírejte RPC bez SSL |

### Miner

| ✅ Dělej | ❌ Nedělej |
|----------|-----------|
| Nastavte `nice 10+` | Neberte 100 % CPU na produkčním serveru |
| Sledujte teploty | Nemine na serverech bez monitoringu |
| Používejte pool mining | Solo mining pouze s >10 % hashrate sítě |

---

# 📎 PŘÍLOHY

---

## 🔧 Řešení problémů / Troubleshooting

### "Permission denied"
```bash
chmod +x zion-miner-linux-x86_64
```

### "cannot be opened" (macOS)
```bash
xattr -d com.apple.quarantine zion-miner-macos-arm64
# Nebo: System Settings → Privacy & Security → Allow Anyway
```

### "Windows Defender zablokoval spuštění"
1. Klikněte "Více informací" → "Přesto spustit"
2. Nebo: Windows Security → Exclusions → přidejte `C:\ZION\`

### "Connection refused" — miner se nepřipojí
```bash
# Test TCP
nc -zv pool.zionterranova.com 3333

# Alternativa přes IP
zion-miner --pool stratum+tcp://77.42.31.72:3333 --wallet ADRESA
```

### "GLIBC not found" (starší Linux)
Vyžaduje Ubuntu 22.04+, Debian 12+, RHEL 9+.
```bash
ldd --version
```

### Node se nesynchronizuje
```bash
sudo journalctl -u zion-node --since "10 min ago"
df -h /var/lib/zion
curl -s http://127.0.0.1:8444/jsonrpc \
  -d '{"jsonrpc":"2.0","method":"get_peer_info","params":{},"id":1}' | jq '.result | length'
```

### Nízký hashrate
1. Teplota CPU: `sensors` / Activity Monitor
2. Jiné procesy: `htop`
3. Jiný algoritmus: `--algorithm blake3`
4. Počet vláken: `--threads $(nproc)`

---

## ❓ FAQ

**Q: Potřebuji vlastní node na těžbu?**  
A: **Ne.** Stačí miner + pool `pool.zionterranova.com:3333`.

**Q: Kolik vydělám?**  
A: (váš hashrate / celkový hashrate sítě) × odměna za blok. Blok ~60s.

**Q: Je to bezpečné?**  
A: Ano. Miner potřebuje pouze veřejnou adresu, nikdy privátní klíč.

**Q: Mohu těžit na Raspberry Pi?**  
A: Ano! Stáhněte `*-linux-arm64`.

**Q: Mohu těžit na Windows?**  
A: Ano! Stáhněte `*-windows-x86_64.exe`. Funguje na Windows 10 i 11.

**Q: Ztratil jsem wallet soubor, mám 24 slov.**  
A: `zion-wallet import-mnemonic --mnemonic "vaše slova..." --out wallet.json`

**Q: Mohu těžit na více strojích se stejnou adresou?**  
A: Ano! Každý stroj může používat stejnou wallet adresu.

**Q: Co je Humanitarian Tithe?**  
A: 10 % z každého bloku automaticky jde na humanitární projekty.

---

## 📖 CLI Reference

### zion-wallet

```
USAGE:  zion-wallet <COMMAND> [OPTIONS]

COMMANDS:
  gen-mnemonic       Nová peněženka (BIP39 mnemonic)
  gen                Náhodný ed25519 keypair
  import-mnemonic    Obnova ze slov
  import-secret-key  Obnova z privátního klíče
  address            Zobrazí adresu
  validate           Ověří ZION adresu
  balance            Zůstatek
  send               Odeslání transakce
  sign               Podpis zprávy
  verify             Ověření podpisu
  info               Info o wallet souboru

OPTIONS:
  --wallet <FILE>         Wallet soubor
  --out <FILE>            Výstupní soubor
  --print                 Zobrazí mnemonic na stdout
  --words <N>             Délka mnemonic (12/15/18/21/24)
  --address <ADDR>        ZION adresa
  --node <URL>            URL node RPC
  --to <ADDR>             Adresa příjemce
  --amount <N>            Částka
  --mnemonic <WORDS>      Slova pro obnovu
  --secret-key <HEX>      Privátní klíč
  --message-hex <HEX>     Zpráva k podpisu
  --public-key-hex <HEX>  Veřejný klíč
  --signature-hex <HEX>   Podpis
  -h, --help | -V, --version
```

### zion-miner

```
USAGE:  zion-miner [OPTIONS]

OPTIONS:
  -p, --pool <URL>         [REQUIRED] Pool (stratum+tcp://host:port)
  -w, --wallet <ADDR>      [REQUIRED] ZION adresa
  -a, --algorithm <ALGO>   cosmic_harmony|randomx|yescrypt|blake3
  -t, --threads <N>        CPU vlákna (0 = auto)
      --gpu                GPU mining
      --ncl                Neural Compute Layer
  -h, --help | -V, --version
```

### zion-node

```
USAGE:  zion-node [OPTIONS]

OPTIONS:
      --network <NET>      mainnet|testnet (default: testnet)
      --rpc-port <PORT>    JSON-RPC port (default: 8444)
      --p2p-port <PORT>    P2P port (default: 8334)
      --data-dir <PATH>    Data dir (default: ./data/zion-core-v1)
      --peers <LIST>       Peer list
  -h, --help | -V, --version
```

---

## 🌐 Veřejné endpointy

| Služba | URL |
|--------|-----|
| **Pool (Stratum)** | `stratum+tcp://pool.zionterranova.com:3333` |
| **Node RPC (HTTPS)** | `https://node.zionterranova.com/jsonrpc` |
| **GitHub Release** | [Releases v2.9.5](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/tag/v2.9.5) |
| **Website** | [zionterranova.com](https://zionterranova.com) |
| **Discord** | [discord.gg/zion-terranova](https://discord.gg/zion-terranova) |

---

**Happy Mining! ⛏️✨**  
**ZION TerraNova v2.9.5 — Built with ❤️ by the Community**
