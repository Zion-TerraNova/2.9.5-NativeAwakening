#!/bin/bash
# ZION TerraNova v2.9.5 — One-click installer (Linux / macOS)
set -euo pipefail

VERSION="v2.9.5"
BASE="https://github.com/Zion-TerraNova/2.9.5-NativeAwakening/releases/download/${VERSION}"
DEST="/usr/local/bin"

# Detect OS + arch
OS=$(uname -s)
ARCH=$(uname -m)

case "${OS}-${ARCH}" in
  Linux-x86_64)   SUFFIX="linux-x86_64"  ;;
  Linux-aarch64)  SUFFIX="linux-arm64"    ;;
  Darwin-arm64)   SUFFIX="macos-arm64"    ;;
  *) echo "❌ Unsupported: ${OS}-${ARCH}"; echo "   Supported: Linux x86_64/arm64, macOS arm64"; exit 1 ;;
esac

echo ""
echo "⚡ ZION TerraNova ${VERSION} Installer"
echo "   OS: ${OS}  Arch: ${ARCH}  → ${SUFFIX}"
echo ""

# What to install
TOOLS="${1:-all}"
case "$TOOLS" in
  all)    BINS="zion-miner zion-wallet zion-node" ;;
  miner)  BINS="zion-miner"  ;;
  wallet) BINS="zion-wallet" ;;
  node)   BINS="zion-node"   ;;
  *)      echo "Usage: $0 [all|miner|wallet|node]"; exit 1 ;;
esac

# Download + install
for BIN in $BINS; do
  FILE="${BIN}-${SUFFIX}"
  echo "📥 Downloading ${BIN}..."

  if command -v wget &>/dev/null; then
    wget -q --show-progress "${BASE}/${FILE}" -O "/tmp/${FILE}"
  else
    curl -fSL "${BASE}/${FILE}" -o "/tmp/${FILE}"
  fi

  chmod +x "/tmp/${FILE}"

  # macOS: remove quarantine
  if [ "$OS" = "Darwin" ]; then
    xattr -d com.apple.quarantine "/tmp/${FILE}" 2>/dev/null || true
  fi

  sudo mv "/tmp/${FILE}" "${DEST}/${BIN}"
  echo "✅ ${BIN} → ${DEST}/${BIN}"
done

echo ""
echo "=== ✅ Installation complete! ==="
echo ""

# Show versions
for BIN in $BINS; do
  echo "  $(${BIN} --version 2>&1)"
done

echo ""
echo "🚀 Quick start:"
echo "  zion-wallet gen-mnemonic --out my-wallet.json --print"
echo "  zion-miner --pool stratum+tcp://pool.zionterranova.com:3333 --wallet YOUR_ADDRESS"
echo ""
