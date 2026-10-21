# sol-trade-router-sdk v0.3.0

Released 2026-10-22. Pins sol-parser-sdk =0.9.1 and sol-trade-sdk =9.0.0.

## Default stock pools / 默认股票代币池子

Remove five pools from the default catalog: two have captured single-transaction withdrawals reducing both vault balances by at least 50%; four have less than $500 in USDC/WSOL-side reserves at the 2026-10-22 04:25 UTC+8 snapshot (one meets both criteria). Defaults now contain 58 stock pools across 33 mints; trading adds the SOL/USDC conversion pool for 59 pools total. COPX.US is no longer a default mint. See [the synchronized catalog notes](docs/STOCK_BRIDGE_SYNC.md) for addresses and evidence.

This is a static catalog refresh, not continuous liquidity monitoring, certified executable depth, or an LP-lock guarantee. Explicit custom catalogs remain supported. No RPC or history scanning was added to parsing or trading hot paths.

中文：默认目录剔除 5 个池子，涵盖整池双边储备单笔下降至少 50% 的历史记录，以及快照中 USDC/WSOL 一侧储备不足 500 美元的池子。保留 58 个股票池、33 个 mint；交易侧加 SOL/USDC 共 59 个池子。该目录为静态快照，不保证实时深度或流动性锁定。

## Changes / 更新

- Synchronize client adapters with the current parser events and trade SDK parameter types.
- Expose the shared stock-price and instance-owned PumpSwap routing configuration through the client SDK, with native offline validation and updated examples/navigation.
- Keep routing limits, snapshot validation and per-hop protections in the trade SDK. Building from prepared inputs performs no RPC.

## Migration / 升级

Re-exported trade configuration now defaults Pump.fun client trades to V3; set a version explicitly for compatibility. Re-exported direct trade builders do not automatically become router-program CPI support. Read the supported Router legs before selecting a route.

This release publishes the client SDK and matching repository source; it does not deploy or upgrade an on-chain program. Existing self-deployment requirements in docs/RELEASE_0.2.0.md still apply.

中文：客户端依赖同步到 parser 0.9.1 和 trade 9.0.1。SDK 转导出的直接交易功能不等于 Router 合约新增对应 CPI 能力；发布不会部署或升级链上合约。
