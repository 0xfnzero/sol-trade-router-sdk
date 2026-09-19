#!/usr/bin/env bash
# Build the on-chain Pinocchio router for SBF.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/programs/sol-trade-router/Cargo.toml"

echo "==> building sol-trade-router (SBF)"
cargo build-sbf --manifest-path "$MANIFEST" --features bpf-entrypoint

echo "==> done"
ls -la "$ROOT"/target/deploy/*.so 2>/dev/null || ls -la "$ROOT"/target/sbpf-solana-solana/release/*.so 2>/dev/null || true
