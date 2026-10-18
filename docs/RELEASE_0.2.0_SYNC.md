# 0.2.0 PumpSwap synchronization — October 18, 2026

This initial crates.io release includes the complete Router SDK and the matching on-chain program source in GitHub tag `v0.2.0`. It uses published `sol-trade-sdk =6.0.2` and `sol-parser-sdk =0.7.14` without local dependency patches.

## Changes

- Validate PumpSwap creator vault authority and ATA against the actual coin creator, quote mint and token program before building any buy or sell. Missing token-program or fee-recipient identities fail locally instead of producing invalid CPIs.
- Reproduce the reported `coin_creator_vault_ata` 3007 failure using transaction `34mNYS59JLuy6q55gS1PbYY2DN9v1mtkuCMhdoXKRaiB5WczkfweVAhuTC7dVyEpLzg94T4HvNd5Eu75Gbkaodbe`. The regression rejects its missing creator accounts and verifies all corrected account identities, privileges and exact-output instruction bytes against the captured snapshot.
- Include the volume flag in the 25-byte exact-output PumpSwap buy instruction.
- Add `LoadMarketRequest::PumpSwapPool { pool }` for explicit custom quote pools, including non-WSOL and Token-2022 quotes.
- Accept exact-input PumpSwap buys as dynamic following hops in both the SDK and Router program. Explicit two/three-hop routes consume fresh intermediate credits, retain the output floor and roll back on invalid layouts, overspending or slippage.
- Keep route intents separate from pool snapshots. A prepared upstream multi-pool route must be converted into explicit Router legs rather than flattened into a single market.

## Validation

- `cargo check --workspace --locked`: passed.
- `cargo test --workspace`: 116 passed; 81 live tests and one doctest ignored.
- `cargo package -p sol-trade-router-sdk --allow-dirty --locked`: passed with registry dependencies.
- Tests from the extracted crate: 97 passed; 81 live tests and one doctest ignored. The reported-failure fixture is included in the package.
- Router and mock DEX SBF builds: passed with tools v1.53.
- Security VM: 15 scenarios passed.
- Multi-hop VM: 96 scenarios passed, including PumpSwap buys/sells, pre-existing balances, Token-2022 transfer fees, underspending, overspending, wrong output mints, invalid volume flags and full rollback.

Machine-readable results and binary hashes are in [summary.json](../tools/validation/release-0.2.0-sync/summary.json). The adjacent VM outputs record individual cases. The final registry package and GitHub release are verified separately against the release commit during publication.

## Compatibility and limits

Install `sol-trade-router-sdk = "=0.2.0"`. New dynamic PumpSwap buys require a matching self-deployed Router program. Publishing the SDK or attaching the compiled program to a GitHub Release does not upgrade an existing deployment. No program was deployed or upgraded and no funded mainnet transactions were broadcast during this release validation.

Local multi-hop tests execute real token transfers but use a mock DEX with synthetic pricing. They verify Router accounting and rollback, not live liquidity, DEX pricing or transaction landing. Optional captured Pump program matrices were not rerun for this synchronization; prior results remain identified as the October 9 baseline. See [release notes](RELEASE_0.2.0.md) for platform and toolchain limits.
