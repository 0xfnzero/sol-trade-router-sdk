# Mainnet test review — 2026-10-08

Existing suites were updated in place. Network tests no longer return early and appear as passing offline tests: 81 router network cases are explicitly ignored by default, and an explicit live run fails on unavailable RPC, missing samples or unsuccessful execution. Positive execution cases use current pool/config loaders and funded WSOL inputs; instruction execution tests with minimum output 1 do not prove quote accuracy. Negative balance cases require balance errors, and fault cases require bank execution logs.

## Router results

| Check | Result |
| --- | --- |
| Workspace compile | Passed |
| SDK offline tests | 93 passed; 81 network cases ignored |
| Program / example tests | 15 / 2 passed |
| Existing all-DEX builder test | 18 legs × legacy/v0/v1 = 54 fully signed paths; wire roundtrip and tampered-message rejection |
| Direct mainnet suite | Initial full run: 37/45 passed; six repaired failures passed targeted reruns, giving 43/45 aggregate verified cases. No subsequent full green run claimed. |
| Still missing eligible live samples | `mainnet_sim_clmm_token2022_prefer`, `mainnet_sim_pumpfun_cashback_buy_soft`; remain explicit failures |
| Current mainnet router | `mainnet_router_pumpswap_buy` fails with Custom(12), `InvalidLeg` |

Current deployment `CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8` has not been upgraded. Its PR #1 wire layout is incompatible with the repaired legacy header. Direct DEX simulation success does not establish router-deployment compatibility. Upgrade planning must consider SDK and program together; this review did not broadcast or deploy.

Pump V3 cold loading retains the user's requested official default `supports_graduation=true`. The captured real program ELF's synthetic boundary results remain as documented in `PUMP_V3_DEPLOYMENT_RECHECK.json`; SDK construction support does not certify cross-graduation execution on the target deployment.

## Shared SDK validation

Workspace evidence is in `../tools/validation/simulation-coverage-20261008` and `../tools/validation/protocol-upgrades-20261008` (outside this repository).

- Rust trade: 75 existing registered cases rerun after authority-signing changes: 71 execution verified, four build-only verified.
- Native business-wire matrix: 22 buy/sell cases across 11 venue names executed successfully through the shared simulation harness. Original business messages and the augmented trade-authority signatures were verified. This is not a claim that each language independently signs the augmented funding transaction.
- Nine negative slippage cases produced the expected protocol error/code/instruction.
- Saved simulation replay: 263 Rust wires without parser errors; 188 cases without ALT replayed by Python/Node/Go without semantic mismatches. RPC projections of simulations are synthetic and are not signed on-chain transactions.
- Latest capture: 120 transactions from ten program addresses, all original signatures verified, including V1. Accumulated corpus: 179 original transactions; Python, Node, Go, Rust and streamer each match 190 independently decoded official events and 3,763 exposed fields without differences. Go RPC and gRPC both pass. Shred's 179-transaction raw instruction/route replay has no errors.

Field comparison found two genuine bugs beyond parse exceptions: Go PumpSwap exact-quote buys kept the minimum output as actual output, and Rust/streamer AMM V4 logs used the first invocation's pool in multi-invocation transactions. Local parser source fixes and real captured regression fixtures now verify Go actual output and Rust RPC/parallel-gRPC/sequential-gRPC account and amount attribution. These sibling changes are not published dependencies of this router PR.

Native suite results: Node trade 4,248 passed; Python trade 4,338 passed; Go trade passed; Node parser 344 passed / eight skipped. Final affected suites: Python parser 420 passed; Go parser passed; Rust parser library 420 passed / one ignored plus the updated captured-transaction integration test passed. Streamer builds with its local parser dependency.

## Signature and execution scope

Only ephemeral keys are used. Business authorities sign the exact simulation message and signatures are checked locally. The public virtual funder's signature is absent and RPC uses `sigVerify=false`; these are successful bank simulations, not fully signed broadcastable transactions. Offline legacy/v0/v1 tests verify all required signatures. No private keys are saved in evidence.

Use `scripts/check.sh` for offline compile/tests. Use `SOLANA_RPC_URL=... scripts/check.sh --mainnet` for all ignored router tests, including current deployment checks; it is expected to fail until deployment compatibility and missing special samples are resolved. `ROUTER_TEST_EVIDENCE_DIR` optionally retains signed public messages, original transactions and simulation responses.

Changed Rust test files pass targeted rustfmt checks and `git diff --check`; repository-wide cargo fmt still reports older formatting differences outside this supplement. Manual review covers all eight supplement files, zero skipped (100%).
