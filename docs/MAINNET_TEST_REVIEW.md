# Mainnet test review — 2026-10-08

Existing suites were updated in place. Network tests no longer return early and appear as passing offline tests: 81 router network cases are explicitly ignored by default, and an explicit live run fails on unavailable RPC, missing samples or unsuccessful execution. Positive execution cases use current pool/config loaders and funded WSOL inputs; instruction execution tests with minimum output 1 do not prove quote accuracy. Negative balance cases require balance errors, and fault cases require bank execution logs.

## Router results

| Check | Result |
| --- | --- |
| Workspace compile | Passed |
| SDK offline tests | 93 passed; 81 network cases ignored |
| Program / example tests | 15 / 2 passed |
| Existing all-DEX builder test | 18 legs × legacy/v0/v1 = 54 fully signed paths; wire roundtrip and tampered-message rejection |
| Direct mainnet suite | Latest complete run: 44 passed / one failed (286.77 seconds). The CLMM reverse case used a dust-sized fixed input; updated to the observed forward output and passed its targeted rerun. All 45 cases are aggregate verified; no complete green rerun after this last test correction is claimed. |
| Special live samples | Token-2022 CLMM and Pump cashback both pass using real cold-loaded state, original signed transaction parsing and successful bank execution. |
| Current mainnet router | `mainnet_router_pumpswap_buy` fails with Custom(12), `InvalidLeg` |

Current deployment `CmNFUmRJL7YcnVn22oZzwG5Xg5WJqbcHEc6BK5mzDNR8` has not been upgraded. Fresh confirmed ProgramData at slot 454491114 still matches the captured ELF (deployment slot 452320587, SHA-256 `88d0e3b52bd10dc69d107f3be3f166c846307e8225b1780881f89d63732f6f46`). Its PR #1 wire layout is incompatible with the repaired legacy header. The existing local ELF security suite now asserts the exact `InvalidLeg(12)` rejection and atomic rollback of the repaired header. The deployed ELF still accepts understated fees, budget overruns and wrong output mints under the short header. See `ROUTER_DEPLOYMENT_RECHECK.json`. Direct DEX simulation success does not establish router-deployment compatibility. Upgrade planning must consider SDK and program together; this review did not broadcast or deploy.

Pump V3 cold loading retains the user's requested official default `supports_graduation=true`. The captured real program ELF's synthetic boundary results remain as documented in `PUMP_V3_DEPLOYMENT_RECHECK.json`; SDK construction support does not certify cross-graduation execution on the target deployment.

## Shared SDK validation

Workspace evidence is in `../tools/validation/simulation-coverage-20261008` and `../tools/validation/protocol-upgrades-20261008` (outside this repository).

- Rust trade: 75 existing registered cases rerun after authority-signing changes: 71 execution verified, four build-only verified.
- Native business-wire matrix: 22 buy/sell cases across 11 venue names executed successfully through the shared simulation harness. Original business messages and the augmented trade-authority signatures were verified. This is not a claim that each language independently signs the augmented funding transaction.
- Nine negative slippage cases produced the expected protocol error/code/instruction.
- Saved simulation replay: 263 Rust wires without parser errors; 188 cases without ALT replayed by Python/Node/Go without semantic mismatches. RPC projections of simulations are synthetic and are not signed on-chain transactions.
- Latest capture: 120 transactions from ten program addresses, all original signatures verified, including V1. Three additional special-pool originals have independently verified signatures. Accumulated corpus: 182 original transactions; Python, Node, Go, Rust and streamer each match 196 independently decoded official events and 3,882 exposed fields without differences. Go RPC and gRPC both pass. Shred's 182-transaction raw instruction/route replay has no errors.

Field comparison found two genuine bugs beyond parse exceptions: Go PumpSwap exact-quote buys kept the minimum output as actual output, and Rust/streamer AMM V4 logs used the first invocation's pool in multi-invocation transactions. Local parser source fixes and real captured regression fixtures now verify Go actual output and Rust RPC/parallel-gRPC/sequential-gRPC account and amount attribution. The router workspace now resolves parser 0.7.10 to fixed Git revision `51460a91ca3326663a59651ac0860744ac10fdc0` through a root Cargo patch, so its test/build results include the AMM fix. This revision is not a crates.io release. Cargo does not inherit a dependency workspace's root patch: external git/path consumers must add the same patch in their own root manifest until the fix is released, as shown in both READMEs. The Rust trade dependency is also pinned to `bf4a01cd340ba8cc84d01cc7e1531fa2412a02a6`: its CLMM cold loader now refreshes the pool bitmap, skips allocated but empty arrays, scans beyond five adjacent arrays, and validates selected array ownership/layout/pool/start. The regression surfaced as `InvalidFirstTickArrayAccount(6024)` before the reverse test could reach its required balance error. Both CLMM positive and reverse targeted simulations pass after correction. The reverse negative-balance test now uses the real forward output as its reverse input, preventing tiny raw units from reaching the protocol dust check before the expected insufficient balance error. These fixes remain separate draft PRs. Reviewable draft PRs: [Rust parser #32](https://github.com/0xfnzero/sol-parser-sdk/pull/32), [Go parser #6](https://github.com/0xfnzero/sol-parser-sdk-golang/pull/6), [Rust trade upgrades and CLMM #123](https://github.com/0xfnzero/sol-trade-sdk/pull/123).

Rust trade final library suite: 388 passed / three ignored. Native suite results: Node trade 4,248 passed; Python trade 4,338 passed; Go trade passed; Node parser 344 passed / eight skipped. Final affected suites: Python parser 420 passed; Go parser passed; Rust parser full test run: 494 passed / one ignored (including 420 library tests and the updated captured-transaction integration test). Streamer builds with its local parser dependency.

## Signature and execution scope

Only ephemeral keys are used. Business authorities sign the exact simulation message and signatures are checked locally. The public virtual funder's signature is absent and RPC uses `sigVerify=false`; these are successful bank simulations, not fully signed broadcastable transactions. Offline legacy/v0/v1 tests verify all required signatures. No private keys are saved in evidence.

Use `scripts/check.sh` for offline compile/tests. Use `SOLANA_RPC_URL=... scripts/check.sh --mainnet` for all ignored router tests, including current deployment checks; it is expected to fail until router deployment compatibility is resolved. The 45 direct protocol tests can be rerun with `cargo test -p sol-trade-router-sdk mainnet_sim_tests:: -- --ignored --nocapture --test-threads=1`. Special pool candidates and discovery provenance are recorded in `scripts/fixtures/live-special-pools.json`; current owners, quote direction and cashback flags are always checked again, and CLMM tick arrays are loaded for the actual input/output direction. `ROUTER_TEST_EVIDENCE_DIR` optionally retains signed public messages, original transactions and simulation responses.

The Pump V2 positive case prioritizes a supported current curve; mints with `PermanentDelegate` (extension 12) remain rejected. This does not assert support for every Token-2022 extension.

The LaunchLab test formerly named `graduated_pool_events` only performed a regular buy; it is renamed `secondary_pool_buy` and does not claim graduation coverage. LaunchLab prioritizes a currently executable SOL-quoted candidate, and Token-2022 CLMM can discover its pool from either observed trade direction before loading arrays for the tested WSOL buy.

Changed Rust test files pass targeted rustfmt checks and `git diff --check`; repository-wide cargo fmt still reports older formatting differences outside this supplement. The earlier supplement reviewed all eight files. This completion reviews all ten changed or added files, including the Cargo pin, test code, installation instructions and public evidence, zero skipped (100%).
