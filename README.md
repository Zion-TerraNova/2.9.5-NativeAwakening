# ⚡ ZION TerraNova v2.9.5 — Native Awakening

**Consciousness-Driven Proof-of-Work Blockchain**

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Release](https://img.shields.io/badge/Release-v2.9.5-blue.svg)](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases)

## 🌟 Overview

ZION TerraNova is a next-generation blockchain built on the **Cosmic Harmony v3 (CHv3)** consensus algorithm — a multi-algorithm proof-of-work system that enables fair, decentralized mining across diverse hardware.

### Key Features

- 🔮 **Cosmic Harmony v3** — Multi-algorithm mining (RandomX, Yescrypt, Blake3, kHeavyHash)
- ⚖️ **Dynamic Difficulty Adjustment** — LWMA-based targeting 60s blocks
- 🛡️ **Reorg Protection** — Soft finality at 60 blocks
- 🏛️ **DAO Governance** — On-chain voting and proposal system
- 💚 **Humanitarian Tithe** — 10% of block rewards to verified causes

---

## ⛏️ Quick Start — Start Mining in 3 Minutes

### 1. Download

Go to **[Releases](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases)** and download the binary for your platform:

| Binary | Platform |
|--------|----------|
| `zion-miner-windows-x86_64.exe` | Windows 11 / 10 — x64 |
| `zion-miner-linux-x86_64` | Linux — Intel / AMD (most servers & PCs) |
| `zion-miner-linux-arm64` | Linux — ARM64 (Raspberry Pi, Oracle Cloud, AWS Graviton) |
| `zion-miner-macos-arm64` | macOS — Apple Silicon (M1/M2/M3/M4) |

### 2. Install

```bash
# Linux (x86_64 example — use arm64 variant if on ARM)
chmod +x zion-miner-linux-x86_64
sudo mv zion-miner-linux-x86_64 /usr/local/bin/zion-miner

# macOS
chmod +x zion-miner-macos-arm64
xattr -d com.apple.quarantine zion-miner-macos-arm64
sudo mv zion-miner-macos-arm64 /usr/local/bin/zion-miner
```

```powershell
# Windows 11 / 10 (PowerShell)
Rename-Item .\zion-miner-windows-x86_64.exe zion-miner.exe
.\zion-miner.exe --version
```

### 3. Mine

```bash
zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet YOUR_ZION_ADDRESS
```

That's it! 🎉 The miner connects to the public ZION pool and starts mining with **Cosmic Harmony v3**.

### Options

```
--pool <URL>            [REQUIRED] Pool URL (stratum+tcp://host:port)
--wallet <ADDRESS>      [REQUIRED] Your ZION wallet address
--algorithm <ALGO>      cosmic_harmony | randomx | yescrypt | blake3
--threads <N>           CPU threads (0 = auto-detect all cores)
--gpu                   Enable GPU mining (Metal/CUDA/OpenCL)
--ncl                   Enable Neural Compute Layer (AI bonus)
```

**Public Pool:** `stratum+tcp://pool.zionterranova.com:3333`  
**Public Node RPC:** `https://node.zionterranova.com/jsonrpc`

> 📖 **Detailed guide (CZ/EN):** [docs/MINING_GUIDE.md](docs/MINING_GUIDE.md)

---

## 💰 Wallet CLI — Generate & Manage Your Wallet

```bash
# Generate a new wallet (24-word mnemonic)
zion-wallet gen-mnemonic --print

# Check balance
zion-wallet balance --address zion1your_address --node https://node.zionterranova.com

# Send ZION
zion-wallet send --to zion1recipient --amount 100 --node https://node.zionterranova.com
```

Download from **[Releases](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases)**:

| Binary | Platform |
|--------|----------|
| `zion-wallet-windows-x86_64.exe` | Windows 11 / 10 — x64 |
| `zion-wallet-linux-x86_64` | Linux Intel/AMD |
| `zion-wallet-linux-arm64` | Linux ARM64 |
| `zion-wallet-macos-arm64` | macOS Apple Silicon |

---

## 🌐 Node CLI — Run Your Own Full Node

```bash
# Start a full node
zion-node --network mainnet --rpc-port 8444 --p2p-port 8334 --data-dir ./data/zion

# Check node status
curl -s http://127.0.0.1:8444/jsonrpc \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"get_info","params":{},"id":1}'
```

Download from **[Releases](https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases)**:

| Binary | Platform |
|--------|----------|
| `zion-node-windows-x86_64.exe` | Windows 11 / 10 — x64 |
| `zion-node-linux-x86_64` | Linux Intel/AMD |
| `zion-node-linux-arm64` | Linux ARM64 |
| `zion-node-macos-arm64` | macOS Apple Silicon |

> 💡 Running a node is **optional** for miners. The public pool handles blockchain communication. Run a node if you want to verify transactions independently or support decentralization.

---

## 📁 Repository Contents

```
├── docs/
│   ├── MINING_GUIDE.md              # 📖 Detailed mining guide (CZ/EN)
│   ├── MAINNET_CONSTITUTION.md      # 🏛️ Mainnet constitution
│   └── whitepaper-v2.9.5/           # 📄 Whitepaper chapters
├── releases/                        # ⛏️ Pre-compiled binaries
│   ├── zion-miner-linux-x86_64      #     Miner — Linux Intel/AMD
│   ├── zion-miner-linux-arm64       #     Miner — Linux ARM64
│   ├── zion-miner-macos-arm64       #     Miner — macOS Apple Silicon
│   ├── zion-miner-windows-x86_64.exe #    Miner — Windows x64
│   ├── zion-node-linux-x86_64       #     Node — Linux Intel/AMD
│   ├── zion-node-linux-arm64        #     Node — Linux ARM64
│   ├── zion-node-macos-arm64        #     Node — macOS Apple Silicon
│   ├── zion-node-windows-x86_64.exe #     Node — Windows x64
│   ├── zion-wallet-linux-x86_64     #     Wallet — Linux Intel/AMD
│   ├── zion-wallet-linux-arm64      #     Wallet — Linux ARM64
│   ├── zion-wallet-macos-arm64      #     Wallet — macOS Apple Silicon
│   └── zion-wallet-windows-x86_64.exe #   Wallet — Windows x64
├── LICENSE                          # MIT License
├── README.md                        # This file
└── ROADMAP.md                       # Development roadmap
```

---

## 📖 Documentation

- **[Complete Guide — Beginner + Pro](docs/MINING_GUIDE.md)** — Wallet, Miner, Node (CZ/EN, Windows/Linux/macOS)
- **[Beginner Quick Start](docs/MINING_GUIDE.md#-quick-start--5-minut-do-těžby)** — 5 minutes to mining
- **[Pro Runbook](docs/MINING_GUIDE.md#-infrastruktura--systemd-služby)** — systemd, nginx, monitoring, security
- **[CLI Reference](docs/MINING_GUIDE.md#-cli-reference)** — All commands & parameters
- **[Whitepaper v2.9.5](docs/whitepaper-v2.9.5/README.md)** — Technical whitepaper
- **[Mainnet Constitution](docs/MAINNET_CONSTITUTION.md)** — Governance rules
- **[Roadmap](ROADMAP.md)** — Development milestones

---

## 🔮 Cosmic Harmony v3

ZION's unique consensus algorithm rotates between multiple PoW algorithms, ensuring:

- **ASIC Resistance** — No single hardware dominates
- **Fair Distribution** — CPU miners stay competitive
- **Energy Efficiency** — Dynamic difficulty reduces waste
- **Security** — Multi-algorithm makes 51% attacks exponentially harder

| Algorithm | Type | Best Hardware |
|-----------|------|---------------|
| Cosmic Harmony | Auto-rotate | 🌟 Recommended |
| RandomX | CPU-optimized | Modern CPUs |
| Yescrypt | Memory-hard | CPUs with large cache |
| Blake3 | Fast hash | Any CPU |

---

## 🌐 Community

- **Website:** [zionterranova.com](https://zionterranova.com)
- **Discord:** [discord.gg/zion-terranova](https://discord.gg/zion-terranova)
- **GitHub:** [github.com/Zion-TerraNova](https://github.com/Zion-TerraNova)

---

## 📜 License

MIT License — see [LICENSE](LICENSE) for details.

---

**Built with ❤️ by the ZION TerraNova Community**
