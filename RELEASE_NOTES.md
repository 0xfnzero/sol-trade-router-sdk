# sol-trade-router-sdk v0.3.0

Released 2026-10-22. Pins sol-parser-sdk =0.9.0 and sol-trade-sdk =9.0.0.

## Changes / 更新

- Synchronize client adapters with the current parser events and trade SDK parameter types.
- Expose the shared stock-price and instance-owned PumpSwap routing configuration through the client SDK, with native offline validation and updated examples/navigation.
- Keep routing limits, snapshot validation and per-hop protections in the trade SDK. Building from prepared inputs performs no RPC.

## Migration / 升级

Re-exported trade configuration now defaults Pump.fun client trades to V3; set a version explicitly for compatibility. Re-exported direct trade builders do not automatically become router-program CPI support. Read the supported Router legs before selecting a route.

This release publishes the client SDK and matching repository source; it does not deploy or upgrade an on-chain program. Existing self-deployment requirements in docs/RELEASE_0.2.0.md still apply.

中文：客户端依赖同步到 parser 0.9.0 和 trade 9.0.0。SDK 转导出的直接交易功能不等于 Router 合约新增对应 CPI 能力；发布不会部署或升级链上合约。
