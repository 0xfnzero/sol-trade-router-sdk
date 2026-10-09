#!/usr/bin/env bash
# Offline checks by default; --mainnet explicitly runs the existing live suites.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if [[ $# -gt 1 || ( $# -eq 1 && "$1" != "--mainnet" ) ]]; then
  echo "Usage: scripts/check.sh [--mainnet]" >&2
  exit 2
fi
cargo check --workspace
cargo test --workspace
if [[ ${1:-} == "--mainnet" ]]; then
  export ROUTER_TEST_EVIDENCE_DIR="${ROUTER_TEST_EVIDENCE_DIR:-$ROOT/target/mainnet-evidence}"
  cargo test -p sol-trade-router-sdk mainnet_ -- --ignored --nocapture --test-threads=1
fi
