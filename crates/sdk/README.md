# Sol Trade Router SDK

Version 0.4.0 provides the Rust client for the matching Pinocchio Router source
in this repository. Deploy that program yourself and pass its program ID with
`RouterTradeConfig::with_program_id` or `RouterClient::with_program_id`.
The historical default program ID is not a compatible deployment for this release.

```toml
[dependencies]
sol-trade-router-sdk = "=0.4.0"
```

Dependencies use published sol-trade-sdk 10.0.0 and sol-parser-sdk 0.9.0. No Git
patches or local sibling checkouts are needed. The SDK preserves a zero-RPC hot
path: load current mint/pool/fee state before building trades.

The paired program supports bound tag-2 output mints, dynamic tags 5/6, and
Pump creator-vault preparation tag 7. Retired dynamic tags 3/4 are rejected.
Keep the configured platform fee and recipient consistent with your deployment.

See the repository README and docs/RELEASE_0.2.0.md for self-deployment and
validation. These tests and preparation steps do not deploy a contract or send
network transactions. Current dependencies support Unix targets; the pinned
Yellowstone 13.5.0 dependency still has an upstream Windows import limitation.


## PumpSwap and parser synchronization (0.4.0)

This release pins `sol-trade-sdk =10.0.0` and `sol-parser-sdk =0.10.0`.
PumpSwap buy, exact-output buy and sell legs validate creator vault authority and
ATA against the actual quote mint/token program before constructing CPI accounts.
Missing token programs or fee recipients are rejected instead of guessed. Exact-output
Buy includes the volume flag; protocol recipients and global volume stay readonly.

Use `LoadMarketRequest::PumpSwapPool { pool }` to cold-load an explicit pool address,
including non-WSOL quote pools. For SOL-funded trades, prepare the resolved bridge
and target snapshots, then call `build_dynamic_quote_buy` for WSOL → quote → target.
PumpSwap can be either hop, with buy/sell chosen by the actual mint direction.
The matching Router program spends only each preceding hop's newly credited output.
Set up the required ATAs and fund/sync WSOL before the route; the builder does no RPC.
The high-level pool guard still requires explicitly trusting noncanonical pools.

A trade SDK `PumpSwapQuoteRoute` may contain splits and merges and cannot be converted
to one `RoutedMarket`; use explicit Router legs and the two/three-hop builders.
Unresolved parser candidates with `third_program` are not trade/reserve snapshots.
Use executed trade events plus current pool state for those snapshots. The old
`PumpRouteIntent` callback has been removed.

Build and deploy the matching Router source to use dynamic PumpSwap buys. Publishing
the SDK does not change an existing on-chain deployment.


Stock bridge synchronization: this crate now uses parser 0.10.0 and trade 9.0.0.
The root exports `stock_prices`, `PumpSwapRoutingConfig`, `PumpSwapRouting` and
`PumpSwapRoutingFeed`. The shared catalog has 33 stock/ETF mints and 58 pools;
trade routing additionally registers WSOL/USDC. Re-exported routing builds
direct trade-sdk instructions, not automatic router-program CPI dispatch.
