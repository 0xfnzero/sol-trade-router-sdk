# sol-trade-router-sdk v0.4.0

Released 2026-10-26.

Require complete verified Pump.fun V1/V2 market snapshots before router quote/build or copy/sniper nonce consumption. Preserve unknown fields instead of inventing mint owners or quote mints. Official multi-hop and unresolved wrappers remain observable parser events requiring cache enrichment; V3 uses its explicit pool API. Update parser/trade dependencies and routing documentation.

Router 报价构建及跟单/狙击领取 nonce 前校验完整市场快照；未知字段不猜测。官方多跳和未知包装回调需缓存补全，V3 使用专用池接口，并同步依赖与文档。

## Validation / 验证

Native-language regression suites and replay/build checks passed for the synchronized changes. Four trade implementations completed curve buy/sell and pool buy/sell unsigned mainnet simulations (16 successful simulations); these small-amount interface checks do not replace fresh production quotes. No transaction was broadcast.
