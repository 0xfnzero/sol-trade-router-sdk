# SDK 0.2.0 release readiness

This release fixes output-mint validation, input-budget accounting, dynamic two/three-hop fresh-credit consumption, Token-2022 fee transfers, and native Pump setup-rent accounting. It rejects unsupported exact-output paths and missing/unbound quote state rather than silently producing a misleading quote. SDK APIs and dynamic route formats changed; use 0.2.0 with matching Router source.

## Installation and self-deployment

The SDK package is prepared for crates.io publication, but this change does not publish it. Before publication use the repository's `main` branch as shown in the root README. The package depends on published `sol-trade-sdk =6.0.0` and `sol-parser-sdk =0.7.12`; no Git or local registry patches are required. Package fixtures and MIT license are included.

Users build/deploy the Router themselves. Use the deployed program ID with `RouterTradeConfig::with_program_id` or `RouterClient::with_program_id`; examples require `ROUTER_PROGRAM_ID`. Initialize that program's own config PDA and match its fee settings. The historical default address is not evidence of compatibility. Matching source supports static Route tag 2, dynamic tags 5/6 and Pump preparation tag 7. Legacy dynamic tags 3/4 are rejected. Nothing in this PR deploys or upgrades a contract.

The verified host is macOS/Unix. The upstream Yellowstone 13.5.0 dependency has an unconditional Unix import; Windows support is not claimed. The SDK manifest declares Rust 1.91; validation used the installed current host toolchain, not a separate minimum-version toolchain run.

## Reproduce validation

Install the Python VM dependency in a dedicated environment:

```sh
python3 -m venv /tmp/router-vm
/tmp/router-vm/bin/pip install solders==0.29.0
ROUTER_VM_PYTHON=/tmp/router-vm/bin/python bash scripts/check-release.sh
```

The script checks/tests the workspace, verifies the cargo package, tests its extracted contents, builds Router and the test-only DEX with SBF tools v1.53, and executes security and multi-hop VM matrices. Use `--allow-dirty` only while validating local edits. It never deploys, publishes or broadcasts. The optional captured Pump matrices require both ELF paths:

```sh
ROUTER_VM_PYTHON=/tmp/router-vm/bin/python \
ROUTER_PUMP_ELF=/path/to/captured-pump.so \
ROUTER_FEE_ELF=/path/to/captured-fee-program.so \
bash scripts/check-release.sh
```

The default fee snapshot is `scripts/fixtures/pump-v4-mainnet-fees.json`; override it with `ROUTER_PUMP_FEE_FIXTURE` when matching a different captured program. The script does not fetch external ELFs implicitly.

## Evidence and limits

The complete check exited successfully on 2026-10-09: 111 workspace tests, 93 extracted-package tests, and 147 local VM scenarios (15 security, 40 multi-hop, 86 native Pump rent, 6 disabled-hook round trips). The package tests run without repository-relative fixtures or workspace patches. Detailed outputs and ELF hashes are in [release evidence](../tools/validation/release-0.2.0/summary.json).

The multi-hop fixture executes genuine SPL/Token-2022 CPIs but uses synthetic 2:1 pricing. It covers pre-existing intermediate balances, 1% transfer fees, slippage, wrong mint/alias accounts, under/over-consumption, and full token-state rollback including withheld fees. It validates Router accounting, not real CPMM pricing. Pump matrices execute captured external programs against constructed account state; they are not current-mainnet liquidity tests.

81 live tests and one doctest remain opt-in. A newly requested Pump buy smoke test failed during `get_slot` after three public-RPC transport errors, before simulation. No new live-suite pass is claimed. Re-run the ignored mainnet tests with a reachable RPC and matching self-deployed Router before using a particular live venue.

The simulation approach follows the Rust `sol-trade-sdk` authority-signing model: real business signers, an isolated virtual funder, explicit RPC signature-verification settings, and separate setup/transport failure classification. Local VM transactions additionally verify signatures before execution; quote tests use official-source golden vectors and independent boundary oracles already included in this PR.
