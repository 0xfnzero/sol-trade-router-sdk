#!/usr/bin/env bash
# Host-side compile check for SDK + program (no SBF toolchain required).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> cargo check -p sol-trade-router-sdk"
cargo check -p sol-trade-router-sdk

echo "==> cargo check -p sol-trade-router"
cargo check -p sol-trade-router

echo "==> ok"
