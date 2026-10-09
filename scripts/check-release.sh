#!/usr/bin/env bash
# Source/package/SBF checks only. Never deploys, publishes or broadcasts.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if [[ $# -gt 1 || ( $# -eq 1 && "$1" != "--allow-dirty" ) ]]; then
  echo "Usage: scripts/check-release.sh [--allow-dirty]" >&2
  exit 2
fi
VM_PYTHON="${ROUTER_VM_PYTHON:-python3}"
"$VM_PYTHON" - <<'PY'
import solders
assert solders.__version__ == "0.29.0", "Install solders==0.29.0 in ROUTER_VM_PYTHON's environment"
PY
cargo check --workspace
cargo test --workspace
PACKAGE_ARGS=()
if [[ ${1:-} == "--allow-dirty" ]]; then PACKAGE_ARGS+=(--allow-dirty); fi
cargo package -p sol-trade-router-sdk "${PACKAGE_ARGS[@]}"
# The extracted registry package must run its tests without repository fixtures.
cargo test --manifest-path target/package/sol-trade-router-sdk-0.2.0/Cargo.toml --locked
cargo build-sbf --tools-version v1.53 --manifest-path programs/sol-trade-router/Cargo.toml --features bpf-entrypoint
cargo build-sbf --tools-version v1.53 --manifest-path scripts/mock-dex/Cargo.toml
"$VM_PYTHON" scripts/test-router-security.py target/deploy/sol_trade_router.so
"$VM_PYTHON" scripts/test-router-multihop.py target/deploy/sol_trade_router.so scripts/mock-dex/target/deploy/router_test_dex.so
# Optional captured external programs; never fetch/deploy them implicitly.
if [[ -n ${ROUTER_PUMP_ELF:-} || -n ${ROUTER_FEE_ELF:-} ]]; then
  : "${ROUTER_PUMP_ELF:?Set both ROUTER_PUMP_ELF and ROUTER_FEE_ELF}"
  : "${ROUTER_FEE_ELF:?Set both ROUTER_PUMP_ELF and ROUTER_FEE_ELF}"
  FEE_FIXTURE="${ROUTER_PUMP_FEE_FIXTURE:-scripts/fixtures/pump-v4-mainnet-fees.json}"
  for MATRIX in --native-rent-matrix --hook-matrix; do
    "$VM_PYTHON" scripts/test-pump-v3-local.py target/deploy/sol_trade_router.so "$ROUTER_PUMP_ELF" \
      --fee-program "$ROUTER_FEE_ELF" --fee-fixture "$FEE_FIXTURE" "$MATRIX"
  done
fi
