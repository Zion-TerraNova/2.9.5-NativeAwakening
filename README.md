# ⚡ ZION TerraNova v2.9.5 — Native Awakening

**Consciousness-Driven Proof-of-Work Blockchain**

[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)

## 🌟 Overview

ZION TerraNova is a next-generation blockchain built on the **Cosmic Harmony v3 (CHv3)** consensus algorithm — a multi-algorithm proof-of-work system that enables fair, decentralized mining across diverse hardware.

### Key Features

- **🔮 Cosmic Harmony v3** — Multi-algorithm mining (kHeavyHash, RandomX, Ethash, SHA-256d, Equihash)
- **⚖️ Dynamic Algorithm Adjustment (DAA)** — LWMA-based difficulty targeting 60s blocks
- **🌐 Multi-Chain Profit Routing** — Automatic switching between external pools for optimal rewards
- **🛡️ Reorg Protection** — Soft finality at 60 blocks, max reorg depth 10
- **🏛️ DAO Governance** — On-chain voting and proposal system
- **💚 Humanitarian Tithe** — 10% of block rewards to verified causes

## 🏗️ Architecture

```
zion-terranova/
├── core/           # Blockchain core (consensus, P2P, storage, RPC)
├── cosmic-harmony/ # CHv3 algorithm engine (5 PoW algorithms)
├── miner/          # Native multi-algo miner
├── pool/           # Mining pool with Stratum v2
├── config/         # Network configuration (devnet, mainnet)
├── docs/           # Whitepaper & technical documentation
└── tests/          # Integration & E2E test suites
```

## 🚀 Quick Start

### Prerequisites

- **Rust** 1.75+ (edition 2021)
- **OpenSSL** development headers
- **libclang** (for FFI bindings)

### Build

```bash
git clone https://github.com/Zion-TerraNova/2.9.5-NativeAwakening.git
cd 2.9.5-NativeAwakening
cargo build --release
```

### Run a Node

```bash
# DevNet (local development)
./target/release/zion-core --config config/devnet.toml

# Connect to MainNet
./target/release/zion-core --config config/mainnet.toml \
  --rpc-port 8334 --p2p-port 8444
```

### Start Mining

```bash
./target/release/zion-miner \
  --pool stratum+tcp://pool.zionterranova.com:3333 \
  --wallet YOUR_WALLET_ADDRESS \
  --algo cosmic-harmony-v3
```

## 📖 Documentation

- [Whitepaper v2.9.5](docs/whitepaper-v2.9.5/README.md)
- [Cosmic Harmony v3 Technical Spec](docs/CHv3/)
- [Quick Start Guide](QUICK_START.md)
- [Mainnet Roadmap](docs/MAINNET_ROADMAP_2026.md)

## 🔧 Configuration

Network configs are in `config/`:

| Network | File | Description |
|---------|------|-------------|
| DevNet | `devnet.toml` | Local development (localhost) |
| MainNet | `mainnet.toml` | Production network |

### Environment Variables

```bash
ZION_BTC_WALLET=your_btc_address    # BTC payout wallet
ZION_XMR_WALLET=your_xmr_address    # XMR payout wallet  
ZION_LOG_LEVEL=info                  # Log level (trace/debug/info/warn/error)
```

## 🧪 Testing

```bash
# Unit tests
cargo test

# Integration tests
cargo test --test '*'

# Benchmarks
cargo bench
```

## 🤝 Contributing

We welcome contributions! Please see our development guidelines:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/my-feature`)
3. Commit your changes (`git commit -am 'Add my feature'`)
4. Push to the branch (`git push origin feature/my-feature`)
5. Open a Pull Request

## 📜 License

MIT License — see [LICENSE](LICENSE) for details.

## 🌐 Links

- **Website:** [zionterranova.com](https://zionterranova.com)
- **GitHub:** [github.com/Zion-TerraNova](https://github.com/Zion-TerraNova)
- **Discord:** [discord.gg/zion-terranova](https://discord.gg/zion-terranova)

---

**Built with ❤️ by the ZION TerraNova Community**

*"Technology with consciousness — mining for a better world."*
