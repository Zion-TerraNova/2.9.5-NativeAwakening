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

### 3. Mine

```bash
zion-miner --wallet YOUR_ZION_ADDRESS
```

That's it! 🎉 The miner connects to the default RPC endpoint and starts mining with **Cosmic Harmony v3**.

### Options

```
--wallet <ADDRESS>      [REQUIRED] Your ZION wallet address
--rpc-url <URL>         RPC endpoint (default: http://127.0.0.1:8080/jsonrpc)
--algorithm <ALGO>      cosmic_harmony | randomx | yescrypt | blake3
--poll-interval <SEC>   Polling interval in seconds (default: 5)
```

> 📖 **Detailed guide (CZ/EN):** [docs/MINING_GUIDE.md](docs/MINING_GUIDE.md)

---

## 📁 Repository Contents

```
├── docs/
│   ├── MINING_GUIDE.md              # 📖 Detailed mining guide (CZ/EN)
│   ├── MAINNET_CONSTITUTION.md      # 🏛️ Mainnet constitution
│   └── whitepaper-v2.9.5/           # 📄 Whitepaper chapters
├── releases/                        # ⛏️ Pre-compiled miner binaries
│   ├── zion-miner-linux-x86_64      #     Linux Intel/AMD
│   ├── zion-miner-linux-arm64       #     Linux ARM64
│   └── zion-miner-macos-arm64       #     macOS Apple Silicon
├── LICENSE                          # MIT License
├── README.md                        # This file
└── ROADMAP.md                       # Development roadmap
```

---

## 📖 Documentation

- **[Mining Guide](docs/MINING_GUIDE.md)** — Step-by-step for beginners (Czech & English)
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
