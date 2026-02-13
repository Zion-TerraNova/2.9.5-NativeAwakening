# ZION TerraNova — Quick Start Guide 🚀

> **Run a ZION node in under 10 minutes.**

---

## Requirements

| Resource | Minimum | Recommended |
|----------|---------|-------------|
| CPU | 2 cores | 4 cores |
| RAM | 2 GB | 4 GB |
| Disk | 10 GB SSD | 50 GB SSD |
| OS | Ubuntu 22.04+ / Debian 12+ | Ubuntu 24.04 |
| Network | 10 Mbps | 100 Mbps |
| Ports | 8334 (P2P), 8444 (RPC) | + 3333 (Stratum), 8080 (Pool API) |

**Docker required**: Docker Engine 24+ and Docker Compose v2+.

---

## 🏃 Option A: Docker (Recommended, ~5 minutes)

### 1. Clone the repository

```bash
git clone https://github.com/Zion-TerraNova/2.9.5-NativeAwakening.git
cd Zion-2.9.5
```

### 2. Configure your wallet address

```bash
# Set your ZION wallet address for mining rewards
export MINER_WALLET="zion1qYOUR_ADDRESS_HERE"

# Optional: set seed peers (default: Helsinki seed node)
export SEED_PEERS="seed1.zionterranova.com:8334,seed3.zionterranova.com:8334,seed4.zionterranova.com:8334"
```

### 3. Start the full stack

```bash
cd docker
docker compose -f docker-compose.testnet.yml up -d
```

This starts 4 containers:
| Container | Port | Description |
|-----------|------|-------------|
| `zion-core` | 8334, 8444 | Blockchain node (P2P + RPC) |
| `zion-pool` | 3333, 8080 | Mining pool (Stratum + Stats API) |
| `zion-miner` | — | CPU miner (auto-starts mining) |
| `zion-redis` | — | Share tracking cache |

### 4. Verify it's working

```bash
# Check all containers are running
docker ps --filter 'name=zion-'

# Check blockchain height
curl -s http://localhost:8444/jsonrpc \
  -d '{"jsonrpc":"2.0","id":1,"method":"get_info"}' \
  -H 'Content-Type: application/json'

# Expected output:
# {"jsonrpc":"2.0","id":1,"result":{"height":465,"difficulty":1648877,"status":"OK",...}}
```

### 5. View logs

```bash
# All services
docker compose -f docker-compose.testnet.yml logs -f

# Just the node
docker logs -f zion-core

# Just the miner
docker logs -f zion-miner
```

### 6. Stop

```bash
docker compose -f docker-compose.testnet.yml down
```

Data persists in Docker volumes. To remove data too: `docker compose down -v`

---

## 🔧 Option B: Build from Source (~15 minutes)

### 1. Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
rustup default stable
```

### 2. Clone and build

```bash
git clone https://github.com/Zion-TerraNova/2.9.5-NativeAwakening.git
cd Zion-2.9.5

# Install build dependencies (Ubuntu/Debian)
sudo apt-get update && sudo apt-get install -y cmake g++ make pkg-config libssl-dev

# Build all binaries (node, pool, miner)
cargo build --release
```

Binaries will be in `target/release/`:
- `zion-core` — Blockchain node
- `zion-pool` — Mining pool
- `zion-miner` — CPU/GPU miner

### 3. Run the node

```bash
./target/release/zion-core \
  --data-dir ./data \
  --rpc-port 8444 \
  --p2p-port 8334 \
  --network testnet \
  --peers "seed1.zionterranova.com:8334,seed3.zionterranova.com:8334,seed4.zionterranova.com:8334"
```

### 4. Run the miner (in another terminal)

```bash
./target/release/zion-miner \
  --pool localhost:3333 \
  --wallet "zion1qYOUR_ADDRESS_HERE" \
  --worker my-miner \
  --algorithm cosmic_harmony_v3
```

---

## 🔍 Useful Commands

### Check node status
```bash
curl -s localhost:8444/jsonrpc \
  -d '{"jsonrpc":"2.0","id":1,"method":"get_info"}' \
  -H 'Content-Type: application/json' | python3 -m json.tool
```

### Check supply
```bash
curl -s localhost:8444/jsonrpc \
  -d '{"jsonrpc":"2.0","id":1,"method":"get_supply"}' \
  -H 'Content-Type: application/json' | python3 -m json.tool
```

### Check connected peers
```bash
curl -s localhost:8444/jsonrpc \
  -d '{"jsonrpc":"2.0","id":1,"method":"get_peer_info"}' \
  -H 'Content-Type: application/json' | python3 -m json.tool
```

---

## 🌐 Network Info

### TestNet Seed Nodes

| Location | IP | P2P | RPC |
|----------|-----|-----|-----|
| 🇫🇮 Helsinki (primary) | seed1.zionterranova.com | :8334 | :8444 |
| 🇺🇸 USA | seed3.zionterranova.com | :8334 | :8444 |
| 🇸🇬 Singapore | seed4.zionterranova.com | :8334 | :8444 |

### Chain Parameters

| Parameter | Value |
|-----------|-------|
| Network | `ZION-TESTNET-V1` |
| Block time | 60 seconds |
| Block reward | 5,400.067 ZION (constant) |
| Algorithm | Cosmic Harmony v3 (multi-algo) |
| DAA | LWMA (60-block window) |
| Consensus | Proof-of-Work, highest accumulated work |

---

## ❓ Troubleshooting

### Node won't sync
```bash
# Check if peers are connected
docker logs zion-core 2>&1 | grep -i "peer\|connect"

# Restart with fresh data
docker compose down -v
docker compose -f docker-compose.testnet.yml up -d
```

### Port already in use
```bash
# Find what's using the port
lsof -i :8334
# Kill it or change ports in docker-compose.yml
```

### Miner shows 0 H/s
```bash
# Check miner logs for errors
docker logs zion-miner 2>&1 | tail -20

# Ensure pool is connected to core
docker logs zion-pool 2>&1 | grep -i "block\|template"
```

### Check disk usage
```bash
docker system df
du -sh /var/lib/docker/volumes/zion*
```

---

## 📚 More Documentation

- [Mainnet Roadmap](../MAINNET_ROADMAP_2026.md) — Full roadmap to launch
- [Constitution](docs/MAINNET_CONSTITUTION.md) — Immutable protocol parameters
- [Economic Model](../ECONOMIC_CALCULATIONS_CORRECT.md) — Token economics
- [Whitepaper](docs/whitepaper/) — Technical whitepaper

---

## 🤝 Join the Network

Every node strengthens ZION. By running a node you:
- ✅ Help decentralize the network
- ✅ Validate transactions independently
- ✅ Earn mining rewards (5,400+ ZION/block)
- ✅ Support the path to Mainnet (31.12.2026)

**Welcome aboard!** 🌟

---

*ZION TerraNova v2.9.5 — "Where technology meets spirit"*
